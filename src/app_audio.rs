//! Isolated app-desktop capture. KIT owns frontend PCM transport and playback.
//! A separate SSH pipe keeps audio independent of PNG/video throughput.
use crate::{
    app_desktop::{command_extra, directory, lock, paths, read, write},
    runtime::Config,
};
use anyhow::{Context, Result, ensure};
use crossbeam_channel::{Receiver, Sender, bounded};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    os::unix::{
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
pub const BLOCK: usize = 1920;
pub const WINDOW: usize = 8;
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Packet {
    Ready,
    Chunk { samples: Vec<i16> },
    Error { message: String },
}
#[derive(Serialize, Deserialize)]
struct Credit {
    blocks: usize,
}
pub fn sink_name(name: &str) -> String {
    format!("star_mux_{}_{}", unsafe { libc::geteuid() }, name)
}
pub fn environment() {
    if std::env::var_os("PULSE_SERVER").is_none() {
        let native = format!("/run/user/{}/pulse/native", unsafe { libc::geteuid() });
        if PathBuf::from(&native).exists() {
            // Internal entry points call this before creating threads.
            unsafe {
                std::env::set_var("PULSE_SERVER", format!("unix:{native}"));
            }
        }
    }
}
fn socket(name: &str) -> Result<PathBuf> {
    paths(name)?;
    Ok(directory()?.join(format!("{name}.audio.sock")))
}
fn owner_alive(owner: u32) -> bool {
    fs::metadata(format!("/proc/{owner}")).is_ok_and(|m| {
        use std::os::unix::fs::MetadataExt;
        m.uid() == unsafe { libc::geteuid() }
    })
}
fn pulse(args: &[&str]) -> Result<String> {
    let result = Command::new("pactl")
        .args(args)
        .output()
        .context("Audio host needs pactl and PipeWire-Pulse or PulseAudio")?;
    ensure!(
        result.status.success(),
        "Audio host: {}",
        String::from_utf8_lossy(&result.stderr).trim()
    );
    Ok(String::from_utf8(result.stdout)?)
}
struct Sink {
    module: String,
}
impl Sink {
    fn new(name: &str) -> Result<Self> {
        let module = pulse(&[
            "load-module",
            "module-null-sink",
            &format!("sink_name={}", sink_name(name)),
            "rate=48000",
            "channels=2",
            "sink_properties=device.description=STAR_MUX_App_Audio",
        ])?;
        let module = module.trim().to_string();
        ensure!(
            !module.is_empty() && module.bytes().all(|b| b.is_ascii_digit()),
            "Invalid audio sink module"
        );
        Ok(Self { module })
    }
}
impl Drop for Sink {
    fn drop(&mut self) {
        let _ = pulse(&["unload-module", &self.module]);
    }
}
fn descendants(owner: u32) -> BTreeSet<u32> {
    let mut pending = vec![owner];
    let mut found = BTreeSet::new();
    while let Some(pid) = pending.pop() {
        if found.len() >= 512 || !found.insert(pid) {
            continue;
        }
        if let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) {
            for task in tasks.flatten().take(512) {
                if let Ok(children) = fs::read_to_string(task.path().join("children")) {
                    pending.extend(
                        children
                            .split_whitespace()
                            .filter_map(|p| p.parse::<u32>().ok()),
                    );
                }
            }
        }
    }
    found
}
fn adopt(name: &str, owner: u32) -> Result<()> {
    // Upgrade already-running desktops without restarting their apps. Only this
    // private compositor's descendants may be moved; never capture a system mix.
    let pids = descendants(owner);
    let inputs: serde_json::Value =
        serde_json::from_str(&pulse(&["--format=json", "list", "sink-inputs"])?)?;
    let sinks: serde_json::Value =
        serde_json::from_str(&pulse(&["--format=json", "list", "sinks"])?)?;
    let name = sink_name(name);
    let index = sinks
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == name))
        .and_then(|s| s["index"].as_u64())
        .context("Private audio sink disappeared")?;
    let native = inputs.as_array().is_some_and(|items| {
        items.iter().any(|i| {
            i["properties"]["application.process.id"].is_null()
                && i["properties"]["client.id"].is_string()
        })
    });
    let clients: BTreeMap<u64, u32> = if native {
        Command::new("pw-dump")
            .output()
            .ok()
            .filter(|r| r.status.success())
            .and_then(|r| serde_json::from_slice::<serde_json::Value>(&r.stdout).ok())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|o| o["type"] == "PipeWire:Interface:Client")
            .filter_map(|o| {
                Some((
                    o["id"].as_u64()?,
                    u32::try_from(
                        o["info"]["props"]["pipewire.sec.pid"]
                            .as_u64()
                            .or_else(|| o["info"]["props"]["application.process.id"].as_u64())?,
                    )
                    .ok()?,
                ))
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    for input in inputs.as_array().into_iter().flatten() {
        let pid = input["properties"]["application.process.id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                input["properties"]["client.id"]
                    .as_str()?
                    .parse::<u64>()
                    .ok()
                    .and_then(|id| clients.get(&id).copied())
            });
        if pid.is_some_and(|pid| pids.contains(&pid))
            && input["sink"].as_u64() != Some(index)
            && let Some(id) = input["index"].as_u64()
        {
            pulse(&["move-sink-input", &id.to_string(), &name])?;
        }
    }
    Ok(())
}
struct Capture {
    child: Child,
    data: Receiver<Vec<i16>>,
    stop: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Capture {
    fn start(name: &str) -> Result<Self> {
        let mut child = Command::new("parec")
            .args([
                "--raw",
                "--format=s16le",
                "--rate=48000",
                "--channels=2",
                "--latency-msec=20",
                &format!("--device={}.monitor", sink_name(name)),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Audio host needs parec")?;
        let mut input = child.stdout.take().unwrap();
        let (tx, data) = bounded(4);
        let drain = data.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let reader = std::thread::spawn(move || {
            let mut bytes = [0u8; BLOCK * 2];
            while !cancel.load(Ordering::Relaxed) && input.read_exact(&mut bytes).is_ok() {
                let samples = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect();
                if tx.is_full() {
                    let _ = drain.try_recv();
                }
                if tx.try_send(samples).is_err() && cancel.load(Ordering::Relaxed) {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            data,
            stop,
            reader: Some(reader),
        })
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(join) = self.reader.take() {
            let _ = join.join();
        }
    }
}
pub fn ensure(name: &str, owner: u32) -> Result<()> {
    let args =
        fs::read(format!("/proc/{owner}/cmdline")).context("App desktop owner is unavailable")?;
    let args: Vec<_> = args.split(|b| *b == 0).collect();
    ensure!(
        owner_alive(owner)
            && args
                .windows(2)
                .any(|a| a[0] == b"--desktop-worker" && a[1] == name.as_bytes()),
        "Audio owner is not this private app desktop"
    );
    let socket = socket(name)?;
    if UnixStream::connect(&socket).is_ok() {
        return Ok(());
    }
    ensure!(owner_alive(owner), "App desktop owner is unavailable");
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory()?.join(format!("{name}.audio.log")))?;
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--desktop-audio-worker", name, &owner.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .process_group(0)
        .spawn()?;
    let start = Instant::now();
    loop {
        if socket.exists() && UnixStream::connect(&socket).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!(
                "App audio startup failed ({status}); check {name}.audio.log. The host needs pactl, parec and a running PulseAudio-compatible server"
            );
        }
        ensure!(
            start.elapsed() < Duration::from_secs(4),
            "App audio startup timed out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub fn internal(mode: &str, name: &str, owner: u32) -> Result<()> {
    if mode == "--desktop-audio-worker" {
        return worker(name, owner);
    }
    ensure(name, owner)?;
    let stream = UnixStream::connect(socket(name)?)?;
    let mut input = stream.try_clone()?;
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut input);
        let _ = input.shutdown(std::net::Shutdown::Both);
    });
    let mut output = std::io::stdout().lock();
    let mut stream = stream;
    let mut bytes = [0; 16384];
    loop {
        let size = stream.read(&mut bytes)?;
        if size == 0 {
            break;
        }
        output.write_all(&bytes[..size])?;
        output.flush()?;
    }
    Ok(())
}
struct AudioViewer {
    socket: UnixStream,
    packets: Sender<Arc<Vec<u8>>>,
    stopped: Arc<AtomicBool>,
    credits: usize,
}
impl AudioViewer {
    fn new(socket: UnixStream) -> Result<Self> {
        let mut writer = socket.try_clone()?;
        writer.set_write_timeout(Some(Duration::from_millis(500)))?;
        let (packets, rx) = bounded::<Arc<Vec<u8>>>(WINDOW);
        let stopped = Arc::new(AtomicBool::new(false));
        let done = stopped.clone();
        std::thread::spawn(move || {
            for bytes in rx {
                if writer.write_all(&bytes).is_err() {
                    break;
                }
            }
            done.store(true, Ordering::Release);
            let _ = writer.shutdown(std::net::Shutdown::Both);
        });
        Ok(Self {
            socket,
            packets,
            stopped,
            credits: 0,
        })
    }
}
impl Drop for AudioViewer {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}
fn worker(name: &str, owner: u32) -> Result<()> {
    let path = socket(name)?;
    let _lease = lock(directory()?.join(format!("{name}.audio.worker.lock")))?;
    ensure!(owner_alive(owner), "App desktop owner is unavailable");
    let _sink = Sink::new(name)?;
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    let (_, stop) = paths(name)?;
    let (tx, rx) = bounded(16);
    let mut generation = 0u64;
    let mut clients: BTreeMap<u64, AudioViewer> = BTreeMap::new();
    let mut capture: Option<Capture> = None;

    let adoption_name = name.to_owned();
    let adoption_stop = stop.clone();
    std::thread::spawn(move || {
        while owner_alive(owner) && !adoption_stop.exists() {
            let _ = adopt(&adoption_name, owner);
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    while owner_alive(owner) && !stop.exists() {
        if let Ok((mut stream, _)) = listener.accept() {
            generation += 1;
            let epoch = generation;
            if clients.len() >= 16 {
                continue;
            }
            stream.set_write_timeout(Some(Duration::from_millis(500)))?;
            if capture.is_none() {
                match Capture::start(name) {
                    Ok(value) => capture = Some(value),
                    Err(error) => {
                        let _ = write(
                            &Packet::Error {
                                message: format!("{error:#}"),
                            },
                            &mut stream,
                        );
                        continue;
                    }
                }
            }
            if write(&Packet::Ready, &mut stream).is_err() {
                continue;
            }
            let mut reader = stream.try_clone()?;
            let tx = tx.clone();
            std::thread::spawn(move || {
                while let Ok(credit) = read::<Credit>(&mut reader) {
                    if tx.send((epoch, Some(credit.blocks))).is_err() {
                        return;
                    }
                }
                let _ = tx.send((epoch, None));
            });
            clients.insert(epoch, AudioViewer::new(stream)?);
        }
        for (epoch, grant) in rx.try_iter() {
            if let Some(viewer) = clients.get_mut(&epoch) {
                if let Some(grant) = grant {
                    viewer.credits = viewer.credits.saturating_add(grant.min(WINDOW)).min(WINDOW);
                } else {
                    clients.remove(&epoch);
                }
            }
        }
        clients.retain(|_, v| !v.stopped.load(Ordering::Acquire));
        if clients.is_empty() {
            capture = None;
        }
        if let Some(source) = capture.as_mut() {
            if let Some(status) = source.child.try_wait()? {
                eprintln!("App audio capture exited ({status})");
                clients.clear();
                capture = None;
            } else if let Ok(samples) = source.data.try_recv() {
                let mut bytes = vec![];
                write(&Packet::Chunk { samples }, &mut bytes)?;
                let bytes = Arc::new(bytes);
                for viewer in clients.values_mut() {
                    if viewer.credits > 0 {
                        if viewer.packets.try_send(bytes.clone()).is_ok() {
                            viewer.credits -= 1;
                        } else {
                            viewer.stopped.store(true, Ordering::Release);
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(capture);
    let _ = fs::remove_file(path);
    Ok(())
}
pub struct Source {
    pub data: Receiver<Vec<i16>>,
    pub errors: Receiver<String>,
    credit: Option<Sender<usize>>,
    child: Child,
    stop: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Source {
    pub fn start(config: &Config, owner: u32) -> Result<Self> {
        let mut child = command_extra(config, "--desktop-audio-relay", &[&owner.to_string()])?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (credit, rx) = bounded::<usize>(WINDOW);
        let (tx, data) = bounded::<Vec<i16>>(WINDOW);
        let (error, errors) = bounded(2);
        let failed = error.clone();
        std::thread::spawn(move || {
            let mut bytes = vec![];
            let _ = stderr.by_ref().take(4096).read_to_end(&mut bytes);
            if !bytes.is_empty() {
                let _ = failed.try_send(String::from_utf8_lossy(&bytes).trim().into());
            }
        });
        std::thread::spawn(move || {
            while let Ok(blocks) = rx.recv() {
                if write(&Credit { blocks }, &mut input).is_err() {
                    break;
                }
            }
        });
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let reader = std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                ensure!(
                    matches!(read::<Packet>(&mut output)?, Packet::Ready),
                    "Invalid app audio handshake"
                );
                while !cancel.load(Ordering::Relaxed) {
                    match read::<Packet>(&mut output)? {
                        Packet::Chunk { samples } => {
                            ensure!(samples.len() == BLOCK, "Invalid app audio block");
                            ensure!(
                                tx.try_send(samples).is_ok(),
                                "App audio exceeded its credit window"
                            );
                        }
                        Packet::Error { message } => anyhow::bail!("{message}"),
                        _ => anyhow::bail!("Unexpected app audio packet"),
                    }
                }
                Ok(())
            })();
            if !cancel.load(Ordering::Relaxed)
                && let Err(e) = result
            {
                let _ = error.try_send(format!("App audio: {e:#}"));
            }
        });
        Ok(Self {
            data,
            errors,
            credit: Some(credit),
            child,
            stop,
            reader: Some(reader),
        })
    }
    pub fn grant(&self, blocks: usize) {
        if let Some(tx) = &self.credit {
            let _ = tx.try_send(blocks.min(WINDOW));
        }
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.credit = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(join) = self.reader.take() {
            let _ = join.join();
        }
    }
}

#[cfg(test)]
mod viewer_tests {
    use super::*;
    #[test]
    fn audio_connections_have_independent_credits_and_lifetimes() {
        let (first, _first_peer) = UnixStream::pair().unwrap();
        let (second, mut second_peer) = UnixStream::pair().unwrap();
        second_peer
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let mut first = AudioViewer::new(first).unwrap();
        let mut second = AudioViewer::new(second).unwrap();
        first.credits = 0;
        second.credits = WINDOW;
        let mut bytes = vec![];
        write(
            &Packet::Chunk {
                samples: vec![42; BLOCK],
            },
            &mut bytes,
        )
        .unwrap();
        second.packets.send(Arc::new(bytes)).unwrap();
        assert!(
            matches!(read::<Packet>(&mut second_peer).unwrap(),Packet::Chunk{samples} if samples==vec![42;BLOCK])
        );
        assert_eq!(first.credits, 0);
        drop(first);
        assert!(!second.stopped.load(Ordering::Acquire));
    }
}
