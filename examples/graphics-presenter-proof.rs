//! Private local-presenter acceptance. Stdout is captured by the fixture;
//! no Kitty window, display, agent, or existing terminal is addressed.
#[path = "../src/font_frontend.rs"]
mod font_frontend;
#[path = "../src/graphics_delivery.rs"]
mod graphics_delivery;
#[path = "../src/graphics_frontend.rs"]
mod graphics_frontend;
use std::io::{Read, Write};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--relay") {
        return font_frontend::relay(std::path::Path::new(
            args.get(2)
                .ok_or_else(|| anyhow::anyhow!("Missing private relay socket"))?,
        ));
    }
    let text = args.get(1).is_some_and(|arg| arg == "--text");
    let path = args
        .get(if text { 2 } else { 1 })
        .ok_or_else(|| anyhow::anyhow!("Missing private fixture socket"))?;
    let _output = font_frontend::LocalOutput::start()?;
    if text {
        let mut out = std::io::stdout().lock();
        out.write_all(b"\x1b[2J\x1b[0;38;2;255;255;255;48;2;0;0;0m\x1b[5;4HXX\x1b[?25l\x1b]1337;SetUserVar=starflux_pixel_proof=MQ==\x1b\\")?;
        out.flush()?;
    }
    // Match deployment: the protocol relay is a separate process, and sends
    // local image requests to the terminal-owned output service over its socket.
    let mut relay = std::process::Command::new(std::env::current_exe()?)
        .arg("--relay")
        .arg(path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    let mut input = relay.stdout.take().unwrap();
    let mut buffer = [0; 8192];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        let mut output = std::io::stdout().lock();
        output.write_all(&buffer[..n])?;
        output.flush()?;
    }
    anyhow::ensure!(relay.wait()?.success(), "Private image relay failed");
    Ok(())
}
