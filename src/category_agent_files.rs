//! File reads for coordinator tools: directory-relative, bounded, no symlinks.
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::Read,
    os::fd::{AsRawFd, FromRawFd},
    path::{Component, Path},
};
pub(crate) fn allowed(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty(),
        "Choose a repository-relative file"
    );
    for part in path.components() {
        let Component::Normal(name) = part else {
            anyhow::bail!("File must stay inside the repository");
        };
        let name = name.to_string_lossy().to_lowercase();
        ensure!(
            !matches!(
                name.as_str(),
                ".git"
                    | ".local"
                    | ".config"
                    | ".cache"
                    | ".claude"
                    | ".herdr"
                    | ".starflux"
                    | "target"
                    | "node_modules"
                    | ".ssh"
                    | ".codex"
                    | ".aws"
                    | ".gnupg"
                    | "auth.json"
                    | "credentials.toml"
                    | "credentials.json"
                    | ".netrc"
                    | ".npmrc"
                    | ".pypirc"
                    | ".mcp.json"
                    | "mcp.json"
                    | ".claude.json"
                    | ".docker"
                    | ".kube"
            ) && !name.starts_with(".env")
                && !name.ends_with(".pem")
                && !name.ends_with(".key")
                && !name.ends_with(".p12")
                && !name.ends_with(".pfx")
                && !name.ends_with(".log")
                && !name.ends_with(".jsonl")
                && !name.ends_with(".state.json")
                && !name.ends_with(".screen.json"),
            "Credential and private runtime paths are excluded from portfolio evidence"
        );
    }
    Ok(())
}
pub(crate) fn read_file(root: &Path, path: &Path) -> Result<String> {
    allowed(path)?;
    let mut directory = File::open(root)?;
    ensure!(
        directory.metadata()?.is_dir(),
        "Repository root is not a directory"
    );
    let parts = path.components().collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            unreachable!()
        };
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(name.as_bytes())?;
        let last = index + 1 == parts.len();
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if last { 0 } else { libc::O_DIRECTORY };
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("Read portfolio evidence file");
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if last {
            ensure!(
                file.metadata()?.is_file(),
                "Evidence must be a regular file"
            );
            let mut bytes = Vec::new();
            file.take(131073).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 131072,
                "Evidence file exceeds 128 KiB; request a smaller source"
            );
            return String::from_utf8(bytes).context("Evidence file is not UTF-8 text");
        }
        directory = file;
    }
    anyhow::bail!("Missing evidence file")
}
pub fn read(
    config: &crate::runtime::Config,
    category: &str,
    project: &str,
    path: &str,
) -> Result<serde_json::Value> {
    let settings = crate::category_agent::read(config, category)?;
    ensure!(
        settings["state"]["profile"]["enabled"] == true,
        "Portfolio Coordinator is disabled"
    );
    let member = crate::category_agent::members(config, category)?
        .into_iter()
        .find(|m| m.workspace == project)
        .context("Project is outside this portfolio")?;
    query(&member, "read", path, "")
}

pub fn query(
    member: &crate::runtime::Config,
    operation: &str,
    path: &str,
    pattern: &str,
) -> Result<serde_json::Value> {
    if member.remote(0) {
        let request = serde_json::json!({"directory":member.directory,"operation":operation,"path":path,"pattern":pattern});
        let mut command = std::process::Command::new("ssh");
        if let Some(config) = &member.ssh_config {
            command.arg("-F").arg(config);
        }
        command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "--"]);
        command.arg(member.host.as_deref().context("Missing project host")?);
        command.arg(format!(
            "{} --portfolio-repository-query {}",
            starkit::terminal_graphics::client::shell_quote(&member.executable),
            starkit::terminal_graphics::client::shell_quote(&request.to_string())
        ));
        return Ok(serde_json::from_slice(&crate::read_query::output(
            command,
        )?)?);
    }
    let request = serde_json::json!({"directory":member.directory.as_deref().context("Project has no repository")?,"operation":operation,"path":path,"pattern":pattern});
    let executable = if cfg!(target_os = "linux") {
        std::path::PathBuf::from("/proc/self/exe")
    } else {
        std::env::current_exe()?
    };
    let mut command = std::process::Command::new(executable);
    command.args(["--category-repository-query", &request.to_string()]);
    Ok(serde_json::from_slice(&crate::read_query::output(
        command,
    )?)?)
}

pub fn local(
    directory: &str,
    operation: &str,
    path: &str,
    pattern: &str,
) -> Result<serde_json::Value> {
    use serde_json::json;
    use sha2::{Digest, Sha256};
    let expanded = if directory == "~" {
        std::env::home_dir().context("Home directory unavailable")?
    } else if let Some(path) = directory.strip_prefix("~/") {
        std::env::home_dir()
            .context("Home directory unavailable")?
            .join(path)
    } else {
        std::path::PathBuf::from(directory)
    };
    let root = std::fs::canonicalize(expanded)?;
    if operation == "read" {
        let text = read_file(&root, Path::new(path))?;
        return Ok(
            json!({"path":path,"sha256":format!("{:x}",Sha256::digest(text.as_bytes())),"text":text}),
        );
    }
    if operation == "git" {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(&root).args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "status",
            "--porcelain=v1",
            "--branch",
        ]);
        let bytes = crate::read_query::output(cmd)?;
        return Ok(
            json!({"git_status":String::from_utf8_lossy(&bytes),"sha256":format!("{:x}",Sha256::digest(&bytes)),"available":true}),
        );
    }
    ensure!(
        matches!(operation, "list" | "search"),
        "Unknown repository operation"
    );
    ensure!(
        path.len() <= 1024 && pattern.len() <= 256,
        "Repository query exceeds limit"
    );
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(&root).args([
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "ls-files",
        "-c",
        "-o",
        "--exclude-standard",
        "-z",
    ]);
    if !path.is_empty() {
        allowed(Path::new(path))?;
    }
    let mut names = match crate::read_query::output(cmd) {
        Ok(bytes) => bytes
            .split(|b| *b == 0)
            .filter_map(|b| std::str::from_utf8(b).ok())
            .filter(|s| !s.is_empty() && s.starts_with(path) && allowed(Path::new(s)).is_ok())
            .take(10000)
            .map(String::from)
            .collect::<Vec<_>>(),
        Err(_) => {
            let mut pending = vec![std::path::PathBuf::new()];
            let mut names = Vec::new();
            let started = std::time::Instant::now();
            while let Some(directory) = pending.pop() {
                if names.len() >= 10000 || started.elapsed() > std::time::Duration::from_secs(1) {
                    break;
                }
                if let Ok(entries) = std::fs::read_dir(root.join(&directory)) {
                    for entry in entries.flatten() {
                        let relative = directory.join(entry.file_name());
                        if allowed(&relative).is_err() {
                            continue;
                        }
                        let Ok(kind) = entry.file_type() else {
                            continue;
                        };
                        if kind.is_symlink() {
                            continue;
                        }
                        if kind.is_dir() {
                            pending.push(relative);
                        } else if kind.is_file() {
                            let name = relative.to_string_lossy().into_owned();
                            if name.starts_with(path) {
                                names.push(name);
                            }
                        }
                    }
                }
            }
            names
        }
    };
    names.sort();
    if operation == "list" {
        let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&names)?));
        return Ok(
            json!({"files":names.iter().take(2000).collect::<Vec<_>>(),"truncated":names.len()>2000,"sha256":hash}),
        );
    }
    ensure!(!pattern.is_empty(), "Search text is empty");
    let mut hits = Vec::new();
    let mut scanned = 0;
    let mut sources = Vec::new();
    for name in names {
        if scanned >= 64 || hits.len() >= 100 {
            break;
        }
        let Ok(text) = read_file(&root, Path::new(&name)) else {
            continue;
        };
        scanned += 1;
        sources.push(json!({"path":name,"sha256":format!("{:x}",Sha256::digest(text.as_bytes()))}));
        for (index, line) in text.lines().enumerate() {
            if line.contains(pattern) {
                hits.push(json!({"path":name,"line":index+1,"text":line.chars().take(300).collect::<String>()}));
                if hits.len() >= 100 {
                    break;
                }
            }
        }
    }
    Ok(json!({"matches":hits,"sources":sources,"files_scanned":scanned,"bounded":true}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repository_reads_are_bounded_and_cannot_follow_links_or_escape() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/readme"), "café 界").unwrap();
        assert_eq!(
            read_file(root.path(), Path::new("src/readme")).unwrap(),
            "café 界"
        );
        for path in [
            "../outside",
            "/etc/passwd",
            ".env",
            ".git/config",
            ".ssh/id_rsa",
            "credentials.toml",
        ] {
            assert!(read_file(root.path(), Path::new(path)).is_err());
        }
        std::os::unix::fs::symlink("src/readme", root.path().join("link")).unwrap();
        assert!(read_file(root.path(), Path::new("link")).is_err());
        std::os::unix::fs::symlink("src", root.path().join("linked-dir")).unwrap();
        assert!(read_file(root.path(), Path::new("linked-dir/readme")).is_err());
        std::fs::write(root.path().join("big"), vec![0u8; 131073]).unwrap();
        assert!(read_file(root.path(), Path::new("big")).is_err());
    }
}
