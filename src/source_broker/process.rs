//! Bounded foreground identity inspection. Command arguments stay on this host;
//! only a recognized harness family and verified process identity are exported.
use super::*;
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Agent {
    pub pid: u32,
    pub start_ticks: u64,
    pub family: String,
}
#[derive(Serialize)]
pub struct Observation {
    pub command: String,
    pub cwd: Option<PathBuf>,
    pub agent: Option<Agent>,
}
fn family(argv: &[String]) -> Option<String> {
    for arg in argv.iter().take(2) {
        let name = Path::new(arg)
            .file_name()?
            .to_str()?
            .trim_end_matches(".js")
            .trim_end_matches(".mjs");
        let family = match name {
            "codex" => "codex",
            "claude" | "claude-code" => "claude",
            "gemini" | "gemini-cli" => "gemini",
            "opencode" => "opencode",
            "aider" => "aider",
            _ => continue,
        };
        return Some(family.into());
    }
    None
}
pub fn inspect(reference: &Reference) -> Result<Observation> {
    ensure!(
        start_ticks(reference.pid)? == reference.start_ticks,
        "Native process changed"
    );
    let root = fs::read_to_string(format!("/proc/{}/stat", reference.pid))?;
    let foreground = root
        .rsplit_once(')')
        .context("Invalid native process status")?
        .1
        .split_whitespace()
        .nth(5)
        .and_then(|s| s.parse::<u32>().ok())
        .context("Native foreground group unavailable")?;
    let mut command = root
        .split_once('(')
        .and_then(|(_, s)| s.rsplit_once(')'))
        .map(|(s, _)| s.to_owned())
        .unwrap_or_default();
    let mut cwd = fs::read_link(format!("/proc/{}/cwd", reference.pid)).ok();
    let mut pending = vec![reference.pid];
    let mut seen = std::collections::BTreeSet::new();
    let mut agent = None;
    while let Some(pid) = pending.pop() {
        if seen.len() >= 64 {
            break;
        }
        if !seen.insert(pid) {
            continue;
        }
        let path = PathBuf::from(format!("/proc/{pid}"));
        let Ok(stat) = fs::read_to_string(path.join("stat")) else {
            continue;
        };
        let Some((_, fields)) = stat.rsplit_once(')') else {
            continue;
        };
        let fields = fields.split_whitespace().collect::<Vec<_>>();
        let ticks = fields.get(19).and_then(|s| s.parse().ok());
        if fields.get(2).and_then(|s| s.parse::<u32>().ok()) == Some(foreground) {
            let comm = stat
                .split_once('(')
                .and_then(|(_, s)| s.rsplit_once(')'))
                .map(|(s, _)| s.to_owned())
                .unwrap_or_default();
            if pid == foreground && ticks.is_some() && start_ticks(pid).ok() == ticks {
                command = comm.clone();
                cwd = fs::read_link(path.join("cwd")).ok();
            }
            let mut file = fs::File::open(path.join("cmdline"))?;
            let mut bytes = Vec::new();
            (&mut file).take(32769).read_to_end(&mut bytes)?;
            if bytes.len() <= 32768 {
                let argv = bytes
                    .split(|b| *b == 0)
                    .filter(|b| !b.is_empty())
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .collect::<Vec<_>>();
                if let Some(family) = family(&argv).or_else(|| family(std::slice::from_ref(&comm)))
                    && let Some(ticks) = ticks
                    && start_ticks(pid).ok() == Some(ticks)
                {
                    command = comm;
                    agent = Some(Agent {
                        pid,
                        start_ticks: ticks,
                        family,
                    });
                    cwd = fs::read_link(path.join("cwd")).ok();
                    break;
                }
            }
        }
        if let Ok(children) = fs::read_to_string(path.join(format!("task/{pid}/children"))) {
            pending.extend(
                children
                    .split_whitespace()
                    .filter_map(|s| s.parse::<u32>().ok())
                    .take(64),
            );
        }
    }
    ensure!(
        start_ticks(reference.pid)? == reference.start_ticks,
        "Native process changed"
    );
    Ok(Observation {
        command,
        cwd,
        agent,
    })
}
