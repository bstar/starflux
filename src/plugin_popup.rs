//! Transient plugin terminals: one reservation per saved session, owned by a
//! controller incarnation. No project pane, agent, layout or snapshot membership.
use crate::{
    automation::fault,
    frontend_commands::Target,
    plugin_store::{Store, read},
    plugins,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Dimension {
    Cells(u16),
    Percent(String),
}
impl Dimension {
    pub fn validate(&self) -> Result<()> {
        if let Self::Percent(s) = self {
            ensure!(
                s.strip_suffix('%').is_some_and(|v| !v.is_empty()
                    && v.bytes().all(|b| b.is_ascii_digit())
                    && v.parse::<u16>().is_ok_and(|n| (1..=100).contains(&n))),
                "Popup percentages must be 1%..100%"
            );
        }
        Ok(())
    }
    fn resolve(&self, max: u16) -> u16 {
        match self {
            Self::Cells(n) => *n,
            Self::Percent(s) => {
                ((u32::from(max) * u32::from(s.trim_end_matches('%').parse::<u16>().unwrap_or(50)))
                    / 100) as u16
            }
        }
        .min(max)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub width: Option<Dimension>,
    pub height: Option<Dimension>,
}
impl Options {
    pub fn validate(&self) -> Result<()> {
        if let Some(d) = &self.width {
            d.validate()?;
        }
        if let Some(d) = &self.height {
            d.validate()?;
        }
        Ok(())
    }
    pub fn bounds(
        &self,
        area: starkit::terminal_graphics::protocol::Rect,
    ) -> Result<(
        starkit::terminal_graphics::protocol::Rect,
        starkit::terminal_graphics::protocol::Rect,
    )> {
        self.validate()?;
        ensure!(
            area.width >= 4 && area.height >= 4,
            "The viewport is too small for a popup terminal"
        );
        let w = self
            .width
            .as_ref()
            .map_or(area.width / 2, |d| d.resolve(area.width))
            .clamp(20.min(area.width), area.width.min(514));
        let h = self
            .height
            .as_ref()
            .map_or(area.height / 2, |d| d.resolve(area.height))
            .clamp(8.min(area.height), area.height.min(194));
        let outer = starkit::terminal_graphics::protocol::Rect {
            x: area.x + (area.width - w) / 2,
            y: area.y + (area.height - h) / 2,
            width: w,
            height: h,
        };
        let inner = starkit::terminal_graphics::protocol::Rect {
            x: outer.x + 1,
            y: outer.y + 1,
            width: w - 2,
            height: h - 2,
        };
        Ok((outer, inner))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: u32,
    pub id: String,
    pub plugin: String,
    pub entrypoint: crate::plugin_panes::Entrypoint,
    pub source: PathBuf,
    pub digest: String,
    pub activation: String,
    pub project: plugins::Project,
    pub focused_pane: usize,
    pub focused_epoch: Option<String>,
    pub owner: Target,
    pub runtime_root: PathBuf,
    pub options: Options,
    pub status: String,
    pub pid: Option<u32>,
    pub start_ticks: Option<u64>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}
pub struct Reservation {
    pub record: Record,
    store: Store,
    _lease: fs::File,
}
fn store() -> Result<Store> {
    plugins::store()?.child("popups")
}
fn filename(id: &str) -> Result<String> {
    Ok(format!(
        "popup-{}.json",
        uuid::Uuid::parse_str(id)?.simple()
    ))
}
fn save(store: &Store, r: &Record) -> Result<()> {
    let bytes = serde_json::to_vec(r)?;
    ensure!(bytes.len() <= 65536, "Popup metadata exceeds 64 KiB");
    store.write(&filename(&r.id)?, &bytes)
}
pub fn get(id: &str) -> Result<Record> {
    let s = store()?;
    let r: Record = read(&s.path.join(filename(id)?), 65536)?;
    ensure!(r.version == 1 && r.id == id, "Invalid popup record");
    Ok(r)
}
fn owner_alive(r: &Record) -> bool {
    fs::metadata(&r.runtime_root).is_ok_and(|m| [m.dev(), m.ino()] == r.owner.namespace)
        && crate::agent_state::boot_id().is_some_and(|b| b.to_string() == r.owner.boot_id)
        && crate::agent_state::process_start(r.owner.pid) == Some(r.owner.start_ticks)
}
fn process_alive(r: &Record) -> bool {
    crate::agent_state::boot_id().is_some_and(|b| b.to_string() == r.owner.boot_id)
        && r.pid.is_some_and(|pid| {
            crate::agent_state::process_start(pid) == r.start_ticks
                && fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
                    s.rsplit_once(')').is_some_and(|(_, s)| {
                        !matches!(s.split_whitespace().next(), Some("Z" | "X"))
                    })
                })
        })
}
pub fn package_busy(plugin: &str) -> Result<bool> {
    let s = store()?;
    for entry in fs::read_dir(&s.path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("popup-") || !name.ends_with(".json") {
            continue;
        }
        let r: Record = read(&entry.path(), 65536)?;
        if r.plugin == plugin && (process_alive(&r) || (r.status == "starting" && owner_alive(&r)))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn status(id: &str) -> Result<Value> {
    let r = get(id)?;
    let state = if process_alive(&r) {
        "running"
    } else if r.error.is_some() {
        "failed"
    } else if r.status == "starting" && owner_alive(&r) {
        "starting"
    } else {
        "closed"
    };
    Ok(
        json!({"id":r.id,"kind":"popup","plugin":r.plugin,"entrypoint":r.entrypoint.id,"owner":r.owner,"alive":process_alive(&r),"status":state,"exit_code":r.exit_code,"error":r.error}),
    )
}
impl Reservation {
    pub fn create(
        owner: Target,
        config: &crate::runtime::Config,
        focused_pane: usize,
        plugin: &str,
        entrypoint: &str,
        digest: &str,
        options: Options,
    ) -> Result<Self> {
        options.validate()?;
        let registry_path = plugins::registry_lock_path()?;
        let _configuration = crate::project_sync::Transaction::try_acquire(registry_path)?
            .ok_or_else(|| {
                fault(
                    "plugin_busy",
                    "Plugin configuration is changing; retry opening the popup",
                )
            })?;
        let registry = plugins::registry()?;
        let entry = registry
            .plugins
            .get(plugin)
            .context("Plugin is not linked")?;
        let (manifest, current) = plugins::package_metadata(entry)?;
        ensure!(
            entry.enabled
                && entry.approved.as_deref() == Some(&current)
                && digest == current
                && manifest.id == plugin,
            "Plugin is disabled or changed; review it again"
        );
        let declared = manifest
            .panes
            .into_iter()
            .find(|p| p.id == entrypoint)
            .context("Popup entrypoint is unavailable")?;
        let options = Options {
            width: options.width.or(declared.width.clone()),
            height: options.height.or(declared.height.clone()),
        };
        options.validate()?;
        let s = store()?;
        let session = crate::session_catalog::group(config);
        let key = format!("session-{:x}.lock", Sha256::digest(session.as_bytes()));
        let lease = s.lock(&key, false)?.ok_or_else(|| {
            fault(
                "popup_busy",
                "This session already has a popup in another window",
            )
        })?;
        let state = crate::project_state::ProjectState::load(config)?;
        let r = Record {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            plugin: plugin.into(),
            entrypoint: declared,
            source: entry.path.clone(),
            digest: current,
            activation: entry.activation.clone(),
            project: plugins::project(config),
            focused_pane,
            focused_epoch: state.terminal_epochs.get(&focused_pane).cloned(),
            runtime_root: crate::runtime::root()?,
            owner,
            options,
            status: "starting".into(),
            pid: None,
            start_ticks: None,
            exit_code: None,
            error: None,
        };
        save(&s, &r)?;
        Ok(Self {
            record: r,
            store: s,
            _lease: lease,
        })
    }
    pub fn argv(&self) -> Result<Vec<String>> {
        Ok(vec![
            crate::plugin_panes::launcher()?
                .to_string_lossy()
                .into_owned(),
            "--plugin-popup-worker".into(),
            self.store.path.to_string_lossy().into_owned(),
            self.record.id.clone(),
        ])
    }
    pub fn current(&self) -> bool {
        self.store.current() && owner_alive(&self.record)
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut r) = get(&self.record.id) {
            r.status = "closed".into();
            let _ = save(&self.store, &r);
        }
    }
}
unsafe extern "C" fn stop_group(_: libc::c_int) {
    unsafe {
        libc::kill(0, libc::SIGKILL);
        libc::_exit(128);
    }
}
extern "C" fn ignore_signal(_: libc::c_int) {}
fn run_worker(store: &Store, id: &str) -> Result<()> {
    let _registry = crate::project_sync::Transaction::acquire(plugins::registry_lock_path()?)?;
    let _claim = store.lock(
        &format!("worker-{}.lock", uuid::Uuid::parse_str(id)?.simple()),
        true,
    )?;
    let mut r = get(id)?;
    ensure!(
        r.status == "starting"
            && owner_alive(&r)
            && unsafe { libc::getppid() } as u32 == r.owner.pid,
        "Popup owner ended or launch was already consumed"
    );
    let registry = plugins::registry()?;
    let entry = registry
        .plugins
        .get(&r.plugin)
        .context("Plugin was unlinked")?;
    let (manifest, digest) = plugins::package_for(entry)?;
    ensure!(
        entry.enabled
            && entry.activation == r.activation
            && entry.path == r.source
            && digest == r.digest
            && entry.approved.as_deref() == Some(&digest)
            && manifest.panes.iter().any(|p| p.id == r.entrypoint.id),
        "Plugin changed before popup launch"
    );
    let declared = manifest
        .panes
        .iter()
        .find(|p| p.id == r.entrypoint.id)
        .context("Popup entrypoint disappeared")?;
    ensure!(
        declared.argv == r.entrypoint.argv
            && declared.cwd == r.entrypoint.cwd
            && declared.title == r.entrypoint.title
            && manifest.id == r.plugin,
        "Popup command metadata no longer matches the approved manifest"
    );
    r.options.validate()?;
    let config = crate::saved(&r.project.id)?;
    ensure!(
        plugins::project(&config) == r.project,
        "Popup project context changed"
    );
    let _worktree = crate::worktrees::ensure_allowed(&config)?;
    let cwd = if r.entrypoint.cwd == "project" {
        ensure!(
            !config.remote(r.focused_pane),
            "Project-directory popups need a project on this controller"
        );
        PathBuf::from(config.directory.context("Project has no directory")?)
    } else {
        r.source.clone()
    };
    let (config_dir, state_dir) = plugins::directories(&plugins::store()?, &r.plugin)?;
    let binary = crate::plugin_panes::launcher()?;
    let context = json!({"version":1,"plugin_id":r.plugin,"entrypoint_id":r.entrypoint.id,"plugin_popup_id":r.id,"project":r.project,"focused_pane":{"pane":r.focused_pane+1,"epoch":r.focused_epoch},"config_dir":config_dir,"state_dir":state_dir,"bin_path":binary});
    let program = r.entrypoint.argv[0].strip_prefix("package:").map_or_else(
        || PathBuf::from(&r.entrypoint.argv[0]),
        |p| r.source.join(p),
    );
    let mut command = Command::new(program);
    command
        .args(&r.entrypoint.argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env_remove("STARFLUX_PANE")
        .env_remove("STARFLUX_PLUGIN_PANE_ID")
        .env_remove("STARFLUX_PLUGIN_ACTION_ID")
        .env_remove("STARFLUX_PLUGIN_JOB_ID")
        .env("STARFLUX_PLUGIN_ID", &r.plugin)
        .env("STARFLUX_PLUGIN_ENTRYPOINT_ID", &r.entrypoint.id)
        .env("STARFLUX_PLUGIN_POPUP_ID", &r.id)
        .env("STARFLUX_PROJECT", &r.project.id)
        .env("STARFLUX_PLUGIN_CONFIG_DIR", config_dir)
        .env("STARFLUX_PLUGIN_STATE_DIR", state_dir)
        .env("STARFLUX_BIN_PATH", binary)
        .env(
            "STARFLUX_PLUGIN_CONTEXT_JSON",
            serde_json::to_string(&context)?,
        );
    r.pid = Some(std::process::id());
    r.start_ticks = crate::agent_state::process_start(std::process::id());
    r.status = "running".into();
    save(store, &r)?;
    let mut child = command.spawn()?;
    drop(_registry);
    let status = child.wait()?;
    r.exit_code = status.code();
    if !status.success() {
        r.error = Some(format!("Popup command exited with {status}"));
    }
    r.status = "exited".into();
    save(store, &r)?;
    Ok(())
}
/// The PTY creates a dedicated session/group. Its supervisor remains the group
/// leader until it kills that group, so cleanup never uses a recycled numeric ID.
pub fn worker(path: &str, id: &str) -> Result<()> {
    ensure!(
        unsafe { libc::getpid() == libc::getpgrp() && libc::getsid(0) == libc::getpid() },
        "Popup launcher must own a dedicated PTY session"
    );
    let s = Store::open(PathBuf::from(path))?;
    ensure!(
        s.path == plugins::data_path().join("popups"),
        "Popup namespace changed"
    );
    let r = get(id)?;
    ensure!(
        owner_alive(&r) && unsafe { libc::getppid() } as u32 == r.owner.pid,
        "Popup owner changed"
    );
    unsafe {
        libc::signal(libc::SIGTERM, stop_group as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, stop_group as *const () as libc::sighandler_t);
        for sig in [libc::SIGINT, libc::SIGQUIT, libc::SIGTSTP] {
            libc::signal(sig, ignore_signal as *const () as libc::sighandler_t);
        }
        ensure!(
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == 0,
            "Cannot bind popup lifetime to its owner"
        );
        if libc::getppid() as u32 != r.owner.pid {
            stop_group(0);
        }
    }
    let outcome = run_worker(&s, id);
    if let Err(error) = outcome {
        eprintln!("Popup: {error:#}");
        if let Ok(mut r) = get(id) {
            r.status = "failed".into();
            r.error = Some(format!("{error:#}"));
            let _ = save(&s, &r);
        }
    }
    unsafe {
        stop_group(0);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use starkit::terminal_graphics::protocol::Rect;
    #[test]
    fn dimensions_are_bounded_and_centered_on_narrow_viewports() {
        for (w, h) in [
            (4, 4),
            (12, 6),
            (40, 12),
            (100, 35),
            (512, 192),
            (2048, 768),
        ] {
            let area = Rect {
                x: 2,
                y: 3,
                width: w,
                height: h,
            };
            for size in [
                None,
                Some(Dimension::Cells(0)),
                Some(Dimension::Cells(u16::MAX)),
                Some(Dimension::Percent("80%".into())),
            ] {
                let options = Options {
                    width: size.clone(),
                    height: size,
                };
                let (outer, inner) = options.bounds(area).unwrap();
                assert!(
                    outer.x >= area.x
                        && outer.y >= area.y
                        && outer.x + outer.width <= area.x + w
                        && outer.y + outer.height <= area.y + h
                );
                assert!(inner.width >= 2 && inner.height >= 1);
                assert!(inner.width <= 512 && inner.height <= 192);
            }
        }
        for bad in ["0%", "101%", "-1%", "50.5%", "abc", "50%extra"] {
            assert!(Dimension::Percent(bad.into()).validate().is_err());
        }
    }
}
