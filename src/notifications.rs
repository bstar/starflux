//! Attention queries, bounded retention and opt-in local desktop delivery.
use crate::{
    agent_state::{Attention, Lifecycle},
    runtime::Config,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    InApp,
    System,
    Off,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u8,
    pub revision: String,
    pub delivery: Delivery,
    pub states: Vec<Lifecycle>,
    pub suppress_active: bool,
    pub retention_days: u32,
    pub max_items_per_project: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            revision: "defaults".into(),
            delivery: Delivery::InApp,
            states: vec![
                Lifecycle::NeedsInput,
                Lifecycle::WaitingForApproval,
                Lifecycle::Completed,
                Lifecycle::Failed,
                Lifecycle::BackendUnavailable,
            ],
            suppress_active: true,
            retention_days: 30,
            max_items_per_project: 2000,
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
impl Settings {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "Unsupported notification settings version"
        );
        ensure!(
            self.retention_days <= 3650,
            "Retention days must be 0–3650 (0 disables age expiry)"
        );
        ensure!(
            (10..=50000).contains(&self.max_items_per_project),
            "Attention limit must be 10–50000 items per project"
        );
        ensure!(
            self.states.len() <= 5 && self.states.iter().all(attention_state),
            "Invalid notification states"
        );
        for (i, s) in self.states.iter().enumerate() {
            ensure!(
                !self.states[..i].contains(s),
                "Duplicate notification state"
            );
        }
        Ok(())
    }
    pub fn wants(&self, event: &Attention) -> bool {
        self.states.contains(&event.state)
    }
}
fn config_path() -> Result<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(".config"))
        .join("starflux");
    ensure!(
        root.is_absolute(),
        "Notification configuration path must be absolute"
    );
    starkit::terminal_graphics::session::private_root(&root)?;
    Ok(root.join("notifications.json"))
}
fn read<T: serde::de::DeserializeOwned>(path: &std::path::Path, max: u64) -> Result<T> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= max,
        "Notification metadata exceeds its size limit"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn settings() -> Result<Settings> {
    let path = config_path()?;
    let value = match read::<Settings>(&path, 65536) {
        Ok(v) => v,
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Settings::default()
        }
        Err(e) => {
            return Err(
                e.context("Read notification settings; existing configuration was preserved")
            );
        }
    };
    value.validate()?;
    Ok(value)
}
pub fn save(mut value: Settings, expected_revision: &str) -> Result<Settings> {
    value.validate()?;
    let _lock = crate::project_sync::Transaction::acquire(config_path()?.with_extension("lock"))?;
    ensure!(
        settings()?.revision == expected_revision,
        "Notification settings changed; reload before editing"
    );
    value.revision = uuid::Uuid::new_v4().to_string();
    crate::automation::persist(&config_path()?, &serde_json::to_vec_pretty(&value)?)?;
    Ok(value)
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadState {
    #[default]
    Unread,
    Read,
    All,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    All,
    Reported,
    Screen,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    pub offset: usize,
    pub limit: usize,
    pub search: String,
    pub read: ReadState,
    pub state: Option<Lifecycle>,
    pub project: Option<String>,
    pub session: Option<String>,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub project: String,
    pub label: String,
    pub session: String,
    pub host: Option<String>,
    pub provider: Option<String>,
    pub conversation_id: Option<String>,
    pub event: Attention,
}
#[derive(Debug, Serialize)]
pub struct Inbox {
    pub total: usize,
    pub offset: usize,
    pub items: Vec<Item>,
    pub warnings: Vec<Value>,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 100,
            search: String::new(),
            read: ReadState::Unread,
            state: None,
            project: None,
            session: None,
            source: Source::All,
        }
    }
}
impl Query {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=250).contains(&self.limit),
            "Inbox page size must be 1–250"
        );
        ensure!(self.offset <= 10000000, "Inbox offset exceeds 10000000");
        ensure!(
            self.search.len() <= 1024 && !self.search.chars().any(char::is_control),
            "Search must be at most 1024 bytes without control characters"
        );
        ensure!(
            self.state.as_ref().is_none_or(attention_state),
            "Choose an attention state"
        );
        Ok(())
    }
    fn matches(&self, i: &Item) -> bool {
        if self.project.as_ref().is_some_and(|p| *p != i.project)
            || self.session.as_ref().is_some_and(|s| *s != i.session)
            || self.state.as_ref().is_some_and(|s| *s != i.event.state)
        {
            return false;
        }
        if match self.read {
            ReadState::Unread => i.event.dismissed,
            ReadState::Read => !i.event.dismissed,
            ReadState::All => false,
        } {
            return false;
        }
        let screen = i.provider.is_none() && i.event.source.starts_with("screen:");
        if (self.source == Source::Screen && !screen)
            || (self.source == Source::Reported && i.provider.is_none())
        {
            return false;
        }
        let text = format!(
            "{} {} {} {} {} {} {} {} {} {:?}",
            i.project,
            i.label,
            i.session,
            i.host.as_deref().unwrap_or("local"),
            i.provider.as_deref().unwrap_or(""),
            i.conversation_id.as_deref().unwrap_or(""),
            i.event.source,
            i.event.run_id,
            i.event.label(),
            serde_json::to_string(&i.event.state).unwrap_or_default()
        )
        .to_lowercase();
        self.search
            .to_lowercase()
            .split_whitespace()
            .all(|word| text.contains(word))
    }
}
pub fn inbox(query: &Query) -> Result<Inbox> {
    query.validate()?;
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    for c in crate::session_catalog::projects() {
        if query.project.as_ref().is_some_and(|p| *p != c.workspace)
            || query
                .session
                .as_ref()
                .is_some_and(|s| *s != crate::session_catalog::group(&c))
        {
            continue;
        }
        let events = match crate::agent_state::attention(&c) {
            Ok(e) => e,
            Err(e) => {
                warnings.push(json!({"project":c.workspace,"error":format!("{e:#}")}));
                continue;
            }
        };
        let records = crate::agent_state::records(&c)
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.report.run_id.clone(), r))
            .collect::<BTreeMap<_, _>>();
        for event in events {
            let record = records.get(&event.run_id);
            let item = Item {
                project: c.workspace.clone(),
                label: c.label.clone().unwrap_or_else(|| c.workspace.clone()),
                session: crate::session_catalog::group(&c),
                host: c.host.clone(),
                provider: record.map(|r| r.report.harness.clone()),
                conversation_id: record.and_then(|r| r.report.conversation_id.clone()),
                event,
            };
            if query.matches(&item) {
                items.push(item);
            }
        }
    }
    items.sort_by(|a, b| {
        b.event
            .observed_ms
            .cmp(&a.event.observed_ms)
            .then(a.project.cmp(&b.project))
            .then(a.event.id.cmp(&b.event.id))
    });
    Ok(Inbox {
        total: items.len(),
        offset: 0,
        items,
        warnings,
    })
}
pub fn query(query: &Query) -> Result<Inbox> {
    let mut result = inbox(query)?;
    let offset = if query.offset >= result.total && result.total > 0 {
        (result.total - 1) / query.limit * query.limit
    } else {
        query.offset
    };
    result.items = result
        .items
        .into_iter()
        .skip(offset)
        .take(query.limit)
        .collect();
    result.offset = offset;
    Ok(result)
}
pub fn retain(events: &mut Vec<Attention>, s: &Settings, now: u64) -> usize {
    let before = events.len();
    if s.retention_days > 0 {
        let cutoff = now.saturating_sub(u64::from(s.retention_days) * 86400000);
        events.retain(|e| e.observed_ms >= cutoff);
    }
    events.sort_by(|a, b| a.observed_ms.cmp(&b.observed_ms).then(a.id.cmp(&b.id)));
    if events.len() > s.max_items_per_project {
        events.drain(..events.len() - s.max_items_per_project);
    }
    before - events.len()
}
pub fn apply_retention() -> Result<Value> {
    let settings = settings()?;
    let mut projects = Vec::new();
    let mut errors = Vec::new();
    for config in crate::session_catalog::projects() {
        match crate::agent_state::prune_attention(&config, &settings) {
            Ok(n) => projects.push(json!({"project":config.workspace,"removed":n})),
            Err(e) => errors.push(json!({"project":config.workspace,"error":format!("{e:#}")})),
        }
    }
    Ok(json!({"projects":projects,"errors":errors,"revision":settings.revision}))
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub project: String,
    pub event: String,
}
pub fn mark_read(items: &[Reference]) -> Result<Value> {
    ensure!(items.len() <= 50000, "Mark-read batch exceeds 50000 items");
    let mut groups = BTreeMap::<String, BTreeSet<String>>::new();
    for item in items {
        crate::runtime::config_path(&item.project)?;
        uuid::Uuid::parse_str(&item.event).context("Invalid attention item identity")?;
        groups
            .entry(item.project.clone())
            .or_default()
            .insert(item.event.clone());
    }
    let mut updated = 0;
    let mut errors = Vec::new();
    for (project, ids) in groups {
        match crate::saved(&project).and_then(|c| crate::agent_state::dismiss_many(&c, &ids)) {
            Ok(n) => updated += n,
            Err(e) => errors.push(json!({"project":project,"error":format!("{e:#}")})),
        }
    }
    Ok(json!({"updated":updated,"errors":errors}))
}
fn delivery_root() -> Result<PathBuf> {
    let root = crate::project_state::data_root()?
        .parent()
        .context("No notification directory")?
        .join("notifications");
    starkit::terminal_graphics::session::private_root(&root)?;
    Ok(root)
}
pub fn system_capability() -> Value {
    let remote =
        std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
    let available = !remote
        && (cfg!(target_os = "macos")
            || (std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
                && (std::env::var_os("DISPLAY").is_some()
                    || std::env::var_os("WAYLAND_DISPLAY").is_some())));
    json!({"available":available,"reason":if remote{"Desktop delivery to an SSH presentation needs client transport support; in-app notifications remain available"}else if !available{"No desktop session is available on this controller"}else{"Local desktop service; delivery is confirmed only when its notification helper accepts the request"}})
}
#[derive(Serialize, Deserialize)]
struct Attempt {
    at_ms: u64,
    outcome: String,
    error: Option<String>,
}
pub fn delivery_status() -> Result<Value> {
    let path = delivery_root()?.join("deliveries.json");
    let records = if path.exists() {
        read::<BTreeMap<String, Attempt>>(&path, 32 * 1024 * 1024)?
    } else {
        BTreeMap::new()
    };
    let total = records.len();
    let mut rows = records.into_iter().collect::<Vec<_>>();
    rows.sort_by_key(|(_, a)| std::cmp::Reverse(a.at_ms));
    let attempts = rows.into_iter().take(200).collect::<BTreeMap<_, _>>();
    Ok(
        json!({"capability":system_capability(),"attempts":attempts,"total_attempts":total,"truncated":total>200}),
    )
}
/// Claim once before contacting the OS. Uncertain/crashed attempts remain visible, never silently retried.
pub fn deliver(items: &[Item], started_ms: u64, active: Option<(&str, usize)>) -> Result<usize> {
    let settings = settings()?;
    if settings.delivery != Delivery::System || system_capability()["available"] != true {
        return Ok(0);
    }
    let path = delivery_root()?.join("deliveries.json");
    let _lock = crate::project_sync::Transaction::acquire(path.with_extension("lock"))?;
    let mut records = if path.exists() {
        read::<BTreeMap<String, Attempt>>(&path, 32 * 1024 * 1024)?
    } else {
        BTreeMap::new()
    };
    if records.len() > 5000 {
        let retained = inbox(&Query {
            read: ReadState::All,
            ..Default::default()
        })?;
        if retained.warnings.is_empty() {
            let ids = retained
                .items
                .iter()
                .map(|i| format!("{}:{}", i.project, i.event.id))
                .collect::<BTreeSet<_>>();
            records.retain(|id, _| ids.contains(id));
        }
    }
    let focused = active_panes();
    let mut sent = 0;
    let mut attempted = 0;
    for item in items.iter().filter(|i| {
        !i.event.dismissed && i.event.observed_ms >= started_ms && settings.wants(&i.event)
    }) {
        let id = format!("{}:{}", item.project, item.event.id);
        if records.contains_key(&id) {
            continue;
        }
        ensure!(
            records.len() < 100000,
            "Desktop delivery ledger is full; inbox retained. Apply notification retention to release expired records"
        );
        let suppress = settings.suppress_active
            && (active == Some((item.project.as_str(), item.event.pane))
                || focused.iter().any(|f| {
                    f.project == item.project
                        && f.pane == item.event.pane
                        && f.epoch == item.event.epoch
                }));
        records.insert(
            id.clone(),
            Attempt {
                at_ms: now_ms(),
                outcome: if suppress {
                    "suppressed_active"
                } else {
                    "indeterminate"
                }
                .into(),
                error: None,
            },
        );
        crate::automation::persist(&path, &serde_json::to_vec(&records)?)?;
        if suppress {
            continue;
        }
        attempted += 1;
        let result = system(
            &format!("STAR/FLUX — {}", item.event.label()),
            &format!(
                "{} · {} · pane {} · {}",
                item.label,
                item.session,
                item.event.pane + 1,
                item.event.source
            ),
        );
        let entry = records.get_mut(&id).unwrap();
        match result {
            Ok(()) => {
                entry.outcome = "submitted".into();
                sent += 1;
            }
            Err(e) => {
                entry.outcome = "failed".into();
                entry.error = Some(format!("{e:#}"));
            }
        }
        crate::automation::persist(&path, &serde_json::to_vec(&records)?)?;
        if attempted >= 3 {
            break;
        }
    }
    Ok(sent)
}

fn system(title: &str, body: &str) -> Result<()> {
    let sanitize = |text: &str, max: usize| {
        text.split_whitespace()
            .flat_map(|s| {
                s.chars()
                    .filter(|c| !c.is_control())
                    .chain(std::iter::once(' '))
            })
            .take(max)
            .collect::<String>()
            .trim()
            .to_owned()
    };
    let title = sanitize(title, 80);
    let body = sanitize(body, 240);
    #[cfg(not(target_os = "macos"))]
    let mut c = {
        let mut c = Command::new("notify-send");
        c.args([
            "--app-name=STAR/FLUX",
            "--icon=dialog-information",
            "--expire-time=8000",
            "--",
            &title,
            &body,
        ]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut c = {
        let mut c = Command::new("/usr/bin/osascript");
        c.args(["-e","on run argv\ndisplay notification (item 2 of argv) with title (item 1 of argv)\nend run","--",&title,&body]);
        c
    };
    let mut child = c
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Start desktop notification helper")?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "Desktop notification helper rejected the request ({status})"
            );
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Desktop notification helper timed out; inbox item retained");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub fn load_query(frontend: &str) -> Result<Query> {
    starkit::terminal_graphics::session::socket_path(&crate::runtime::root()?, frontend)?;
    let path = crate::project_state::data_root()?.join(format!("{frontend}.attention-query.json"));
    if !path.exists() {
        return Ok(Query::default());
    }
    let query: Query = read(&path, 8192)?;
    query.validate()?;
    Ok(query)
}
pub fn save_query(frontend: &str, query: &Query) -> Result<()> {
    starkit::terminal_graphics::session::socket_path(&crate::runtime::root()?, frontend)?;
    query.validate()?;
    crate::automation::persist(
        &crate::project_state::data_root()?.join(format!("{frontend}.attention-query.json")),
        &serde_json::to_vec(query)?,
    )
}
#[derive(Serialize, Deserialize)]
struct Focus {
    project: String,
    pane: usize,
    epoch: Option<String>,
    focused: bool,
    pid: u32,
    start: u64,
    at_ms: u64,
}
pub struct FocusWriter {
    tx: crossbeam_channel::Sender<(String, Focus)>,
    rx: crossbeam_channel::Receiver<(String, Focus)>,
    start: u64,
}
impl FocusWriter {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::bounded::<(String, Focus)>(1);
        let worker = rx.clone();
        std::thread::spawn(move || {
            while let Ok(mut update) = worker.recv() {
                while let Ok(latest) = worker.try_recv() {
                    update = latest;
                }
                let _ = (|| -> Result<()> {
                    let root = crate::runtime::root()?;
                    starkit::terminal_graphics::session::socket_path(&root, &update.0)?;
                    starkit::fs::write_private(
                        &root.join(format!("{}.notification-focus.json", update.0)),
                        &serde_json::to_vec(&update.1)?,
                    )?;
                    Ok(())
                })();
            }
        });
        Self {
            tx,
            rx,
            start: crate::agent_state::process_start(std::process::id()).unwrap_or(0),
        }
    }
    pub fn publish(
        &self,
        frontend: &str,
        config: &Config,
        pane: usize,
        epoch: Option<String>,
        focused: bool,
    ) {
        let payload = (
            frontend.to_owned(),
            Focus {
                project: config.workspace.clone(),
                pane,
                epoch,
                focused,
                pid: std::process::id(),
                start: self.start,
                at_ms: now_ms(),
            },
        );
        if let Err(crossbeam_channel::TrySendError::Full(payload)) = self.tx.try_send(payload) {
            let _ = self.rx.try_recv();
            let _ = self.tx.try_send(payload);
        }
    }
}
fn active_panes() -> Vec<Focus> {
    let now = now_ms();
    let Ok(root) = crate::runtime::root() else {
        return vec![];
    };
    let Ok(entries) = fs::read_dir(root) else {
        return vec![];
    };
    entries
        .flatten()
        .filter(|p| {
            p.file_name()
                .to_string_lossy()
                .ends_with(".notification-focus.json")
        })
        .filter_map(|p| read::<Focus>(&p.path(), 4096).ok())
        .filter(|f| {
            f.focused
                && now >= f.at_ms
                && now - f.at_ms <= 6000
                && crate::agent_state::process_start(f.pid) == Some(f.start)
        })
        .collect()
}
#[derive(Debug)]
pub struct Scan {
    pub unread: Vec<String>,
    pub eligible: Vec<String>,
    pub error: Option<String>,
}
pub fn scan(started_ms: u64) -> Scan {
    let mut error = None;
    let unread = match inbox(&Query::default()) {
        Ok(i) => {
            if !i.warnings.is_empty() {
                error = Some(format!(
                    "Attention data unavailable for {} project(s); inbox counts are partial",
                    i.warnings.len()
                ));
            }
            i.items
        }
        Err(e) => {
            return Scan {
                unread: vec![],
                eligible: vec![],
                error: Some(format!("{e:#}")),
            };
        }
    };
    let mut eligible = vec![];
    match settings() {
        Ok(s) => {
            if let Err(e) = maybe_retention(&s) {
                error = Some(format!("Notification retention: {e:#}"));
            }
            if s.delivery != Delivery::Off {
                eligible = unread
                    .iter()
                    .filter(|i| s.wants(&i.event))
                    .map(|i| format!("{}:{}", i.project, i.event.id))
                    .collect();
            }
            if let Err(e) = deliver(&unread, started_ms, None) {
                error = Some(format!("Desktop delivery: {e:#}"));
            }
        }
        Err(e) => error = Some(format!("Notification settings: {e:#}")),
    }
    Scan {
        unread: unread
            .iter()
            .map(|i| format!("{}:{}", i.project, i.event.id))
            .collect(),
        eligible,
        error,
    }
}

fn maybe_retention(s: &Settings) -> Result<()> {
    let path = delivery_root()?.join("retention.json");
    let _lock = crate::project_sync::Transaction::acquire(path.with_extension("lock"))?;
    let last = if path.exists() {
        read::<Value>(&path, 4096)?
    } else {
        Value::Null
    };
    let now = now_ms();
    if last["revision"] == s.revision
        && last["at_ms"]
            .as_u64()
            .is_some_and(|at| now >= at && now - at < 60000)
    {
        ensure!(
            last["errors"].as_array().is_some_and(|v| v.is_empty()),
            "Some projects could not apply retention; saved data was preserved"
        );
        return Ok(());
    }
    let result = apply_retention()?;
    crate::automation::persist(
        &path,
        &serde_json::to_vec(&json!({"revision":s.revision,"at_ms":now,"errors":result["errors"]}))?,
    )?;
    ensure!(
        result["errors"].as_array().is_some_and(|v| v.is_empty()),
        "Some projects could not apply retention; saved data was preserved"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn event(at: u64, index: u64) -> Attention {
        Attention {
            source: "fixture:provider".into(),
            id: format!("{index:032}"),
            pane: 0,
            epoch: Some("epoch".into()),
            run_id: "run".into(),
            sequence: index,
            state: Lifecycle::Completed,
            observed_ms: at,
            dismissed: false,
        }
    }
    #[test]
    fn age_and_count_retention_keep_newest_including_future_events() {
        let now = 3 * 86400000;
        let policy = Settings {
            retention_days: 1,
            max_items_per_project: 10,
            ..Default::default()
        };
        let mut events = (0..12)
            .map(|i| event(now - 1000 + i, i))
            .collect::<Vec<_>>();
        events.push(event(1, 50));
        events.push(event(now + 86400000, 51));
        assert_eq!(retain(&mut events, &policy, now), 4);
        assert_eq!(events.len(), 10);
        assert!(events.iter().all(|e| e.observed_ms >= now - 86400000));
        assert_eq!(events.last().unwrap().sequence, 51);
        let unlimited_age = Settings {
            retention_days: 0,
            ..policy
        };
        let mut old = vec![event(1, 1), event(now, 2)];
        assert_eq!(retain(&mut old, &unlimited_age, now), 0);
    }
    #[test]
    fn unicode_search_and_reported_source_require_identity() {
        let mut item = Item {
            project: "original".into(),
            label: "Build 界".into(),
            session: "work".into(),
            host: Some("workbox".into()),
            provider: Some("codex".into()),
            conversation_id: Some("CAFÉ-thread".into()),
            event: event(1, 1),
        };
        let q = Query {
            search: "café 界 completed".into(),
            source: Source::Reported,
            ..Default::default()
        };
        assert!(q.matches(&item));
        item.event.dismissed = true;
        assert!(!q.matches(&item));
        let history = Query {
            read: ReadState::Read,
            ..q
        };
        assert!(history.matches(&item));
        item.provider = None;
        item.event.source = "screen:heuristic".into();
        assert!(!history.matches(&item));
        let screen = Query {
            source: Source::Screen,
            ..history
        };
        assert!(screen.matches(&item));
        assert!(
            Query {
                limit: 251,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
