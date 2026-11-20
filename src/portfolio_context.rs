//! Portfolio context: a user-owned `PORTFOLIO.md` shared by a portfolio's
//! coordinator and every Codex agent started in its member projects.
//!
//! The file lives at the shared root directory of the portfolio's local member
//! projects unless the user chooses another local path. STAR/FLUX reads it with
//! the same bounds as other coordinator evidence and never edits it except to
//! create the initial template on explicit request. Running agents keep the
//! revision they started with; the UI reports agents that need a restart.
use crate::runtime::Config;
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

pub const FILE: &str = "PORTFOLIO.md";
pub const LIMIT: usize = 32 * 1024;
/// Marker embedded in delivered instructions so a running agent's revision can
/// be read back from its command line without extra saved state.
const MARKER: &str = "starflux-portfolio-context";

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct Settings {
    version: u32,
    path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// No local member directories, so there is no default root.
    Unset,
    Missing,
    Ready,
    Invalid,
}

#[derive(Clone, Debug, Serialize)]
pub struct Context {
    pub category: String,
    pub name: String,
    pub path: Option<PathBuf>,
    pub default_path: Option<PathBuf>,
    pub custom_path: bool,
    pub status: Status,
    pub error: Option<String>,
    #[serde(skip)]
    pub text: Option<String>,
    pub revision: Option<String>,
    pub bytes: usize,
    pub modified_ms: Option<u64>,
}

impl Context {
    pub fn ready(&self) -> Option<(&str, &str)> {
        Some((self.text.as_deref()?, self.revision.as_deref()?))
    }
}

fn settings_path(config: &Config, category: &str) -> Result<PathBuf> {
    Ok(crate::category_agent::storage_path(config, category)?.join("context.json"))
}

fn settings(config: &Config, category: &str) -> Settings {
    settings_path(config, category)
        .ok()
        .filter(|p| p.exists())
        .and_then(|p| crate::plugin_store::read(&p, 64 * 1024).ok())
        .unwrap_or_default()
}

fn name(config: &Config, category: &str) -> String {
    crate::categories::Catalog::load(config)
        .ok()
        .and_then(|c| c.all().into_iter().find(|g| g.id == category))
        .map(|g| g.name)
        .unwrap_or_else(|| category.into())
}

/// The shared root directory of the portfolio's local member projects.
pub fn default_path(config: &Config, category: &str) -> Option<PathBuf> {
    let directories = crate::category_agent::members(config, category)
        .ok()?
        .into_iter()
        .filter(|m| m.host.is_none())
        .filter_map(|m| m.directory.map(PathBuf::from))
        .filter(|d| d.is_absolute())
        .collect::<Vec<_>>();
    common_root(&directories).map(|root| root.join(FILE))
}

fn common_root(directories: &[PathBuf]) -> Option<PathBuf> {
    let mut root = directories.first()?.clone();
    for directory in &directories[1..] {
        while !directory.starts_with(&root) {
            root = root.parent()?.to_path_buf();
        }
    }
    // Never suggest the filesystem root; a portfolio spanning `/` has no shared root.
    (root.parent().is_some()).then_some(root)
}

fn hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))[..12].into()
}

fn read_bounded(path: &Path) -> Result<(String, Option<u64>)> {
    ensure!(
        path.is_absolute(),
        "Portfolio context path must be absolute"
    );
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "{FILE} must be a regular file, not a link"
    );
    let mut bytes = Vec::new();
    (&mut file).take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= LIMIT,
        "{FILE} exceeds 32 KiB; shorten it so every agent can afford it"
    );
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);
    Ok((
        String::from_utf8(bytes).context("Portfolio context is not UTF-8 text")?,
        modified,
    ))
}

pub fn load(config: &Config, category: &str) -> Result<Context> {
    ensure!(
        category != crate::categories::UNASSIGNED,
        "Unassigned projects have no portfolio context"
    );
    let saved = settings(config, category);
    let default_path = default_path(config, category);
    let path = saved.path.clone().or_else(|| default_path.clone());
    let mut context = Context {
        category: category.into(),
        name: name(config, category),
        custom_path: saved.path.is_some(),
        path: path.clone(),
        default_path,
        status: Status::Unset,
        error: None,
        text: None,
        revision: None,
        bytes: 0,
        modified_ms: None,
    };
    let Some(path) = path else {
        return Ok(context);
    };
    match read_bounded(&path) {
        Ok((text, modified)) => {
            context.status = Status::Ready;
            context.bytes = text.len();
            context.revision = Some(hash(&text));
            context.modified_ms = modified;
            context.text = Some(text);
        }
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            context.status = Status::Missing;
        }
        Err(e) => {
            context.status = Status::Invalid;
            context.error = Some(format!("{e:#}"));
        }
    }
    Ok(context)
}

/// Choose a custom local path, or `None` to return to the shared project root.
pub fn set_path(config: &Config, category: &str, path: Option<PathBuf>) -> Result<()> {
    if let Some(path) = &path {
        ensure!(path.is_absolute(), "Enter an absolute local path");
        ensure!(
            path.file_name().is_some() && !path.ends_with(".."),
            "Enter a file path"
        );
    }
    let root = crate::category_agent::root_path(config, category)?;
    starcore::terminal_graphics::session::private_root(&root)?;
    let settings = Settings { version: 1, path };
    crate::automation::persist(
        &settings_path(config, category)?,
        &serde_json::to_vec(&settings)?,
    )
}

pub fn template(config: &Config, category: &str) -> Result<String> {
    let mut members = String::new();
    for m in crate::category_agent::members(config, category)? {
        members.push_str(&format!(
            "- **{}** (`{}`{}): TODO: role in this portfolio.\n",
            m.label.as_deref().unwrap_or(&m.workspace),
            m.directory.as_deref().unwrap_or("?"),
            m.host
                .as_deref()
                .map(|h| format!(" on {h}"))
                .unwrap_or_default()
        ));
    }
    Ok(format!(
        "# {name} portfolio\n\n\
         This file is shared with the portfolio coordinator and every Codex agent\n\
         that STAR/FLUX starts in these projects. Keep it short: every agent pays\n\
         for it on every turn (limit 32 KiB). Agents started before an edit keep\n\
         the old version until restarted.\n\n\
         ## Purpose\n\nTODO: what this group of projects is for and who it serves.\n\n\
         ## Members and roles\n\n{members}\n\
         ## Shared code and dependencies\n\n\
         TODO: shared crates, libraries, protocols and assets, and which project owns each.\n\n\
         ## Cross-project conventions\n\n\
         TODO: naming, build, release, testing and style rules every member follows.\n\n\
         ## Change ripple rules\n\n\
         TODO: when a change in one project requires follow-up in another\n\
         (for example, a shared crate API change needs each consumer updated and tested).\n\n\
         ## Coordination rules\n\n\
         - The portfolio coordinator never edits code or files, including this one.\n  \
           It proposes scoped tasks that the user approves and hands to the owning project's agent.\n\
         - A project agent changes only its own project. Work needed in a sibling\n  \
           project is reported back as a follow-up, not done directly.\n\
         - Keep build output, logs and artifacts inside each project's ignored\n  \
           directories, bounded in size, and clean up runaway output promptly.\n",
        name = name(config, category),
    ))
}

/// Create the template at the configured or default path. Never overwrites.
pub fn create(config: &Config, category: &str) -> Result<PathBuf> {
    let context = load(config, category)?;
    let path = context
        .path
        .context("No local member projects; choose a path for PORTFOLIO.md first")?;
    ensure!(context.status == Status::Missing, "{FILE} already exists");
    let text = template(config, category)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .with_context(|| format!("Create {}", path.display()))?;
    std::io::Write::write_all(&mut file, text.as_bytes())?;
    Ok(path)
}

/// Append the coordinator charter as an Objectives section. The user then
/// clears the charter explicitly; nothing is removed here.
pub fn append_charter(config: &Config, category: &str, charter: &str) -> Result<()> {
    ensure!(!charter.trim().is_empty(), "The charter is empty");
    let context = load(config, category)?;
    let (text, _) = context.ready().context("Create PORTFOLIO.md first")?;
    let addition = format!("\n## Objectives\n\n{}\n", charter.trim());
    ensure!(
        text.len() + addition.len() <= LIMIT,
        "Adding the charter would exceed 32 KiB"
    );
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(context.path.context("Missing path")?)?;
    std::io::Write::write_all(&mut file, addition.as_bytes())?;
    Ok(())
}

/// Developer instructions delivered to project agents.
pub fn instructions(name: &str, revision: &str, text: &str, project: &str) -> String {
    format!(
        "[{MARKER}:{revision}] This project ({project}) belongs to the \"{name}\" portfolio. \
         The portfolio context below describes the sibling projects, shared code and \
         cross-project rules. Use it when work could affect other projects. Change only this \
         project; report work needed in a sibling project as a follow-up instead of doing it. \
         Treat it as guidance from the user, subordinate to direct user instructions.\n\n\
         --- PORTFOLIO.md (revision {revision}) ---\n{text}"
    )
}

fn codex_index(argv: &[String]) -> Option<usize> {
    argv.iter().position(|arg| {
        Path::new(arg)
            .file_name()
            .is_some_and(|name| name == "codex")
    })
}

/// Insert portfolio developer instructions into a Codex command line.
/// Previous STAR/FLUX portfolio instructions are replaced; a user's own
/// `developer_instructions` override is left untouched and wins.
pub fn with_instructions(argv: Vec<String>, instructions: &str) -> Vec<String> {
    let Some(index) = codex_index(&argv) else {
        return argv;
    };
    let mut out = argv[..=index].to_vec();
    let mut rest = argv[index + 1..].iter().peekable();
    let mut user_override = false;
    while let Some(arg) = rest.next() {
        if (arg == "-c" || arg == "--config")
            && let Some(value) = rest.peek()
            && value.starts_with("developer_instructions=")
        {
            if value.contains(MARKER) {
                rest.next();
                continue;
            }
            user_override = true;
        }
        if arg.starts_with("--config=developer_instructions=") {
            if arg.contains(MARKER) {
                continue;
            }
            user_override = true;
        }
        out.push(arg.clone());
    }
    if !user_override {
        // `-c` values are TOML; a JSON string literal is a valid TOML basic string.
        let value = serde_json::to_string(instructions).unwrap_or_else(|_| "\"\"".into());
        out.splice(
            index + 1..index + 1,
            ["-c".into(), format!("developer_instructions={value}")],
        );
    }
    out
}

/// Apply the project's portfolio context to an agent command line.
pub fn apply(config: &Config, argv: Vec<String>) -> Vec<String> {
    if codex_index(&argv).is_none() {
        return argv;
    }
    let Ok(catalog) = crate::categories::Catalog::load(config) else {
        return argv;
    };
    let category = catalog.category(&config.workspace);
    if category == crate::categories::UNASSIGNED {
        return argv;
    }
    let Ok(context) = load(config, category) else {
        return argv;
    };
    let Some((text, revision)) = context.ready() else {
        return argv;
    };
    let project = config.label.as_deref().unwrap_or(&config.workspace);
    with_instructions(argv, &instructions(&context.name, revision, text, project))
}

/// The context revision a running local agent was started with.
pub fn launched_revision(cmdline: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(cmdline);
    let start = text.find(&format!("[{MARKER}:"))? + MARKER.len() + 2;
    let end = text[start..].find(']')? + start;
    Some(text[start..end].into())
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AgentCoverage {
    pub current: usize,
    pub stale: usize,
    pub missing: usize,
    pub unknown: usize,
    pub agents: Vec<Value>,
}

/// Compare running Codex agents in member projects with the current revision.
pub fn coverage(config: &Config, category: &str, revision: Option<&str>) -> AgentCoverage {
    let mut out = AgentCoverage::default();
    for member in crate::category_agent::members(config, category).unwrap_or_default() {
        let Ok(records) = crate::agent_state::records(&member) else {
            continue;
        };
        for record in records {
            if record.report.harness != "codex"
                || matches!(record.report.state, crate::agent_state::Lifecycle::Stopped)
            {
                continue;
            }
            let label = member.label.clone().unwrap_or(member.workspace.clone());
            let state = if member.host.is_some() {
                out.unknown += 1;
                "unknown (remote host)"
            } else {
                match std::fs::read(format!("/proc/{}/cmdline", record.report.agent_pid)) {
                    Err(_) => continue,
                    Ok(cmdline) => match (launched_revision(&cmdline), revision) {
                        (Some(a), Some(b)) if a == b => {
                            out.current += 1;
                            "current"
                        }
                        (Some(_), _) => {
                            out.stale += 1;
                            "stale: restart to apply the current PORTFOLIO.md"
                        }
                        (None, Some(_)) => {
                            out.missing += 1;
                            "started without portfolio context: restart to apply"
                        }
                        (None, None) => continue,
                    },
                }
            };
            out.agents.push(json!({"project":member.workspace,"label":label,"pane":record.pane+1,"state":state}));
        }
    }
    out
}

/// Header and project-row labels, keyed by portfolio ID and `project:<workspace>`.
pub fn badges(config: &Config) -> BTreeMap<String, String> {
    let mut badges = BTreeMap::new();
    let Ok(catalog) = crate::categories::Catalog::load(config) else {
        return badges;
    };
    for group in catalog.all() {
        if group.id == crate::categories::UNASSIGNED {
            continue;
        }
        let Ok(context) = load(config, &group.id) else {
            continue;
        };
        let label = match context.status {
            Status::Unset | Status::Missing => "Context: none".to_string(),
            Status::Invalid => "Context: unreadable".to_string(),
            Status::Ready => {
                let cover = coverage(config, &group.id, context.revision.as_deref());
                for agent in &cover.agents {
                    if agent["state"] != "current"
                        && let Some(project) = agent["project"].as_str()
                    {
                        badges.insert(
                            format!("project:{project}"),
                            "context outdated · restart agent".into(),
                        );
                    }
                }
                let restart = cover.stale + cover.missing;
                if restart > 0 {
                    format!(
                        "Context ✓ · {restart} agent{} need{} restart",
                        if restart == 1 { "" } else { "s" },
                        if restart == 1 { "s" } else { "" }
                    )
                } else {
                    "Context ✓".to_string()
                }
            }
        };
        badges.insert(group.id, label);
    }
    badges
}

pub fn read(config: &Config, category: &str) -> Result<Value> {
    let context = load(config, category)?;
    let cover = coverage(config, category, context.revision.as_deref());
    let preview = context
        .text
        .as_deref()
        .map(|t| t.lines().take(60).collect::<Vec<_>>().join("\n"));
    Ok(json!({
        "context": context,
        "preview": preview,
        "limit": LIMIT,
        "coverage": cover,
    }))
}

/// Context for coordinator evidence: the full text when available.
pub fn evidence(config: &Config, category: &str) -> Value {
    match load(config, category) {
        Ok(c) => json!({
            "status": c.status,
            "path": c.path,
            "revision": c.revision,
            "bytes": c.bytes,
            "text": c.text,
            "error": c.error,
        }),
        Err(e) => json!({"status":"unavailable","error":format!("{e:#}")}),
    }
}

/// Prefix for an approved handoff when the receiving agent did not start with
/// the current context (or its revision cannot be read).
pub fn handoff_prefix(member: &Config, category: &str, agent_pid: u32) -> Option<String> {
    let config = member;
    let context = load(config, category).ok()?;
    let (text, revision) = context.ready()?;
    let started = if member.host.is_none() {
        std::fs::read(format!("/proc/{agent_pid}/cmdline"))
            .ok()
            .and_then(|c| launched_revision(&c))
    } else {
        None
    };
    let project = member.label.as_deref().unwrap_or(&member.workspace);
    Some(if started.as_deref() == Some(revision) {
        format!(
            "[Portfolio task from the \"{}\" coordinator · PORTFOLIO.md revision {revision}, already in your instructions]\n\n",
            context.name
        )
    } else {
        format!(
            "[Portfolio task from the \"{}\" coordinator]\n{}\n--- end of portfolio context ---\n\n",
            context.name,
            instructions(&context.name, revision, text, project)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn instructions_follow_the_codex_executable() {
        let out = with_instructions(s(&["env", "A=1", "/usr/bin/codex", "resume", "x"]), "hi");
        assert_eq!(
            out,
            s(&[
                "env",
                "A=1",
                "/usr/bin/codex",
                "-c",
                "developer_instructions=\"hi\"",
                "resume",
                "x"
            ])
        );
    }
    #[test]
    fn non_codex_commands_are_unchanged() {
        assert_eq!(with_instructions(s(&["claude"]), "hi"), s(&["claude"]));
        assert_eq!(
            with_instructions(s(&["bash", "-l"]), "hi"),
            s(&["bash", "-l"])
        );
    }
    #[test]
    fn previous_portfolio_instructions_are_replaced() {
        let old = format!("developer_instructions=\"[{MARKER}:aaa] old\"");
        let out = with_instructions(s(&["codex", "-c", &old, "resume", "x"]), "new");
        assert_eq!(
            out,
            s(&[
                "codex",
                "-c",
                "developer_instructions=\"new\"",
                "resume",
                "x"
            ])
        );
    }
    #[test]
    fn a_user_override_wins() {
        let argv = s(&["codex", "-c", "developer_instructions=\"mine\""]);
        assert_eq!(with_instructions(argv.clone(), "new"), argv);
    }
    #[test]
    fn instruction_values_are_quoted_toml_strings() {
        let out = with_instructions(s(&["codex"]), "line \"one\"\nline two");
        let value = out[2].strip_prefix("developer_instructions=").unwrap();
        let parsed: String = serde_json::from_str(value).unwrap();
        assert_eq!(parsed, "line \"one\"\nline two");
    }
    #[test]
    fn launched_revision_is_read_from_the_command_line() {
        let text = instructions("STAR", "0123456789ab", "body", "starflux");
        let argv = with_instructions(s(&["codex"]), &text).join("\0");
        assert_eq!(
            launched_revision(argv.as_bytes()).as_deref(),
            Some("0123456789ab")
        );
        assert_eq!(launched_revision(b"codex\0resume"), None);
    }
    #[test]
    fn the_default_root_is_the_shared_parent() {
        let p = |v: &str| PathBuf::from(v);
        assert_eq!(
            common_root(&[p("/home/u/projects/star/a"), p("/home/u/projects/star/b")]),
            Some(p("/home/u/projects/star"))
        );
        assert_eq!(
            common_root(&[p("/home/u/projects/star/a")]),
            Some(p("/home/u/projects/star/a"))
        );
        assert_eq!(common_root(&[p("/a/x"), p("/b/y")]), None);
        assert_eq!(common_root(&[]), None);
    }
    #[test]
    fn oversized_and_linked_files_are_rejected() {
        let dir = std::env::temp_dir().join(format!("pc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.md");
        std::fs::write(&big, vec![b'a'; LIMIT + 1]).unwrap();
        assert!(read_bounded(&big).is_err());
        let ok = dir.join("ok.md");
        std::fs::write(&ok, "fine").unwrap();
        assert_eq!(read_bounded(&ok).unwrap().0, "fine");
        let link = dir.join("link.md");
        let _ = std::os::unix::fs::symlink(&ok, &link);
        assert!(read_bounded(&link).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
