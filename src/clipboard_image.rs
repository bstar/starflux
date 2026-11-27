//! Images on the desktop clipboard, read by the process that presents panes.
//!
//! Agents such as Claude Code read clipboard images themselves on Ctrl+V. That
//! fails in a pane whose environment has no desktop session (a terminal opened
//! by an owner started from a console, or any pane on an SSH host). STAR/FLUX
//! always runs inside the user's desktop session, so it reads the image here,
//! stages it as a file and pastes the path, the same way a dropped image is
//! delivered. Text on the clipboard is left to the pane's own Ctrl+V.

use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Largest clipboard image delivered; matches the dropped-image limit.
const LIMIT: usize = 32 * 1024 * 1024;

/// The image type to request when the clipboard holds an image and no text.
/// Screenshots offer only image types. A copy that also offers text (a
/// spreadsheet cell, a web page selection) keeps Ctrl+V's ordinary meaning.
pub fn image_offer(types: &str) -> Option<(&'static str, &'static str)> {
    let types: Vec<&str> = types.lines().map(str::trim).collect();
    let text = types
        .iter()
        .any(|t| t.starts_with("text/plain") || matches!(*t, "UTF8_STRING" | "STRING" | "TEXT"));
    if text {
        return None;
    }
    [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
        ("image/gif", "gif"),
    ]
    .into_iter()
    .find(|(mime, _)| types.contains(mime))
}

/// Run `wl-paste` with `args`, returning at most `limit` bytes of its output,
/// or nothing when it fails or outlives `timeout`.
fn wl_paste(args: &[&str], limit: usize, timeout: Duration) -> Option<Vec<u8>> {
    let mut child = crate::host_env::command("wl-paste")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let read = (&mut stdout).take(limit as u64 + 1).read_to_end(&mut data);
        read.ok().map(|_| data)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let data = reader.join().ok().flatten()?;
    (status?.success() && data.len() <= limit).then_some(data)
}

/// The clipboard's image type and file extension, when it holds an image
/// and no text. Without a Wayland session (or `wl-paste`) there is none.
pub fn offered() -> Option<(&'static str, &'static str)> {
    std::env::var_os("WAYLAND_DISPLAY").filter(|v| !v.is_empty())?;
    let types = wl_paste(&["--list-types"], 64 * 1024, Duration::from_millis(500))?;
    image_offer(&String::from_utf8_lossy(&types))
}

/// The clipboard's data of type `mime`.
pub fn data(mime: &str) -> Option<Vec<u8>> {
    let data = wl_paste(
        &["--no-newline", "--type", mime],
        LIMIT,
        Duration::from_secs(3),
    )?;
    (!data.is_empty()).then_some(data)
}

/// The clipboard's image and its file extension, when it holds one and no text.
pub fn read() -> Option<(Vec<u8>, &'static str)> {
    let (mime, extension) = offered()?;
    Some((data(mime)?, extension))
}

/// `--receive-clipboard-image <extension>` on an SSH host: store the image on
/// stdin in a private directory and print its path, for the window there to
/// deliver like a dropped file. Images older than an hour are removed.
pub fn receive(extension: &str) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::io::Write;
    ensure!(
        matches!(extension, "png" | "jpg" | "webp" | "gif"),
        "Unsupported clipboard image type"
    );
    let mut data = Vec::new();
    std::io::stdin()
        .lock()
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut data)?;
    ensure!(!data.is_empty(), "The clipboard image is empty");
    ensure!(data.len() <= LIMIT, "The clipboard image is over 32 MiB");
    let root = crate::runtime::root()?.join("clipboard-images");
    std::fs::create_dir_all(&root)?;
    std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    let hour = std::time::SystemTime::now() - Duration::from_secs(3600);
    for entry in std::fs::read_dir(&root)?.flatten() {
        if entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|modified| modified < hour)
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    let stage = tempfile::Builder::new()
        .prefix("clip-")
        .tempdir_in(&root)?
        .keep();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let path = stage.join(format!("clipboard-{stamp}.{extension}"));
    std::fs::write(&path, &data)?;
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{}",
        path.to_str().context("Clipboard path must be UTF-8")?
    )?;
    Ok(())
}

/// Where a window served from an SSH host takes clipboard images.
pub struct Remote {
    pub host: String,
    pub ssh_config: Option<std::path::PathBuf>,
    pub executable: String,
}

/// Copy this computer's clipboard image to the SSH host; returns the path
/// there. `mime`/`extension` come from [`offered`].
pub fn send(remote: &Remote, mime: &str, extension: &str) -> anyhow::Result<String> {
    use anyhow::{Context, bail, ensure};
    use std::io::Write;
    let data = data(mime).context("The clipboard image could not be read")?;
    let mut command = std::process::Command::new("ssh");
    if let Some(path) = &remote.ssh_config {
        command.arg("-F").arg(path);
    }
    let mut child = command
        .args(["-T", "-o", "ConnectTimeout=10", "--", &remote.host])
        .arg(shell_words::join([
            remote.executable.as_str(),
            "--receive-clipboard-image",
            extension,
        ]))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .context("Missing clipboard transfer stream")?
        .write_all(&data)?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let path = String::from_utf8(out.stdout)?.trim().to_owned();
    ensure!(
        path.starts_with('/') && !path.chars().any(char::is_control),
        "The SSH host returned an invalid clipboard path"
    );
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::image_offer;

    #[test]
    fn screenshots_are_images_and_text_keeps_ctrl_v() {
        assert_eq!(image_offer("image/png\n"), Some(("image/png", "png")));
        assert_eq!(
            image_offer("image/jpeg\nimage/png\n"),
            Some(("image/png", "png"))
        );
        assert_eq!(image_offer("text/plain;charset=utf-8\nimage/png\n"), None);
        assert_eq!(image_offer("UTF8_STRING\nimage/png"), None);
        assert_eq!(
            image_offer("text/html\nimage/webp"),
            Some(("image/webp", "webp"))
        );
        assert_eq!(image_offer(""), None);
        assert_eq!(image_offer("application/x-kde-cutselection"), None);
    }
}
