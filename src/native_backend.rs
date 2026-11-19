//! Normal workspace adapter for retained native sources. Existing external
//! owners keep their backend; every native operation carries a durable epoch.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Pane {
    pub namespace: PathBuf,
    pub binding: crate::native_protocol::Binding,
    #[serde(default)]
    pub previous: Option<crate::native_protocol::Binding>,
}
impl Pane {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.namespace.is_absolute(),
            "Native source namespace must be absolute"
        );
        #[cfg(target_os = "linux")]
        {
            self.binding.validate()?;
            if let Some(previous) = &self.previous {
                previous.validate()?;
                ensure!(
                    previous.project == self.binding.project && previous.pane == self.binding.pane,
                    "Previous native reference belongs to another slot"
                );
            }
        }
        #[cfg(not(target_os = "linux"))]
        ensure!(
            self.binding.version == 1 && self.binding.pane < 32,
            "Unsupported native source reference"
        );
        Ok(())
    }
}
pub fn reference(config: &crate::runtime::Config, pane: usize) -> Result<Option<Pane>> {
    let path = crate::project_state::data_path().join(format!("{}.model.json", config.workspace));
    let state: Value = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let reference = match state.get("native_sources") {
        None => None,
        Some(Value::Object(sources)) => sources
            .get(&pane.to_string())
            .map(|value| serde_json::from_value::<Pane>(value.clone()))
            .transpose()?,
        Some(_) => anyhow::bail!("Invalid native source ownership map"),
    };
    if let Some(r) = &reference {
        ensure!(
            !config.externally_owned(pane)
                && config.adopted_terminal(pane).is_none()
                && state["terminal_plugins"][pane.to_string()].is_null(),
            "Native pane has conflicting backend ownership"
        );
        r.validate()?;
        ensure!(
            state["terminal_epochs"][pane.to_string()] == r.binding.terminal_epoch,
            "Native pane epoch changed"
        );
        ensure!(
            r.binding.project == config.workspace && r.binding.pane == pane,
            "Native reference belongs to another project or pane"
        );
    }
    Ok(reference)
}
pub fn command(config: &crate::runtime::Config, pane: usize, role: &str) -> Command {
    if config.remote(pane) {
        let mut c = Command::new("ssh");
        if let Some(path) = &config.ssh_config {
            c.arg("-F").arg(path);
        }
        c.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "--",
            config.host.as_ref().unwrap(),
        ]);
        c.arg(shell_words::join([config.desktop_helper.as_str(), role]));
        c
    } else {
        let mut c = Command::new("/proc/self/exe");
        c.arg(role);
        c
    }
}
pub fn rpc(config: &crate::runtime::Config, pane: usize, request: Value) -> Result<Value> {
    let bytes = serde_json::to_vec(&request)?;
    ensure!(
        bytes.len() <= 2 * 1024 * 1024,
        "Native request exceeds limit"
    );
    use std::{
        io::{Seek, SeekFrom},
        time::{Duration, Instant},
    };
    let mut output = tempfile::tempfile()?;
    let mut errors = tempfile::tempfile()?;
    let mut child = command(config, pane, "--native-pane-rpc")
        .stdin(Stdio::piped())
        .stdout(output.try_clone()?)
        .stderr(errors.try_clone()?)
        .spawn()?;
    let input = child
        .stdin
        .take()
        .context("Native control input unavailable")?;
    let send = std::thread::spawn(move || {
        let mut input = input;
        input.write_all(&bytes)
    });
    let began = Instant::now();
    let status = loop {
        if output.metadata()?.len() > 4 * 1024 * 1024
            || errors.metadata()?.len() > 65536
            || began.elapsed() > Duration::from_secs(20)
        {
            let _ = child.kill();
            let _ = child.wait();
            let _ = send.join();
            anyhow::bail!(
                "Native control exceeded its output/time limit; delivery may be unknown and was not replayed"
            );
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    send.join()
        .map_err(|_| anyhow::anyhow!("Native request writer failed"))??;
    ensure!(
        output.metadata()?.len() <= 4 * 1024 * 1024 && errors.metadata()?.len() <= 65536,
        "Native control output exceeds limit"
    );
    output.seek(SeekFrom::Start(0))?;
    errors.seek(SeekFrom::Start(0))?;
    let mut stdout = Vec::new();
    let mut stderr = String::new();
    output.take(4 * 1024 * 1024 + 1).read_to_end(&mut stdout)?;
    errors.take(65537).read_to_string(&mut stderr)?;
    ensure!(status.success(), "Native control: {}", stderr.trim());
    Ok(serde_json::from_slice(&stdout)?)
}
#[cfg(target_os = "linux")]
fn namespace() -> Result<PathBuf> {
    Ok(crate::project_state::data_root()?
        .parent()
        .context("Native data namespace unavailable")?
        .to_owned())
}
#[cfg(target_os = "linux")]
pub fn run_rpc() -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 2 * 1024 * 1024,
        "Native request exceeds limit"
    );
    let request: Value = serde_json::from_slice(&bytes)?;
    let result = dispatch(request)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
#[cfg(target_os = "linux")]
fn same_launch_intent(
    a: &crate::native_protocol::Binding,
    b: &crate::native_protocol::Binding,
) -> bool {
    let mut a = a.clone();
    let mut b = b.clone();
    a.source = None;
    b.source = None;
    a == b
}
#[cfg(target_os = "linux")]
fn check_pending_slot(target: &Pane) -> Result<()> {
    let expected = &target.binding;
    let saved = crate::source_broker::service::read_slot(
        &target.namespace,
        &expected.project,
        expected.pane,
    )?
    .context("Native pending slot disappeared")?;
    ensure!(
        if expected.source.is_some() {
            saved == *expected
        } else {
            same_launch_intent(&saved, expected)
        },
        "Native pending slot changed"
    );
    Ok(())
}
#[cfg(target_os = "linux")]
fn dispatch(request: Value) -> Result<Value> {
    use crate::source_broker::{self, service};
    match request["op"].as_str().context("Missing native operation")? {
        "launch-defaults" => {
            let home = std::env::home_dir().context("Remote home unavailable")?;
            let value = request["directory"].as_str().unwrap_or("~");
            let directory = if value == "~" {
                home.clone()
            } else if let Some(path) = value.strip_prefix("~/") {
                home.join(path)
            } else {
                PathBuf::from(value)
            };
            ensure!(
                directory.is_absolute(),
                "Native project directory must be absolute"
            );
            Ok(
                json!({"directory":directory,"shell":std::env::var("SHELL").unwrap_or_else(|_|"/bin/bash".into())}),
            )
        }
        "discover" => {
            let data = namespace()?;
            let handle = service::ensure(&data, Path::new("/proc/self/exe"))?;
            let hello = handle.connect()?.call(source_broker::Action::Hello)?;
            ensure!(
                hello["capabilities"].as_array().is_some_and(|c| [
                    "screen-history-v1",
                    "guarded-input-v1"
                ]
                .iter()
                .all(|required| c.iter().any(|v| v == required))),
                "Existing native owner cannot serve workspace adapters; kept running"
            );
            let previous = if let (Some(project), Some(pane)) =
                (request["project"].as_str(), request["pane"].as_u64())
            {
                service::read_slot(&data, project, usize::try_from(pane)?)?
            } else {
                None
            };
            Ok(json!({"namespace":data,"owner":handle.owner,"previous":previous}))
        }
        "open" => {
            let target: Pane = serde_json::from_value(request["target"].clone())?;
            target.validate()?;
            let launch: source_broker::Launch = serde_json::from_value(request["launch"].clone())?;
            let handle = service::existing(&target.namespace, &target.binding.owner)?;
            ensure!(
                launch.id == target.binding.source_id
                    && launch.epoch == target.binding.source_epoch
                    && launch.digest()? == target.binding.launch_digest,
                "Native launch does not match recorded intent"
            );
            let record = handle.open_slot(
                &target.binding.project,
                target.binding.pane,
                &target.binding.terminal_epoch,
                &launch,
                target.previous.as_ref(),
            )?;
            Ok(serde_json::to_value(Pane {
                namespace: target.namespace,
                binding: record,
                previous: None,
            })?)
        }
        "resolve-pending" => {
            let target: Pane = serde_json::from_value(request["target"].clone())?;
            target.validate()?;
            let expected = &target.binding;
            ensure!(expected.source.is_none(), "Native source is already known");
            check_pending_slot(&target)?;
            if source_broker::retirement::owner_dead(&expected.owner)? {
                source_broker::retirement::verify(&target.namespace, expected)?;
                return Ok(json!({"sealed":true,"owner_lost":true}));
            }
            // Preserve discovery-only recovery with older owners: a lost Open
            // reply can already have a known retained source and needs no fence.
            if let Ok(record) = service::reattach_slot(
                &target.namespace,
                &expected.project,
                expected.pane,
                &expected.terminal_epoch,
            ) {
                ensure!(
                    same_launch_intent(&record, expected),
                    "Native pending intent changed during discovery"
                );
                return Ok(
                    json!({"sealed":false,"target":Pane{namespace:target.namespace,binding:record,previous:None}}),
                );
            }
            let handle = service::existing(&target.namespace, &expected.owner)?;
            let mut connection = handle.connect()?;
            let hello = connection.call(source_broker::Action::Hello)?;
            ensure!(
                hello["capabilities"]
                    .as_array()
                    .is_some_and(|caps| caps.iter().any(|cap| cap == "launch-fence-v1")),
                "Existing owner cannot cancel an unstarted launch safely"
            );
            let resolved = connection.call(source_broker::Action::ResolveLaunch {
                id: expected.source_id.clone(),
                epoch: expected.source_epoch.clone(),
                digest: expected.launch_digest.clone(),
                seal: true,
            })?;
            ensure!(
                service::read_slot(&target.namespace, &expected.project, expected.pane)?.as_ref()
                    == Some(expected),
                "Native pending slot changed during discovery"
            );
            if resolved["status"] == "sealed" {
                return Ok(json!({"sealed":true}));
            }
            ensure!(
                resolved["status"] == "ready",
                "Native launch is still opening or has an uncertain outcome; no replacement launched"
            );
            let record = service::reattach_slot(
                &target.namespace,
                &expected.project,
                expected.pane,
                &expected.terminal_epoch,
            )?;
            ensure!(
                same_launch_intent(&record, expected),
                "Native pending intent changed during discovery"
            );
            Ok(
                json!({"sealed":false,"target":Pane{namespace:target.namespace,binding:record,previous:None}}),
            )
        }
        "replacement-owner" => {
            let target: Pane = serde_json::from_value(request["target"].clone())?;
            target.validate()?;
            check_pending_slot(&target)?;
            source_broker::retirement::verify(&target.namespace, &target.binding)?;
            let previous = service::read_slot(
                &target.namespace,
                &target.binding.project,
                target.binding.pane,
            )?
            .context("Native slot disappeared")?;
            ensure!(
                same_launch_intent(&previous, &target.binding),
                "Native slot intent changed"
            );
            source_broker::retirement::verify(&target.namespace, &previous)?;
            let handle = service::ensure(&target.namespace, Path::new("/proc/self/exe"))?;
            let hello = handle.connect()?.call(source_broker::Action::Hello)?;
            ensure!(
                hello["capabilities"].as_array().is_some_and(|caps| [
                    "retirement-tree-v1",
                    "retirement-record-v1"
                ]
                .iter()
                .all(|required| caps.iter().any(|cap| cap == required))),
                "Current owner cannot provide certified retirement; its existing sources were retained"
            );
            ensure!(
                service::read_slot(
                    &target.namespace,
                    &target.binding.project,
                    target.binding.pane
                )?
                .as_ref()
                    == Some(&previous),
                "Native slot changed during owner replacement"
            );
            Ok(json!({"owner":handle.owner,"previous":previous}))
        }
        "retire" => {
            use std::time::{Duration, Instant};
            ensure!(
                request["exited_only"].is_null() || request["exited_only"].is_boolean(),
                "Invalid native retirement mode"
            );
            let target: Pane = serde_json::from_value(request["target"].clone())?;
            target.validate()?;
            let expected = &target.binding;
            ensure!(
                service::read_slot(&target.namespace, &expected.project, expected.pane)?.as_ref()
                    == Some(expected),
                "Native slot changed before retirement"
            );
            if source_broker::retirement::owner_dead(&expected.owner)? {
                source_broker::retirement::verify(&target.namespace, expected)?;
                if request["exited_only"] == true {
                    return Ok(json!({"retired":false}));
                }
                return Ok(json!({"retired":true,"owner_lost":true}));
            }
            let source = expected
                .source
                .as_ref()
                .context("Native launch outcome is unknown; source was not replaced or replayed")?;
            let handle = service::existing(&target.namespace, &expected.owner)?;
            let mut connection = handle.connect()?;
            let hello = connection.call(source_broker::Action::Hello)?;
            ensure!(
                hello["capabilities"].as_array().is_some_and(|caps| [
                    "retirement-eof-v1",
                    "retirement-tree-v1"
                ]
                .iter()
                .all(|required| caps.iter().any(|cap| cap == required))),
                "Existing native owner lacks safe retirement support; its processes were left running"
            );
            let started = Instant::now();
            let mut close_sent = false;
            loop {
                let list = connection.call(source_broker::Action::List)?;
                let entries = list.as_array().context("Invalid native source listing")?;
                let info = entries
                    .iter()
                    .find(|info| info["source"]["id"] == source.id);
                if let Some(info) = info {
                    ensure!(
                        serde_json::from_value::<crate::native_protocol::Reference>(
                            info["source"].clone()
                        )? == *source,
                        "Native source identity changed before retirement"
                    );
                    ensure!(
                        info["exited"] != true || info["retirement_verified"] == true,
                        "Native supervisor was lost; descendants are unverified and the source was not replaced"
                    );
                    if request["exited_only"] == true
                        && (info["exited"] != true
                            || info["eof"] != true
                            || info["buffered_output"] != 0)
                    {
                        return Ok(json!({"retired":false}));
                    }
                    if !close_sent
                        && info["closing"] != true
                        && let Err(error) = connection.call(source_broker::Action::Close {
                            source: source.clone(),
                        })
                    {
                        // Natural exit or another already-authorized close
                        // can remove the source between List and Close.
                        // Only a fresh successful listing can resolve that
                        // race; a transport error never grants replacement.
                        let current = connection.call(source_broker::Action::List)?;
                        if current.as_array().is_none_or(|entries| {
                            entries.iter().any(|info| info["source"]["id"] == source.id)
                        }) {
                            return Err(error);
                        }
                    }
                    close_sent = true;
                } else {
                    ensure!(
                        crate::agent_state::boot_id()
                            .map(|id| id.to_string())
                            .as_deref()
                            != Some(source.boot.as_str())
                            || crate::agent_state::process_start(source.pid)
                                != Some(source.start_ticks),
                        "Native source disappeared but its process is still alive; not replaced"
                    );
                    ensure!(
                        service::read_slot(&target.namespace, &expected.project, expected.pane)?
                            .as_ref()
                            == Some(expected),
                        "Native slot changed during retirement"
                    );
                    return Ok(json!({"retired":true}));
                }
                ensure!(
                    started.elapsed() < Duration::from_secs(5),
                    "Native source is still closing; no replacement launched"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        "transfer" => {
            let target: Pane = serde_json::from_value(request["target"].clone())?;
            target.validate()?;
            let expected = &target.binding;
            let project = request["project"]
                .as_str()
                .context("Missing destination project")?;
            let pane = request["pane"]
                .as_u64()
                .and_then(|pane| usize::try_from(pane).ok())
                .context("Missing destination pane")?;
            ensure!(
                service::read_slot(&target.namespace, &expected.project, expected.pane)?.as_ref()
                    == Some(expected),
                "Native slot changed before the move"
            );
            ensure!(
                !source_broker::retirement::owner_dead(&expected.owner)?,
                "Native owner is gone; recover the terminal before moving it"
            );
            let source = expected
                .source
                .as_ref()
                .context("Native launch outcome is unknown; the terminal cannot move")?;
            let handle = service::existing(&target.namespace, &expected.owner)?;
            let list = handle.connect()?.call(source_broker::Action::List)?;
            let live = list.as_array().is_some_and(|entries| {
                entries.iter().any(|info| {
                    info["source"]["id"] == source.id
                        && info["exited"] != true
                        && info["closing"] != true
                })
            });
            ensure!(live, "Native terminal has exited; it was not moved");
            let moved = service::transfer_slot(&target.namespace, expected, project, pane)?;
            Ok(serde_json::to_value(Pane {
                namespace: target.namespace,
                binding: moved,
                previous: None,
            })?)
        }
        "reattach" => {
            let target: Pane = serde_json::from_value(request["target"].clone())?;
            target.validate()?;
            let record = service::reattach_slot(
                &target.namespace,
                &target.binding.project,
                target.binding.pane,
                &target.binding.terminal_epoch,
            )?;
            ensure!(
                record.owner == target.binding.owner
                    && record.source_id == target.binding.source_id
                    && record.source_epoch == target.binding.source_epoch
                    && record.launch_digest == target.binding.launch_digest,
                "Native reference changed"
            );
            Ok(serde_json::to_value(Pane {
                namespace: target.namespace,
                binding: record,
                previous: None,
            })?)
        }
        "call" => {
            let target: Pane = serde_json::from_value(request["target"].clone())?;
            target.validate()?;
            if request["action"] == "inspect"
                && source_broker::retirement::owner_dead(&target.binding.owner)?
            {
                check_pending_slot(&target)?;
                let proof = source_broker::retirement::verify(&target.namespace, &target.binding);
                let verified = proof.is_ok();
                return Ok(
                    json!({"backend":"native","alive":false,"state":"disconnected","owner_lost":true,
                    "native_owner_recovery_available":verified,"native_pending_recovery_available":verified&&target.binding.source.is_none(),
                    "native_retirement_available":verified&&target.binding.source.is_some(),
                    "error":proof.err().map_or_else(||"Owner ended; supervisor tree retirement verified; recover explicitly".into(),|error|format!("{error:#}"))}),
                );
            }
            if target.binding.source.is_none() && request["action"] == "inspect" {
                let expected = &target.binding;
                check_pending_slot(&target)?;
                let handle = service::existing(&target.namespace, &expected.owner)?;
                let mut connection = handle.connect()?;
                let hello = connection.call(source_broker::Action::Hello)?;
                let supported = hello["capabilities"]
                    .as_array()
                    .is_some_and(|caps| caps.iter().any(|cap| cap == "launch-fence-v1"));
                if supported {
                    let status = connection.call(source_broker::Action::ResolveLaunch {
                        id: expected.source_id.clone(),
                        epoch: expected.source_epoch.clone(),
                        digest: expected.launch_digest.clone(),
                        seal: false,
                    })?;
                    if status["status"] != "ready" {
                        return Ok(
                            json!({"backend":"native","alive":false,"state":"unknown","native_retirement_available":false,
                            "native_pending_recovery_available":status["status"]=="not_started"||status["status"]=="sealed","launch_status":status["status"],"error":"Native launch is pending; explicit recovery never replays the saved command"}),
                        );
                    }
                }
            }
            let record = service::reattach_slot(
                &target.namespace,
                &target.binding.project,
                target.binding.pane,
                &target.binding.terminal_epoch,
            )?;
            ensure!(
                record.owner == target.binding.owner
                    && record.source_id == target.binding.source_id
                    && record.source_epoch == target.binding.source_epoch
                    && record.launch_digest == target.binding.launch_digest,
                "Native pane reference changed"
            );
            let handle = service::existing(&target.namespace, &record.owner)?;
            let source = record
                .source
                .context("Native launch outcome is unavailable")?;
            let action = request["action"]
                .as_str()
                .context("Missing native action")?;
            let mut connection = handle.connect()?;
            match action {
                "inspect" => {
                    let hello = connection.call(source_broker::Action::Hello)?;
                    let mut info = connection.call(source_broker::Action::Inspect {
                        source: source.clone(),
                    })?;
                    info["backend"] = json!("native");
                    info["native_retirement_available"] = json!(
                        hello["capabilities"].as_array().is_some_and(|caps| {
                            ["retirement-eof-v1", "retirement-tree-v1"]
                                .iter()
                                .all(|required| caps.iter().any(|cap| cap == required))
                        }) && (info["exited"] != true || info["retirement_verified"] == true)
                    );
                    info["pid"] = json!(source.pid);
                    info["pid_start_ticks"] = json!(source.start_ticks);
                    info["boot_id"] = json!(source.boot);
                    info["state"] = json!(if info["alive"] == true {
                        "unknown"
                    } else {
                        "stopped"
                    });
                    if info["alive"] == true {
                        let observed = source_broker::process::inspect(&source)?;
                        info["command"] = json!(observed.command);
                        info["cwd"] = json!(observed.cwd);
                        info["agent"] = json!(observed.agent.as_ref().map(|a| &a.family));
                        info["agent_pid"] = json!(observed.agent.as_ref().map(|a| a.pid));
                        info["agent_start_ticks"] =
                            json!(observed.agent.as_ref().map(|a| a.start_ticks));
                    }
                    info["output_fingerprint"] =
                        json!(format!("native:{}:{}", source.id, info["revision"]));
                    if info["agent"].is_string() || request["include_screen"] == true {
                        let screen = connection
                            .call(source_broker::Action::Screen { source, history: 0 })?;
                        if info["agent"].is_string() {
                            info["state"] = json!(crate::activity::screen_state(
                                screen["text"].as_str().unwrap_or_default()
                            ));
                        }
                        info["saved_screen"] = screen["screen"].clone();
                    }
                    Ok(info)
                }
                "screen" => connection.call(source_broker::Action::Screen { source, history: 0 }),
                "history" => connection.call(source_broker::Action::Screen {
                    source,
                    history: 1000,
                }),
                "guarded-input" => connection.call(source_broker::Action::GuardedInput {
                    source,
                    data: request["data"]
                        .as_str()
                        .context("Missing native input")?
                        .into(),
                    agent: serde_json::from_value(request["agent"].clone())?,
                }),
                "input" => connection.call(source_broker::Action::Input {
                    source,
                    data: request["data"]
                        .as_str()
                        .context("Missing native input")?
                        .into(),
                }),
                "resize" => connection.call(source_broker::Action::Resize {
                    source,
                    size: serde_json::from_value(request["size"].clone())?,
                }),
                "stop" => connection.call(source_broker::Action::Close { source }),
                _ => anyhow::bail!("Native action is not implemented: {action}"),
            }
        }
        _ => anyhow::bail!("Unknown native operation"),
    }
}
#[cfg(not(target_os = "linux"))]
pub fn run_rpc() -> Result<()> {
    anyhow::bail!("The native source owner requires Linux")
}

#[cfg(target_os = "linux")]
fn launch_spec(
    config: &crate::runtime::Config,
    pane: usize,
    agent: Option<&str>,
) -> Result<crate::source_broker::Launch> {
    let theme = crate::settings::Settings::appearance()?;
    let rgb = |text: &str| -> [u8; 3] {
        std::array::from_fn(|i| {
            u8::from_str_radix(text.get(1 + i * 2..3 + i * 2).unwrap_or("00"), 16).unwrap_or(0)
        })
    };
    let argv = match agent {
        Some(command) if !config.remote(pane) => crate::agent_hooks::with_session_hook(
            shell_words::split(command)?,
            &std::env::current_exe()?.to_string_lossy(),
        ),
        Some(command) => shell_words::split(command)?,
        None => vec![
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into()),
            "-l".into(),
        ],
    };
    let directory = if config.remote(pane) {
        PathBuf::from(config.directory.as_deref().unwrap_or("~"))
    } else {
        config
            .directory
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?)
    };
    let mut launch = crate::source_broker::Launch {
        id: uuid::Uuid::new_v4().to_string(),
        epoch: uuid::Uuid::new_v4().to_string(),
        argv,
        directory,
        size: crate::source_broker::Size {
            rows: 24,
            columns: 80,
            cell: [8, 16],
        },
        palette: crate::source_broker::Palette {
            foreground: rgb(&theme.fg),
            background: rgb(&theme.bg),
            ansi: std::array::from_fn(|i| {
                rgb(theme.ansi.get(i).map(String::as_str).unwrap_or(&theme.fg))
            }),
        },
    };
    // Resolve a host's home/default shell on that host, not on the controller.
    if config.remote(pane) {
        let resolved = rpc(
            config,
            pane,
            json!({"op":"launch-defaults","directory":config.directory,"shell":agent.is_none()}),
        )?;
        launch.directory = serde_json::from_value(resolved["directory"].clone())?;
        if agent.is_none() {
            launch.argv = vec![
                resolved["shell"]
                    .as_str()
                    .context("Remote shell unavailable")?
                    .into(),
                "-l".into(),
            ];
        }
    }
    Ok(launch)
}

#[cfg(target_os = "linux")]
fn pending_command(
    config: &crate::runtime::Config,
    state: &crate::project_state::ProjectState,
    pane: usize,
) -> Option<String> {
    if !state.terminal_launch_pending.contains(&pane) {
        return None;
    }
    state.terminal_commands.get(&pane).cloned().or_else(|| {
        let agents = state.agent_ids();
        let agent = agents
            .iter()
            .find(|id| config.canonical_pane(**id) == pane)?;
        let command = config.agent.as_ref()?;
        Some(if agents.first() == Some(agent) {
            command.clone()
        } else {
            crate::runtime::fresh_agent_command(command)
        })
    })
}
#[cfg(target_os = "linux")]
pub fn ensure_source(
    config: &crate::runtime::Config,
    pane: usize,
    _agent: Option<&str>,
) -> Result<bool> {
    use crate::{
        native_protocol::{Binding, Identity},
        project_state::ProjectState,
    };
    if config.externally_owned(pane)
        || config.adopted_terminal(pane).is_some()
        || crate::plugin_panes::owner(config, pane)?.is_some()
    {
        return Ok(false);
    }
    if let Some(target) = reference(config, pane)? {
        let ready: Pane =
            serde_json::from_value(rpc(config, pane, json!({"op":"reattach","target":target}))?)?;
        let _lock = crate::project_sync::transaction(config)?;
        let mut state = ProjectState::load(config)?;
        ensure!(
            state.native_sources.get(&pane) == Some(&target),
            "Native pane changed during reconnect"
        );
        state.native_sources.insert(pane, ready);
        state.save(config)?;
        return Ok(true);
    }
    if !config.native_terminals {
        return Ok(false);
    }
    // A pre-existing tmux session remains on its original backend, including
    // an exited retained screen. New tabs can opt into the native owner.
    let mut probe = config.command(pane, &config.request(pane, "inspect"), false);
    let out = probe.output()?;
    ensure!(
        out.status.success(),
        "Could not verify existing backend; no native source launched"
    );
    let info: Value = serde_json::from_slice(&out.stdout)?;
    if info["state"] != "missing" {
        return Ok(false);
    }
    // Select from shared pending metadata, not this frontend's launch hint.
    // Network/default-shell discovery stays outside the project transaction.
    let snapshot = ProjectState::load(config)?;
    let expected_epoch = snapshot.terminal_epochs.get(&pane).cloned();
    let requested = pending_command(config, &snapshot, pane);
    let wanted = launch_spec(config, pane, requested.as_deref())?;
    let fallback = requested
        .as_ref()
        .map(|_| launch_spec(config, pane, None))
        .transpose()?;
    let discovered = rpc(
        config,
        pane,
        json!({"op":"discover","project":config.workspace,"pane":pane}),
    )?;
    let namespace: PathBuf = serde_json::from_value(discovered["namespace"].clone())?;
    let owner: Identity = serde_json::from_value(discovered["owner"].clone())?;
    let (target, launch) = {
        let _lock = crate::project_sync::transaction(config)?;
        let mut state = ProjectState::load(config)?;
        ensure!(
            state.terminal_ids().contains(&pane)
                && state.terminal_epochs.get(&pane) == expected_epoch.as_ref(),
            "Native terminal changed before launch"
        );
        if let Some(existing) = state.native_sources.get(&pane) {
            (existing.clone(), None)
        } else {
            ensure!(
                pending_command(config, &state, pane) == requested,
                "Native launch command changed during discovery; no source launched"
            );
            let epoch = state
                .terminal_epochs
                .entry(pane)
                .or_insert_with(|| uuid::Uuid::new_v4().to_string())
                .clone();
            let authorized = requested.is_some()
                && crate::cold_start::consume_launch(config, pane, Some(&epoch))?;
            let launch = if authorized {
                wanted
            } else {
                fallback.unwrap_or(wanted)
            };
            let target = Pane {
                namespace,
                previous: serde_json::from_value(discovered["previous"].clone())?,
                binding: Binding {
                    version: 1,
                    project: config.workspace.clone(),
                    pane,
                    terminal_epoch: epoch,
                    owner,
                    source_id: launch.id.clone(),
                    source_epoch: launch.epoch.clone(),
                    launch_digest: launch.digest()?,
                    source: None,
                },
            };
            state.native_sources.insert(pane, target.clone());
            state.terminal_launch_pending.retain(|id| *id != pane);
            state.save(config)?;
            (target, Some(launch))
        }
    };
    // Another frontend may have won while discovery was in progress. Inspect
    // its durable intent; never open a different command under its identity.
    let ready: Pane = if let Some(launch) = launch {
        serde_json::from_value(rpc(
            config,
            pane,
            json!({"op":"open","target":target,"launch":launch}),
        )?)?
    } else {
        serde_json::from_value(rpc(config, pane, json!({"op":"reattach","target":target}))?)?
    };
    let _lock = crate::project_sync::transaction(config)?;
    let mut state = ProjectState::load(config)?;
    ensure!(
        state
            .native_sources
            .get(&pane)
            .is_some_and(|current| current == &target || current == &ready),
        "Native source created but pane changed; source kept running"
    );
    state.native_sources.insert(pane, ready);
    state.save(config)?;
    Ok(true)
}
#[cfg(not(target_os = "linux"))]
pub fn ensure_source(_: &crate::runtime::Config, _: usize, _: Option<&str>) -> Result<bool> {
    Ok(false)
}
#[cfg(target_os = "linux")]
pub fn recover(
    original: &crate::runtime::Config,
    pane: usize,
    epoch: Option<&str>,
) -> Result<crate::runtime::Config> {
    use crate::{native_protocol::Binding, project_state::ProjectState};
    let config_path =
        crate::project_state::data_root()?.join(format!("{}.json", original.workspace));
    let (config, mut target) = {
        let _lock = crate::project_sync::transaction(original)?;
        let config: crate::runtime::Config = serde_json::from_slice(&std::fs::read(&config_path)?)?;
        config.validate()?;
        let state = ProjectState::load(&config)?;
        ensure!(
            state.terminal_ids().contains(&pane)
                && state.terminal_epochs.get(&pane).map(String::as_str) == epoch,
            "Terminal changed; native recovery canceled"
        );
        ensure!(
            !config.externally_owned(pane)
                && config.adopted_terminal(pane).is_none()
                && crate::plugin_panes::owner(&config, pane)?.is_none(),
            "Native recovery cannot replace an externally owned terminal"
        );
        let target = reference(&config, pane)?.context("Native source reference unavailable")?;
        (config, target)
    };
    let _worktree = crate::worktrees::ensure_allowed(&config)?;
    let launch = launch_spec(&config, pane, None)?;
    let mut sealed = false;
    let mut owner_lost = false;
    if target.binding.source.is_none() {
        let resolved = rpc(
            &config,
            pane,
            json!({"op":"resolve-pending","target":target}),
        )?;
        sealed = resolved["sealed"] == true;
        owner_lost = resolved["owner_lost"] == true;
        if !sealed {
            let ready: Pane = serde_json::from_value(resolved["target"].clone())?;
            let _lock = crate::project_sync::transaction(&config)?;
            let mut state = ProjectState::load(&config)?;
            ensure!(
                state.native_sources.get(&pane) == Some(&target),
                "Native intent changed during recovery discovery"
            );
            state.native_sources.insert(pane, ready.clone());
            state.save(&config)?;
            target = ready;
        }
    }
    // Resolve shell/home before touching the old source. No saved agent command
    // is ever reused by recovery, including after an interrupted request.
    if !sealed {
        let retired = rpc(&config, pane, json!({"op":"retire","target":target}))?;
        ensure!(retired["retired"] == true, "Native source is not retired");
        owner_lost = retired["owner_lost"] == true;
    }
    let (recovery_owner, previous) = if owner_lost {
        let result = rpc(
            &config,
            pane,
            json!({"op":"replacement-owner","target":target}),
        )?;
        (
            serde_json::from_value(result["owner"].clone())?,
            serde_json::from_value(result["previous"].clone())?,
        )
    } else {
        (target.binding.owner.clone(), target.binding.clone())
    };
    let pending = {
        let _lock = crate::project_sync::transaction(&config)?;
        let latest: crate::runtime::Config = serde_json::from_slice(&std::fs::read(&config_path)?)?;
        ensure!(
            serde_json::to_value(&latest)? == serde_json::to_value(&config)?,
            "Project changed during native recovery; no shell launched"
        );
        let mut state = ProjectState::load(&config)?;
        ensure!(
            state.terminal_ids().contains(&pane)
                && state.terminal_epochs.get(&pane).map(String::as_str) == epoch
                && state.native_sources.get(&pane) == Some(&target),
            "Terminal changed during native recovery; no shell launched"
        );
        let epoch = uuid::Uuid::new_v4().to_string();
        let pending = Pane {
            namespace: target.namespace.clone(),
            previous: Some(previous),
            binding: Binding {
                version: 1,
                project: config.workspace.clone(),
                pane,
                terminal_epoch: epoch.clone(),
                owner: recovery_owner,
                source_id: launch.id.clone(),
                source_epoch: launch.epoch.clone(),
                launch_digest: launch.digest()?,
                source: None,
            },
        };
        crate::agent_state::preserve_conversation(
            &config,
            pane,
            state.terminal_epochs.get(&pane).map(String::as_str),
            &epoch,
        )?;
        state.terminal_epochs.insert(pane, epoch);
        state.native_sources.insert(pane, pending.clone());
        state.terminal_commands.remove(&pane);
        state.terminal_launch_pending.retain(|id| *id != pane);
        state.save(&config)?;
        pending
    };
    let ready: Pane = serde_json::from_value(rpc(
        &config,
        pane,
        json!({"op":"open","target":pending,"launch":launch}),
    )?)?;
    let _lock = crate::project_sync::transaction(&config)?;
    let mut state = ProjectState::load(&config)?;
    ensure!(
        state.native_sources.get(&pane) == Some(&pending),
        "Native recovery result changed; source retained without replay"
    );
    state.native_sources.insert(pane, ready);
    state.save(&config)?;
    Ok(config)
}

pub fn call(
    config: &crate::runtime::Config,
    pane: usize,
    request: &Value,
) -> Result<Option<Value>> {
    use base64::Engine;
    let Some(target) = reference(config, pane)? else {
        return Ok(None);
    };
    let action = request["action"]
        .as_str()
        .context("Missing terminal action")?;
    let target =
        if matches!(action, "stop" | "terminate" | "reap") && target.binding.source.is_none() {
            serde_json::from_value(rpc(config, pane, json!({"op":"reattach","target":target}))?)?
        } else {
            target
        };
    let mut native = json!({"op":"call","target":target,"action":action,"include_screen":request["include_screen"]});
    match action {
        "inspect" => (),
        "history" | "capture" => {
            native["action"] = json!(if action == "history" {
                "history"
            } else {
                "screen"
            })
        }
        "stop" | "terminate" => {
            if request.get("expected_epoch").is_some() {
                ensure!(
                    request["expected_epoch"] == target.binding.terminal_epoch,
                    "Native terminal epoch changed before termination"
                );
            }
            if let Some(expected) = request["expected_pid"].as_u64() {
                let source = target
                    .binding
                    .source
                    .as_ref()
                    .context("Native process identity unavailable")?;
                ensure!(
                    expected == u64::from(source.pid)
                        && request["expected_start_ticks"] == source.start_ticks
                        && request["expected_boot_id"] == source.boot,
                    "Native termination identity changed"
                );
            }
            return Ok(Some(rpc(
                config,
                pane,
                json!({"op":"retire","target":target}),
            )?));
        }
        "paste" | "key" | "input" => {
            let health = rpc(
                config,
                pane,
                json!({"op":"call","target":target,"action":"inspect"}),
            )?;
            let bytes = if action == "key" {
                match request["key"].as_str() {
                    Some("Enter") => vec![13],
                    Some("C-c") => vec![3],
                    Some("Escape") => vec![27],
                    _ => anyhow::bail!("Unsupported native key"),
                }
            } else {
                let text = request["text"].as_str().context("Missing native text")?;
                ensure!(text.len() <= 1024 * 1024, "Native paste exceeds 1 MiB");
                if action == "input" {
                    ensure!(
                        request["expected_pid"] == health["pid"]
                            && request["expected_start_ticks"] == health["pid_start_ticks"]
                            && request["expected_boot_id"] == health["boot_id"]
                            && request["expected_agent_pid"] == health["agent_pid"]
                            && request["expected_agent_start_ticks"] == health["agent_start_ticks"],
                        "Native input identity changed"
                    );
                    ensure!(
                        text.len() <= 32768
                            && !text
                                .chars()
                                .any(|c| c.is_control() && !matches!(c, '\n' | '\t')),
                        "Invalid native automation input"
                    );
                    ensure!(
                        !text.contains(['\n', '\t'])
                            || health["input_modes"]["bracketed_paste"] == true,
                        "Multiline input requires bracketed paste support"
                    );
                }
                let clean = if action == "paste" {
                    text.replace('\x1b', "")
                } else {
                    text.into()
                };
                let mut bytes =
                    if health["input_modes"]["bracketed_paste"] == true && !clean.is_empty() {
                        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
                    } else {
                        clean.into_bytes()
                    };
                if request["submit"] == true {
                    bytes.push(13);
                }
                bytes
            };
            native["action"] = json!(if action == "input" {
                "guarded-input"
            } else {
                "input"
            });
            native["agent"] = if health["agent_pid"].is_number() {
                json!({"pid":health["agent_pid"],"start_ticks":health["agent_start_ticks"],"family":health["agent"]})
            } else {
                Value::Null
            };
            for chunk in bytes.chunks(1024 * 1024) {
                native["data"] = json!(base64::engine::general_purpose::STANDARD.encode(chunk));
                rpc(config, pane, native.clone())?;
            }
            return Ok(Some(
                json!({"queued":true,"submitted":request["submit"]==true,"bytes":bytes.len(),"delivery":"queued","backend":"native"}),
            ));
        }
        "reap" => {
            let result = rpc(
                config,
                pane,
                json!({"op":"retire","target":target,"exited_only":true}),
            )?;
            return Ok(Some(json!({"reaped":result["retired"] == true})));
        }
        "restart" | "recover" => {
            anyhow::bail!("Native recovery requires a fresh explicit pane epoch")
        }
        "upload" => return Ok(None),
        _ => anyhow::bail!("Unsupported native terminal action: {action}"),
    }
    Ok(Some(rpc(config, pane, native)?))
}

pub fn metadata_command(
    config: &crate::runtime::Config,
    pane: usize,
    request: &Value,
) -> Option<Command> {
    use base64::Engine;
    if !matches!(
        request["action"].as_str(),
        Some("inspect" | "history" | "capture")
    ) {
        return None;
    }
    let target = match reference(config, pane) {
        Ok(Some(target)) => target,
        Ok(None) => return None,
        // A malformed native reference must never select a legacy backend.
        Err(_) => return Some(Command::new("false")),
    };
    let action = match request["action"].as_str() {
        Some("capture") => "screen",
        Some("history") => "history",
        _ => "inspect",
    };
    let payload = json!({"op":"call","target":target,"action":action,"include_screen":request["include_screen"],"output":request["action"]});
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&payload).ok()?);
    if config.remote(pane) {
        let mut c = command(config, pane, "--native-pane-query");
        // command's final SSH argument is a shell-quoted remote executable.
        let mut args = c.get_args().map(|v| v.to_owned()).collect::<Vec<_>>();
        let last = args.pop()?;
        args.push(
            format!(
                "{} {}",
                last.to_string_lossy(),
                shell_words::quote(&encoded)
            )
            .into(),
        );
        let mut replacement = Command::new(c.get_program());
        replacement.args(args);
        c = replacement;
        Some(c)
    } else {
        let mut c = command(config, pane, "--native-pane-query");
        c.arg(encoded);
        Some(c)
    }
}
#[cfg(target_os = "linux")]
pub fn run_query(encoded: &str) -> Result<()> {
    use base64::Engine;
    ensure!(
        encoded.len() <= 32768,
        "Native metadata request exceeds limit"
    );
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    let request: Value = serde_json::from_slice(&bytes)?;
    let output = request["output"].as_str().unwrap_or("inspect").to_owned();
    let result = dispatch(request)?;
    if output == "history" {
        std::io::stdout().write_all(
            &base64::engine::general_purpose::STANDARD.decode(
                result["history_base64"]
                    .as_str()
                    .context("Native history unavailable")?,
            )?,
        )?;
    } else if output == "capture" {
        print!("{}", result["text"].as_str().unwrap_or_default());
    } else {
        println!("{}", serde_json::to_string(&result)?);
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub fn run_query(_: &str) -> Result<()> {
    anyhow::bail!("Native source owner requires Linux")
}

#[cfg(test)]
mod tests {
    #[test]
    fn model_rejects_native_references_from_an_old_pane_or_host_boot() {
        use crate::native_protocol::{Binding, Identity, Reference};
        let boot = uuid::Uuid::new_v4().to_string();
        let epoch = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let source_epoch = uuid::Uuid::new_v4().to_string();
        let target = super::Pane {
            namespace: "/private/native-fixture".into(),
            previous: None,
            binding: Binding {
                version: 1,
                project: "fixture".into(),
                pane: 0,
                terminal_epoch: epoch.clone(),
                owner: Identity {
                    instance: uuid::Uuid::new_v4().to_string(),
                    boot: boot.clone(),
                    pid: 1234,
                    start_ticks: 1,
                },
                source_id: id.clone(),
                source_epoch: source_epoch.clone(),
                launch_digest: "ab".repeat(32),
                source: Some(Reference {
                    id,
                    epoch: source_epoch,
                    boot,
                    pid: 1235,
                    start_ticks: 2,
                }),
            },
        };
        let mut state = crate::project_state::ProjectState::default();
        state.terminal_epochs.insert(0, epoch);
        state.native_sources.insert(0, target.clone());
        state.validate().unwrap();
        state
            .native_sources
            .get_mut(&0)
            .unwrap()
            .binding
            .terminal_epoch = uuid::Uuid::new_v4().to_string();
        assert!(state.validate().is_err());
        state.native_sources.insert(0, target);
        state
            .native_sources
            .get_mut(&0)
            .unwrap()
            .binding
            .source
            .as_mut()
            .unwrap()
            .boot = uuid::Uuid::new_v4().to_string();
        assert!(state.validate().is_err());
    }
}
