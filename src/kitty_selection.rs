//! Read the inner Kitty selection over its private local control socket.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};
const LIMIT: usize = 1024 * 1024;
pub fn read(project: &str, pane: usize) -> Result<String> {
    let socket = crate::runtime::root()?.join(format!("kitty-{project}-{pane}.sock"));
    let mut stream = UnixStream::connect(socket).context("Kitty selection socket")?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let command = json!({"cmd":"get-text","version":[0,35,0],"no_response":false,
        "payload":{"extent":"selection","ansi":false,"clear_selection":false}});
    stream.write_all(b"\x1bP@kitty-cmd")?;
    serde_json::to_writer(&mut stream, &command)?;
    stream.write_all(b"\x1b\\")?;
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let n = stream.read(&mut chunk)?;
        ensure!(n > 0, "Kitty selection response ended early");
        bytes.extend_from_slice(&chunk[..n]);
        ensure!(bytes.len() <= LIMIT * 6 + 4096, "Selection exceeds 1 MiB");
        if bytes.ends_with(b"\x1b\\") {
            break;
        }
    }
    let prefix = b"\x1bP@kitty-cmd";
    ensure!(
        bytes.starts_with(prefix),
        "Invalid Kitty selection response"
    );
    let response: Value = serde_json::from_slice(&bytes[prefix.len()..bytes.len() - 2])?;
    ensure!(
        response["ok"] == true,
        "Kitty selection: {}",
        response["error"]
    );
    let text = response["data"]
        .as_str()
        .context("Missing Kitty selection text")?
        .to_owned();
    ensure!(text.len() <= LIMIT, "Selection exceeds 1 MiB");
    Ok(text)
}
