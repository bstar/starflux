//! Portfolio health: disk usage of member projects and their agents, with
//! runaway-growth alerts. Measurement only: STAR/FLUX never deletes anything.
//! The coordinator turns alerts into cleanup proposals for the owning
//! project's agent, which run only after user approval.
//!
//! A cheap free-space check runs about once a minute and a bounded scan of each
//! local project about every 15 minutes, from whichever frontend is open. A
//! private lock keeps concurrent frontends from scanning twice.
use crate::runtime::Config;
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const GIB: u64 = 1 << 30;
const CHECK_EVERY_MS: u64 = 60_000;
const SCAN_EVERY_MS: u64 = 15 * 60_000;
const SCAN_BUDGET: Duration = Duration::from_secs(20);
const SCAN_ENTRIES: usize = 2_000_000;
const LARGEST: usize = 8;
/// A single file growing this much between scans is a runaway.
const RUNAWAY_GROWTH: u64 = GIB;
const HISTORY: usize = 90;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileUsage {
    pub path: String,
    pub bytes: u64,
    pub category: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectUsage {
    pub project: String,
    pub label: String,
    pub directory: Option<String>,
    pub host: Option<String>,
    pub scanned_ms: u64,
    /// Allocated bytes (sparse images count only written blocks).
    pub total: u64,
    pub categories: BTreeMap<String, u64>,
    pub largest: Vec<FileUsage>,
    pub complete: bool,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Filesystem {
    pub path: String,
    pub total: u64,
    pub free: u64,
    /// `[time_ms, free_bytes]`, oldest first.
    pub history: Vec<[u64; 2]>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Alert {
    pub severity: String,
    pub kind: String,
    pub project: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Health {
    pub version: u32,
    pub checked_ms: u64,
    pub scanned_ms: u64,
    pub filesystems: BTreeMap<String, Filesystem>,
    pub projects: Vec<ProjectUsage>,
    /// Agent storage and context measurements (see `agent_context`).
    pub agents: Vec<crate::agent_context::AgentUsage>,
    pub agent_storage: Option<FileUsage>,
    pub alerts: Vec<Alert>,
}

fn now_ms() -> u64 {
    crate::category_agent_runtime::now()
}

fn path(config: &Config, category: &str) -> Result<PathBuf> {
    Ok(crate::category_agent::storage_path(config, category)?.join("health.json"))
}

pub fn load(config: &Config, category: &str) -> Health {
    path(config, category)
        .ok()
        .filter(|p| p.exists())
        .and_then(|p| crate::plugin_store::read(&p, 8 * 1024 * 1024).ok())
        .unwrap_or_default()
}

fn save(config: &Config, category: &str, health: &Health) -> Result<()> {
    let root = crate::category_agent::storage_path(config, category)?;
    starcore::terminal_graphics::session::private_root(&root)?;
    crate::automation::persist(&path(config, category)?, &serde_json::to_vec(health)?)
}

pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes >= 1 << 40 {
        format!("{:.1} TB", b / (1u64 << 40) as f64)
    } else if bytes >= GIB {
        format!("{:.1} GB", b / GIB as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.0} MB", b / (1u64 << 20) as f64)
    } else {
        format!("{:.0} KB", b / 1024.0)
    }
}

/// Classify a directory by name; `None` keeps the parent's category.
fn directory_category(name: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    Some(match name.as_str() {
        ".git" => "git",
        "target" | "build" | "dist" | "out" | "node_modules" | "intermediate"
        | "deriveddatacache" | "binaries" | ".gradle" | "__pycache__" | ".venv" | "venv"
        | ".next" | ".cache" | "zig-cache" | ".zig-cache" | ".build" | "obj" | "result" => "build",
        "logs" | "log" | "traces" => "logs",
        "artifacts" | "output" | "outputs" | "reports" | "snapshots" | "checkpoints" => "artifacts",
        "images" => "images",
        _ if name.starts_with("cmake-build-") => "build",
        _ => return None,
    })
}

fn file_category<'a>(name: &str, inherited: &'a str) -> &'a str {
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "img" | "iso" | "qcow2" | "vdi" | "vmdk" | "vhd" | "vhdx" if inherited != "git" => "images",
        "log" if inherited == "other" => "logs",
        _ => inherited,
    }
}

struct Scan {
    total: u64,
    categories: BTreeMap<String, u64>,
    largest: Vec<FileUsage>,
    complete: bool,
}

/// Walk one directory tree without following links or leaving its filesystem.
fn scan(root: &Path, budget: Duration) -> Result<Scan> {
    let began = Instant::now();
    let device = std::fs::symlink_metadata(root)
        .with_context(|| format!("Read {}", root.display()))?
        .dev();
    let mut out = Scan {
        total: 0,
        categories: BTreeMap::new(),
        largest: Vec::new(),
        complete: true,
    };
    let mut stack = vec![(root.to_path_buf(), "other")];
    let mut entries = 0usize;
    while let Some((directory, category)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in read.flatten() {
            entries += 1;
            if entries > SCAN_ENTRIES || began.elapsed() > budget {
                out.complete = false;
                stack.clear();
                break;
            }
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            if meta.dev() != device {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let bytes = meta.blocks() * 512;
            if meta.is_dir() {
                stack.push((entry.path(), directory_category(&name).unwrap_or(category)));
                continue;
            }
            let file_category = file_category(&name, category);
            out.total += bytes;
            *out.categories.entry(file_category.into()).or_default() += bytes;
            if out.largest.len() < LARGEST || out.largest.last().is_some_and(|f| f.bytes < bytes) {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or(name);
                out.largest.push(FileUsage {
                    path: relative,
                    bytes,
                    category: file_category.into(),
                });
                out.largest.sort_by_key(|f| std::cmp::Reverse(f.bytes));
                out.largest.truncate(LARGEST);
            }
        }
    }
    Ok(out)
}

/// Allocated size of a directory tree (bounded; `None` if unreadable).
pub fn directory_bytes(path: &Path) -> Option<u64> {
    scan(path, SCAN_BUDGET).ok().map(|s| s.total)
}

fn statvfs(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| {
        let unit = s.f_frsize as u64;
        (s.f_blocks as u64 * unit, s.f_bavail as u64 * unit)
    })
}

fn local_directories(config: &Config, category: &str) -> Vec<Config> {
    crate::category_agent::members(config, category)
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.host.is_none() && m.directory.is_some())
        .collect()
}

/// Update free-space history; returns the filesystems' keys and paths.
fn check_filesystems(config: &Config, category: &str, health: &mut Health) {
    let now = now_ms();
    let mut seen = Vec::new();
    for member in local_directories(config, category) {
        let directory = PathBuf::from(member.directory.unwrap_or_default());
        let Ok(meta) = std::fs::metadata(&directory) else {
            continue;
        };
        let key = meta.dev().to_string();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key.clone());
        let Some((total, free)) = statvfs(&directory) else {
            continue;
        };
        let fs = health.filesystems.entry(key).or_default();
        fs.path = directory.to_string_lossy().into_owned();
        fs.total = total;
        fs.free = free;
        fs.history.push([now, free]);
        if fs.history.len() > HISTORY {
            fs.history.drain(..fs.history.len() - HISTORY);
        }
    }
    health.filesystems.retain(|k, _| seen.contains(k));
    health.checked_ms = now;
}

/// Free-space trend: bytes lost per hour over the last 30 minutes.
pub fn loss_per_hour(fs: &Filesystem) -> Option<f64> {
    let newest = fs.history.last()?;
    let oldest = fs
        .history
        .iter()
        .find(|[t, _]| newest[0].saturating_sub(*t) <= 30 * 60_000)?;
    let elapsed = newest[0]
        .checked_sub(oldest[0])
        .filter(|e| *e >= 5 * 60_000)?;
    Some((oldest[1] as f64 - newest[1] as f64) * 3_600_000.0 / elapsed as f64)
}

pub fn alerts(health: &Health, previous: &[ProjectUsage]) -> Vec<Alert> {
    let mut out = Vec::new();
    let alert = |severity: &str, kind: &str, project: Option<&str>, message: String| Alert {
        severity: severity.into(),
        kind: kind.into(),
        project: project.map(String::from),
        message,
    };
    for fs in health.filesystems.values() {
        let percent = (fs.free * 100).checked_div(fs.total).unwrap_or(100);
        if fs.free < 20 * GIB || percent < 5 {
            out.push(alert(
                "critical",
                "low_space",
                None,
                format!(
                    "Only {} free ({percent}%) on the drive holding {}",
                    human(fs.free),
                    fs.path
                ),
            ));
        } else if percent < 10 {
            out.push(alert(
                "warning",
                "low_space",
                None,
                format!(
                    "{} free ({percent}%) on the drive holding {}",
                    human(fs.free),
                    fs.path
                ),
            ));
        }
        if let Some(loss) = loss_per_hour(fs)
            && loss > GIB as f64
        {
            let hours = fs.free as f64 / loss;
            if hours < 24.0 {
                out.push(alert(
                    "critical",
                    "filling_fast",
                    None,
                    format!(
                        "Drive holding {} is losing {}/hour and will be full in about {:.0} hours",
                        fs.path,
                        human(loss as u64),
                        hours.max(1.0)
                    ),
                ));
            }
        }
    }
    for project in &health.projects {
        let Some(before) = previous.iter().find(|p| p.project == project.project) else {
            continue;
        };
        let elapsed = project.scanned_ms.saturating_sub(before.scanned_ms).max(1);
        for file in &project.largest {
            let old = before
                .largest
                .iter()
                .find(|f| f.path == file.path)
                .map(|f| f.bytes)
                .unwrap_or(0);
            let grown = file.bytes.saturating_sub(old);
            if grown >= RUNAWAY_GROWTH && elapsed <= 2 * 3_600_000 {
                out.push(alert(
                    "critical",
                    "runaway_file",
                    Some(&project.project),
                    format!(
                        "{}: {} grew by {} in {} minutes (now {})",
                        project.label,
                        file.path,
                        human(grown),
                        elapsed / 60_000,
                        human(file.bytes)
                    ),
                ));
            }
        }
        let grown = project.total.saturating_sub(before.total);
        if grown >= 20 * GIB && elapsed <= 2 * 3_600_000 {
            out.push(alert(
                "warning",
                "project_growth",
                Some(&project.project),
                format!(
                    "{} grew by {} in {} minutes",
                    project.label,
                    human(grown),
                    elapsed / 60_000
                ),
            ));
        }
    }
    for project in &health.projects {
        for file in &project.largest {
            if file.category == "logs" && file.bytes >= 10 * GIB {
                out.push(alert(
                    "warning",
                    "large_log",
                    Some(&project.project),
                    format!(
                        "{}: log file {} is {}",
                        project.label,
                        file.path,
                        human(file.bytes)
                    ),
                ));
            }
        }
    }
    if let Some(storage) = &health.agent_storage
        && storage.bytes >= 20 * GIB
    {
        out.push(alert(
            "warning",
            "agent_storage",
            None,
            format!(
                "Agent session logs in {} use {}",
                storage.path,
                human(storage.bytes)
            ),
        ));
    }
    for agent in &health.agents {
        out.extend(agent.alerts());
    }
    out
}

/// Sample one portfolio. `deep` rescans member projects.
pub fn sample(config: &Config, category: &str, deep: bool) -> Result<Health> {
    let mut health = load(config, category);
    let previous = health.projects.clone();
    health.version = 1;
    check_filesystems(config, category, &mut health);
    if deep {
        let mut projects = Vec::new();
        for member in crate::category_agent::members(config, category).unwrap_or_default() {
            let label = member.label.clone().unwrap_or(member.workspace.clone());
            let mut usage = ProjectUsage {
                project: member.workspace.clone(),
                label,
                directory: member.directory.clone(),
                host: member.host.clone(),
                scanned_ms: now_ms(),
                ..Default::default()
            };
            match (&member.host, &member.directory) {
                (Some(host), _) => {
                    usage.note = Some(format!(
                        "On {host}; remote disk scanning is not available yet"
                    ))
                }
                (None, None) => usage.note = Some("No project directory".into()),
                (None, Some(directory)) => match scan(Path::new(directory), SCAN_BUDGET) {
                    Ok(s) => {
                        usage.total = s.total;
                        usage.categories = s.categories;
                        usage.largest = s.largest;
                        usage.complete = s.complete;
                        if !s.complete {
                            usage.note =
                                Some("Scan stopped at its time limit; totals are partial".into());
                        }
                    }
                    Err(e) => usage.note = Some(format!("{e:#}")),
                },
            }
            projects.push(usage);
        }
        health.projects = projects;
        health.scanned_ms = now_ms();
        health.agent_storage = crate::agent_context::session_storage();
    }
    health.agents = crate::agent_context::measure(config, category);
    health.alerts = alerts(&health, &previous);
    save(config, category, &health)?;
    Ok(health)
}

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Called from a frontend's periodic work. Starts at most one background
/// sampler per process; a private lock prevents duplicates across frontends.
pub fn tick(config: &Config) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let config = config.clone();
    std::thread::spawn(move || {
        let _ = (|| -> Result<()> {
            let catalog = crate::categories::Catalog::load(&config)?;
            for group in catalog.all() {
                let health = load(&config, &group.id);
                let now = now_ms();
                if now.saturating_sub(health.checked_ms) < CHECK_EVERY_MS {
                    continue;
                }
                let root = crate::category_agent::storage_path(&config, &group.id)?;
                starcore::terminal_graphics::session::private_root(&root)?;
                let Some(_lock) =
                    crate::project_sync::Transaction::try_acquire(root.join("health.lock"))?
                else {
                    continue;
                };
                let deep = now.saturating_sub(health.scanned_ms) >= SCAN_EVERY_MS;
                let _ = sample(&config, &group.id, deep);
            }
            Ok(())
        })();
        RUNNING.store(false, Ordering::SeqCst);
    });
}

/// Explicit "check now" from the UI.
pub fn check_now(config: &Config, category: &str) -> Result<()> {
    let root = crate::category_agent::storage_path(config, category)?;
    starcore::terminal_graphics::session::private_root(&root)?;
    let _lock = crate::project_sync::Transaction::acquire(root.join("health.lock"))?;
    sample(config, category, true).map(|_| ())
}

/// Header and project-row labels, keyed like `portfolio_context::badges`.
pub fn badges(config: &Config) -> BTreeMap<String, String> {
    let mut badges = BTreeMap::new();
    let Ok(catalog) = crate::categories::Catalog::load(config) else {
        return badges;
    };
    for group in catalog.all() {
        let health = load(config, &group.id);
        if health.checked_ms == 0 {
            continue;
        }
        let critical = health
            .alerts
            .iter()
            .filter(|a| a.severity == "critical")
            .count();
        let warnings = health.alerts.len() - critical;
        if critical + warnings > 0 {
            badges.insert(
                group.id.clone(),
                if critical > 0 {
                    format!("⚠ Health: {critical} critical")
                } else {
                    format!("Health: {warnings} warning(s)")
                },
            );
        }
        for alert in &health.alerts {
            if let Some(project) = &alert.project {
                badges
                    .entry(format!("project:{project}"))
                    .or_insert_with(|| match alert.kind.as_str() {
                        "runaway_file" => "⚠ runaway file".into(),
                        "project_growth" => "⚠ growing fast".into(),
                        "large_log" => "large log".into(),
                        k if k.starts_with("context") => "agent context large".into(),
                        _ => "health warning".into(),
                    });
            }
        }
    }
    badges
}

pub fn read(config: &Config, category: &str) -> Value {
    let health = load(config, category);
    let filesystems = health
        .filesystems
        .values()
        .map(|fs| json!({"path":fs.path,"total":fs.total,"free":fs.free,"loss_per_hour":loss_per_hour(fs)}))
        .collect::<Vec<_>>();
    json!({"health":health,"filesystems":filesystems})
}

/// Compact evidence for the coordinator.
pub fn evidence(config: &Config, category: &str) -> Value {
    let health = load(config, category);
    if health.checked_ms == 0 {
        return json!({"status":"not_sampled_yet"});
    }
    let projects = health
        .projects
        .iter()
        .map(|p| json!({"project":p.project,"label":p.label,"bytes":p.total,"by_category":p.categories,"largest_files":p.largest.iter().take(5).collect::<Vec<_>>(),"complete":p.complete,"note":p.note}))
        .collect::<Vec<_>>();
    let filesystems = health
        .filesystems
        .values()
        .map(|fs| json!({"path":fs.path,"total":fs.total,"free":fs.free,"loss_per_hour":loss_per_hour(fs)}))
        .collect::<Vec<_>>();
    json!({
        "status":"sampled",
        "checked_ms":health.checked_ms,
        "scanned_ms":health.scanned_ms,
        "filesystems":filesystems,
        "projects":projects,
        "agent_storage":health.agent_storage,
        "agents":health.agents,
        "alerts":health.alerts,
        "measurement":"allocated bytes; btrfs compression is not reflected; remote projects are not scanned",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categories_follow_directories_and_extensions() {
        assert_eq!(directory_category("target"), Some("build"));
        assert_eq!(directory_category("Intermediate"), Some("build"));
        assert_eq!(directory_category("logs"), Some("logs"));
        assert_eq!(directory_category("src"), None);
        assert_eq!(file_category("disk.img", "other"), "images");
        assert_eq!(file_category("run.log", "other"), "logs");
        assert_eq!(file_category("run.log", "build"), "build");
        assert_eq!(file_category("pack.img", "git"), "git");
    }
    #[test]
    fn scans_sizes_and_skips_links() {
        let dir = std::env::temp_dir().join(format!("ph-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("logs/a.txt"), vec![1u8; 256 * 1024]).unwrap();
        std::fs::write(dir.join("src/main.rs"), vec![1u8; 4096]).unwrap();
        std::os::unix::fs::symlink("/", dir.join("escape")).unwrap();
        let s = scan(&dir, Duration::from_secs(5)).unwrap();
        assert!(s.complete);
        assert!(s.categories["logs"] >= 256 * 1024);
        assert!(s.categories["other"] >= 4096);
        assert_eq!(s.largest[0].path, "logs/a.txt");
        assert!(s.total < 4 * 1024 * 1024, "followed a link");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn runaway_growth_between_scans_is_critical() {
        let file = |bytes| FileUsage {
            path: "logs/run.txt".into(),
            bytes,
            category: "logs".into(),
        };
        let before = ProjectUsage {
            project: "p".into(),
            label: "P".into(),
            scanned_ms: 0,
            total: GIB,
            largest: vec![file(GIB)],
            ..Default::default()
        };
        let after = ProjectUsage {
            scanned_ms: 15 * 60_000,
            total: 6 * GIB,
            largest: vec![file(6 * GIB)],
            ..before.clone()
        };
        let health = Health {
            projects: vec![after],
            ..Default::default()
        };
        let found = alerts(&health, &[before]);
        assert!(
            found
                .iter()
                .any(|a| a.kind == "runaway_file" && a.severity == "critical")
        );
        assert!(!found.iter().any(|a| a.kind == "project_growth"));
    }
    #[test]
    fn a_falling_drive_projects_its_full_time() {
        let fs = Filesystem {
            path: "/x".into(),
            total: 1000 * GIB,
            free: 100 * GIB,
            history: vec![[0, 110 * GIB], [30 * 60_000, 100 * GIB]],
        };
        assert!((loss_per_hour(&fs).unwrap() - 20.0 * GIB as f64).abs() < 1.0);
        let mut health = Health::default();
        health.filesystems.insert("1".into(), fs);
        let found = alerts(&health, &[]);
        assert!(found.iter().any(|a| a.kind == "filling_fast"));
        assert!(!found.iter().any(|a| a.kind == "low_space"));
    }
}
