//! Persistent, multiplexed native presentation. The connector owns no source;
//! closing a view/process only detaches watches. No input is replayed on loss.
mod resources;
use crate::{
    native_backend::Pane,
    native_protocol::Reference,
    runtime::Config,
    source_broker::{Palette, Size},
};
use anyhow::{Context, Result, ensure};
use crossbeam_channel::{Receiver, Sender, bounded};
use serde_json::{Value, json};
use starcore::terminal_graphics::terminal_cells::Frame;
use std::{
    collections::{BTreeMap, VecDeque},
    io::{BufReader, Write},
    os::fd::AsRawFd,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
const INPUT_LIMIT: usize = 16 * 1024 * 1024;
#[derive(Clone, PartialEq, Eq)]
struct Settings {
    epoch: u64,
    size: Size,
    palette: Palette,
    visible: bool,
    pixels: bool,
}
struct State {
    id: String,
    source: Reference,
    settings: Mutex<Settings>,
    alive: AtomicBool,
    input: Mutex<VecDeque<Input>>,
    queued: AtomicUsize,
    latest: Mutex<Option<Snapshot>>,
    applied: Mutex<Option<(u64, u64)>>,
    error: Mutex<Option<String>>,
    input_error: Mutex<Option<String>>,
    uncertain_input: AtomicBool,
    /// Wakes this view's connector loop; set once the view joins a hub.
    wake: OnceLock<Sender<()>>,
}
impl State {
    fn poke(&self) {
        if let Some(wake) = self.wake.get() {
            let _ = wake.try_send(());
        }
    }
}
enum InputData {
    Raw(Vec<u8>),
    Paste(String),
}
impl InputData {
    fn len(&self) -> usize {
        match self {
            Self::Raw(v) => v.len(),
            Self::Paste(s) => s.len(),
        }
    }
}
struct Input {
    data: InputData,
    memory: Arc<crate::graphics_resources::Lease>,
}
impl Input {
    fn len(&self) -> usize {
        self.data.len()
    }
}
pub struct Snapshot {
    pub metadata: crate::graphics_owner::metadata::Metadata,
    pub clipboard_floor: u64,
    pub screen: vt100::Screen,
    pub exited: bool,
    pub epoch: u64,
    pub sequence: u64,
    pub memory: Arc<crate::graphics_resources::Lease>,
    pub pixels: Option<Arc<ImagePlanes>>,
}
pub struct ImagePlanes {
    pub wire: crate::graphics_planes::Wire,
    _memory: Arc<crate::graphics_resources::Lease>,
}
pub fn history_memory(
    rows: u16,
    columns: u16,
    bytes: &[u8],
) -> Result<Arc<crate::graphics_resources::Lease>> {
    let retained = (usize::from(rows)
        + bytes.iter().filter(|b| **b == b'\n').count()
        + bytes.len() / usize::from(columns.max(1)))
    .min(4000 + usize::from(rows));
    let charge = retained * usize::from(columns) * 256 + bytes.len() * 4 + 4096;
    Ok(Arc::new(
        resources::shared()
            .presentation
            .reserve(charge)
            .context("Native history presentation is busy")?,
    ))
}
pub struct View {
    state: Arc<State>,
    _hub: Arc<Hub>,
    origin: (Box<Config>, usize, Pane),
}
#[derive(Clone)]
pub struct InputSink(Arc<State>);
impl InputSink {
    fn enqueue(&self, input: InputData) -> Result<()> {
        self.enqueue_with(input, &resources::shared().input)
    }
    fn enqueue_with(
        &self,
        input: InputData,
        budget: &crate::graphics_resources::Budget,
    ) -> Result<()> {
        ensure!(
            input.len() <= 1024 * 1024 + 12,
            "Native input exceeds 1 MiB"
        );
        ensure!(
            self.0.alive.load(Ordering::Acquire),
            "Native view is closed"
        );
        let mut queue = self.0.input.lock().unwrap();
        if let Some(error) = self.0.input_error.lock().unwrap().as_ref() {
            anyhow::bail!("Native input unavailable: {error}");
        }
        let n = input.len();
        ensure!(queue.len() < 1024, "Native input queue is busy");
        let memory = budget.reserve(n).context("Native input budget is busy")?;
        self.0
            .queued
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(n).filter(|v| *v <= INPUT_LIMIT)
            })
            .map_err(|_| anyhow::anyhow!("Native input queue is busy"))?;
        queue.push_back(Input {
            data: input,
            memory: Arc::new(memory),
        });
        crate::latency_trace::mark("1 view-input-queued");
        drop(queue);
        self.0.poke();
        Ok(())
    }
    pub fn raw(&self, bytes: Vec<u8>) -> Result<()> {
        self.enqueue(InputData::Raw(bytes))
    }
    pub fn paste(&self, text: &str) -> Result<()> {
        ensure!(text.len() <= 1024 * 1024, "Paste exceeds 1 MiB");
        self.enqueue(InputData::Paste(text.replace('\x1b', "")))
    }
}
struct Hub {
    key: String,
    views: Mutex<Vec<Weak<State>>>,
    identities: AtomicUsize,
    _slot: crate::graphics_resources::Lease,
    stopped: Arc<AtomicBool>,
    wake: Sender<()>,
}
static HUBS: OnceLock<Mutex<Vec<Weak<Hub>>>> = OnceLock::new();
/// Signalled whenever a native frame is ready for the controller to take, so it
/// ticks at once instead of on its next interval.
static OUTPUT_READY: OnceLock<(Sender<()>, Receiver<()>)> = OnceLock::new();
fn output_ready() -> &'static (Sender<()>, Receiver<()>) {
    OUTPUT_READY.get_or_init(|| bounded(1))
}
pub fn output_ready_receiver() -> Receiver<()> {
    output_ready().1.clone()
}
impl Drop for Hub {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}
impl Drop for View {
    fn drop(&mut self) {
        self.state.alive.store(false, Ordering::Release);
        self.state.poke();
    }
}
fn disconnect(state: &State, error: String) {
    let mut input = state.input.lock().unwrap();
    if state.queued.load(Ordering::Acquire) > 0 {
        state.uncertain_input.store(true, Ordering::Release);
    }
    input.clear();
    state.queued.store(0, Ordering::Release);
    *state.input_error.lock().unwrap() = Some(error.clone());
    *state.error.lock().unwrap() = Some(error);
}
fn palette(theme: &crate::appearance::Appearance) -> Palette {
    let rgb = |s: &str| {
        std::array::from_fn(|i| {
            u8::from_str_radix(s.get(1 + i * 2..3 + i * 2).unwrap_or("00"), 16).unwrap_or(0)
        })
    };
    Palette {
        foreground: rgb(&theme.fg),
        background: rgb(&theme.bg),
        ansi: std::array::from_fn(|i| {
            rgb(theme.ansi.get(i).map(String::as_str).unwrap_or(&theme.fg))
        }),
    }
}
impl View {
    pub fn spawn(config: &Config, pane: usize, target: Pane) -> Result<Self> {
        target.validate()?;
        let source = target
            .binding
            .source
            .clone()
            .context("Native launch is not confirmed")?;
        let theme = crate::settings::Settings::appearance()?;
        let state = Arc::new(State {
            id: uuid::Uuid::new_v4().to_string(),
            source,
            settings: Mutex::new(Settings {
                epoch: 1,
                size: Size {
                    rows: 24,
                    columns: 80,
                    cell: [8, 16],
                },
                palette: palette(&theme),
                visible: true,
                pixels: false,
            }),
            alive: AtomicBool::new(true),
            input: Mutex::new(VecDeque::new()),
            queued: AtomicUsize::new(0),
            latest: Mutex::new(None),
            applied: Mutex::new(None),
            error: Mutex::new(None),
            input_error: Mutex::new(None),
            uncertain_input: AtomicBool::new(false),
            wake: OnceLock::new(),
        });
        let key = serde_json::to_string(&(
            config.remote(pane).then_some(&config.host),
            &config.ssh_config,
            &config.desktop_helper,
            &target.namespace,
            &target.binding.owner,
        ))?;
        let hubs = HUBS.get_or_init(|| Mutex::new(Vec::new()));
        let mut list = hubs.lock().unwrap();
        list.retain(|v| v.strong_count() > 0);
        for hub in list
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|h| h.key == key && !h.stopped.load(Ordering::Acquire))
        {
            let mut views = hub.views.lock().unwrap();
            views.retain(|s| s.upgrade().is_some_and(|s| s.alive.load(Ordering::Acquire)));
            if views.len() < 8 && hub.identities.load(Ordering::Acquire) < 64 {
                hub.identities.fetch_add(1, Ordering::AcqRel);
                views.push(Arc::downgrade(&state));
                drop(views);
                let _ = state.wake.set(hub.wake.clone());
                state.poke();
                return Ok(Self {
                    state,
                    _hub: hub,
                    origin: (Box::new(config.clone()), pane, target),
                });
            }
        }
        let slot = resources::shared()
            .connections
            .reserve(1)
            .context("Native connections are busy")?;
        let mut command = connector(config, pane, &target)?;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child
            .stdin
            .take()
            .context("Native connector input unavailable")?;
        let output = child
            .stdout
            .take()
            .context("Native connector output unavailable")?;
        let (wake, woken) = bounded(1);
        let _ = state.wake.set(wake.clone());
        let hub = Arc::new(Hub {
            key,
            views: Mutex::new(vec![Arc::downgrade(&state)]),
            identities: AtomicUsize::new(1),
            _slot: slot,
            stopped: Arc::new(AtomicBool::new(false)),
            wake,
        });
        list.push(Arc::downgrade(&hub));
        let (tx, rx) = bounded(32);
        let stopped = hub.stopped.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            while !stopped.load(Ordering::Acquire) {
                let result = resources::read(&mut reader, &resources::shared().incoming);
                let failed = result.is_err();
                if tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let weak = Arc::downgrade(&hub);
        std::thread::spawn(move || {
            let result = run(weak.clone(), input, rx, woken);
            if let Some(h) = weak.upgrade() {
                let error = result.err().map_or_else(
                    || "Native view disconnected; input was not replayed".into(),
                    |e| format!("{e:#}"),
                );
                for state in h.views.lock().unwrap().iter().filter_map(Weak::upgrade) {
                    disconnect(&state, error.clone());
                }
                h.stopped.store(true, Ordering::Release);
            }
            let _ = child.kill();
            let _ = child.wait();
        });
        Ok(Self {
            state,
            _hub: hub,
            origin: (Box::new(config.clone()), pane, target),
        })
    }
    pub fn input_uncertain(&self) -> bool {
        self.state.uncertain_input.load(Ordering::Acquire)
    }
    pub fn disconnected(&self) -> bool {
        self._hub.stopped.load(Ordering::Acquire)
    }
    /// Reattach only to the exact retained owner/source. Pending input belongs
    /// to the failed transport and is deliberately absent from the new state.
    pub fn reconnect(&self) -> Result<Self> {
        let next = Self::spawn(&self.origin.0, self.origin.1, self.origin.2.clone())?;
        *next.state.settings.lock().unwrap() = self.state.settings.lock().unwrap().clone();
        Ok(next)
    }
    pub fn input(&self) -> InputSink {
        InputSink(self.state.clone())
    }
    pub fn configure(
        &self,
        rows: u16,
        columns: u16,
        cell: [u16; 2],
        theme: Option<&crate::appearance::Appearance>,
    ) {
        let mut settings = self.state.settings.lock().unwrap();
        let mut next = settings.clone();
        next.size = Size {
            rows,
            columns,
            cell,
        };
        if let Some(theme) = theme {
            next.palette = palette(theme);
        }
        if next != *settings {
            next.epoch = settings.epoch.saturating_add(1);
            *settings = next;
            drop(settings);
            self.state.poke();
        }
    }
    pub fn set_palette(&self, theme: &crate::appearance::Appearance) {
        let s = self.state.settings.lock().unwrap().clone();
        self.configure(s.size.rows, s.size.columns, s.size.cell, Some(theme));
    }
    pub fn pixels(&self, enabled: bool) {
        let mut s = self.state.settings.lock().unwrap();
        if s.pixels != enabled {
            s.pixels = enabled;
            s.epoch = s.epoch.saturating_add(1);
            drop(s);
            self.state.poke();
        }
    }
    pub fn visible(&self, visible: bool) {
        let mut s = self.state.settings.lock().unwrap();
        if s.visible != visible {
            s.visible = visible;
            s.epoch = s.epoch.saturating_add(1);
            drop(s);
            self.state.poke();
        }
    }
    pub fn take(&self) -> Option<Snapshot> {
        let snapshot = self.state.latest.lock().unwrap().take()?;
        crate::latency_trace::mark("10 ui-frame-taken");
        if snapshot.epoch != self.state.settings.lock().unwrap().epoch {
            return None;
        }
        *self.state.applied.lock().unwrap() = Some((snapshot.epoch, snapshot.sequence));
        // The owner sends the next frame only after this acknowledgement.
        self.state.poke();
        Some(snapshot)
    }
    pub fn error(&self) -> Option<String> {
        self.state.error.lock().unwrap().clone()
    }
}
fn connector(config: &Config, pane: usize, target: &Pane) -> Result<Command> {
    let args = vec![
        "--native-source-attach".to_owned(),
        target.namespace.to_string_lossy().into_owned(),
        serde_json::to_string(&target.binding.owner)?,
    ];
    if config.remote(pane) {
        let mut c = crate::native_backend::command(config, pane, "--native-source-attach");
        let mut argv = c.get_args().map(|a| a.to_owned()).collect::<Vec<_>>();
        argv.pop();
        argv.push(
            shell_words::join(
                std::iter::once(config.desktop_helper.as_str())
                    .chain(args.iter().map(String::as_str)),
            )
            .into(),
        );
        let mut command = Command::new(c.get_program());
        command.args(argv);
        c = command;
        Ok(c)
    } else {
        let mut c = Command::new("/proc/self/exe");
        c.args(args);
        Ok(c)
    }
}
struct Packet {
    bytes: Vec<u8>,
    offset: usize,
    _memory: crate::graphics_resources::Lease,
}
struct Canvas {
    parser: vt100::Parser,
    _memory: Option<Arc<crate::graphics_resources::Lease>>,
}
impl Canvas {
    fn empty() -> Self {
        Self {
            parser: vt100::Parser::new(1, 1, 0),
            _memory: None,
        }
    }
}
struct PeerView {
    state: Weak<State>,
    epoch: u64,
    watch: bool,
    source_size: Option<Size>,
    last_resize_size: Option<Size>,
    canvas: Arc<Mutex<Canvas>>,
    conversion: Option<Conversion>,
    sequence: u64,
    inflight: usize,
    tail: Option<(Vec<u8>, usize, Arc<crate::graphics_resources::Lease>)>,
}
enum Pending {
    Hello,
    Resize(String, u64),
    Watch(String, u64),
    Input(String, usize, Arc<crate::graphics_resources::Lease>),
    Inspect(String),
    Other,
}
fn send(
    queue: &mut VecDeque<Packet>,
    pending: &mut BTreeMap<u64, Pending>,
    sequence: &mut u64,
    kind: Pending,
    action: Value,
) -> Result<()> {
    ensure!(pending.len() < 256, "Native connector requests are busy");
    *sequence = sequence
        .checked_add(1)
        .context("Native request sequence exhausted")?;
    let (bytes, memory) = resources::encode(
        &json!({"version":1,"id":*sequence,"action":action}),
        &resources::shared().transport,
    )?;
    ensure!(
        queue
            .iter()
            .map(|p| p.bytes.len() - p.offset)
            .sum::<usize>()
            + bytes.len()
            <= 2 * 1024 * 1024,
        "Native connector output is busy"
    );
    queue.push_back(Packet {
        bytes,
        offset: 0,
        _memory: memory,
    });
    pending.insert(*sequence, kind);
    Ok(())
}
fn watch(
    queue: &mut VecDeque<Packet>,
    pending: &mut BTreeMap<u64, Pending>,
    seq: &mut u64,
    state: &State,
    s: &Settings,
) -> Result<()> {
    send(
        queue,
        pending,
        seq,
        Pending::Watch(state.id.clone(), s.epoch),
        json!({"op":"watch","source":state.source,"view":state.id,"epoch":s.epoch,"generation":s.epoch,"focused":true,"size":s.size,"palette":s.palette,"pixels":s.pixels}),
    )
}
// Reject only work which has not crossed the connector. Partially written and
// acknowledged-as-queued input has an uncertain outcome and must never replay.
fn reject_input(
    state: &State,
    peer: &mut PeerView,
    queue: &mut VecDeque<Packet>,
    pending: &mut BTreeMap<u64, Pending>,
    error: String,
) {
    let mut input = state.input.lock().unwrap();
    *state.input_error.lock().unwrap() = Some(error.clone());
    *state.error.lock().unwrap() = Some(error);
    let mut discarded: usize = input.drain(..).map(|v| v.len()).sum();
    if let Some((bytes, offset, _memory)) = peer.tail.take() {
        discarded += bytes.len() - offset;
    }
    queue.retain(|packet| {
        if packet.offset != 0 {
            return true;
        }
        let id = serde_json::from_slice::<Value>(&packet.bytes)
            .ok()
            .and_then(|v| v["id"].as_u64());
        let Some(id) = id else {
            return true;
        };
        if let Some(Pending::Input(view, n, _)) = pending.get(&id)
            && view == &state.id
        {
            discarded += n;
            peer.inflight = peer.inflight.saturating_sub(*n);
            pending.remove(&id);
            return false;
        }
        true
    });
    state.queued.fetch_sub(discarded, Ordering::AcqRel);
}
struct Conversion {
    metadata: crate::graphics_owner::metadata::Metadata,
    clipboard_floor: u64,
    state: Weak<State>,
    canvas: Arc<Mutex<Canvas>>,
    settings: Settings,
    frame: Frame,
    modes: Value,
    links: crate::native_links::Links,
    sequence: u64,
    exited: bool,
    pixels: Option<crate::graphics_planes::Wire>,
    memory: Arc<crate::graphics_resources::Lease>,
}
fn converter() -> Sender<Conversion> {
    let (tx, rx) = bounded::<Conversion>(8);
    std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let Some(state) = job.state.upgrade() else {
                continue;
            };
            if state.settings.lock().unwrap().epoch != job.settings.epoch {
                continue;
            }
            let mut canvas = job.canvas.lock().unwrap();
            if job.frame.complete {
                *canvas = Canvas {
                    parser: vt100::Parser::new(
                        job.settings.size.rows,
                        job.settings.size.columns,
                        0,
                    ),
                    _memory: Some(job.memory.clone()),
                };
            }
            if let Err(error) = apply(&mut canvas.parser, &job.frame, &job.modes) {
                *state.error.lock().unwrap() = Some(format!("Native frame conversion: {error:#}"));
                continue;
            }
            if let Err(error) = job.links.apply(canvas.parser.screen_mut()) {
                *state.error.lock().unwrap() =
                    Some(format!("Native hyperlink conversion: {error:#}"));
                continue;
            }
            let screen = canvas.parser.screen().visible_snapshot();
            drop(canvas);
            if state.settings.lock().unwrap().epoch == job.settings.epoch {
                crate::latency_trace::mark("9 view-frame-converted");
                *state.latest.lock().unwrap() = Some(Snapshot {
                    metadata: job.metadata,
                    clipboard_floor: job.clipboard_floor,
                    screen,
                    exited: job.exited,
                    epoch: job.settings.epoch,
                    sequence: job.sequence,
                    pixels: job.pixels.map(|wire| {
                        Arc::new(ImagePlanes {
                            wire,
                            _memory: job.memory.clone(),
                        })
                    }),
                    memory: job.memory,
                });
                let _ = output_ready().0.try_send(());
            }
        }
    });
    tx
}
fn run(
    hub: Weak<Hub>,
    mut writer: std::process::ChildStdin,
    incoming: Receiver<Result<resources::Incoming>>,
    wake: Receiver<()>,
) -> Result<()> {
    let fd = writer.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "Cannot configure native connector writes"
    );
    let mut queue = VecDeque::new();
    let mut pending = BTreeMap::new();
    let mut seq = 0;
    let mut hello = false;
    let mut peers: BTreeMap<String, PeerView> = BTreeMap::new();
    let mut retry = Instant::now();
    let conversions = converter();
    send(
        &mut queue,
        &mut pending,
        &mut seq,
        Pending::Hello,
        json!({"op":"hello"}),
    )?;
    loop {
        let Some(h) = hub.upgrade() else {
            return Ok(());
        };
        if h.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        let states = h
            .views
            .lock()
            .unwrap()
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|s| s.alive.load(Ordering::Acquire))
            .collect::<Vec<_>>();
        drop(h);
        for peer in peers.values_mut() {
            if let Some(job) = peer.conversion.take() {
                match conversions.try_send(job) {
                    Ok(()) => (),
                    Err(crossbeam_channel::TrySendError::Full(job)) => peer.conversion = Some(job),
                    Err(_) => anyhow::bail!("Native frame worker stopped"),
                }
            }
        }
        for state in &states {
            peers.entry(state.id.clone()).or_insert_with(|| PeerView {
                state: Arc::downgrade(state),
                epoch: 0,
                watch: false,
                source_size: None,
                last_resize_size: None,
                canvas: Arc::new(Mutex::new(Canvas::empty())),
                conversion: None,
                sequence: 0,
                inflight: 0,
                tail: None,
            });
        }
        let removed = peers
            .iter()
            .filter(|(_, v)| {
                v.state
                    .upgrade()
                    .is_none_or(|s| !s.alive.load(Ordering::Acquire))
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in removed {
            if let Some(v) = peers.remove(&id)
                && v.watch
            {
                send(
                    &mut queue,
                    &mut pending,
                    &mut seq,
                    Pending::Other,
                    json!({"op":"unwatch","view":id,"epoch":v.epoch}),
                )?;
            }
        }
        if hello {
            let presentation_ready = Instant::now() >= retry;
            for state in &states {
                let s = state.settings.lock().unwrap().clone();
                let v = peers.get_mut(&state.id).unwrap();
                if presentation_ready && s.epoch != v.epoch {
                    if !s.visible {
                        if v.watch {
                            send(
                                &mut queue,
                                &mut pending,
                                &mut seq,
                                Pending::Other,
                                json!({"op":"unwatch","view":state.id,"epoch":v.epoch}),
                            )?;
                        }
                        v.epoch = s.epoch;
                        v.watch = false;
                    } else if !pending.values().any(
                        |p| matches!(p,Pending::Resize(id,_)|Pending::Watch(id,_) if id==&state.id),
                    ) {
                        if v.last_resize_size.as_ref() != Some(&s.size) {
                            send(
                                &mut queue,
                                &mut pending,
                                &mut seq,
                                Pending::Resize(state.id.clone(), s.epoch),
                                json!({"op":"resize","source":state.source,"size":s.size}),
                            )?;
                        } else {
                            let mut watch_settings = s.clone();
                            if let Some(source) = &v.source_size {
                                watch_settings.size.rows = s.size.rows.min(source.rows);
                                watch_settings.size.columns = s.size.columns.min(source.columns);
                            }
                            watch(&mut queue, &mut pending, &mut seq, state, &watch_settings)?;
                            v.epoch = s.epoch;
                            v.watch = true;
                            v.sequence = 0;
                        }
                    }
                }
                if let Some((epoch, sequence)) = state.applied.lock().unwrap().take() {
                    send(
                        &mut queue,
                        &mut pending,
                        &mut seq,
                        Pending::Other,
                        json!({"op":"ack","view":state.id,"epoch":epoch,"sequence":sequence}),
                    )?;
                }
                // A terminal follows the view someone is typing in. When another
                // view resized it since this one did, this view shows only the
                // top of a taller grid (cutting off an agent's input line), so
                // take the size back before the input lands.
                if s.visible
                    && !state.input.lock().unwrap().is_empty()
                    && v.last_resize_size
                        .as_ref()
                        .zip(v.source_size.as_ref())
                        .is_some_and(|(mine, source)| {
                            (mine.rows, mine.columns) != (source.rows, source.columns)
                        })
                    && !pending
                        .values()
                        .any(|p| matches!(p, Pending::Resize(id, _) if id == &state.id))
                {
                    v.last_resize_size = None;
                    let mut current = state.settings.lock().unwrap();
                    current.epoch = current
                        .epoch
                        .checked_add(1)
                        .context("View epoch exhausted")?;
                }
                if state.input_error.lock().unwrap().is_none()
                    && v.inflight < 256 * 1024
                    && pending.len() < 128
                {
                    if v.tail.is_none()
                        && let Some(input) = state.input.lock().unwrap().pop_front()
                    {
                        match input.data {
                            InputData::Raw(bytes) => v.tail = Some((bytes, 0, input.memory)),
                            InputData::Paste(text) => {
                                let n = text.len();
                                send(
                                    &mut queue,
                                    &mut pending,
                                    &mut seq,
                                    Pending::Input(state.id.clone(), n, input.memory),
                                    json!({"op":"paste","source":state.source,"text":text}),
                                )?;
                                v.inflight += n;
                            }
                        }
                    }
                    if let Some((bytes, offset, memory)) = v.tail.as_mut() {
                        let end = (*offset + 16384).min(bytes.len());
                        let n = end - *offset;
                        use base64::Engine;
                        send(
                            &mut queue,
                            &mut pending,
                            &mut seq,
                            Pending::Input(state.id.clone(), n, memory.clone()),
                            json!({"op":"input","source":state.source,"data":base64::engine::general_purpose::STANDARD.encode(&bytes[*offset..end])}),
                        )?;
                        v.inflight += n;
                        *offset = end;
                        if end == bytes.len() {
                            v.tail = None;
                        }
                    }
                }
            }
        }
        for _ in 0..32 {
            let message = match incoming.try_recv() {
                Ok(value) => value?,
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(_) => anyhow::bail!("Native connector closed; input delivery is unknown"),
            };
            let value = &message.value;
            if let Some(event) = value["event"].as_str() {
                let Some(id) = value["view"].as_str() else {
                    continue;
                };
                let Some(v) = peers.get_mut(id) else {
                    continue;
                };
                let Some(state) = v.state.upgrade() else {
                    continue;
                };
                let settings = state.settings.lock().unwrap().clone();
                if value["epoch"] != settings.epoch {
                    continue;
                }
                if event == "source_closed" {
                    reject_input(
                        &state,
                        v,
                        &mut queue,
                        &mut pending,
                        "Native source closed".into(),
                    );
                    continue;
                }
                if event == "view_error" {
                    if !pending
                        .values()
                        .any(|p| matches!(p, Pending::Inspect(view) if view == id))
                    {
                        send(
                            &mut queue,
                            &mut pending,
                            &mut seq,
                            Pending::Inspect(id.into()),
                            json!({"op":"inspect","source":state.source}),
                        )?;
                    }
                    *state.error.lock().unwrap() = Some(
                        value["error"]
                            .as_str()
                            .unwrap_or("Native view failed")
                            .into(),
                    );
                    continue;
                }
                if event != "cells" {
                    continue;
                }
                ensure!(
                    value["source"] == serde_json::to_value(&state.source)?,
                    "Native frame source changed"
                );
                let sequence = value["sequence"]
                    .as_u64()
                    .context("Native frame sequence missing")?;
                crate::latency_trace::mark("8 view-frame-received");
                let frame: Frame = serde_json::from_value(value["frame"].clone())?;
                frame.validate()?;
                let metadata_charge = crate::graphics_owner::metadata::Metadata::preflight(
                    &value["terminal_metadata"],
                )?;
                let link_charge = crate::native_links::Links::preflight(
                    &value["links"],
                    settings.size.rows,
                    settings.size.columns,
                )?;
                let mut link_memory = resources::shared()
                    .presentation
                    .reserve(link_charge + metadata_charge)
                    .context("Native hyperlink snapshots are busy")?;
                let links = if value["links"].is_null() {
                    crate::native_links::Links::default()
                } else {
                    serde_json::from_value::<crate::native_links::Links>(value["links"].clone())?
                };
                links.validate(frame.rect.height, frame.rect.width)?;
                let metadata = if value["terminal_metadata"].is_null() {
                    crate::graphics_owner::metadata::Metadata::default()
                } else {
                    serde_json::from_value::<crate::graphics_owner::metadata::Metadata>(
                        value["terminal_metadata"].clone(),
                    )?
                };
                let clipboard_floor = if value["terminal_metadata"].is_null() {
                    0
                } else {
                    value["clipboard_floor"]
                        .as_u64()
                        .context("Invalid terminal clipboard baseline")?
                };
                ensure!(
                    clipboard_floor <= metadata.clipboard_sequence,
                    "Terminal clipboard sequence precedes attachment"
                );
                ensure!(
                    metadata.clipboard.is_some() == (metadata.clipboard_sequence > clipboard_floor),
                    "Terminal clipboard event does not match its baseline"
                );
                let grid: [u16; 2] = serde_json::from_value(value["source_grid"].clone())?;
                ensure!(
                    grid[0] > 0 && grid[0] <= 192 && grid[1] > 0 && grid[1] <= 512,
                    "Invalid retained source grid"
                );
                ensure!(
                    frame.stream == id && frame.generation == settings.epoch,
                    "Native frame viewport changed"
                );
                v.source_size = Some(Size {
                    rows: grid[0],
                    columns: grid[1],
                    cell: settings.size.cell,
                });
                if frame.rect.height != settings.size.rows.min(grid[0])
                    || frame.rect.width != settings.size.columns.min(grid[1])
                {
                    let mut current = state.settings.lock().unwrap();
                    current.epoch = current
                        .epoch
                        .checked_add(1)
                        .context("View epoch exhausted")?;
                    continue;
                }
                if !frame.complete && value["base_sequence"] != v.sequence {
                    anyhow::bail!("Native frame base changed");
                }
                let pixels = if value["pixels"].is_null() {
                    None
                } else {
                    let pixels: crate::graphics_planes::Wire =
                        serde_json::from_value(value["pixels"].clone())?;
                    ensure!(
                        settings.pixels
                            && pixels.width
                                == u32::from(frame.rect.width) * u32::from(settings.size.cell[0])
                            && pixels.height
                                == u32::from(frame.rect.height) * u32::from(settings.size.cell[1])
                            && pixels.bytes() <= 3 * 1024 * 1024,
                        "Native plane geometry changed"
                    );
                    Some(pixels)
                };
                v.sequence = sequence;
                *state.error.lock().unwrap() = None;
                let charge =
                    usize::from(settings.size.columns) * usize::from(settings.size.rows) * 256
                        + frame
                            .rows
                            .iter()
                            .flat_map(|row| &row.runs)
                            .map(|run| run.text.len() * 4)
                            .sum::<usize>()
                        + 4096
                        + links.memory_bytes(settings.size.rows, settings.size.columns)
                        + metadata.memory_bytes();
                ensure!(
                    link_memory.try_resize(charge + pixels.as_ref().map_or(0, |p| p.bytes() * 4)),
                    "Native view snapshots are busy"
                );
                let memory = Arc::new(link_memory);
                v.conversion = Some(Conversion {
                    metadata,
                    clipboard_floor,
                    state: Arc::downgrade(&state),
                    canvas: v.canvas.clone(),
                    settings,
                    frame,
                    modes: value["input_modes"].clone(),
                    links,
                    sequence,
                    exited: value["exited"] == true,
                    pixels,
                    memory,
                });
                continue;
            }
            let Some(id) = value["id"].as_u64() else {
                anyhow::bail!("Invalid native connector response");
            };
            let Some(kind) = pending.remove(&id) else {
                anyhow::bail!("Unknown native connector acknowledgement");
            };
            let ok = value["ok"] == true;
            match kind {
                Pending::Hello => {
                    ensure!(
                        ok && value["result"]["capabilities"].as_array().is_some_and(|v| [
                            "native-paste-v1",
                            "view-grid-v1",
                            "image-planes-v1"
                        ]
                        .iter()
                        .all(|cap| v.iter().any(|v| v == cap))),
                        "Native owner cannot serve view input; kept running"
                    );
                    hello = true;
                }
                Pending::Resize(id, epoch) => {
                    if let Some(v) = peers.get_mut(&id) {
                        if ok {
                            let size: Size =
                                serde_json::from_value(value["result"]["size"].clone())?;
                            v.last_resize_size = Some(size.clone());
                            v.source_size = Some(size);
                        } else {
                            retry = Instant::now() + Duration::from_millis(30);
                        }
                        if let Some(state) = v.state.upgrade()
                            && state.settings.lock().unwrap().epoch != epoch
                        {
                            v.epoch = 0;
                        }
                    }
                }
                Pending::Watch(id, epoch) => {
                    if !ok && let Some(v) = peers.get_mut(&id) {
                        v.epoch = 0;
                        v.watch = false;
                        retry = Instant::now() + Duration::from_millis(100);
                        if let Some(state) = v.state.upgrade() {
                            *state.error.lock().unwrap() =
                                Some(format!("Native watch {epoch}: {}", value["error"]));
                            // Another view can resize between our Resize reply
                            // and Watch request. Re-probe before retrying the
                            // clipped view rather than reusing a stale grid.
                            if !pending
                                .values()
                                .any(|p| matches!(p, Pending::Inspect(view) if view == &id))
                            {
                                send(
                                    &mut queue,
                                    &mut pending,
                                    &mut seq,
                                    Pending::Inspect(id.clone()),
                                    json!({"op":"inspect","source":state.source}),
                                )?;
                            }
                        }
                    }
                }
                Pending::Inspect(id) => {
                    if ok && let Some(v) = peers.get_mut(&id) {
                        v.source_size =
                            Some(serde_json::from_value(value["result"]["size"].clone())?);
                        if let Some(state) = v.state.upgrade() {
                            let mut settings = state.settings.lock().unwrap();
                            settings.epoch = settings
                                .epoch
                                .checked_add(1)
                                .context("View epoch exhausted")?;
                        }
                    }
                }
                Pending::Input(id, n, _memory) => {
                    if let Some(v) = peers.get_mut(&id) {
                        v.inflight = v.inflight.saturating_sub(n);
                        if let Some(state) = v.state.upgrade() {
                            state.queued.fetch_sub(n, Ordering::AcqRel);
                            if !ok {
                                reject_input(
                                    &state,
                                    v,
                                    &mut queue,
                                    &mut pending,
                                    format!("Native input was not accepted: {}", value["error"]),
                                );
                            }
                        }
                    }
                }
                Pending::Other => (),
            }
        }
        for _ in 0..4 {
            let Some(packet) = queue.front_mut() else {
                break;
            };
            match writer.write(&packet.bytes[packet.offset..]) {
                Ok(0) => anyhow::bail!("Native connector input closed"),
                Ok(n) => {
                    crate::latency_trace::mark("2 view-request-written");
                    packet.offset += n;
                    if packet.offset == packet.bytes.len() {
                        queue.pop_front();
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }
        // Sleep until the owner replies or a view has input, an acknowledgement
        // or new settings. Only blocked writes, a busy converter or a pending
        // retry need a short timer; a fixed poll here cost every keystroke.
        if !incoming.is_empty() {
            continue;
        }
        // Hand frames received this turn to the converter now.
        for peer in peers.values_mut() {
            if let Some(job) = peer.conversion.take() {
                match conversions.try_send(job) {
                    Ok(()) => (),
                    Err(crossbeam_channel::TrySendError::Full(job)) => peer.conversion = Some(job),
                    Err(_) => anyhow::bail!("Native frame worker stopped"),
                }
            }
        }
        let blocked = !queue.is_empty() || peers.values().any(|v| v.conversion.is_some());
        let timeout = if blocked {
            Duration::from_millis(2)
        } else {
            match retry.checked_duration_since(Instant::now()) {
                Some(until) => until.clamp(Duration::from_millis(1), Duration::from_millis(50)),
                None => Duration::from_millis(50),
            }
        };
        let mut select = crossbeam_channel::Select::new();
        select.recv(&incoming);
        select.recv(&wake);
        let _ = select.ready_timeout(timeout);
        while wake.try_recv().is_ok() {}
    }
}
fn apply(parser: &mut vt100::Parser, frame: &Frame, modes: &Value) -> Result<()> {
    let mut bytes = b"\x1b]8;;\x1b\\".to_vec();
    for (mode, on) in [
        (1, modes["application_cursor"] == true),
        (66, modes["application_keypad"] == true),
        (2004, modes["bracketed_paste"] == true),
    ] {
        write!(bytes, "\x1b[?{mode}{}", if on { 'h' } else { 'l' })?;
    }
    bytes.extend_from_slice(b"\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l");
    let mouse = match modes["mouse"].as_str() {
        Some("Press") => Some(9),
        Some("PressRelease") => Some(1000),
        Some("ButtonMotion") => Some(1002),
        Some("AnyMotion") => Some(1003),
        _ => None,
    };
    if let Some(mode) = mouse {
        write!(bytes, "\x1b[?{mode}h")?;
    }
    match modes["mouse_encoding"].as_str() {
        Some("Sgr") => bytes.extend_from_slice(b"\x1b[?1006h"),
        Some("Utf8") => bytes.extend_from_slice(b"\x1b[?1005h"),
        _ => (),
    }
    for row in &frame.rows {
        write!(bytes, "\x1b[{};1H", row.index + 1)?;
        for run in &row.runs {
            bytes.extend_from_slice(b"\x1b[0m");
            for (bit, code) in [(1, 1), (2, 2), (4, 3), (8, 4), (16, 7)] {
                if run.attributes & bit != 0 {
                    write!(bytes, "\x1b[{code}m")?;
                }
            }
            let [r, g, b] = run.foreground;
            write!(bytes, "\x1b[38;2;{r};{g};{b}m")?;
            let [r, g, b] = run.background;
            write!(bytes, "\x1b[48;2;{r};{g};{b}m")?;
            bytes.extend_from_slice(run.text.as_bytes());
        }
    }
    if let Some([x, y]) = frame.cursor {
        write!(bytes, "\x1b[{};{}H\x1b[?25h", y + 1, x + 1)?;
    } else {
        bytes.extend_from_slice(b"\x1b[?25l");
    }
    parser.process(&bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Arc<State> {
        Arc::new(State {
            id: uuid::Uuid::new_v4().to_string(),
            source: Reference {
                id: uuid::Uuid::new_v4().to_string(),
                epoch: uuid::Uuid::new_v4().to_string(),
                boot: uuid::Uuid::new_v4().to_string(),
                pid: 2,
                start_ticks: 1,
            },
            settings: Mutex::new(Settings {
                epoch: 1,
                size: Size {
                    rows: 2,
                    columns: 8,
                    cell: [8, 16],
                },
                palette: Palette {
                    foreground: [255; 3],
                    background: [0; 3],
                    ansi: [[0; 3]; 16],
                },
                visible: true,
                pixels: false,
            }),
            alive: AtomicBool::new(true),
            input: Mutex::new(VecDeque::new()),
            queued: AtomicUsize::new(0),
            latest: Mutex::new(None),
            applied: Mutex::new(None),
            error: Mutex::new(None),
            input_error: Mutex::new(None),
            uncertain_input: AtomicBool::new(false),
            wake: OnceLock::new(),
        })
    }
    #[test]
    fn shared_input_admission_covers_multiple_panes_and_releases_on_drop() {
        let budget = crate::graphics_resources::Budget::new(8);
        let first = state();
        let second = state();
        InputSink(first.clone())
            .enqueue_with(InputData::Raw(vec![0; 5]), &budget)
            .unwrap();
        assert!(
            InputSink(second.clone())
                .enqueue_with(InputData::Raw(vec![0; 5]), &budget)
                .is_err()
        );
        assert_eq!(second.queued.load(Ordering::Acquire), 0);
        assert_eq!(budget.used(), 5);
        drop(first);
        assert_eq!(budget.used(), 0);
        InputSink(second.clone())
            .enqueue_with(InputData::Paste("hello".into()), &budget)
            .unwrap();
        let input = second.input.lock().unwrap().pop_front().unwrap();
        let pending = Pending::Input(second.id.clone(), input.len(), input.memory.clone());
        drop(input);
        drop(second);
        assert_eq!(
            budget.used(),
            5,
            "pending delivery must keep its reservation"
        );
        drop(pending);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn disconnect_discards_pending_input_and_records_uncertain_delivery() {
        let state = state();
        let budget = crate::graphics_resources::Budget::new(32);
        InputSink(state.clone())
            .enqueue_with(InputData::Raw(vec![0; 12]), &budget)
            .unwrap();
        disconnect(&state, "connection lost".into());
        assert_eq!(budget.used(), 0);
        assert_eq!(state.queued.load(Ordering::Acquire), 0);
        assert!(state.uncertain_input.load(Ordering::Acquire));
        assert!(InputSink(state).raw(vec![1]).is_err());
    }
    #[test]
    fn rendering_failure_does_not_disable_input() {
        let state = state();
        *state.error.lock().unwrap() = Some("View exceeds retained grid".into());
        InputSink(state.clone())
            .raw(b"still responsive".to_vec())
            .unwrap();
        assert_eq!(state.queued.load(Ordering::Acquire), 16);
    }
    #[test]
    fn rejection_discards_unsent_input_and_preserves_unknown_delivery() {
        let state = state();
        state.input.lock().unwrap().push_back(Input {
            data: InputData::Raw(vec![0; 4]),
            memory: Arc::new(resources::shared().input.reserve(4).unwrap()),
        });
        state.queued.store(25, Ordering::Release);
        let mut peer = PeerView {
            state: Arc::downgrade(&state),
            epoch: 1,
            watch: true,
            source_size: None,
            last_resize_size: None,
            canvas: Arc::new(Mutex::new(Canvas::empty())),
            conversion: None,
            sequence: 1,
            inflight: 15,
            tail: Some((
                vec![0; 8],
                2,
                Arc::new(resources::shared().input.reserve(8).unwrap()),
            )),
        };
        let mut queue = VecDeque::from([
            Packet {
                bytes: b"{\"id\":1}\n".to_vec(),
                offset: 0,
                _memory: resources::shared().transport.reserve(16).unwrap(),
            },
            Packet {
                bytes: b"{\"id\":2}\n".to_vec(),
                offset: 1,
                _memory: resources::shared().transport.reserve(16).unwrap(),
            },
        ]);
        let mut pending = BTreeMap::from([
            (
                1,
                Pending::Input(
                    state.id.clone(),
                    5,
                    Arc::new(resources::shared().input.reserve(5).unwrap()),
                ),
            ),
            (
                2,
                Pending::Input(
                    state.id.clone(),
                    3,
                    Arc::new(resources::shared().input.reserve(3).unwrap()),
                ),
            ),
            (
                3,
                Pending::Input(
                    state.id.clone(),
                    7,
                    Arc::new(resources::shared().input.reserve(7).unwrap()),
                ),
            ),
        ]);
        reject_input(
            &state,
            &mut peer,
            &mut queue,
            &mut pending,
            "source closed".into(),
        );
        assert!(state.input.lock().unwrap().is_empty());
        assert!(peer.tail.is_none());
        assert_eq!(state.queued.load(Ordering::Acquire), 10);
        assert_eq!(peer.inflight, 10);
        assert_eq!(queue.len(), 1);
        assert!(!pending.contains_key(&1));
        assert!(pending.contains_key(&2) && pending.contains_key(&3));
        *state.error.lock().unwrap() = None; // A later successful frame cannot re-enable rejected input.
        assert!(InputSink(state.clone()).raw(vec![1]).is_err());
        assert_eq!(state.queued.load(Ordering::Acquire), 10);
    }
    #[test]
    fn background_conversion_preserves_unicode_modes_and_view_padding() {
        use starcore::terminal_graphics::{
            protocol::Rect,
            terminal_cells::{Row, Run},
        };
        let state = state();
        let tx = converter();
        let frame = Frame {
            stream: state.id.clone(),
            generation: 1,
            rect: Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 1,
            },
            complete: true,
            rows: vec![Row {
                index: 0,
                runs: vec![Run {
                    text: "界x ".into(),
                    columns: 4,
                    foreground: [1, 2, 3],
                    background: [4, 5, 6],
                    attributes: 1,
                }],
            }],
            cursor: Some([3, 0]),
        };
        frame.validate().unwrap();
        let budget = crate::graphics_resources::Budget::new(65536);
        let canvas = Arc::new(Mutex::new(Canvas::empty()));
        tx.send(Conversion {
            state: Arc::downgrade(&state),
            canvas: canvas.clone(),
            settings: state.settings.lock().unwrap().clone(),
            frame,
            modes: json!({"application_cursor":true,"bracketed_paste":true}),
            metadata: crate::graphics_owner::metadata::Metadata::default(),
            clipboard_floor: 0,
            links: crate::native_links::Links::default(),
            sequence: 7,
            exited: false,
            pixels: None,
            memory: Arc::new(budget.reserve(8192).unwrap()),
        })
        .unwrap_or_else(|_| panic!("worker closed"));
        let deadline = Instant::now() + Duration::from_secs(2);
        let snapshot = loop {
            if let Some(snapshot) = state.latest.lock().unwrap().take() {
                break snapshot;
            }
            assert!(Instant::now() < deadline, "conversion did not complete");
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(snapshot.screen.size(), (2, 8));
        assert_eq!(snapshot.screen.cell(0, 0).unwrap().contents(), "界");
        assert!(snapshot.screen.cell(0, 0).unwrap().bold());
        assert_eq!(snapshot.screen.cell(0, 2).unwrap().contents(), "x");
        assert!(snapshot.screen.bracketed_paste());
        assert!(snapshot.screen.application_cursor());
        assert_eq!(snapshot.screen.cursor_position(), (0, 3));
        assert_eq!(snapshot.sequence, 7);
        let ui_memory = snapshot.memory.clone();
        drop(snapshot);
        assert_eq!(
            budget.used(),
            8192,
            "canvas and UI still retain copied pixels/cells"
        );
        drop(canvas);
        assert_eq!(
            budget.used(),
            8192,
            "the accepted UI screen must remain charged"
        );
        drop(ui_memory);
        drop(tx);
        let deadline = Instant::now() + Duration::from_secs(2);
        while budget.used() != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(budget.used(), 0);
    }
}
