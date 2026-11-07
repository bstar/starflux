use super::*;
use std::{
    io::{BufRead, Read, Seek, Write},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
};
pub(super) fn write(output: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
fn parse(bytes: &[u8]) -> std::result::Result<Request, Value> {
    if bytes.len() > LIMIT {
        return Err(error(
            Value::Null,
            "request_too_large",
            "Request exceeds 64 KiB",
            "not_applied",
        ));
    }
    serde_json::from_slice(bytes).map_err(|e| {
        error(
            Value::Null,
            "invalid_request",
            &e.to_string(),
            "not_applied",
        )
    })
}
fn child(request: &Request) -> Result<Value> {
    let mut input = tempfile::tempfile()?;
    input.write_all(&serde_json::to_vec(request)?)?;
    input.rewind()?;
    let mut output = tempfile::tempfile()?;
    let mut process = Command::new(std::env::current_exe()?)
        .arg("--automation-worker")
        .stdin(input)
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let start = Instant::now();
    // Two 512-KiB streams can expand past 6 MiB when JSON escapes control bytes.
    let limit = if request.method == "plugins.job" {
        8
    } else {
        4
    } * 1024
        * 1024;
    let outcome = if mutating(&request.method) {
        "unknown"
    } else {
        "not_applied"
    };
    let result = loop {
        if start.elapsed() > request.timeout() || output.metadata()?.len() > limit {
            unsafe {
                libc::kill(-(process.id() as i32), libc::SIGKILL);
            }
            let _ = process.wait();
            return Ok(error(
                request.id.clone(),
                "deadline_exceeded",
                "Request exceeded its time or response-size limit",
                outcome,
            ));
        }
        if let Some(status) = process.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !result.success() {
        return Ok(error(
            request.id.clone(),
            "worker_failed",
            "Automation worker exited before returning a result",
            outcome,
        ));
    }
    ensure!(
        output.metadata()?.len() <= limit,
        "Automation response exceeds its bounded method limit"
    );
    output.rewind()?;
    let mut bytes = Vec::new();
    output.take(limit + 1).read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).context("Read automation response")
}
fn handle(bytes: &[u8], output: &mut impl Write) -> Result<()> {
    let request = match parse(bytes) {
        Ok(r) => r,
        Err(response) => return write(output, &response),
    };
    if let Err(e) = request.validate() {
        return write(output, &failed(request.id, e));
    }
    if request.method == "events.subscribe" {
        return events::subscribe(&request, output);
    }
    let response = child(&request).unwrap_or_else(|e| {
        error(
            request.id.clone(),
            "transport_error",
            &format!("{e:#}"),
            if mutating(&request.method) {
                "unknown"
            } else {
                "not_applied"
            },
        )
    });
    write(output, &response)
}
pub(super) fn run(request: Option<&str>) -> Result<()> {
    if let Err(e) = crate::plugin_runner::ensure() {
        eprintln!("Plugin workflows: {e:#}");
    }
    if let Err(e) = crate::snapshot_service::ensure() {
        eprintln!("Background snapshots: {e:#}");
    }
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if let Some(request) = request {
        return handle(request.as_bytes(), &mut output);
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    loop {
        let mut bytes = Vec::new();
        let n = input
            .by_ref()
            .take((LIMIT + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if n == 0 {
            return Ok(());
        }
        if bytes.len() > LIMIT {
            write(
                &mut output,
                &error(
                    Value::Null,
                    "request_too_large",
                    "Request exceeds 64 KiB; connection closed",
                    "not_applied",
                ),
            )?;
            return Ok(());
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        handle(&bytes, &mut output)?;
    }
}
pub(super) fn worker() -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    let response = match parse(&bytes) {
        Ok(request) => execute(&request),
        Err(response) => response,
    };
    write(&mut std::io::stdout().lock(), &response)
}
pub(super) fn remote(
    host: &str,
    executable: &str,
    config: Option<&std::path::Path>,
    request: Option<&str>,
) -> Result<()> {
    let mut command = Command::new("ssh");
    if let Some(config) = config {
        command.arg("-F").arg(config);
    }
    command
        .args([
            "-T",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "--",
            host,
        ])
        .arg(shell_words::join([executable, "--api-stdio"]));
    if let Some(request) = request {
        ensure!(request.len() <= LIMIT, "Request exceeds 64 KiB");
        let mut input = tempfile::tempfile()?;
        input.write_all(request.as_bytes())?;
        input.write_all(b"\n")?;
        input.rewind()?;
        command.stdin(input);
    }
    let status = command.status()?;
    ensure!(
        status.success(),
        "SSH automation connection closed ({status})"
    );
    Ok(())
}
