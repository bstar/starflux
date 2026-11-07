//! Durable, explicit agent identity and lifecycle reports. No transcript scraping.
use crate::{project_state::ProjectState, runtime::Config};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Unknown,
    Idle,
    Working,
    NeedsInput,
    WaitingForApproval,
    Completed,
    Failed,
    Stopped,
    BackendUnavailable,
}
impl Lifecycle {
    fn label(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Idle => "idle",
            Self::Working => "working",
            Self::NeedsInput => "needs_input",
            Self::WaitingForApproval => "waiting_for_approval",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::BackendUnavailable => "backend_unavailable",
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub run_id: String,
    pub sequence: u64,
    pub epoch: Option<String>,
    pub agent_pid: u32,
    pub agent_start_ticks: u64,
    /// Host boot identity; absent only in records saved by older releases.
    #[serde(default)]
    pub boot_id: Option<String>,
    pub harness: String,
    pub source: String,
    pub state: Lifecycle,
    #[serde(default)]
    pub replaces_run: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub resume_argv: Option<Vec<String>>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub pane: usize,
    pub observed_ms: u64,
    #[serde(flatten)]
    pub report: Report,
}
#[derive(Default, Deserialize, Serialize)]
struct Store {
    #[serde(default)]
    observed: BTreeMap<usize, String>,
    #[serde(default)]
    attention: Vec<Attention>,
    #[serde(default)]
    active: BTreeMap<usize, String>,
    #[serde(default)]
    runs: BTreeMap<String, Record>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Attention {
    #[serde(default)]
    pub source: String,
    pub id: String,
    pub pane: usize,
    pub epoch: Option<String>,
    pub run_id: String,
    pub sequence: u64,
    pub state: Lifecycle,
    pub observed_ms: u64,
    pub dismissed: bool,
}
impl Attention {
    pub fn label(&self) -> &'static str {
        match self.state {
            Lifecycle::NeedsInput => "Question pending",
            Lifecycle::WaitingForApproval => "Approval requested",
            Lifecycle::Completed => "Completed",
            Lifecycle::Failed => "Failed",
            Lifecycle::BackendUnavailable => "Backend unavailable",
            _ => "Agent update",
        }
    }
}
fn attention_state(state: &Lifecycle) -> bool {
    matches!(
        state,
        Lifecycle::NeedsInput
            | Lifecycle::WaitingForApproval
            | Lifecycle::Completed
            | Lifecycle::Failed
            | Lifecycle::BackendUnavailable
    )
}
fn path(config: &Config) -> Result<std::path::PathBuf> {
    config.validate()?;
    Ok(crate::project_state::data_root()?.join(format!("{}.agents.json", config.workspace)))
}
fn load(config: &Config) -> Result<Store> {
    match fs::read(path(config)?) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).context("Read saved agent records")?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(e.into()),
    }
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn process_start(pid: u32) -> Option<u64> {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()?
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}
pub fn boot_id() -> Option<uuid::Uuid> {
    uuid::Uuid::parse_str(
        fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()?
            .trim(),
    )
    .ok()
}
fn token(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}
impl Report {
    fn validate(&self) -> Result<()> {
        uuid::Uuid::parse_str(&self.run_id).context("run_id must be a UUID")?;
        if let Some(boot) = &self.boot_id {
            uuid::Uuid::parse_str(boot).context("boot_id must be a UUID from --agent-status")?;
        }
        ensure!(self.sequence > 0, "sequence starts at 1");
        ensure!(
            self.agent_pid > 0 && self.agent_start_ticks > 0,
            "Provide the agent process identity from --agent-status"
        );
        for text in [&self.harness, &self.source] {
            ensure!(
                token(text, 80)
                    && text
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_:./".contains(&c)),
                "Invalid harness or source identifier"
            );
        }
        ensure!(
            self.conversation_id
                .as_ref()
                .is_none_or(|id| token(id, 512)),
            "Invalid conversation reference"
        );
        if let Some(argv) = &self.resume_argv {
            ensure!(
                !argv.is_empty() && argv.len() <= 64 && argv.iter().all(|s| token(s, 4096)),
                "Invalid resume argv"
            );
            ensure!(
                self.conversation_id
                    .as_ref()
                    .is_some_and(|id| argv.iter().any(|arg| arg == id)),
                "Resume argv must contain the exact conversation reference as an argument"
            );
            ensure!(!argv[0].starts_with('-'), "Invalid resume executable");
        }
        Ok(())
    }
}
fn accept(
    store: &mut Store,
    state: &ProjectState,
    pane: usize,
    health: &Value,
    report: Report,
    now: u64,
) -> Result<Record> {
    report.validate()?;
    ensure!(
        report
            .boot_id
            .as_deref()
            .is_some_and(|boot| health["boot_id"].as_str() == Some(boot)),
        "Host boot identity is missing or changed; refresh --agent-status before reporting"
    );
    ensure!(
        state.terminal_ids().contains(&pane),
        "Terminal no longer exists"
    );
    ensure!(
        state.terminal_epochs.get(&pane).cloned() == report.epoch,
        "Terminal generation changed; report rejected"
    );
    ensure!(
        health["alive"] == true
            && health["agent_pid"].as_u64() == Some(u64::from(report.agent_pid))
            && health["agent_start_ticks"].as_u64() == Some(report.agent_start_ticks),
        "Agent process changed; report rejected"
    );
    ensure!(
        health["agent"].as_str() == Some(report.harness.as_str()),
        "Harness does not match this terminal's foreground agent"
    );
    if let Some(previous) = store.runs.get(&report.run_id) {
        ensure!(
            previous.pane == pane
                && previous.report.epoch == report.epoch
                && previous.report.agent_pid == report.agent_pid
                && previous.report.agent_start_ticks == report.agent_start_ticks
                && previous.report.boot_id == report.boot_id
                && previous.report.harness == report.harness
                && previous.report.source == report.source,
            "Run identity changed"
        );
        ensure!(
            store.active.get(&pane) == Some(&report.run_id),
            "Run is no longer active"
        );
        ensure!(
            report.sequence > previous.report.sequence,
            "Stale or duplicate sequence"
        );
        ensure!(
            previous.report.conversation_id.is_none()
                || previous.report.conversation_id == report.conversation_id,
            "Conversation identity changed; register a new run"
        );
        ensure!(
            previous.report.resume_argv.is_none()
                || previous.report.resume_argv == report.resume_argv,
            "Resume command changed; register a new run"
        );
    } else {
        ensure!(report.sequence == 1, "A new run starts at sequence 1");
        if let Some(old) = store.active.get(&pane).and_then(|id| store.runs.get(id))
            && old.report.epoch == report.epoch
            && old.report.agent_pid == report.agent_pid
            && old.report.agent_start_ticks == report.agent_start_ticks
            && old.report.boot_id == report.boot_id
        {
            ensure!(
                report.replaces_run.as_ref() == Some(&old.report.run_id),
                "Replacing a run in the same process requires replaces_run to match the current run"
            );
        }
    }
    let changed = store
        .runs
        .get(&report.run_id)
        .is_none_or(|old| old.report.state != report.state);
    if changed && attention_state(&report.state) {
        store.attention.push(Attention {
            source: report.source.clone(),
            id: uuid::Uuid::new_v4().to_string(),
            pane,
            epoch: report.epoch.clone(),
            run_id: report.run_id.clone(),
            sequence: report.sequence,
            state: report.state.clone(),
            observed_ms: now,
            dismissed: false,
        });
    }
    let record = Record {
        pane,
        observed_ms: now,
        report,
    };
    store.active.insert(pane, record.report.run_id.clone());
    store
        .runs
        .insert(record.report.run_id.clone(), record.clone());
    Ok(record)
}
pub fn publish(config: &Config, pane: usize, report: Report) -> Result<Record> {
    let _lock = crate::project_sync::transaction(config)?;
    let state = ProjectState::load(config)?;
    let mut health = config.call(pane, &config.request(pane, "inspect"))?;
    stamp_local(config, pane, &mut health);
    let mut store = load(config)?;
    let event_changed = store
        .runs
        .get(&report.run_id)
        .is_none_or(|old| old.report.state != report.state);
    let record = accept(&mut store, &state, pane, &health, report, now_ms())?;
    let pruned = prune_store(&mut store);
    starkit::fs::write_private(&path(config)?, &serde_json::to_vec_pretty(&store)?)?;
    if event_changed || pruned > 0 {
        crate::automation::journal::agents_saved(
            config,
            store.runs.values(),
            store.attention.iter(),
        )?;
    }
    Ok(record)
}
pub fn records(config: &Config) -> Result<Vec<Record>> {
    Ok(load(config)?.runs.into_values().collect())
}
pub fn attention(config: &Config) -> Result<Vec<Attention>> {
    Ok(load(config)?.attention)
}
pub fn dismiss(config: &Config, id: &str) -> Result<()> {
    let _lock = crate::project_sync::transaction(config)?;
    let mut store = load(config)?;
    store
        .attention
        .iter_mut()
        .find(|a| a.id == id)
        .context("Attention item no longer exists")?
        .dismissed = true;
    starkit::fs::write_private(&path(config)?, &serde_json::to_vec_pretty(&store)?)?;
    crate::automation::journal::agents_saved(config, store.runs.values(), store.attention.iter())?;
    Ok(())
}
fn prune_store(store: &mut Store) -> usize {
    crate::notifications::settings()
        .map(|s| crate::notifications::retain(&mut store.attention, &s, now_ms()))
        .unwrap_or(0)
}
pub fn prune_attention(
    config: &Config,
    settings: &crate::notifications::Settings,
) -> Result<usize> {
    let _lock = crate::project_sync::transaction(config)?;
    let mut store = load(config)?;
    let removed = crate::notifications::retain(&mut store.attention, settings, now_ms());
    if removed > 0 {
        starkit::fs::write_private(&path(config)?, &serde_json::to_vec_pretty(&store)?)?;
        crate::automation::journal::agents_saved(
            config,
            store.runs.values(),
            store.attention.iter(),
        )?;
    }
    Ok(removed)
}
pub fn dismiss_many(config: &Config, ids: &std::collections::BTreeSet<String>) -> Result<usize> {
    let _lock = crate::project_sync::transaction(config)?;
    let mut store = load(config)?;
    ensure!(
        ids.iter()
            .all(|id| store.attention.iter().any(|a| &a.id == id)),
        "Attention items changed; refresh the inbox before marking this batch read"
    );
    let mut updated = 0;
    for event in &mut store.attention {
        if ids.contains(&event.id) && !event.dismissed {
            event.dismissed = true;
            updated += 1;
        }
    }
    if updated > 0 {
        starkit::fs::write_private(&path(config)?, &serde_json::to_vec_pretty(&store)?)?;
        crate::automation::journal::agents_saved(
            config,
            store.runs.values(),
            store.attention.iter(),
        )?;
    }
    Ok(updated)
}
pub fn resume_argv(config: &Config, pane: usize, run: &str) -> Result<Vec<String>> {
    let store = load(config)?;
    let state = ProjectState::load(config)?;
    let record = store
        .runs
        .get(run)
        .context("Saved conversation no longer exists")?;
    ensure!(
        store.active.get(&pane).is_some_and(|id| id == run)
            && record.pane == pane
            && state.terminal_ids().contains(&pane)
            && state.terminal_epochs.get(&pane) == record.report.epoch.as_ref(),
        "Pane or conversation changed; reopen its menu"
    );
    record.report.validate()?;
    record
        .report
        .resume_argv
        .clone()
        .context("This integration has not registered a resume command")
}
fn stamp_local(config: &Config, pane: usize, health: &mut Value) {
    if !config.remote(pane) {
        health["boot_id"] = json!(boot_id().map(|id| id.to_string()));
        if let Some(pid) = health["pid"].as_u64().and_then(|p| u32::try_from(p).ok()) {
            health["pid_start_ticks"] = json!(process_start(pid));
        }
    }
    if !config.remote(pane)
        && let Some(pid) = health["agent_pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
    {
        health["agent_start_ticks"] = json!(process_start(pid));
    }
}
pub fn enrich(config: &Config, pane: usize, health: &mut Value) {
    stamp_local(config, pane, health);
    let state = ProjectState::load(config).ok();
    let store = load(config).ok();
    let record = store
        .as_ref()
        .and_then(|s| s.active.get(&pane).and_then(|id| s.runs.get(id)))
        .filter(|r| {
            state
                .as_ref()
                .is_some_and(|s| s.terminal_epochs.get(&pane) == r.report.epoch.as_ref())
        });
    apply_health(health, record, now_ms());
}
pub fn observe_attention(
    config: &Config,
    pane: usize,
    expected_epoch: Option<&str>,
    health: &Value,
) -> Result<()> {
    if health["health"]["source"] != "screen" || health["alive"] != true {
        return Ok(());
    }
    let (Some(pid), Some(start)) = (
        health["agent_pid"].as_u64(),
        health["agent_start_ticks"].as_u64(),
    ) else {
        return Ok(());
    };
    let lifecycle = match health["state"].as_str().unwrap_or("unknown") {
        "needs_input" => Lifecycle::NeedsInput,
        "waiting_for_approval" => Lifecycle::WaitingForApproval,
        "backend_unavailable" => Lifecycle::BackendUnavailable,
        "failed" => Lifecycle::Failed,
        _ => Lifecycle::Unknown,
    };
    let _lock = crate::project_sync::transaction(config)?;
    let state = ProjectState::load(config)?;
    if !state.terminal_ids().contains(&pane)
        || state.terminal_epochs.get(&pane).map(String::as_str) != expected_epoch
    {
        return Ok(());
    }
    let mut store = load(config)?;
    let epoch = state.terminal_epochs.get(&pane).cloned();
    let signature = format!(
        "{:?}:{}:{pid}:{start}:{}",
        epoch, health["boot_id"], health["state"]
    );
    if store.observed.get(&pane) == Some(&signature) {
        return Ok(());
    }
    store.observed.insert(pane, signature);
    let attention_changed = attention_state(&lifecycle);
    if attention_changed {
        store.attention.push(Attention {
            source: "screen:heuristic".into(),
            id: uuid::Uuid::new_v4().to_string(),
            pane,
            epoch,
            run_id: format!("screen:{pid}:{start}"),
            sequence: 0,
            state: lifecycle,
            observed_ms: now_ms(),
            dismissed: false,
        });
    }
    let pruned = prune_store(&mut store);
    starkit::fs::write_private(&path(config)?, &serde_json::to_vec_pretty(&store)?)?;
    if attention_changed || pruned > 0 {
        crate::automation::journal::agents_saved(
            config,
            store.runs.values(),
            store.attention.iter(),
        )?;
    }
    Ok(())
}
fn apply_health(health: &mut Value, record: Option<&Record>, now: u64) {
    let alive = health["alive"] == true;
    let agent = alive && health["agent_pid"].as_u64().is_some();
    let current = record.filter(|r| {
        agent
            && health["agent_pid"].as_u64() == Some(u64::from(r.report.agent_pid))
            && health["agent_start_ticks"].as_u64() == Some(r.report.agent_start_ticks)
            && r.report
                .boot_id
                .as_deref()
                .is_some_and(|boot| health["boot_id"].as_str() == Some(boot))
    });
    let fresh = current.is_some_and(|r| now >= r.observed_ms && now - r.observed_ms <= 60_000);
    // Explicit backend failure on the live screen takes precedence over an
    // older integration heartbeat claiming that work is still in progress.
    let backend_failed = agent && health["state"] == "backend_unavailable";
    if fresh && !backend_failed {
        health["state"] = json!(current.unwrap().report.state.label());
    }
    let source = if fresh && !backend_failed {
        "integration"
    } else if agent {
        "screen"
    } else {
        "process"
    };
    health["agent_record"] = json!(record);
    health["health"] = json!({
        "terminal":if alive {"alive"} else if health["exited"]==true {"exited"} else {"unreachable"},
        "agent_process":if agent {"running"} else if alive && health["agent"].is_null() {"absent"} else {"unknown"},
        "transport":if health["bridge_connected"]==false || health["state"]=="disconnected" {"disconnected"} else if alive {"connected"} else {"unknown"},
        "agent_backend":if backend_failed || health["state"]=="backend_unavailable" {"unavailable"} else {"unknown"},
        "lifecycle":health["state"],"source":source,"confidence":if source=="integration" {"reported"} else if source=="screen" {"heuristic"} else {"unknown"},
        "report_stale":record.is_some() && !fresh,"report_process_matches":current.is_some(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attention_coalesces_heartbeats_but_preserves_new_transitions() {
        let (state, health, mut report) = fixture();
        let mut store = Store::default();
        report.state = Lifecycle::WaitingForApproval;
        accept(&mut store, &state, 0, &health, report.clone(), 1).unwrap();
        report.sequence = 2;
        accept(&mut store, &state, 0, &health, report.clone(), 2).unwrap();
        assert_eq!(store.attention.len(), 1);
        store.attention[0].dismissed = true;
        report.sequence = 3;
        report.state = Lifecycle::Working;
        accept(&mut store, &state, 0, &health, report.clone(), 3).unwrap();
        report.sequence = 4;
        report.state = Lifecycle::WaitingForApproval;
        accept(&mut store, &state, 0, &health, report, 4).unwrap();
        assert_eq!(store.attention.len(), 2);
        assert!(store.attention[0].dismissed);
        assert!(!store.attention[1].dismissed);
        let saved: Store = serde_json::from_slice(&serde_json::to_vec(&store).unwrap()).unwrap();
        assert_eq!(saved.attention.len(), 2);
    }
    fn fixture() -> (ProjectState, Value, Report) {
        let mut state = ProjectState::default();
        state.terminal_epochs.insert(0, "epoch".into());
        let health = json!({"alive":true,"agent":"codex","agent_pid":123,"agent_start_ticks":456,"boot_id":"7a307521-f2e5-4647-8d7e-b1330366037e","state":"idle"});
        let report = Report {
            run_id: uuid::Uuid::new_v4().to_string(),
            sequence: 1,
            epoch: Some("epoch".into()),
            agent_pid: 123,
            agent_start_ticks: 456,
            boot_id: Some("7a307521-f2e5-4647-8d7e-b1330366037e".into()),
            harness: "codex".into(),
            source: "test:hook".into(),
            state: Lifecycle::Working,
            replaces_run: None,
            conversation_id: Some("exact-thread".into()),
            resume_argv: Some(vec!["codex".into(), "resume".into(), "exact-thread".into()]),
        };
        (state, health, report)
    }
    #[test]
    fn reject_delayed_reports_and_conversation_replacement() {
        let (state, health, report) = fixture();
        let mut store = Store::default();
        accept(&mut store, &state, 0, &health, report.clone(), 1000).unwrap();
        assert!(accept(&mut store, &state, 0, &health, report.clone(), 1001).is_err());
        let mut changed = report.clone();
        changed.sequence = 2;
        changed.conversation_id = Some("other-thread".into());
        changed.resume_argv = None;
        assert!(accept(&mut store, &state, 0, &health, changed, 1001).is_err());
        let mut next = report.clone();
        next.run_id = uuid::Uuid::new_v4().to_string();
        assert!(accept(&mut store, &state, 0, &health, next.clone(), 1002).is_err());
        next.replaces_run = Some(report.run_id.clone());
        accept(&mut store, &state, 0, &health, next, 1003).unwrap();
        let mut delayed = report;
        delayed.sequence = 99;
        assert!(accept(&mut store, &state, 0, &health, delayed, 1004).is_err());
        assert_eq!(
            store.runs.len(),
            2,
            "Old conversation references must remain available"
        );
    }
    #[test]
    fn reject_recycled_panes_and_processes() {
        let (mut state, mut health, report) = fixture();
        let mut store = Store::default();
        health["agent_start_ticks"] = json!(457);
        assert!(accept(&mut store, &state, 0, &health, report.clone(), 1000).is_err());
        health["agent_start_ticks"] = json!(456);
        state.terminal_epochs.insert(0, "recycled".into());
        assert!(accept(&mut store, &state, 0, &health, report.clone(), 1000).is_err());
        state.terminal_epochs.insert(0, "epoch".into());
        health["agent"] = json!("claude");
        assert!(accept(&mut store, &state, 0, &health, report, 1000).is_err());
    }
    #[test]
    fn reboot_invalidates_process_match_but_keeps_conversation_reference() {
        let (state, mut health, report) = fixture();
        let mut store = Store::default();
        let record = accept(&mut store, &state, 0, &health, report.clone(), 1000).unwrap();
        // PIDs, start ticks and wall-clock freshness all collide across boots.
        health["boot_id"] = json!("088f251c-a7de-43cd-a07c-3032d2c12f71");
        let mut delayed = report.clone();
        delayed.sequence = 2;
        assert!(accept(&mut store, &state, 0, &health, delayed, 1001).is_err());
        let mut reused_run = report.clone();
        reused_run.sequence = 2;
        reused_run.boot_id = health["boot_id"].as_str().map(str::to_owned);
        assert!(accept(&mut store, &state, 0, &health, reused_run.clone(), 1001).is_err());
        reused_run.run_id = uuid::Uuid::new_v4().to_string();
        reused_run.sequence = 1;
        accept(&mut store, &state, 0, &health, reused_run, 1001).unwrap();
        assert_eq!(
            store.runs.len(),
            2,
            "Previous conversation must stay archived"
        );
        apply_health(&mut health, Some(&record), 1001);
        assert_eq!(health["health"]["report_process_matches"], false);
        assert_eq!(health["health"]["source"], "screen");
        assert_eq!(health["agent_record"]["conversation_id"], "exact-thread");
        let mut legacy = serde_json::to_value(&record).unwrap();
        legacy.as_object_mut().unwrap().remove("boot_id");
        let legacy: Record = serde_json::from_value(legacy).unwrap();
        legacy.report.validate().unwrap();
        health["boot_id"] = json!(report.boot_id);
        apply_health(&mut health, Some(&legacy), 1001);
        assert_eq!(health["health"]["report_process_matches"], false);
        assert!(
            accept(
                &mut Store::default(),
                &state,
                0,
                &health,
                legacy.report,
                1002
            )
            .is_err()
        );
    }
    #[test]
    fn health_layers_distinguish_backend_failure_and_stale_reports() {
        let (_, health, report) = fixture();
        let record = Record {
            pane: 0,
            observed_ms: 1000,
            report,
        };
        let mut fresh = health.clone();
        apply_health(&mut fresh, Some(&record), 1001);
        assert_eq!(fresh["state"], "working");
        assert_eq!(fresh["health"]["source"], "integration");
        let mut stale = health.clone();
        apply_health(&mut stale, Some(&record), 61001);
        assert_eq!(stale["state"], "idle");
        assert_eq!(stale["health"]["report_stale"], true);
        let mut failed = health.clone();
        failed["state"] = json!("backend_unavailable");
        apply_health(&mut failed, Some(&record), 1002);
        assert_eq!(failed["health"]["agent_process"], "running");
        assert_eq!(failed["health"]["agent_backend"], "unavailable");
        assert_eq!(failed["state"], "backend_unavailable");
        let mut shell = health;
        shell["agent_pid"] = Value::Null;
        shell["agent"] = Value::Null;
        apply_health(&mut shell, Some(&record), 1003);
        assert_eq!(shell["health"]["terminal"], "alive");
        assert_eq!(shell["health"]["agent_process"], "absent");
        assert_eq!(shell["agent_record"]["conversation_id"], "exact-thread");
    }
    #[test]
    fn resume_requires_an_explicit_reference() {
        let (_, _, mut report) = fixture();
        report.resume_argv = Some(vec!["codex".into(), "resume".into(), "--last".into()]);
        assert!(report.validate().is_err());
        assert!(process_start(std::process::id()).is_some_and(|s| s > 0));
    }
}
