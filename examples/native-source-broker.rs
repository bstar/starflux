//! Private native-source worker/stdio transport acceptance helper. Not an
//! installed application backend. Source commands arrive only over private IPC.
#[path = "../src/graphics_decode.rs"]
mod graphics_decode;
#[path = "../src/graphics_owner.rs"]
mod graphics_owner;
#[path = "../src/graphics_paint.rs"]
mod graphics_paint;
#[path = "../src/graphics_reply.rs"]
mod graphics_reply;
#[path = "../src/graphics_requests.rs"]
mod graphics_requests;
#[path = "../src/graphics_resources.rs"]
mod graphics_resources;
#[path = "../src/graphics_store.rs"]
mod graphics_store;
#[path = "../src/graphics_stream.rs"]
mod graphics_stream;
#[path = "../src/graphics_transfer.rs"]
mod graphics_transfer;
#[path = "../src/source_broker.rs"]
#[cfg(target_os = "linux")]
mod source_broker;
#[cfg(target_os = "linux")]
use anyhow::{Context, Result, ensure};
#[cfg(target_os = "linux")]
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        net::UnixStream,
    },
    path::Path,
};
#[cfg(target_os = "linux")]
fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("--native-source-service") if args.len() == 3 => {
            source_broker::service::run(Path::new(&args[2]))
        }
        Some("--discover") if args.len() == 3 => {
            let handle =
                source_broker::service::ensure(Path::new(&args[2]), &std::env::current_exe()?)?;
            println!("{}", serde_json::to_string(&handle.owner)?);
            Ok(())
        }

        Some("--native-source-stdio") if args.len() == 3 => {
            source_broker::service::stdio(Path::new(&args[2]), &std::env::current_exe()?, None)
        }
        Some("--native-source-attach") if args.len() == 4 => source_broker::service::stdio(
            Path::new(&args[2]),
            &std::env::current_exe()?,
            Some(&serde_json::from_str(&args[3])?),
        ),
        Some("--slot-read") if args.len() == 5 => {
            let record =
                source_broker::service::read_slot(Path::new(&args[2]), &args[3], args[4].parse()?)?;
            println!("{}", serde_json::to_string(&record)?);
            Ok(())
        }
        Some("--slot-reattach") if args.len() == 6 => {
            let record = source_broker::service::reattach_slot(
                Path::new(&args[2]),
                &args[3],
                args[4].parse()?,
                &args[5],
            )?;
            println!("{}", serde_json::to_string(&record)?);
            Ok(())
        }
        Some("--slot-open") if args.len() == 6 => {
            #[derive(serde::Deserialize)]
            struct Input {
                launch: source_broker::Launch,
                expected: Option<source_broker::service::Binding>,
            }
            let mut bytes = Vec::new();
            std::io::stdin().take(32769).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 32768, "Private fixture request too large");
            let input: Input = serde_json::from_slice(&bytes)?;
            let handle =
                source_broker::service::ensure(Path::new(&args[2]), &std::env::current_exe()?)?;
            let record = handle.open_slot(
                &args[3],
                args[4].parse()?,
                &args[5],
                &input.launch,
                input.expected.as_ref(),
            )?;
            println!("{}", serde_json::to_string(&record)?);
            Ok(())
        }
        Some("--serve") if args.len() == 5 => {
            let mut file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&args[4])?;
            let m = file.metadata()?;
            ensure!(
                m.is_file() && m.uid() == unsafe { libc::getuid() } && m.mode() & 0o077 == 0,
                "Broker key file is not private"
            );
            let mut token = String::new();
            Read::by_ref(&mut file)
                .take(65)
                .read_to_string(&mut token)?;
            source_broker::Broker::bind(Path::new(&args[2]), Path::new(&args[3]), token)?.run()
        }
        Some("--relay") if args.len() == 3 => {
            let mut socket = UnixStream::connect(&args[2])?;
            let mut input = socket.try_clone()?;
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut std::io::stdin().lock(), &mut input);
                let _ = input.shutdown(std::net::Shutdown::Write);
            });
            let mut output = std::io::stdout().lock();
            let mut bytes = [0; 8192];
            loop {
                let n = socket.read(&mut bytes)?;
                if n == 0 {
                    break;
                }
                output.write_all(&bytes[..n])?;
                output.flush()?;
            }
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "Expected --serve SOCKET RECEIPTS KEYFILE or --relay SOCKET"
        ))
        .context("Private native broker proof"),
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The private native source broker proof requires Linux pidfds");
    std::process::exit(2);
}
