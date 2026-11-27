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
    let text = types.iter().any(|t| {
        t.starts_with("text/plain") || matches!(*t, "UTF8_STRING" | "STRING" | "TEXT")
    });
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

/// The clipboard's image and its file extension, when it holds one and no
/// text. Without a Wayland session (or `wl-paste`) there is nothing to read.
pub fn read() -> Option<(Vec<u8>, &'static str)> {
    std::env::var_os("WAYLAND_DISPLAY").filter(|v| !v.is_empty())?;
    let types = wl_paste(&["--list-types"], 64 * 1024, Duration::from_millis(500))?;
    let (mime, extension) = image_offer(&String::from_utf8_lossy(&types))?;
    let data = wl_paste(&["--no-newline", "--type", mime], LIMIT, Duration::from_secs(3))?;
    (!data.is_empty()).then_some((data, extension))
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
        assert_eq!(image_offer("text/html\nimage/webp"), Some(("image/webp", "webp")));
        assert_eq!(image_offer(""), None);
        assert_eq!(image_offer("application/x-kde-cutselection"), None);
    }
}
