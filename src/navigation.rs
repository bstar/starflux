//! Durable sidebar preferences and observed activity; never stores terminal text.
use crate::{activity::Activity, runtime::Config};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Recent {
    #[serde(default)]
    signals_version: u8,
    #[serde(default)]
    source_sequence: u64,
    #[serde(default)]
    observed_sequence: Option<u64>,
    #[serde(default)]
    at: u64,
    #[serde(default)]
    signals: BTreeMap<String, String>,
}
#[derive(Serialize, Deserialize)]
struct SessionOrder {
    manual_order: Vec<String>,
    recent: bool,
    high_first: bool,
}
fn merge_visible_order(
    previous: &[String],
    visible: &[String],
    present: &BTreeSet<String>,
) -> Vec<String> {
    let visible_set = visible.iter().collect::<BTreeSet<_>>();
    let mut ordered = visible.iter();
    let mut merged = Vec::new();
    let mut seen = BTreeSet::new();
    for old in previous.iter().filter(|id| present.contains(*id)) {
        let next = if visible_set.contains(old) {
            ordered.next().unwrap_or(old)
        } else {
            old
        };
        if seen.insert(next.clone()) {
            merged.push(next.clone());
        }
    }
    for remaining in ordered {
        if seen.insert(remaining.clone()) {
            merged.push(remaining.clone());
        }
    }
    merged
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Navigation {
    pub recent: bool,
    pub high_first: bool,
    pub audio_muted: bool,
    pub manual_order: Vec<String>,
    pub closed_projects: Vec<String>,
    history: BTreeMap<String, Recent>,
    #[serde(skip)]
    pub dirty: bool,
    #[serde(skip)]
    priorities: BTreeMap<String, u8>,
    #[serde(skip)]
    presentation_until: BTreeMap<String, u64>,
}
impl Default for Navigation {
    fn default() -> Self {
        Self {
            recent: false,
            high_first: true,
            audio_muted: false,
            manual_order: vec![],
            closed_projects: vec![],
            history: BTreeMap::new(),
            dirty: false,
            priorities: BTreeMap::new(),
            presentation_until: BTreeMap::new(),
        }
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl Navigation {
    pub fn load_session_order(&mut self, config: &Config) -> Result<()> {
        let path = crate::session_catalog::order_path(config)?;
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let order: SessionOrder = serde_json::from_slice(&bytes)?;
        self.manual_order = order.manual_order;
        self.recent = order.recent;
        self.high_first = order.high_first;
        Ok(())
    }
    pub fn save_session_order(&self, config: &Config) -> Result<()> {
        let path = crate::session_catalog::order_path(config)?;
        let _lock = crate::project_sync::Transaction::acquire(path.with_extension("lock"))?;
        let previous = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SessionOrder>(&bytes).ok())
            .map(|v| v.manual_order)
            .unwrap_or_default();
        let present = crate::project_state::catalog(config)
            .into_iter()
            .map(|p| p.workspace)
            .collect::<BTreeSet<_>>();
        let order = SessionOrder {
            manual_order: merge_visible_order(&previous, &self.manual_order, &present),
            recent: self.recent,
            high_first: self.high_first,
        };
        starkit::fs::write_private(&path, &serde_json::to_vec_pretty(&order)?)?;
        crate::project_state::catalog_changed()
    }
    pub fn load(controller: &str) -> Result<Self> {
        let path = crate::project_state::data_root()?.join(format!("{controller}.navigation.json"));
        if !path.exists() {
            return Ok(Self::default());
        }
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        let migrate = value.get("manual_order").is_none();
        let mut navigation: Self = serde_json::from_value(value)?;
        if migrate {
            navigation.recent = false;
            navigation.dirty = true;
        }
        Ok(navigation)
    }
    pub fn save(&mut self, controller: &str) -> Result<()> {
        let path = crate::project_state::data_root()?.join(format!("{controller}.navigation.json"));
        starkit::fs::write_private(&path, &serde_json::to_vec_pretty(self)?)?;
        self.dirty = false;
        Ok(())
    }
    /// Reconnecting a view can resize/redraw a terminal without new work.
    /// Baseline through settling and the first complete observation afterward.
    pub fn presentation_changed(&mut self, project: &str, at: u64) {
        self.presentation_until.insert(project.into(), at + 1000);
    }
    pub fn filter_open(&self, projects: &mut Vec<Config>) {
        projects.retain(|project| !self.closed_projects.contains(&project.workspace));
    }
    pub fn open_project(&mut self, id: &str) {
        let before = self.closed_projects.len();
        self.closed_projects.retain(|closed| closed != id);
        self.dirty |= before != self.closed_projects.len();
    }
    pub fn close_project(&mut self, id: &str) {
        if !self.closed_projects.iter().any(|closed| closed == id) {
            self.closed_projects.push(id.into());
            self.dirty = true;
        }
    }
    pub fn reconcile(&mut self, projects: &[Config]) {
        let present: BTreeSet<_> = projects.iter().map(|p| p.workspace.as_str()).collect();
        let previous = self.manual_order.clone();
        let mut seen = BTreeSet::new();
        self.manual_order
            .retain(|id| present.contains(id.as_str()) && seen.insert(id.clone()));
        for project in projects {
            if seen.insert(project.workspace.clone()) {
                self.manual_order.push(project.workspace.clone());
            }
        }
        self.dirty |= previous != self.manual_order;
    }
    /// Insert at a gap in the displayed manual list, retaining every project.
    pub fn reorder(&mut self, id: &str, gap: usize) -> bool {
        let Some(from) = self.manual_order.iter().position(|p| p == id) else {
            return false;
        };
        let target = gap
            .min(self.manual_order.len())
            .saturating_sub(usize::from(gap > from));
        if target == from {
            return false;
        }
        let id = self.manual_order.remove(from);
        self.manual_order.insert(target, id);
        self.dirty = true;
        true
    }
    pub fn touch(&mut self, project: &str, at: u64) {
        let recent = self.history.entry(project.into()).or_default();
        recent.at = recent.at.max(at);
        self.dirty = true;
    }
    pub fn observe(&mut self, activity: &BTreeMap<String, Activity>, at: u64) {
        for (project, activity) in activity {
            self.priorities.insert(
                project.clone(),
                if activity.working > 0 {
                    3
                } else if activity.waiting > 0 {
                    2
                } else if activity.running > 0 {
                    1
                } else {
                    0
                },
            );
            // A failed/disconnected inspection is not new project activity.
            if activity.signals.is_empty() {
                continue;
            }
            let presentation = self.presentation_until.get(project).copied();
            if presentation.is_some_and(|until| at > until) {
                self.presentation_until.remove(project);
            }
            let recent = self.history.entry(project.clone()).or_default();
            if activity.source_sequence > 0
                && activity.source_sequence
                    != recent.observed_sequence.unwrap_or(recent.source_sequence)
            {
                if recent.source_sequence > 0 && presentation.is_none() {
                    recent.at = recent.at.max(at);
                }
                // Historical rank is only a seed. Presentation-driven backend
                // sequence changes must not silently reorder un-timestamped rows.
                if recent.source_sequence == 0 {
                    recent.source_sequence = activity.source_sequence;
                }
                recent.observed_sequence = Some(activity.source_sequence);
                self.dirty = true;
            }
            if recent.signals == activity.signals {
                continue;
            }
            // The first sample establishes a baseline, not fabricated history.
            if presentation.is_none()
                && recent.signals_version == 1
                && activity
                    .signals
                    .iter()
                    .any(|(pane, signal)| recent.signals.get(pane).is_some_and(|old| old != signal))
            {
                recent.at = recent.at.max(at);
            }
            recent.signals = activity.signals.clone();
            recent.signals_version = 1;
            self.dirty = true;
        }
    }
    pub fn last_activity(&self, project: &str) -> Option<u64> {
        self.history.get(project).map(|r| r.at).filter(|at| *at > 0)
    }
    fn high_compare(&self, a: &str, b: &str) -> Ordering {
        let priority = self
            .priorities
            .get(b)
            .unwrap_or(&0)
            .cmp(self.priorities.get(a).unwrap_or(&0));
        if priority != Ordering::Equal {
            return priority;
        }
        if !self.recent {
            return Ordering::Equal;
        }
        let key = |id: &str| {
            self.history.get(id).and_then(|r| {
                if r.at > 0 {
                    Some((1, r.at))
                } else if r.source_sequence > 0 {
                    Some((0, r.source_sequence))
                } else {
                    None
                }
            })
        };
        match (key(a), key(b)) {
            (Some(a), Some(b)) => b.cmp(&a),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        let order = self.high_compare(a, b);
        if self.high_first {
            order
        } else {
            order.reverse()
        }
    }
    pub fn sort(&mut self, projects: &mut [Config]) {
        self.reconcile(projects);
        if !self.recent {
            let positions: BTreeMap<_, _> = self
                .manual_order
                .iter()
                .enumerate()
                .map(|(i, id)| (id.as_str(), i))
                .collect();
            projects.sort_by_key(|p| {
                positions
                    .get(p.workspace.as_str())
                    .copied()
                    .unwrap_or(usize::MAX)
            });
            return;
        }
        projects.sort_by(|a, b| {
            let order = self.high_compare(&a.workspace, &b.workspace).then_with(|| {
                a.herdr
                    .as_ref()
                    .map(|h| h.order)
                    .unwrap_or(usize::MAX)
                    .cmp(&b.herdr.as_ref().map(|h| h.order).unwrap_or(usize::MAX))
                    .then_with(|| a.workspace.cmp(&b.workspace))
            });
            if self.high_first {
                order
            } else {
                order.reverse()
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_order_preserves_hidden_projects_and_inserts_new_projects() {
        let values = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let present = values(&["a", "hidden", "b", "c", "new"])
            .into_iter()
            .collect();
        assert_eq!(
            merge_visible_order(
                &values(&["a", "hidden", "b", "c"]),
                &values(&["c", "b", "a"]),
                &present
            ),
            values(&["c", "hidden", "b", "a"])
        );
        assert_eq!(
            merge_visible_order(
                &values(&["a", "hidden", "b"]),
                &values(&["new", "b", "a"]),
                &present
            ),
            values(&["new", "hidden", "b", "a"])
        );
    }
    fn projects(ids: &[&str]) -> Vec<Config> {
        ids.iter().map(|id| serde_json::from_value(serde_json::json!({"workspace":id,"host":null,"directory":null,"agent":null,"executable":"starflux"})).unwrap()).collect()
    }
    #[test]
    fn closed_projects_stay_closed_across_catalog_reload_and_can_reopen() {
        let mut nav = Navigation::default();
        nav.close_project("b");
        let mut restored: Navigation =
            serde_json::from_value(serde_json::to_value(&nav).unwrap()).unwrap();
        let mut items = projects(&["a", "b", "c"]);
        restored.filter_open(&mut items);
        restored.sort(&mut items);
        assert_eq!(
            items
                .iter()
                .map(|p| p.workspace.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        restored.open_project("b");
        let mut items = projects(&["a", "b", "c"]);
        restored.filter_open(&mut items);
        assert_eq!(items.len(), 3);
    }
    #[test]
    fn manual_order_survives_activity_reload_and_catalog_changes() {
        let mut nav = Navigation::default();
        let mut items = projects(&["a", "b", "c", "d"]);
        nav.sort(&mut items);
        assert!(nav.reorder("d", 0));
        assert_eq!(nav.manual_order, ["d", "a", "b", "c"]);
        assert!(nav.reorder("a", 4));
        assert_eq!(nav.manual_order, ["d", "b", "c", "a"]);
        assert!(!nav.reorder("b", 2));
        nav.touch("a", 100);
        nav.sort(&mut items);
        assert_eq!(
            items
                .iter()
                .map(|p| p.workspace.as_str())
                .collect::<Vec<_>>(),
            ["d", "b", "c", "a"]
        );
        nav.recent = true;
        nav.sort(&mut items);
        assert_eq!(items[0].workspace, "a");
        nav.recent = false;
        let mut nav: Navigation =
            serde_json::from_slice(&serde_json::to_vec(&nav).unwrap()).unwrap();
        let mut items = projects(&["new", "a", "b", "d"]);
        nav.sort(&mut items);
        assert_eq!(nav.manual_order, ["d", "b", "a", "new"]);
        assert_eq!(items[0].workspace, "d");
    }
    #[test]
    fn selection_does_not_change_historical_source_rank() {
        let mut nav = Navigation {
            recent: true,
            ..Default::default()
        };
        let mut samples = BTreeMap::new();
        for (id, seq) in [("a", 1), ("b", 2)] {
            samples.insert(
                id.into(),
                Activity {
                    source_sequence: seq,
                    signals: BTreeMap::from([("pane".into(), "baseline".into())]),
                    ..Default::default()
                },
            );
        }
        nav.observe(&samples, 100);
        nav.presentation_changed("a", 200);
        samples.get_mut("a").unwrap().source_sequence = 10;
        nav.observe(&samples, 500);
        nav.observe(&samples, 1500);
        nav.observe(&samples, 1600);
        assert_eq!(nav.last_activity("a"), None);
        assert_eq!(nav.compare("a", "b"), Ordering::Greater);
        samples.get_mut("a").unwrap().source_sequence = 11;
        nav.observe(&samples, 1700);
        assert_eq!(nav.compare("a", "b"), Ordering::Less);
    }

    #[test]
    fn selecting_and_discovering_panes_do_not_become_work_activity() {
        let mut nav = Navigation::default();
        let mut samples = BTreeMap::from([(
            "a".into(),
            Activity {
                source_sequence: 1,
                signals: BTreeMap::from([("pane".into(), "old".into())]),
                ..Default::default()
            },
        )]);
        nav.observe(&samples, 100);
        nav.touch("a", 150);
        nav.presentation_changed("a", 200);
        samples
            .get_mut("a")
            .unwrap()
            .signals
            .insert("pane".into(), "attachment redraw".into());
        samples.get_mut("a").unwrap().source_sequence = 2;
        nav.observe(&samples, 500);
        samples
            .get_mut("a")
            .unwrap()
            .signals
            .insert("pane".into(), "settled resize".into());
        nav.observe(&samples, 1500);
        assert_eq!(nav.last_activity("a"), Some(150));
        samples
            .get_mut("a")
            .unwrap()
            .signals
            .insert("new-pane".into(), "initial".into());
        nav.observe(&samples, 1600);
        assert_eq!(nav.last_activity("a"), Some(150));
        samples
            .get_mut("a")
            .unwrap()
            .signals
            .insert("pane".into(), "real output".into());
        nav.observe(&samples, 1700);
        assert_eq!(nav.last_activity("a"), Some(1700));
    }

    #[test]
    fn active_projects_sort_on_first_sample_and_low_reverses_the_entire_high_order() {
        let mut nav = Navigation {
            recent: true,
            ..Default::default()
        };
        let mut samples = BTreeMap::new();
        for (id, status, sequence) in [
            ("busy-old", "working", 1),
            ("busy-new", "working", 2),
            ("dec", "needs_permission", 3),
            ("idle", "idle", 100),
        ] {
            let mut activity = Activity {
                source_sequence: sequence,
                ..Default::default()
            };
            activity.observe(status);
            activity.signals.insert("pane".into(), "unchanged".into());
            samples.insert(id.into(), activity);
        }
        nav.observe(&samples, 1000);
        assert_eq!(nav.last_activity("busy-old"), None);
        assert_eq!(nav.compare("busy-new", "busy-old"), Ordering::Less);
        assert_eq!(nav.compare("busy-old", "dec"), Ordering::Less);
        assert_eq!(nav.compare("dec", "idle"), Ordering::Less);
        let mut projects: Vec<Config> = ["busy-old","busy-new","dec","idle","unknown-a","unknown-b"].into_iter().map(|id|
            serde_json::from_value(serde_json::json!({"workspace":id,"host":null,"directory":null,"agent":null,"executable":"starflux"})).unwrap()).collect();
        nav.sort(&mut projects);
        let mut high: Vec<_> = projects.iter().map(|p| p.workspace.clone()).collect();
        nav.high_first = false;
        nav.sort(&mut projects);
        high.reverse();
        assert_eq!(
            projects
                .iter()
                .map(|p| p.workspace.clone())
                .collect::<Vec<_>>(),
            high
        );
        assert_eq!(projects.last().unwrap().workspace, "busy-new");
    }
    #[test]
    fn recent_order_tracks_changes_not_polling_or_busy_counts_and_survives_reload() {
        let mut nav = Navigation {
            recent: true,
            ..Default::default()
        };
        let mut samples = BTreeMap::from([("project".into(), Activity::default())]);
        samples
            .get_mut("project")
            .unwrap()
            .signals
            .insert("pane".into(), "revision-1".into());
        nav.observe(&samples, 100);
        assert_eq!(nav.last_activity("project"), None);
        nav.observe(&samples, 200);
        samples.get_mut("project").unwrap().working = 5;
        nav.observe(&samples, 300);
        assert_eq!(nav.last_activity("project"), None);
        samples
            .get_mut("project")
            .unwrap()
            .signals
            .insert("pane".into(), "revision-2".into());
        nav.observe(&samples, 400);
        assert_eq!(nav.last_activity("project"), Some(400));
        samples.get_mut("project").unwrap().working = 0;
        nav.observe(&samples, 400);
        nav.touch("other", 500);
        assert_eq!(nav.compare("other", "project"), Ordering::Less);
        assert_eq!(nav.compare("missing", "project"), Ordering::Greater);
        nav.high_first = false;
        assert_eq!(nav.compare("other", "project"), Ordering::Greater);
        assert_eq!(nav.compare("missing", "project"), Ordering::Less);
        let mut restored: Navigation =
            serde_json::from_slice(&serde_json::to_vec(&nav).unwrap()).unwrap();
        restored.observe(&samples, 900);
        assert_eq!(restored.last_activity("project"), Some(400));
        assert!(!restored.high_first && restored.recent);
        restored.recent = false;
        assert_eq!(restored.compare("other", "project"), Ordering::Equal);
    }
}
