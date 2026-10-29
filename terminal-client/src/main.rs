//! Fixed local endpoint used by the STAR/KIT Kitty watcher; no host commands run here.
use anyhow::{Result, ensure};
use starkit::terminal_graphics::{client, protocol::Input};
fn event(event: &starkit::crossterm::event::Event) -> Option<Input> {
    match event {
        starkit::crossterm::event::Event::FocusGained => Some(Input::Notice {
            message: "focus-canvas:terminals".into(),
        }),
        starkit::crossterm::event::Event::Osc72(text) => Some(Input::Osc72 { text: text.clone() }),
        _ => None,
    }
}
fn main() -> Result<()> {
    match run() {
        Err(error) if error.to_string() == "Graphical session: STARFLUX_RELOAD_UI" => Ok(()),
        result => {
            if let Err(error) = &result {
                record_error(error);
            }
            result
        }
    }
}
fn record_error(error: &anyhow::Error) {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let Some(home) = std::env::home_dir() else {
        return;
    };
    let root = home.join(".local/state/starflux");
    let _ = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&root);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(root.join("terminal-client.log"))
    {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(file, "{time}: {error:#}");
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--check-audio") {
        use starkit::terminal_graphics::{
            audio,
            media::{ToClient, ToHost},
            protocol::ClientMessage,
        };
        // A silent device probe: credits after the initial window prove that
        // converted samples reached and drained from the real output ring.
        let (tx, rx) = crossbeam_channel::bounded(16);
        let mut frontend = audio::Frontend::default();
        frontend.receive(&ToClient::AudioOpen {
            session: 1,
            epoch: 1,
        })?;
        let start = std::time::Instant::now();
        let mut supplied = 0;
        while start.elapsed() < std::time::Duration::from_secs(3) {
            frontend.tick(&tx);
            for message in rx.try_iter() {
                match message {
                    ClientMessage::Media {
                        message: ToHost::AudioError { message, .. },
                    } => anyhow::bail!("{message}"),
                    ClientMessage::Media {
                        message: ToHost::AudioCredit { blocks, .. },
                    } => {
                        for _ in 0..blocks {
                            frontend.receive(&ToClient::AudioChunk {
                                session: 1,
                                epoch: 1,
                                samples: vec![0; audio::BLOCK_SAMPLES],
                            })?;
                            supplied += 1;
                        }
                    }
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        frontend.reset();
        ensure!(
            supplied > audio::WINDOW_BLOCKS * 2,
            "Local audio output did not drain"
        );
        println!("Local audio output ready ({supplied} silent PCM blocks)");
        return Ok(());
    }
    if args.len() == 1 || args[1] == "--help" || args[1] == "-h" {
        println!(
            "STAR/FLUX audio-enabled local terminal frontend\nUsage: starflux-terminal-client SOCKET\nStarted by the STAR/KIT Kitty watcher on the listening machine."
        );
        return Ok(());
    }
    ensure!(args.len() == 2, "Expected one private terminal socket");
    client::run_terminal_socket_with_events(std::path::Path::new(&args[1]), event)
}
