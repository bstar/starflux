//! Bounded, private domain change journal shared by producers and subscriptions.
use super::*;
use std::collections::{BTreeMap, VecDeque};
const EVENTS: usize = 2048;
const BYTES: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Change {
    pub sequence: u64,
    pub kind: String,
    pub resource: String,
    pub source: String,
    pub data: Value,
    #[serde(default)]
    pub emitted_ms: u64,
}
#[derive(Deserialize, Serialize)]
struct Journal {
    id: String,
    sequence: u64,
    bytes: usize,
    events: VecDeque<Change>,
    latest: BTreeMap<String, Value>,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            sequence: 0,
            bytes: 0,
            events: VecDeque::new(),
            latest: BTreeMap::new(),
        }
    }
}
pub(super) fn root() -> Result<PathBuf> {
    let path = crate::project_state::data_root()?
        .parent()
        .context("No data directory")?
        .join("automation");
    private_root(&path)?;
    Ok(path)
}
fn load(path: &std::path::Path) -> Result<Journal> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("Read automation event journal"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Journal::default()),
        Err(e) => Err(e.into()),
    }
}
fn entity(resource: &str) -> &'static str {
    match resource.split('/').next().unwrap_or("") {
        "sessions" => "session",
        "projects" => "project",
        "panes" => "pane",
        "agents" => "agent.reported",
        _ => "attention",
    }
}
impl Journal {
    fn append(&mut self, key: &str, data: Value, change: &str, source: &str) -> Result<()> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .context("Event sequence exhausted")?;
        let event = Change {
            sequence: self.sequence,
            kind: format!("{}.{change}", entity(key)),
            resource: key.into(),
            source: source.into(),
            data,
            emitted_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |t| t.as_millis() as u64),
        };
        self.bytes += serde_json::to_vec(&event)?.len();
        self.events.push_back(event);
        while self.events.len() > EVENTS || self.bytes > BYTES {
            let Some(event) = self.events.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(serde_json::to_vec(&event)?.len());
        }
        Ok(())
    }
    fn update(
        &mut self,
        current: BTreeMap<String, Value>,
        scope: impl Fn(&str) -> bool,
        source: &str,
    ) -> Result<bool> {
        let before = self.sequence;
        for (key, value) in &current {
            if self.latest.get(key) == Some(value) {
                continue;
            }
            self.append(
                key,
                value.clone(),
                if self.latest.contains_key(key) {
                    "changed"
                } else {
                    "created"
                },
                source,
            )?;
            self.latest.insert(key.clone(), value.clone());
        }
        let removed = self
            .latest
            .keys()
            .filter(|key| scope(key) && !current.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>();
        for key in removed {
            self.append(&key, Value::Null, "removed", source)?;
            self.latest.remove(&key);
        }
        Ok(self.sequence != before)
    }
    fn cursor(&self) -> String {
        format!("{}:{}", self.id, self.sequence)
    }
    fn changes(&self, cursor: &str) -> Result<Vec<Change>> {
        let (id, sequence) = cursor.rsplit_once(':').ok_or_else(|| {
            fault(
                "invalid_cursor",
                "Cursor must contain a journal identity and sequence",
            )
        })?;
        let sequence: u64 = sequence
            .parse()
            .map_err(|_| fault("invalid_cursor", "Invalid event sequence"))?;
        if id != self.id {
            return Err(fault(
                "cursor_expired",
                "The event journal changed; subscribe without a cursor for a fresh snapshot",
            ));
        }
        if sequence > self.sequence {
            return Err(fault(
                "invalid_cursor",
                "Cursor is ahead of the event journal",
            ));
        }
        if self.events.front().map_or(sequence < self.sequence, |e| {
            sequence < e.sequence.saturating_sub(1)
        }) {
            return Err(fault(
                "cursor_expired",
                "Events preceding the retention boundary were removed; subscribe for a fresh snapshot",
            ));
        }
        Ok(self
            .events
            .iter()
            .filter(|event| event.sequence > sequence)
            .take(64)
            .cloned()
            .collect())
    }
}
fn keyed(value: &Value, resources: &[String]) -> BTreeMap<String, Value> {
    events::keyed(value, resources)
        .into_iter()
        .map(|(key, (_, value))| (key, value))
        .collect()
}
fn update(value: &Value, resources: &[&str], project: &str) -> Result<()> {
    let root = root()?;
    let _lock = project_sync::Transaction::acquire(root.join("events.lock"))?;
    let path = root.join("events.json");
    let mut journal = load(&path)?;
    let selected = resources.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if journal.update(
        keyed(value, &selected),
        |key| {
            key.split('/').nth(1) == Some(project)
                && key
                    .split('/')
                    .next()
                    .is_some_and(|resource| resources.contains(&resource))
        },
        "domain",
    )? {
        persist(&path, &serde_json::to_vec(&journal)?)?;
    }
    Ok(())
}
pub(crate) fn project_saved(config: &Config, state: &ProjectState) -> Result<()> {
    if crate::worktrees::removed(config) {
        return Ok(());
    }
    let panes = state
        .terminal_ids()
        .into_iter()
        .filter(|id| config.canonical_pane(*id) == *id)
        .map(|id| pane_resource(config, state, id))
        .collect::<Vec<_>>();
    update(
        &json!({"projects":[project_resource(config)],"panes":panes}),
        &["projects", "panes"],
        &config.workspace,
    )
}
pub(crate) fn agents_saved<'a>(
    config: &Config,
    records: impl Iterator<Item = &'a agent_state::Record>,
    items: impl Iterator<Item = &'a agent_state::Attention>,
) -> Result<()> {
    update(
        &json!({"agents":records.map(|r|agent_resource(config,r)).collect::<Vec<_>>(),"attention":items.map(|a|attention_resource(config,a)).collect::<Vec<_>>()}),
        &["agents", "attention"],
        &config.workspace,
    )
}
pub(crate) fn snapshot() -> Result<(Value, String)> {
    let root = root()?;
    let _lock = project_sync::Transaction::acquire(root.join("events.lock"))?;
    let path = root.join("events.json");
    let mut journal = load(&path)?;
    let mut metadata = metadata(None)?;
    let sessions_available = match crate::session_catalog::list() {
        Ok(sessions) => {
            metadata["sessions"] = serde_json::to_value(sessions)?;
            true
        }
        Err(error) => {
            metadata["warnings"]
                .as_array_mut()
                .unwrap()
                .push(json!({"source":"sessions","error":format!("{error:#}")}));
            false
        }
    };
    let warnings = metadata["warnings"].as_array().unwrap();
    let resources = ["projects", "panes", "agents", "attention", "sessions"].map(str::to_owned);
    let changed = journal.update(
        keyed(&metadata, &resources),
        |key| {
            if key.starts_with("sessions/") {
                return sessions_available;
            }
            !warnings
                .iter()
                .any(|w| w["project"].as_str() == key.split('/').nth(1))
        },
        "reconciled",
    )?;
    if changed || !path.exists() {
        persist(&path, &serde_json::to_vec(&journal)?)?;
    }
    Ok((metadata, journal.cursor()))
}
pub(crate) fn sessions_saved() -> Result<()> {
    let root = root()?;
    let _lock = project_sync::Transaction::acquire(root.join("events.lock"))?;
    let path = root.join("events.json");
    let mut journal = load(&path)?;
    let current = json!({"sessions":crate::session_catalog::list()?});
    if journal.update(
        keyed(&current, &["sessions".into()]),
        |key| key.starts_with("sessions/"),
        "domain",
    )? {
        persist(&path, &serde_json::to_vec(&journal)?)?;
    }
    Ok(())
}
/// Journal a discrete occurrence (`pane.moved`, `pane.focused`,
/// `pane.exited`, `project.focused`) for hooks and subscribers. Unlike
/// resource changes it is not part of the state snapshot, so it is never
/// reported as created or removed.
pub(crate) fn occurred(resource: &str, change: &str, data: Value) -> Result<()> {
    let root = root()?;
    let _lock = project_sync::Transaction::acquire(root.join("events.lock"))?;
    let path = root.join("events.json");
    let mut journal = load(&path)?;
    journal.append(resource, data, change, "domain")?;
    persist(&path, &serde_json::to_vec(&journal)?)
}
/// [`occurred`] off the caller's thread; a frontend must not wait on the
/// journal lock, and a lost occurrence only means a hook does not run.
pub(crate) fn occurred_later(resource: String, change: &'static str, data: Value) {
    std::thread::spawn(move || {
        let _ = occurred(&resource, change, data);
    });
}
pub(crate) fn read(cursor: &str) -> Result<Vec<Change>> {
    let root = root()?;
    let _lock = project_sync::Transaction::acquire(root.join("events.lock"))?;
    load(&root.join("events.json"))?.changes(cursor)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occurrences_are_journaled_without_entering_resource_state() {
        let mut j = Journal::default();
        let cursor = j.cursor();
        let pane = json!({"project":"p","pane":1});
        j.update(
            [("panes/p/1".into(), pane.clone())].into_iter().collect(),
            |_| true,
            "domain",
        )
        .unwrap();
        for change in ["focused", "exited", "moved"] {
            j.append("panes/p/1", pane.clone(), change, "domain")
                .unwrap();
        }
        j.append("projects/p", json!({"id":"p"}), "focused", "domain")
            .unwrap();
        let kinds = j
            .changes(&cursor)
            .unwrap()
            .into_iter()
            .map(|c| c.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                "pane.created",
                "pane.focused",
                "pane.exited",
                "pane.moved",
                "project.focused"
            ]
        );
        // The pane's resource state is unchanged, so the next update is silent.
        assert!(
            !j.update(
                [("panes/p/1".into(), pane)].into_iter().collect(),
                |_| true,
                "domain",
            )
            .unwrap()
        );
    }
    #[test]
    fn transitions_survive_rapid_updates_and_retention_is_explicit() {
        let mut j = Journal::default();
        let beginning = j.cursor();
        for state in [
            "working",
            "completed",
            "working",
            "needs_input",
            "completed",
        ] {
            j.update(
                [("agents/p/r".into(), json!({"state":state}))]
                    .into_iter()
                    .collect(),
                |_| false,
                "domain",
            )
            .unwrap();
        }
        assert_eq!(j.changes(&beginning).unwrap().len(), 5);
        let persisted: Journal = serde_json::from_slice(&serde_json::to_vec(&j).unwrap()).unwrap();
        assert_eq!(persisted.cursor(), j.cursor());
        for n in 0..EVENTS {
            j.append("panes/p/1", json!(n), "changed", "domain")
                .unwrap();
        }
        assert!(j.events.len() <= EVENTS && j.bytes <= BYTES);
        assert_eq!(
            j.changes(&beginning)
                .unwrap_err()
                .downcast_ref::<Fault>()
                .unwrap()
                .code,
            "cursor_expired"
        );
        let last = j.cursor();
        assert!(j.changes(&last).unwrap().is_empty());
        let mut oversized = Journal::default();
        let cursor = oversized.cursor();
        oversized
            .append(
                "projects/p",
                json!("x".repeat(BYTES + 1)),
                "changed",
                "domain",
            )
            .unwrap();
        assert!(oversized.events.is_empty());
        assert_eq!(
            oversized
                .changes(&cursor)
                .unwrap_err()
                .downcast_ref::<Fault>()
                .unwrap()
                .code,
            "cursor_expired"
        );
    }
}
