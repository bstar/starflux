//! Guarded, headless imports of externally owned sessions.
use crate::{
    automation::fault,
    session_catalog::{self, Entry},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::PathBuf,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub source: String,
    pub session: String,
    pub socket: Option<PathBuf>,
    pub fingerprint: String,
}
fn socket_stamp(path: &std::path::Path) -> Result<Value> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.file_type().is_socket() && meta.uid() == unsafe { libc::geteuid() },
        "Source socket is not owned by this user"
    );
    Ok(json!([meta.dev(), meta.ino(), meta.uid()]))
}
fn hash(value: &Value) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
fn herdr_server_stamp(socket: &std::path::Path) -> Result<Value> {
    use std::os::fd::AsRawFd;
    let stamp = socket_stamp(socket)?;
    let connection = std::os::unix::net::UnixStream::connect(socket)?;
    let mut credentials = unsafe { std::mem::zeroed::<libc::ucred>() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                connection.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut size,
            )
        } == 0,
        "Cannot identify source server"
    );
    ensure!(
        credentials.uid == unsafe { libc::geteuid() } && credentials.pid > 0,
        "Source server is not owned by this user"
    );
    let pid = credentials.pid as u32;
    let start = crate::agent_state::process_start(pid).context("Source server exited")?;
    Ok(json!([stamp, pid, start]))
}
fn prepare(entry: &Entry) -> Result<(Reference, Option<Value>)> {
    let boot = crate::agent_state::boot_id()
        .context("Host boot identity unavailable")?
        .to_string();
    if let Some(source) = &entry.tmux {
        source.validate()?;
        let before = socket_stamp(&source.socket)?;
        let mut command = source.command(&[
            "display-message",
            "-p",
            "-t",
            &source.session,
            "#{pid}\t#{session_id}\t#{session_created}\t#{session_name}",
        ]);
        command.stdin(std::process::Stdio::null());
        let status = crate::read_query::output(command)?;
        let status = String::from_utf8(status)?;
        let fields: Vec<_> = status.trim_end().split('\t').collect();
        ensure!(
            fields.len() == 4 && fields[1] == source.session,
            "Source session identity changed"
        );
        let pid = fields[0].parse::<u32>()?;
        let start = crate::agent_state::process_start(pid).context("tmux server exited")?;
        let topology = crate::read_query::output(source.command(&[
            "list-panes",
            "-s",
            "-t",
            &source.session,
            "-F",
            "#{window_id}\t#{pane_id}\t#{pane_pid}\t#{pane_dead}",
        ]))?;
        let mut topology: Vec<_> = String::from_utf8(topology)?
            .lines()
            .map(str::to_owned)
            .collect();
        topology.sort();
        ensure!(
            socket_stamp(&source.socket)? == before
                && crate::agent_state::process_start(pid) == Some(start),
            "tmux server changed during discovery"
        );
        return Ok((
            Reference {
                source: "tmux".into(),
                session: source.session.clone(),
                socket: Some(source.socket.clone()),
                fingerprint: hash(&json!([boot, before, pid, start, fields, topology]))?,
            },
            None,
        ));
    }
    ensure!(entry.source == "herdr", "Unsupported import source");
    let socket = crate::herdr::socket_path(&entry.name)?;
    let before = herdr_server_stamp(&socket)?;
    let snapshot = crate::herdr::live_snapshot(&entry.name)?;
    let snap = &snapshot["result"]["snapshot"];
    let mut identities = Vec::new();
    for (resource, fields) in [
        ("workspaces", vec!["workspace_id"]),
        ("tabs", vec!["workspace_id", "tab_id"]),
        (
            "panes",
            vec!["workspace_id", "tab_id", "pane_id", "terminal_id"],
        ),
    ] {
        let rows = snap[resource]
            .as_array()
            .context("Herdr snapshot lacks source identities")?;
        for row in rows {
            let mut identity = vec![Value::String(resource.into())];
            for field in &fields {
                ensure!(
                    row[*field]
                        .as_str()
                        .is_some_and(|id| !id.is_empty() && id.len() <= 128),
                    "Herdr snapshot contains an invalid source identity"
                );
                identity.push(row[*field].clone());
            }
            identities.push(serde_json::to_string(&identity)?);
        }
    }
    identities.sort();
    ensure!(
        herdr_server_stamp(&socket)? == before,
        "Herdr server changed during discovery"
    );
    Ok((
        Reference {
            source: "herdr".into(),
            session: entry.name.clone(),
            socket: None,
            fingerprint: hash(&json!([boot, before, identities]))?,
        },
        Some(snapshot),
    ))
}
pub fn discover() -> Result<Value> {
    let mut sources = Vec::new();
    let mut warnings = Vec::new();
    for entry in session_catalog::external() {
        match prepare(&entry) {
            Ok((reference,_))=>sources.push(json!({"name":entry.name,"source":entry.source,"detail":entry.detail,"reference":reference})),
            Err(_)=>warnings.push(json!({"source":entry.source,"name":entry.name,"code":"source_unavailable"})),
        }
    }
    sources.sort_by_key(|v| v["reference"].to_string());
    Ok(json!({"sources":sources,"warnings":warnings,"source_processes_preserved":true}))
}
pub fn import(reference: Reference) -> Result<Value> {
    if reference.fingerprint.len() != 64
        || !reference.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
        || !matches!(reference.source.as_str(), "tmux" | "herdr")
        || reference.session.is_empty()
        || reference.session.len() > 128
        || (reference.source == "tmux"
            && !reference.socket.as_ref().is_some_and(|p| p.is_absolute()))
        || (reference.source == "herdr" && reference.socket.is_some())
    {
        return Err(fault("invalid_params", "Invalid source import reference"));
    }
    let root = crate::project_state::data_root()?;
    let _lock = crate::project_sync::Transaction::acquire(root.join("session-import.lock"))?;
    let entry = session_catalog::external()
        .into_iter()
        .find(|entry| {
            entry.source == reference.source
                && if let Some(tmux) = &entry.tmux {
                    reference.socket.as_ref() == Some(&tmux.socket)
                        && reference.session == tmux.session
                } else {
                    reference.socket.is_none() && reference.session == entry.name
                }
        })
        .ok_or_else(|| {
            fault(
                "source_unavailable",
                "Source session is no longer discoverable",
            )
        })?;
    let (current, snapshot) = prepare(&entry).map_err(|_| {
        fault(
            "source_unavailable",
            "Source session is no longer available",
        )
    })?;
    if current != reference {
        return Err(fault(
            "stale_source",
            "Source session changed; discover it again before importing",
        ));
    }
    let config = session_catalog::import_prepared(&entry, snapshot)?;
    let projects: Vec<_> = session_catalog::projects()
        .into_iter()
        .filter(|p| {
            if let Some(source) = &p.tmux {
                current.source == "tmux"
                    && current.socket.as_ref() == Some(&source.socket)
                    && current.session == source.session
            } else if let Some(source) = &p.herdr {
                current.source == "herdr" && current.session == source.session
            } else {
                false
            }
        })
        .map(|p| json!({"project":p.workspace,"source":if p.herdr.is_some(){"herdr"}else{"tmux"}}))
        .collect();
    let _ = crate::snapshot_service::ensure();
    Ok(
        json!({"session":session_catalog::group(&config),"project":config.workspace,"projects":projects,"source_processes_preserved":true,"reference":current}),
    )
}
