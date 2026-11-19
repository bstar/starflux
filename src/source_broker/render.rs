//! Bounded, off-loop presentation. Snapshots share immutable image allocations;
//! render/output budgets never borrow the decoder's working reservation.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct Snapshot {
    screen: vt100::Screen,
    store: Option<crate::graphics_store::Store>,
    history: Option<u16>,
    source: Reference,
    revision: u64,
    modes: Value,
    metadata: crate::graphics_owner::metadata::Metadata,
    exited: bool,
    input_available: bool,
    size: Size,
    palette: Palette,
    _memory: Lease,
}
#[derive(Clone)]
pub(super) enum Target {
    Request {
        peer: u64,
        id: u64,
    },
    Watch {
        clipboard_floor: u64,
        peer: u64,
        view: String,
        epoch: u64,
        sequence: u64,
        generation: u64,
        focused: bool,
        pixels: bool,
    },
}
impl Target {
    pub fn peer(&self) -> u64 {
        match self {
            Self::Request { peer, .. } | Self::Watch { peer, .. } => *peer,
        }
    }
}
pub(super) struct Job {
    pub tag: u64,
    pub target: Target,
    pub snapshot: Snapshot,
    pub cancelled: Arc<AtomicBool>,
    pub base: Option<(u64, Arc<Picture>)>,
}
pub(super) struct Packet {
    pub bytes: Vec<u8>,
    pub offset: usize,
    pub watch: Option<(String, u64)>,
    pub _memory: Lease,
}
pub(super) struct Picture {
    pub clipboard_sequence: u64,
    pub planes: Option<Arc<crate::graphics_planes::Encoded>>,
    pub frame: starcore::terminal_graphics::terminal_cells::Frame,
    _memory: Lease,
}
pub(super) struct Rendered {
    pub packet: Packet,
    pub picture: Option<Arc<Picture>>,
}
pub(super) struct Finished {
    pub tag: u64,
    pub target: Target,
    pub source: Reference,
    pub result: std::result::Result<Rendered, String>,
}
pub(super) struct Renderer {
    jobs: crossbeam_channel::Sender<Job>,
    done: crossbeam_channel::Receiver<Finished>,
}
impl Snapshot {
    pub fn capture_screen(source: &Source, history: u16, resources: &Resources) -> Result<Self> {
        ensure!(history <= 1000, "Native history exceeds retained row limit");
        let memory = resources
            .presentation
            .reserve(
                (usize::from(source.size.rows) + usize::from(history))
                    * usize::from(source.size.columns)
                    * 128
                    + 1024 * 1024,
            )
            .context("Native screen snapshots are busy")?;
        Ok(Self {
            screen: source
                .owner
                .screen()
                .retained_snapshot(usize::from(history)),
            store: None,
            history: Some(history),
            source: source.reference.clone(),
            revision: source.revision,
            modes: source.modes(),
            metadata: source.owner.metadata().projection(u64::MAX),
            size: source.size.clone(),
            palette: Palette {
                foreground: [0; 3],
                background: [0; 3],
                ansi: [[0; 3]; 16],
            },
            exited: source.exit,
            input_available: !source.exit
                && !source.eof
                && !source.input_failed
                && source.stopping_at.is_none(),
            _memory: memory,
        })
    }
    pub fn capture(
        source: &Source,
        size: Size,
        palette: Palette,
        clipboard_floor: u64,
        resources: &Resources,
    ) -> Result<Self> {
        let geometry = size.geometry()?;
        ensure!(
            geometry.rows <= source.size.rows && geometry.columns <= source.size.columns,
            "View exceeds the retained grid"
        );
        // A cell can retain a compact glyph, style and optional shared link.
        // Include worst-case frame run/text metadata before allocating a clone.
        let memory = resources
            .presentation
            .reserve(
                usize::from(source.size.rows) * usize::from(source.size.columns) * 256
                    + 1024 * 1024,
            )
            .context("Presentation snapshots are busy")?;
        Ok(Self {
            screen: source.owner.screen().visible_snapshot(),
            store: Some(source.owner.store().clone()),
            history: None,
            source: source.reference.clone(),
            revision: source.revision,
            modes: source.modes(),
            metadata: source.owner.metadata().projection(clipboard_floor),
            exited: source.exit,
            input_available: !source.exit
                && !source.eof
                && !source.input_failed
                && source.stopping_at.is_none(),
            size,
            palette,
            _memory: memory,
        })
    }
}
struct Bounded {
    bytes: Vec<u8>,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > OUTPUT {
            return Err(io::Error::other("Presentation response exceeds limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[derive(Serialize)]
struct WireFrame<'a> {
    stream: &'a str,
    generation: u64,
    rect: starcore::terminal_graphics::protocol::Rect,
    complete: bool,
    rows: Vec<&'a starcore::terminal_graphics::terminal_cells::Row>,
    cursor: Option<[u16; 2]>,
}
impl<'a> WireFrame<'a> {
    fn new(
        frame: &'a starcore::terminal_graphics::terminal_cells::Frame,
        base: Option<&Picture>,
    ) -> Self {
        let base = base.filter(|p| {
            p.frame.stream == frame.stream
                && p.frame.generation == frame.generation
                && p.frame.rect == frame.rect
        });
        let rows = frame
            .rows
            .iter()
            .filter(|row| {
                base.is_none_or(|p| p.frame.rows.get(usize::from(row.index)) != Some(*row))
            })
            .collect();
        Self {
            stream: &frame.stream,
            generation: frame.generation,
            rect: frame.rect,
            complete: base.is_none(),
            rows,
            cursor: frame.cursor,
        }
    }
}
#[derive(Serialize)]
struct Payload<'a> {
    source: &'a Reference,
    revision: u64,
    input_modes: &'a Value,
    source_grid: [u16; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pixels: Option<&'a crate::graphics_planes::Wire>,
    frame: WireFrame<'a>,
    links: &'a crate::native_links::Links,
    terminal_metadata: &'a crate::graphics_owner::metadata::Metadata,
    clipboard_floor: u64,
    read_only: bool,
    exited: bool,
    input_available: bool,
}
#[derive(Serialize)]
struct Response<'a> {
    version: u8,
    id: u64,
    ok: bool,
    result: Payload<'a>,
}
#[derive(Serialize)]
struct Event<'a> {
    version: u8,
    event: &'static str,
    view: &'a str,
    epoch: u64,
    sequence: u64,
    base_sequence: Option<u64>,
    #[serde(flatten)]
    payload: Payload<'a>,
}
impl Job {
    fn render(self, resources: &Resources, transport: &Budget) -> Result<Rendered> {
        ensure!(
            !self.cancelled.load(Ordering::Acquire),
            "View was cancelled"
        );
        if self.snapshot.history.is_some() {
            return self.render_screen(transport);
        }
        let s = &self.snapshot;
        let mut resources = resources.clone();
        resources.working = resources.presentation.clone();
        let store = s
            .store
            .as_ref()
            .context("Graphics snapshot is unavailable")?;
        let tiles = store.placeholder_tiles(&s.screen, s.size.cell);
        let mut raster = crate::graphics_paint::compose_tiles(
            store,
            crate::graphics_paint::View {
                origin: [0, 0],
                width: u32::from(s.size.columns) * u32::from(s.size.cell[0]),
                height: u32::from(s.size.rows) * u32::from(s.size.cell[1]),
                cell: s.size.cell,
                alternate: s.screen.alternate_screen(),
            },
            &[],
            &tiles,
            &resources,
        )
        .map_err(|e| anyhow::anyhow!("Source composition: {e:?}"))?;
        let planes = if matches!(&self.target, Target::Watch { pixels: true, .. }) {
            Some(crate::graphics_planes::encode(
                &s.screen,
                &raster,
                self.base.as_ref().and_then(|(_, p)| p.planes.as_ref()),
                &resources,
            )?)
        } else {
            None
        };
        if planes.is_some() {
            raster.layers.clear();
        }
        let (stream, generation, focused) = match &self.target {
            Target::Request { .. } => (s.source.id.clone(), 1, true),
            Target::Watch {
                view,
                generation,
                focused,
                ..
            } => (view.clone(), *generation, *focused),
        };
        let frame = crate::graphics_paint::cells(
            &s.screen,
            &raster,
            crate::graphics_paint::CellView {
                stream,
                generation,
                focused,
                rect: starcore::terminal_graphics::protocol::Rect {
                    x: 0,
                    y: 0,
                    width: s.size.columns,
                    height: s.size.rows,
                },
            },
            |color, fg| s.palette.color(color, fg),
        )
        .map_err(|e| anyhow::anyhow!("Source cells: {e:?}"))?;
        drop(raster);
        let links = crate::native_links::Links::capture(&s.screen, s.size.rows, s.size.columns)?;
        ensure!(
            !self.cancelled.load(Ordering::Acquire),
            "View was cancelled"
        );
        // Vec growth can temporarily reserve up to twice the wire size.
        let mut memory = transport
            .reserve(OUTPUT * 2)
            .context("Presentation transport is busy")?;
        let mut out = Bounded { bytes: Vec::new() };
        let payload = Payload {
            source: &s.source,
            revision: s.revision,
            input_modes: &s.modes,
            source_grid: [s.screen.size().0, s.screen.size().1],
            pixels: planes
                .as_ref()
                .filter(|p| {
                    self.base
                        .as_ref()
                        .and_then(|(_, old)| old.planes.as_ref())
                        .is_none_or(|old| old.wire.id != p.wire.id)
                })
                .map(|p| &p.wire),
            frame: WireFrame::new(&frame, self.base.as_ref().map(|(_, p)| p.as_ref())),
            links: &links,
            terminal_metadata: &s.metadata,
            clipboard_floor: match &self.target {
                Target::Watch {
                    clipboard_floor, ..
                } => *clipboard_floor,
                Target::Request { .. } => s.metadata.clipboard_sequence,
            },
            read_only: true,
            exited: s.exited,
            input_available: s.input_available,
        };
        let watch = match &self.target {
            Target::Request { id, .. } => {
                serde_json::to_writer(
                    &mut out,
                    &Response {
                        version: 1,
                        id: *id,
                        ok: true,
                        result: payload,
                    },
                )?;
                None
            }
            Target::Watch {
                view,
                epoch,
                sequence,
                ..
            } => {
                serde_json::to_writer(
                    &mut out,
                    &Event {
                        version: 1,
                        event: "cells",
                        view,
                        epoch: *epoch,
                        sequence: *sequence,
                        base_sequence: self.base.as_ref().map(|(sequence, _)| *sequence),
                        payload,
                    },
                )?;
                Some((view.clone(), *epoch))
            }
        };
        out.write_all(b"\n")?;
        drop(links);
        memory.shrink_to(out.bytes.capacity());
        let picture_bytes = std::mem::size_of::<Picture>()
            + frame.stream.capacity()
            + frame.rows.capacity()
                * std::mem::size_of::<starcore::terminal_graphics::terminal_cells::Row>()
            + frame
                .rows
                .iter()
                .map(|r| {
                    r.runs.capacity()
                        * std::mem::size_of::<starcore::terminal_graphics::terminal_cells::Run>()
                        + r.runs.iter().map(|run| run.text.capacity()).sum::<usize>()
                })
                .sum::<usize>();
        let Snapshot {
            screen,
            store,
            metadata,
            _memory: mut snapshot_memory,
            ..
        } = self.snapshot;
        let clipboard_sequence = metadata.clipboard_sequence;
        drop(metadata);
        drop(screen);
        drop(store);
        ensure!(
            snapshot_memory.try_resize(picture_bytes),
            "Presentation frame exceeds reservation"
        );
        let picture = Arc::new(Picture {
            clipboard_sequence,
            planes,
            frame,
            _memory: snapshot_memory,
        });
        Ok(Rendered {
            packet: Packet {
                bytes: out.bytes,
                offset: 0,
                watch,
                _memory: memory,
            },
            picture: Some(picture),
        })
    }
}
impl Job {
    fn render_screen(mut self, transport: &Budget) -> Result<Rendered> {
        let history = self.snapshot.history.unwrap_or(0);
        let screen = &mut self.snapshot.screen;
        let result = if history == 0 {
            let ansi = screen
                .contents_formatted_bounded(1024 * 1024)
                .context("Native screen exceeds 1 MiB")?;
            let (rows, columns) = screen.size();
            let (row, col) = screen.cursor_position();
            json!({"source":self.snapshot.source,"revision":self.snapshot.revision,"text":screen.contents(),"input_modes":self.snapshot.modes,"terminal_metadata":self.snapshot.metadata,"screen":{"rows":rows,"columns":columns,"ansi_base64":STANDARD.encode(&ansi),"cursor":[col,row],"alternate":screen.alternate_screen(),"cursor_visible":!screen.hide_cursor()}})
        } else {
            let bytes = history_bytes(screen, usize::from(history))?;
            json!({"source":self.snapshot.source,"revision":self.snapshot.revision,"history_base64":STANDARD.encode(bytes)})
        };
        ensure!(
            !self.cancelled.load(Ordering::Acquire),
            "Screen request was cancelled"
        );
        let Target::Request { id, .. } = self.target else {
            anyhow::bail!("Screen requests cannot be watched");
        };
        let mut memory = transport
            .reserve(OUTPUT * 2)
            .context("Native screen transport is busy")?;
        let mut out = Bounded { bytes: Vec::new() };
        serde_json::to_writer(
            &mut out,
            &json!({"version":1,"id":id,"ok":true,"result":result}),
        )?;
        out.write_all(b"\n")?;
        memory.shrink_to(out.bytes.capacity());
        Ok(Rendered {
            packet: Packet {
                bytes: out.bytes,
                offset: 0,
                watch: None,
                _memory: memory,
            },
            picture: None,
        })
    }
}
fn history_bytes(screen: &mut vt100::Screen, maximum: usize) -> Result<Vec<u8>> {
    let limit = 2 * 1024 * 1024;
    let mut bytes = Vec::new();
    screen.set_scrollback(maximum);
    let retained = screen.scrollback();
    let (rows, columns) = screen.size();
    for absolute in 0..retained + usize::from(rows) {
        screen.set_scrollback(retained.saturating_sub(absolute));
        let local = absolute.saturating_sub(retained) as u16;
        // Format a single row as a standalone bounded grid. No terminal
        // callbacks, alternate-buffer switches or source mode queries occur.
        let mut last = None;
        let mut last_link = None;
        for column in 0..columns {
            let cell = screen
                .cell(local, column)
                .context("Native history row unavailable")?;
            if cell.is_wide_continuation() {
                continue;
            }
            let link = screen
                .hyperlink(local, column)
                .map(|uri| (screen.hyperlink_params(local, column).unwrap_or(""), uri));
            if last_link != link {
                if let Some((params, uri)) = link {
                    // The retained parser has already validated these strings.
                    // Check the complete escape's size before allocating it.
                    ensure!(
                        bytes.len() + params.len() + uri.len() + 256 <= limit,
                        "Native history links exceed 2 MiB"
                    );
                    bytes.extend_from_slice(b"\x1b]8;");
                    bytes.extend_from_slice(params.as_bytes());
                    bytes.push(b';');
                    bytes.extend_from_slice(uri.as_bytes());
                    bytes.extend_from_slice(b"\x1b\\");
                } else {
                    bytes.extend_from_slice(b"\x1b]8;;\x1b\\");
                }
                last_link = link;
            }
            let style = (
                cell.fgcolor(),
                cell.bgcolor(),
                cell.bold(),
                cell.dim(),
                cell.italic(),
                cell.underline(),
                cell.inverse(),
            );
            if last != Some(style) {
                bytes.extend_from_slice(b"\x1b[0m");
                for (enabled, code) in [
                    (cell.bold(), 1),
                    (cell.dim(), 2),
                    (cell.italic(), 3),
                    (cell.underline(), 4),
                    (cell.inverse(), 7),
                ] {
                    if enabled {
                        write!(bytes, "\x1b[{code}m")?;
                    }
                }
                for (color, foreground) in [(cell.fgcolor(), true), (cell.bgcolor(), false)] {
                    let base = if foreground { 38 } else { 48 };
                    match color {
                        vt100::Color::Default => (),
                        vt100::Color::Idx(i) => write!(bytes, "\x1b[{base};5;{i}m")?,
                        vt100::Color::Rgb(r, g, b) => write!(bytes, "\x1b[{base};2;{r};{g};{b}m")?,
                    }
                }
                last = Some(style);
            }
            bytes.extend_from_slice(if cell.has_contents() {
                cell.contents().as_bytes()
            } else {
                b" "
            });
            ensure!(bytes.len() + 256 <= limit, "Native history exceeds 2 MiB");
        }
        if last_link.is_some() {
            bytes.extend_from_slice(b"\x1b]8;;\x1b\\");
        }
        if !screen.row_wrapped(local) {
            bytes.extend_from_slice(b"\r\n");
        }
    }
    screen.set_scrollback(0);
    Ok(bytes)
}
impl Renderer {
    pub fn new(resources: Resources, transport: Budget) -> Self {
        let (jobs, rx) = crossbeam_channel::bounded::<Job>(4);
        let (tx, done) = crossbeam_channel::bounded(4);
        for _ in 0..2 {
            let rx = rx.clone();
            let tx = tx.clone();
            let resources = resources.clone();
            let transport = transport.clone();
            std::thread::spawn(move || {
                for job in rx {
                    let tag = job.tag;
                    let target = job.target.clone();
                    let source = job.snapshot.source.clone();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        job.render(&resources, &transport)
                    }))
                    .map_err(|_| "Presentation worker failed".to_owned())
                    .and_then(|v| v.map_err(|e| format!("{e:#}")));
                    if tx
                        .send(Finished {
                            tag,
                            target,
                            source,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        Self { jobs, done }
    }
    pub fn submit(&self, job: Job) -> Result<()> {
        self.jobs
            .try_send(job)
            .map_err(|_| anyhow::anyhow!("Presentation workers are busy"))
    }
    pub fn receive(&self) -> Option<Finished> {
        self.done.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use starcore::terminal_graphics::{
        protocol::Rect,
        terminal_cells::{Frame, Row, Run},
    };
    fn frame() -> Frame {
        Frame {
            stream: "private-view".into(),
            generation: 7,
            rect: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            complete: true,
            rows: vec![Row {
                index: 0,
                runs: vec![Run {
                    text: "x".into(),
                    columns: 1,
                    foreground: [255; 3],
                    background: [0; 3],
                    attributes: 0,
                }],
            }],
            cursor: Some([0, 0]),
        }
    }
    #[test]
    fn history_export_preserves_logical_wrap_unicode_styles_and_the_source() {
        let mut parser = vt100::Parser::new(3, 10, 20);
        parser.process("\x1b[31mABCDEFGHIJK界Z\x1b[0m\r\nsecond\r\nthird\r\n".as_bytes());
        let original = parser.screen().contents_formatted();
        let mut copy = parser.screen().retained_snapshot(20);
        let bytes = history_bytes(&mut copy, 20).unwrap();
        assert!(
            bytes
                .windows(b"\x1b[38;5;1m".len())
                .any(|b| b == b"\x1b[38;5;1m")
        );
        let mut restored = vt100::Parser::new(3, 10, 20);
        restored.process(&bytes);
        restored.screen_mut().set_scrollback(usize::MAX);
        assert!(restored.screen().contents().contains("ABCDEFGHIJ"));
        assert_eq!(parser.screen().contents_formatted(), original);
    }
    #[test]
    fn history_export_keeps_hidden_targets_on_wide_wrapped_labels_and_closes_them() {
        let mut source = vt100::Parser::new(3, 8, 20);
        source.process("\x1b]8;id=review;https://example.invalid/hidden;part=1\x1b\\ABC界éLONG\x1b]8;;\x1b\\!\r\nplain\r\nmore\r\n".as_bytes());
        let original = source.screen().contents_formatted();
        let mut copy = source.screen().retained_snapshot(20);
        let bytes = history_bytes(&mut copy, 20).unwrap();
        let mut restored = vt100::Parser::new(3, 8, 20);
        restored.process(&bytes);
        restored.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(
            restored.screen().hyperlink(0, 3),
            Some("https://example.invalid/hidden;part=1")
        );
        assert_eq!(
            restored.screen().hyperlink(0, 4),
            Some("https://example.invalid/hidden;part=1")
        );
        assert_eq!(restored.screen().hyperlink_params(0, 3), Some("id=review"));
        assert_eq!(
            restored.screen().hyperlink(1, 0),
            Some("https://example.invalid/hidden;part=1")
        );
        assert!(restored.screen().hyperlink(1, 2).is_none());
        assert!(restored.screen().hyperlink(2, 0).is_none());
        assert_eq!(source.screen().contents_formatted(), original);
    }
    #[test]
    fn delta_keeps_cursor_updates_and_resets_when_host_generation_changes() {
        let budget = Budget::new(1);
        let base = Picture {
            clipboard_sequence: 0,
            planes: None,
            frame: frame(),
            _memory: budget.reserve(1).unwrap(),
        };
        let mut current = frame();
        current.cursor = None;
        let delta = WireFrame::new(&current, Some(&base));
        assert!(!delta.complete);
        assert!(delta.rows.is_empty());
        assert!(delta.cursor.is_none());
        let encoded = serde_json::to_vec(&delta).unwrap();
        let decoded: Frame = serde_json::from_slice(&encoded).unwrap();
        decoded.validate().unwrap();
        current.generation = 8;
        let complete = WireFrame::new(&current, Some(&base));
        assert!(complete.complete);
        assert_eq!(complete.rows.len(), 1);
        current.generation = 7;
        current.rows[0].runs[0].text = "y".into();
        assert_eq!(WireFrame::new(&current, Some(&base)).rows.len(), 1);
    }
    #[test]
    fn serializer_rejects_oversize_before_accepting_an_unbounded_output() {
        let mut out = Bounded {
            bytes: vec![0; OUTPUT - 2],
        };
        assert!(out.write_all(b"123").is_err());
        assert_eq!(out.bytes.len(), OUTPUT - 2);
        out.write_all(b"12").unwrap();
        assert_eq!(out.bytes.len(), OUTPUT);
        assert!(out.write_all(b"\n").is_err());
    }
}
