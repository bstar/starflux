//! PTY-backed terminals drawn as STAR/KIT text surfaces, without a display server.
use anyhow::{Context, Result, ensure};
use crossbeam_channel::{Receiver, Sender, bounded};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use starkit::native_surface::{PixelRect as R, Primitive, Surface};
use std::{
    fs,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct FontMetrics {
    pub millipixels: u32,
    pub cell: [u16; 2],
    pub sizes: [[u16; 2]; 23],
}
impl FontMetrics {
    pub fn valid(&self) -> bool {
        (4000..=96000).contains(&self.millipixels)
            && std::iter::once(&self.cell)
                .chain(&self.sizes)
                .all(|s| s.iter().all(|n| (1..=256).contains(n)))
    }
}
const PAD: u16 = 12;
const PASTE_LIMIT: usize = 1024 * 1024;

pub fn direct() -> bool {
    std::env::var("STAR_TERMINAL_RENDERER").as_deref() != Ok("kitty-pixels")
}

pub fn socket(project: &str, pane: usize) -> Result<PathBuf> {
    Ok(crate::runtime::root()?.join(format!(
        "t-{}-{}",
        std::process::id(),
        socket_suffix(project, pane)
    )))
}
pub(crate) fn socket_suffix(project: &str, pane: usize) -> String {
    use sha2::{Digest, Sha256};
    // Project names and temporary runtime roots can together exceed sun_path.
    // Keep the process discriminator while bounding the project contribution.
    let digest = Sha256::digest(project.as_bytes());
    let key = digest[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!("{key}-{pane}.sock")
}

#[derive(Clone)]
struct Writer {
    tx: Sender<Vec<u8>>,
    bracketed: Arc<AtomicBool>,
    queued: Arc<AtomicUsize>,
}
impl Writer {
    fn send(&self, bytes: Vec<u8>) -> Result<()> {
        ensure!(
            bytes.len() <= PASTE_LIMIT + 12,
            "Terminal input exceeds 1 MiB"
        );
        let size = bytes.len();
        self.queued
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                count
                    .checked_add(size)
                    .filter(|total| *total <= 16 * PASTE_LIMIT)
            })
            .map_err(|_| anyhow::anyhow!("Terminal input is busy"))?;
        if let Err(error) = self.tx.try_send(bytes) {
            self.queued.fetch_sub(size, Ordering::Relaxed);
            return Err(error).context("Terminal input is busy or disconnected");
        }
        Ok(())
    }
    fn paste(&self, text: &str) -> Result<()> {
        ensure!(text.len() <= PASTE_LIMIT, "Paste exceeds 1 MiB");
        let text = text.replace('\x1b', "");
        self.send(if self.bracketed.load(Ordering::Acquire) {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.into_bytes()
        })
    }
}

pub fn paste(path: &Path, text: &str) -> Result<()> {
    ensure!(text.len() <= PASTE_LIMIT, "Paste exceeds 1 MiB");
    let mut stream = UnixStream::connect(path).context("Direct terminal is disconnected")?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    stream.write_all(&(text.len() as u32).to_be_bytes())?;
    stream.write_all(text.as_bytes())?;
    let mut reply = [0];
    stream.read_exact(&mut reply)?;
    ensure!(
        reply[0] == 1,
        "Direct terminal rejected paste; retry after it drains input"
    );
    Ok(())
}

#[derive(Default)]
struct Replies {
    bytes: Vec<u8>,
    clipboard: Option<String>,
    palette: Option<[String; 2]>,
}
impl vt100::Callbacks for Replies {
    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, _: &[u8], data: &[u8]) {
        use base64::Engine;
        if data.len() <= 87384
            && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
        {
            self.clipboard = String::from_utf8(bytes).ok();
        }
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i: Option<u8>,
        j: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let p = params.first().and_then(|v| v.first()).copied().unwrap_or(0);
        let reply = match (i, j, c, p) {
            (None, None, 'n', 5) => "\x1b[0n".into(),
            (None, None, 'n', 6) => {
                let (r, c) = screen.cursor_position();
                format!("\x1b[{};{}R", r + 1, c + 1)
            }
            (None, None, 'c', _) => "\x1b[?1;2c".into(),
            (Some(b'>'), None, 'c', _) => "\x1b[>0;1;0c".into(),
            // Explicitly report unsupported private modes instead of enabling protocols we cannot implement.
            (Some(b'?'), Some(b'$'), 'p', _) => format!("\x1b[?{p};0$y"),
            _ => String::new(),
        };
        if self.bytes.len() + reply.len() <= 8192 {
            self.bytes.extend_from_slice(reply.as_bytes());
        }
    }
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        if params.get(1) == Some(&b"?".as_slice()) {
            let color = match params.first().copied() {
                Some(b"10") => self
                    .palette
                    .as_ref()
                    .map(|p| p[0].as_str())
                    .unwrap_or("#dcd7ba"),
                Some(b"11") => self
                    .palette
                    .as_ref()
                    .map(|p| p[1].as_str())
                    .unwrap_or("#1f1f28"),
                _ => return,
            };
            let components = (1..7)
                .step_by(2)
                .map(|i| color[i..i + 2].repeat(2))
                .collect::<Vec<_>>()
                .join("/");
            let reply = format!(
                "\x1b]{};rgb:{components}\x1b\\",
                String::from_utf8_lossy(params[0])
            );
            if self.bytes.len() + reply.len() <= 8192 {
                self.bytes.extend_from_slice(reply.as_bytes());
            }
        }
    }
}

struct AttachmentChild(Box<dyn Child + Send + Sync>);
impl Drop for AttachmentChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub struct Terminal {
    parser: vt100::Parser<Replies>,
    master: Box<dyn MasterPty + Send>,
    child: AttachmentChild,
    writer: Writer,
    output: Receiver<Vec<u8>>,
    path: PathBuf,
    stopped: Arc<AtomicBool>,
    pub exited: bool,
    pub exited_successfully: bool,
    pub pid: Option<u32>,
    font_size: u16,
    metrics: Option<FontMetrics>,
    selection: Option<((u16, u16), (u16, u16))>,
    mouse_button: Option<u8>,
    clipboard: Option<String>,
    padding: [u16; 2],
    native_previous: Option<starkit::terminal_graphics::terminal_cells::Frame>,
    native_dirty: bool,
    native_cell: Option<[u16; 2]>,
}
impl Terminal {
    pub fn spawn(argv: &[String], path: PathBuf) -> Result<Self> {
        ensure!(!argv.is_empty(), "Empty terminal attachment command");
        let pair = native_pty_system().openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 800,
            pixel_height: 528,
        })?;
        let mut command = CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        // Attachment helpers choose the project's directory themselves. Match
        // ordinary subprocesses here instead of portable-pty's implicit HOME,
        // which need not exist in a service or isolated package builder.
        command.cwd(std::env::current_dir()?);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        for name in [
            "KITTY_WINDOW_ID",
            "KITTY_PID",
            "KITTY_LISTEN_ON",
            "TERM_PROGRAM",
            "TERM_PROGRAM_VERSION",
            "TMUX",
            "TMUX_PANE",
            "WINDOWID",
            "DISPLAY",
            "WAYLAND_DISPLAY",
        ] {
            command.env_remove(name);
        }
        let child = AttachmentChild(pair.slave.spawn_command(command)?);
        drop(pair.slave);
        let pid = child.0.process_id();
        let mut reader = pair.master.try_clone_reader()?;
        let mut output_writer = pair.master.take_writer()?;
        // Keystrokes must survive a normal input burst before the writer is
        // scheduled. Keep the old 16 MiB byte budget independently of message count.
        let (input_tx, input_rx) = bounded::<Vec<u8>>(1024);
        let queued = Arc::new(AtomicUsize::new(0));
        let writer_queued = queued.clone();
        std::thread::spawn(move || {
            for bytes in input_rx {
                let size = bytes.len();
                let result = output_writer
                    .write_all(&bytes)
                    .and_then(|_| output_writer.flush());
                writer_queued.fetch_sub(size, Ordering::Relaxed);
                if result.is_err() {
                    break;
                }
            }
        });
        let (tx, output) = bounded(32);
        std::thread::spawn(move || {
            let mut buffer = [0; 8192];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let writer = Writer {
            tx: input_tx,
            bracketed: Arc::new(AtomicBool::new(false)),
            queued,
        };
        let stopped = Arc::new(AtomicBool::new(false));
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let done = stopped.clone();
        let input = writer.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    let accepted = (|| -> Result<()> {
                        let mut size = [0; 4];
                        stream.read_exact(&mut size)?;
                        let size = u32::from_be_bytes(size) as usize;
                        ensure!(size <= PASTE_LIMIT, "Paste too large");
                        let mut bytes = vec![0; size];
                        stream.read_exact(&mut bytes)?;
                        input.paste(std::str::from_utf8(&bytes)?)
                    })();
                    let _ = stream.write_all(&[u8::from(accepted.is_ok())]);
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
        Ok(Self {
            parser: vt100::Parser::new_with_callbacks(24, 80, 1000, Replies::default()),
            master: pair.master,
            child,
            writer,
            output,
            path,
            stopped,
            exited: false,
            exited_successfully: false,
            pid,
            font_size: crate::project_state::default_terminal_font_size(),
            metrics: None,
            selection: None,
            mouse_button: None,
            clipboard: None,
            padding: [PAD; 2],
            native_previous: None,
            native_dirty: true,
            native_cell: None,
        })
    }
    pub fn set_metrics(&mut self, metrics: Option<FontMetrics>) {
        self.metrics = metrics.filter(FontMetrics::valid);
    }
    fn font_millipixels(&self) -> u32 {
        if self.font_size == 0 {
            self.metrics.map(|m| m.millipixels).unwrap_or(16000)
        } else {
            u32::from(self.font_size) * 1000
        }
    }
    fn cell_size(&self) -> [u16; 2] {
        if let Some(cell) = self.native_cell {
            return cell;
        }
        if let Some(m) = self.metrics {
            if self.font_size == 0 {
                m.cell
            } else {
                m.sizes[usize::from(self.font_size.clamp(10, 32) - 10)]
            }
        } else {
            // Compatibility for old frontends which cannot report font measurements.
            let size = if self.font_size == 0 {
                16
            } else {
                self.font_size
            };
            [(size * 10 + 8) / 16, (size * 22 + 8) / 16]
        }
    }
    fn surface(&self, width: u16, height: u16, background: &str) -> Surface {
        let mut surface = Surface::new(width, height, background.into()).with_physical_text();
        surface.cell_size = Some(self.cell_size());
        // Additive wire hint: the pinned older KIT ignores it; the installed
        // candidate keeps fractional font sizes and shapes complete terminal runs.
        let mut value = serde_json::to_value(&surface).expect("native surface serialization");
        value["terminal_font_millipixels"] = serde_json::json!(self.font_millipixels());
        serde_json::from_value(value).expect("native terminal surface")
    }
    pub fn set_font_size(&mut self, font_size: u16) {
        self.font_size = if font_size == 0 {
            0
        } else {
            font_size.clamp(10, 32)
        };
    }
    pub fn set_palette(&mut self, theme: &crate::appearance::Appearance) {
        self.native_dirty = true;
        self.parser.callbacks_mut().palette = Some([theme.fg.clone(), theme.bg.clone()]);
    }
    pub fn poll(&mut self) -> Result<()> {
        for bytes in self.output.try_iter().take(32) {
            self.parser.process(&bytes);
            self.native_dirty = true;
        }
        self.writer
            .bracketed
            .store(self.parser.screen().bracketed_paste(), Ordering::Release);
        let reply = std::mem::take(&mut self.parser.callbacks_mut().bytes);
        if !reply.is_empty() {
            self.writer.send(reply)?;
        }
        if let Some(text) = self.parser.callbacks_mut().clipboard.take() {
            self.clipboard = Some(text);
        }
        if let Some(status) = self.child.0.try_wait()? {
            self.exited = true;
            self.exited_successfully = status.success();
        }
        Ok(())
    }
    pub fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        let cols = (width.saturating_sub(PAD * 2) / self.cell_size()[0]).clamp(2, 512);
        let rows = (height.saturating_sub(PAD * 2) / self.cell_size()[1]).clamp(1, 192);
        if self.parser.screen().size() != (rows, cols) {
            self.selection = None;
            self.parser.screen_mut().set_size(rows, cols);
            self.master.resize(PtySize {
                rows,
                cols,
                pixel_width: width,
                pixel_height: height,
            })?;
        }
        Ok(())
    }
    pub fn reset_native(&mut self) {
        self.native_previous = None;
        self.native_dirty = true;
    }
    pub fn resize_native(
        &mut self,
        rect: starkit::terminal_graphics::protocol::Rect,
        padding: [u16; 2],
        cell: [u16; 2],
    ) -> Result<()> {
        self.padding = padding;
        self.native_cell = Some(cell);
        self.font_size = 0; // The parent terminal owns the font and cell metrics.
        let rows = rect.height.clamp(1, 192);
        let cols = rect.width.clamp(2, 512);
        if self.parser.screen().size() != (rows, cols) {
            self.parser.screen_mut().set_size(rows, cols);
            self.master.resize(PtySize {
                rows,
                cols,
                pixel_width: cols * self.cell_size()[0],
                pixel_height: rows * self.cell_size()[1],
            })?;
            self.native_previous = None;
            self.native_dirty = true;
            self.selection = None;
        }
        Ok(())
    }
    pub fn native_frame(
        &mut self,
        stream: String,
        generation: u64,
        rect: starkit::terminal_graphics::protocol::Rect,
        theme: &crate::appearance::Appearance,
        focused: bool,
    ) -> Option<starkit::terminal_graphics::terminal_cells::Frame> {
        use starkit::terminal_graphics::terminal_cells::{Frame, Row, Run};
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        let cursor = (focused
            && !screen.hide_cursor()
            && screen.scrollback() == 0
            && row < rect.height
            && col < rect.width)
            .then_some([col, row]);
        if !self.native_dirty
            && let Some(old) = self.native_previous.as_mut().filter(|old| {
                old.stream == stream && old.generation == generation && old.rect == rect
            })
        {
            if old.cursor == cursor {
                return None;
            }
            old.cursor = cursor;
            return Some(Frame {
                stream,
                generation,
                rect,
                complete: false,
                rows: vec![],
                cursor,
            });
        }
        self.native_dirty = false;
        let rgb = |value: String| -> [u8; 3] {
            std::array::from_fn(|i| {
                u8::from_str_radix(value.get(1 + i * 2..3 + i * 2).unwrap_or("00"), 16).unwrap_or(0)
            })
        };
        let screen = self.parser.screen();
        let mut rows = Vec::with_capacity(usize::from(rect.height));
        for row in 0..rect.height {
            let mut runs: Vec<Run> = vec![];
            let mut previous_style = None;
            for col in 0..rect.width {
                let cell = screen.cell(row, col)?;
                if cell.is_wide_continuation() {
                    if let Some(run) = runs.last_mut() {
                        run.columns += 1;
                    }
                    continue;
                }
                let selected = self.selected(row, col);
                let attributes = u8::from(cell.bold())
                    | (u8::from(cell.dim()) << 1)
                    | (u8::from(cell.italic()) << 2)
                    | (u8::from(cell.underline()) << 3)
                    | (u8::from(cell.inverse() && !selected) << 4);
                let style = (cell.fgcolor(), cell.bgcolor(), attributes, selected);
                let text = if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                };
                if previous_style == Some(style) {
                    let run = runs.last_mut().unwrap();
                    run.columns += 1;
                    run.text.push_str(text);
                } else {
                    let fg = rgb(color(cell.fgcolor(), &theme.fg, theme));
                    let bg = rgb(if selected {
                        theme.selection.clone()
                    } else {
                        color(cell.bgcolor(), &theme.bg, theme)
                    });
                    runs.push(Run {
                        text: text.into(),
                        columns: 1,
                        foreground: fg,
                        background: bg,
                        attributes,
                    });
                    previous_style = Some(style);
                }
            }
            rows.push(Row { index: row, runs });
        }
        let (row, col) = screen.cursor_position();
        let cursor = (focused
            && !screen.hide_cursor()
            && screen.scrollback() == 0
            && row < rect.height
            && col < rect.width)
            .then_some([col, row]);
        let complete = self.native_previous.as_ref().is_none_or(|old| {
            old.stream != stream || old.generation != generation || old.rect != rect
        });
        let changed = if complete {
            rows.clone()
        } else {
            rows.iter()
                .zip(&self.native_previous.as_ref().unwrap().rows)
                .filter(|(a, b)| a != b)
                .map(|(row, _)| row.clone())
                .collect()
        };
        if !complete
            && changed.is_empty()
            && self.native_previous.as_ref().unwrap().cursor == cursor
        {
            return None;
        }
        self.native_previous = Some(Frame {
            stream: stream.clone(),
            generation,
            rect,
            complete: true,
            rows,
            cursor,
        });
        Some(Frame {
            stream,
            generation,
            rect,
            complete,
            rows: changed,
            cursor,
        })
    }
    pub fn text(&self) -> String {
        self.parser.screen().contents()
    }
    pub fn release(&mut self) {
        if self.mouse_button.take().is_some()
            && self.parser.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None
        {
            let _ = self.writer.send(b"\x1b[<0;1;1m".to_vec());
        }
    }
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }
    pub fn key(&mut self, code: &str, modifiers: u8) -> Result<()> {
        self.native_dirty = true;
        if modifiers & 3 == 3 && code.eq_ignore_ascii_case("char:c") {
            self.copy_selection();
            return Ok(());
        }
        self.selection = None;
        self.parser.screen_mut().set_scrollback(0);
        self.writer.send(key_bytes(
            code,
            modifiers,
            self.parser.screen().application_cursor(),
        ))
    }
    pub fn paste(&self, text: &str) -> Result<()> {
        self.writer.paste(text)
    }
    pub fn pointer(
        &mut self,
        action: &str,
        button: u8,
        point: [f64; 2],
        modifiers: u8,
    ) -> Result<()> {
        if action != "move" || self.selection.is_some() {
            self.native_dirty = true;
        }
        let screen = self.parser.screen();
        let x = ((point[0] - f64::from(self.padding[0])).max(0.) / f64::from(self.cell_size()[0]))
            as u16
            + 1;
        let y = ((point[1] - f64::from(self.padding[1])).max(0.) / f64::from(self.cell_size()[1]))
            as u16
            + 1;
        if screen.mouse_protocol_mode() == vt100::MouseProtocolMode::None || modifiers & 1 != 0 {
            if action.starts_with("scroll_") {
                self.selection = None;
                let offset = screen.scrollback();
                self.parser
                    .screen_mut()
                    .set_scrollback(if action == "scroll_up" {
                        offset + 3
                    } else {
                        offset.saturating_sub(3)
                    });
            } else {
                let (rows, cols) = screen.size();
                let cell = (
                    y.saturating_sub(1).min(rows - 1),
                    x.saturating_sub(1).min(cols - 1),
                );
                match action {
                    "down" if button == 0 => {
                        self.selection = Some((cell, cell));
                        self.mouse_button = Some(0);
                    }
                    "move" if self.mouse_button.is_some() => {
                        if let Some((_, end)) = &mut self.selection {
                            *end = cell;
                        }
                    }
                    "up" => {
                        self.mouse_button = None;
                        if let Some((_, end)) = &mut self.selection {
                            *end = cell;
                        }
                        self.copy_selection();
                    }
                    _ => {}
                }
            }
            return Ok(());
        }
        if action == "down" {
            self.mouse_button = Some(button);
        }
        if action == "move"
            && screen.mouse_protocol_mode() != vt100::MouseProtocolMode::AnyMotion
            && !(screen.mouse_protocol_mode() == vt100::MouseProtocolMode::ButtonMotion
                && self.mouse_button.is_some())
        {
            return Ok(());
        }
        if action == "up" {
            self.mouse_button = None;
            if screen.mouse_protocol_mode() == vt100::MouseProtocolMode::Press {
                return Ok(());
            }
        }
        let mut b = match action {
            "scroll_up" => 64,
            "scroll_down" => 65,
            "move" => {
                32 + self
                    .mouse_button
                    .map(|b| match b {
                        1 => 2,
                        2 => 1,
                        _ => 0,
                    })
                    .unwrap_or(3)
            }
            _ => match button {
                1 => 2,
                2 => 1,
                _ => 0,
            },
        };
        if modifiers & 1 != 0 {
            b += 4;
        }
        if modifiers & 4 != 0 {
            b += 8;
        }
        if modifiers & 2 != 0 {
            b += 16;
        }
        let bytes = if screen.mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr {
            format!(
                "\x1b[<{b};{x};{y}{}",
                if action == "up" { 'm' } else { 'M' }
            )
            .into_bytes()
        } else {
            vec![
                27,
                b'[',
                b'M',
                32 + if action == "up" { 3 } else { b },
                (x.min(223) + 32) as u8,
                (y.min(223) + 32) as u8,
            ]
        };
        self.writer.send(bytes)
    }
    fn copy_selection(&mut self) {
        if let Some((a, b)) = self.selection.filter(|(a, b)| a != b) {
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            let text = self
                .parser
                .screen()
                .contents_between(a.0, a.1, b.0, b.1 + 1);
            self.clipboard = Some(text.chars().take(65536).collect());
        }
    }
    fn selected(&self, row: u16, col: u16) -> bool {
        self.selection.is_some_and(|(a, b)| {
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            a != b && (row, col) >= a && (row, col) <= b
        })
    }
    /// Cell-only composition for popup overlays; never rasterize terminal text.
    pub fn paint_cells(
        &self,
        buffer: &mut starkit::ratatui::buffer::Buffer,
        rect: starkit::terminal_graphics::protocol::Rect,
        theme: &crate::appearance::Appearance,
    ) {
        use starkit::ratatui::style::{Color, Modifier, Style};
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        for row in 0..rows.min(rect.height) {
            for col in 0..cols.min(rect.width) {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let selected = self.selected(row, col);
                let fg = color(cell.fgcolor(), &theme.fg, theme)
                    .parse::<Color>()
                    .unwrap_or(Color::Reset);
                let bg = if selected {
                    theme.selection.clone()
                } else {
                    color(cell.bgcolor(), &theme.bg, theme)
                }
                .parse::<Color>()
                .unwrap_or(Color::Reset);
                let mut style = Style::default().fg(fg).bg(bg);
                for (enabled, modifier) in [
                    (cell.bold(), Modifier::BOLD),
                    (cell.dim(), Modifier::DIM),
                    (cell.italic(), Modifier::ITALIC),
                    (cell.underline(), Modifier::UNDERLINED),
                    (cell.inverse() && !selected, Modifier::REVERSED),
                ] {
                    if enabled {
                        style = style.add_modifier(modifier);
                    }
                }
                let x = rect.x + col;
                let y = rect.y + row;
                if x < buffer.area.width && y < buffer.area.height {
                    buffer.set_stringn(
                        x,
                        y,
                        if cell.has_contents() {
                            cell.contents()
                        } else {
                            " "
                        },
                        usize::from((rect.width - col).min(buffer.area.width - x)),
                        style,
                    );
                }
            }
        }
    }
    pub fn paint(
        &self,
        rect: R,
        theme: &crate::appearance::Appearance,
        surfaces: &mut Vec<Surface>,
    ) {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let mut surface = self.surface(rect.width, rect.height, &theme.bg);
        for row in 0..rows {
            let mut col = 0;
            while col < cols {
                let cell = screen.cell(row, col).unwrap();
                let start = col;
                let mut text = String::new();
                let style = (
                    cell.fgcolor(),
                    cell.bgcolor(),
                    cell.bold(),
                    cell.inverse(),
                    cell.underline(),
                    self.selected(row, col),
                );
                while col < cols {
                    let c = screen.cell(row, col).unwrap();
                    if (
                        c.fgcolor(),
                        c.bgcolor(),
                        c.bold(),
                        c.inverse(),
                        c.underline(),
                        self.selected(row, col),
                    ) != style
                    {
                        break;
                    }
                    if !c.is_wide_continuation() {
                        text.push_str(if c.has_contents() { c.contents() } else { " " });
                    }
                    col += 1;
                }
                let x = PAD + start * self.cell_size()[0];
                let y = PAD + row * self.cell_size()[1];
                let r = R::new(
                    x,
                    y,
                    ((col - start) * self.cell_size()[0]).min(rect.width.saturating_sub(x)),
                    self.cell_size()[1].min(rect.height.saturating_sub(y)),
                );
                if r.width == 0 || r.height == 0 {
                    continue;
                }
                let mut fg = color(style.0, &theme.fg, theme);
                let mut bg = color(style.1, &theme.bg, theme);
                if style.3 {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if style.5 {
                    bg = theme.selection.clone();
                    fg = theme.fg.clone();
                }
                if bg != theme.bg {
                    surface.fill(r, &bg, 0);
                }
                if !text.trim().is_empty() {
                    surface.nodes.push(Primitive::Text {
                        rect: r,
                        text,
                        color: fg.clone(),
                        size: (self.font_millipixels() as f32 / 1000.).round() as u16,
                        bold: style.2,
                        mono: true,
                    });
                }
                if style.4 {
                    surface.fill(R::new(r.x, r.y + r.height - 2, r.width, 1), &fg, 0);
                }
            }
            // Bound each native surface even for adversarial alternating cell styles.
            if surface.nodes.len() > 2500 {
                surfaces.push(surface);
                surface = self.surface(rect.width, rect.height, "transparent");
            }
        }
        if !screen.hide_cursor() && screen.scrollback() == 0 {
            let (row, col) = screen.cursor_position();
            let x = PAD + col * self.cell_size()[0];
            let y = PAD + row * self.cell_size()[1];
            if x + self.cell_size()[0] <= rect.width && y + self.cell_size()[1] <= rect.height {
                surface.nodes.push(Primitive::Border {
                    rect: R::new(x, y, self.cell_size()[0], self.cell_size()[1]),
                    color: theme.accent.clone(),
                    radius: 0,
                });
            }
        }
        surfaces.push(surface);
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = fs::remove_file(&self.path);
    }
}
fn color(value: vt100::Color, default: &str, theme: &crate::appearance::Appearance) -> String {
    match value {
        vt100::Color::Default => default.into(),
        vt100::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        vt100::Color::Idx(i) if i < 16 => theme.ansi[i as usize].clone(),
        vt100::Color::Idx(i) if i >= 232 => {
            let n = 8 + (i - 232) * 10;
            format!("#{n:02x}{n:02x}{n:02x}")
        }
        vt100::Color::Idx(i) => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            format!(
                "#{:02x}{:02x}{:02x}",
                level(n / 36),
                level(n / 6 % 6),
                level(n % 6)
            )
        }
    }
}
fn key_bytes(code: &str, modifiers: u8, application: bool) -> Vec<u8> {
    let shift = modifiers & 1 != 0;
    let alt = modifiers & 4 != 0;
    let control = modifiers & 2 != 0;
    let m = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(control);
    if let Some(c) = code.strip_prefix("char:").and_then(|s| s.chars().next()) {
        let c = if shift {
            r#"`1234567890-=[]\;',./"#
                .chars()
                .zip(r##"~!@#$%^&*()_+{}|:"<>?"##.chars())
                .find_map(|(base, shifted)| (base == c).then_some(shifted))
                .unwrap_or_else(|| c.to_ascii_uppercase())
        } else {
            c
        };
        let mut out = vec![];
        if alt {
            out.push(27);
        }
        if control && (c.is_ascii_alphabetic() || " @[_\\]^?".contains(c)) {
            out.push(if c == '?' {
                127
            } else {
                (c.to_ascii_uppercase() as u8) & 31
            });
        } else {
            out.extend_from_slice(c.to_string().as_bytes());
        }
        return out;
    }
    let suffix = match code {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(s) = suffix {
        return if m > 1 {
            format!("\x1b[1;{m}{s}")
        } else {
            format!("\x1b{}{s}", if application { 'O' } else { '[' })
        }
        .into_bytes();
    }
    let num = match code {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        _ => code
            .strip_prefix("f:")
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| {
                [0, 0, 0, 0, 0, 15, 17, 18, 19, 20, 21, 23, 24]
                    .get(n)
                    .copied()
            })
            .filter(|n| *n != 0),
    };
    if let Some(n) = num {
        return if m > 1 {
            format!("\x1b[{n};{m}~")
        } else {
            format!("\x1b[{n}~")
        }
        .into_bytes();
    }
    if let Some(n) = code
        .strip_prefix("f:")
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=4).contains(n))
    {
        let s = (b'P' + n - 1) as char;
        return if m > 1 {
            format!("\x1b[1;{m}{s}")
        } else {
            format!("\x1bO{s}")
        }
        .into_bytes();
    }
    let bytes = match code {
        "enter" => b"\r".as_slice(),
        "tab" if shift => b"\x1b[Z",
        "tab" => b"\t",
        "backtab" => b"\x1b[Z",
        "backspace" => b"\x7f",
        "escape" => b"\x1b",
        _ => b"",
    };
    let mut out = vec![];
    if alt && !bytes.is_empty() {
        out.push(27);
    }
    out.extend_from_slice(bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_pty_frames_preserve_unicode_styles_and_only_changed_rows() {
        let temp = tempfile::tempdir().unwrap();
        let mut terminal =
            Terminal::spawn(&["cat".into()], temp.path().join("native.sock")).unwrap();
        let rect = starkit::terminal_graphics::protocol::Rect {
            x: 5,
            y: 7,
            width: 20,
            height: 4,
        };
        terminal.resize_native(rect, [12, 12], [8, 19]).unwrap();
        terminal
            .parser
            .process("\x1b[1;31mHi世界\x1b[0m\r\nline2".as_bytes());
        let theme = crate::appearance::Appearance::load("kanagawa").unwrap();
        let first = terminal
            .native_frame("native".into(), 1, rect, &theme, true)
            .unwrap();
        first.validate().unwrap();
        let mut backdrop = starkit::ratatui::buffer::Buffer::empty(
            starkit::ratatui::layout::Rect::new(0, 0, 25, 12),
        );
        terminal.paint_cells(&mut backdrop, rect, &theme);
        assert_eq!(backdrop[(5, 7)].symbol(), "H");
        assert_eq!(backdrop[(7, 7)].symbol(), "世");
        assert_eq!(backdrop[(9, 7)].symbol(), "界");
        assert!(
            backdrop[(5, 7)]
                .modifier
                .contains(starkit::ratatui::style::Modifier::BOLD)
        );
        assert_eq!(
            backdrop[(5, 7)].fg,
            starkit::ratatui::style::Color::Rgb(
                first.rows[0].runs[0].foreground[0],
                first.rows[0].runs[0].foreground[1],
                first.rows[0].runs[0].foreground[2]
            )
        );
        assert!(first.complete);
        assert_eq!(first.rows.len(), 4);
        assert!(
            first.rows[0]
                .runs
                .iter()
                .any(|run| run.text.contains("Hi世界") && run.attributes & 1 != 0)
        );
        assert!(
            terminal
                .native_frame("native".into(), 1, rect, &theme, true)
                .is_none()
        );
        let unfocused = terminal
            .native_frame("native".into(), 1, rect, &theme, false)
            .unwrap();
        assert!(unfocused.rows.is_empty());
        assert_eq!(unfocused.cursor, None);
        let focused = terminal
            .native_frame("native".into(), 1, rect, &theme, true)
            .unwrap();
        assert!(focused.rows.is_empty());
        assert!(focused.cursor.is_some());
        terminal.parser.process(b"\x1b[2;1HChanged");
        terminal.native_dirty = true;
        let next = terminal
            .native_frame("native".into(), 1, rect, &theme, true)
            .unwrap();
        next.validate().unwrap();
        assert!(!next.complete);
        assert_eq!(next.rows.len(), 1);
        assert_eq!(next.rows[0].index, 1);
        terminal.reset_native();
        assert!(
            terminal
                .native_frame("native".into(), 1, rect, &theme, true)
                .unwrap()
                .complete
        );
    }
    #[test]
    fn pty_resize_paste_and_native_surface_contract() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paste");
        let program = "import os,sys,tty,time;tty.setraw(0);os.write(1,b'\\x1b[?2004hREADY\\r\\n\\x1b[31mUnicode: caf\\xc3\\xa9 \\xe4\\xb8\\x96\\xe7\\x95\\x8c');data=os.read(0,4096);open(sys.argv[1],'wb').write(data);time.sleep(5)";
        let mut terminal = Terminal::spawn(
            &[
                "python3".into(),
                "-c".into(),
                program.into(),
                path.display().to_string(),
            ],
            dir.path().join("input.sock"),
        )
        .unwrap();
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while !terminal.text().contains("READY") {
            terminal.poll().unwrap();
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        paste(&terminal.path, "café 世界").unwrap();
        while !path.exists() {
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            fs::read(path).unwrap(),
            "\x1b[200~café 世界\x1b[201~".as_bytes()
        );
        terminal.pointer("down", 0, [13., 13.], 0).unwrap();
        terminal.pointer("move", 0, [53., 13.], 0).unwrap();
        terminal.pointer("up", 0, [53., 13.], 0).unwrap();
        assert_eq!(terminal.take_clipboard().as_deref(), Some("READY"));
        let theme = crate::appearance::Appearance::load("kanagawa").unwrap();
        for (width, height, font_size) in [
            (200, 100, 10),
            (1000, 700, 16),
            (1000, 700, 24),
            (8000, 4000, 32),
        ] {
            terminal.set_font_size(font_size);
            terminal.resize(width, height).unwrap();
            let mut surfaces = vec![];
            terminal.paint(R::new(0, 0, width, height), &theme, &mut surfaces);
            for surface in surfaces {
                surface.validate().unwrap();
            }
        }
    }
    #[test]
    fn menu_keys_and_modified_arrows_use_terminal_sequences() {
        assert_eq!(key_bytes("down", 0, false), b"\x1b[B");
        assert_eq!(key_bytes("up", 0, true), b"\x1bOA");
        assert_eq!(key_bytes("left", 2, false), b"\x1b[1;5D");
        assert_eq!(key_bytes("char:c", 2, false), [3]);
        assert_eq!(key_bytes("char:é", 4, false), "\x1bé".as_bytes());
        assert_eq!(key_bytes("char:A", 0, false), b"A");
        assert_eq!(key_bytes("char:!", 1, false), b"!");
        assert_eq!(key_bytes("char:1", 1, false), b"!");
        assert_eq!(key_bytes("char:a", 1, false), b"A");
        assert_eq!(key_bytes("char:a", 2, false), [1]);
    }
    #[test]
    fn fragmented_queries_and_alternate_screen_preserve_state() {
        let mut p = vt100::Parser::new_with_callbacks(24, 80, 20, Replies::default());
        for part in [
            b"hello\x1b[".as_slice(),
            b"6n\x1b[?1049h\x1b[31mMENU",
            b"\x1b[?1049l",
        ] {
            p.process(part);
        }
        assert_eq!(p.callbacks().bytes, b"\x1b[1;6R");
        assert!(p.screen().contents().contains("hello"));
        assert!(!p.screen().contents().contains("MENU"));
    }
}
