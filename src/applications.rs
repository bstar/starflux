//! Host-side application registry. GIO owns desktop entry precedence and Exec expansion.
use anyhow::{Context, Result, ensure};
use gio::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub description: String,
    pub executable: String,
    pub command: String,
}
impl Application {
    pub fn matches(&self, query: &str) -> bool {
        let haystack = format!(
            "{} {} {} {}",
            self.name, self.description, self.executable, self.id
        )
        .to_lowercase();
        query
            .split_whitespace()
            .all(|word| haystack.contains(&word.to_lowercase()))
    }
}
pub fn installed() -> Result<Vec<Application>> {
    let helper = std::env::current_exe()?.display().to_string();
    let mut apps = gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .filter_map(|a| {
            let id = a.id()?.to_string();
            Some(Application {
                command: shell_words::join([helper.as_str(), "--launch-system-application", &id]),
                id,
                name: a.display_name().to_string(),
                description: a.description().map_or(String::new(), |s| s.to_string()),
                executable: a.executable().display().to_string(),
            })
        })
        .collect::<Vec<_>>();
    apps.sort_by_cached_key(|a| (a.name.to_lowercase(), a.id.clone()));
    apps.dedup_by(|a, b| a.id == b.id);
    Ok(apps)
}
pub fn launch(id: &str) -> Result<()> {
    let original = gio_unix::DesktopAppInfo::new(id)
        .context("Application is no longer registered on this host")?;
    let path = original
        .filename()
        .context("Application has no desktop entry")?;
    let entry = gio::glib::KeyFile::new();
    entry.load_from_file(path, gio::glib::KeyFileFlags::NONE)?;
    // Launch inside this compositor, without a D-Bus request that could reuse a
    // window outside it. The project/launch dialog supplies the working directory.
    entry.set_boolean("Desktop Entry", "DBusActivatable", false);
    let _ = entry.remove_key("Desktop Entry", "Path");
    let app =
        gio_unix::DesktopAppInfo::from_keyfile(&entry).context("Invalid registered application")?;
    #[cfg(target_os = "linux")]
    ensure!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0,
        "Could not supervise application launch"
    );
    app.launch(&[], None::<&gio::AppLaunchContext>)?;
    // Retain the launched process tree beneath the compositor for audio routing
    // and lifecycle tracking, including launchers that fork before opening a window.
    loop {
        let result = unsafe { libc::waitpid(-1, std::ptr::null_mut(), 0) };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            if error.raw_os_error() == Some(libc::ECHILD) {
                break;
            }
            return Err(error.into());
        }
    }
    Ok(())
}
pub fn on_host(config: &crate::runtime::Config) -> Result<Vec<Application>> {
    let mut child = crate::app_desktop::command_extra(config, "--system-applications", &[])?
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    use std::io::Read;
    let mut stdout = child
        .stdout
        .take()
        .context("Application catalog stdout missing")?;
    let mut stderr = child
        .stderr
        .take()
        .context("Application catalog stderr missing")?;
    let output = std::thread::spawn(move || {
        let mut bytes = vec![];
        stdout
            .by_ref()
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = vec![];
        stderr
            .by_ref()
            .take(65536)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let start = std::time::Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() > std::time::Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Application catalog timed out on the app host");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let status = child.wait()?;
    let stdout = output
        .join()
        .map_err(|_| anyhow::anyhow!("Catalog reader stopped"))??;
    let stderr = errors
        .join()
        .map_err(|_| anyhow::anyhow!("Catalog error reader stopped"))??;
    ensure!(
        status.success(),
        "Application catalog: {}",
        String::from_utf8_lossy(&stderr).trim()
    );
    ensure!(
        stdout.len() <= 8 * 1024 * 1024,
        "Application catalog exceeds limit"
    );
    Ok(serde_json::from_slice(&stdout)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_search_matches_all_words_across_name_and_executable() {
        let app = Application {
            id: "org.test.Editor.desktop".into(),
            name: "Sample Editor".into(),
            description: "Text tools".into(),
            executable: "/opt/sample editor/bin/run".into(),
            command: String::new(),
        };
        assert!(app.matches("EDITOR /opt"));
        assert!(app.matches(""));
        assert!(!app.matches("editor missing"));
    }
}
