//! Bounded, off-loop presentation. Snapshots share immutable image allocations;
//! render/output budgets never borrow the decoder's working reservation.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct Snapshot {
    screen: vt100::Screen,
    store: crate::graphics_store::Store,
    source: Reference,
    revision: u64,
    modes: Value,
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
        peer: u64,
        view: String,
        epoch: u64,
        sequence: u64,
        generation: u64,
        focused: bool,
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
    pub frame: starkit::terminal_graphics::terminal_cells::Frame,
    _memory: Lease,
}
pub(super) struct Rendered {
    pub packet: Packet,
    pub picture: Arc<Picture>,
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
    pub fn capture(
        source: &Source,
        size: Size,
        palette: Palette,
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
            store: source.owner.store().clone(),
            source: source.reference.clone(),
            revision: source.revision,
            modes: source.modes(),
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
    rect: starkit::terminal_graphics::protocol::Rect,
    complete: bool,
    rows: Vec<&'a starkit::terminal_graphics::terminal_cells::Row>,
    cursor: Option<[u16; 2]>,
}
impl<'a> WireFrame<'a> {
    fn new(
        frame: &'a starkit::terminal_graphics::terminal_cells::Frame,
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
    frame: WireFrame<'a>,
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
        let s = &self.snapshot;
        let mut resources = resources.clone();
        resources.working = resources.presentation.clone();
        let raster = crate::graphics_paint::compose(
            &s.store,
            crate::graphics_paint::View {
                origin: [0, 0],
                width: u32::from(s.size.columns) * u32::from(s.size.cell[0]),
                height: u32::from(s.size.rows) * u32::from(s.size.cell[1]),
                cell: s.size.cell,
                alternate: s.screen.alternate_screen(),
            },
            &[],
            &resources,
        )
        .map_err(|e| anyhow::anyhow!("Source composition: {e:?}"))?;
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
                rect: starkit::terminal_graphics::protocol::Rect {
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
            frame: WireFrame::new(&frame, self.base.as_ref().map(|(_, p)| p.as_ref())),
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
        memory.shrink_to(out.bytes.capacity());
        let picture_bytes = std::mem::size_of::<Picture>()
            + frame.stream.capacity()
            + frame.rows.capacity()
                * std::mem::size_of::<starkit::terminal_graphics::terminal_cells::Row>()
            + frame
                .rows
                .iter()
                .map(|r| {
                    r.runs.capacity()
                        * std::mem::size_of::<starkit::terminal_graphics::terminal_cells::Run>()
                        + r.runs.iter().map(|run| run.text.capacity()).sum::<usize>()
                })
                .sum::<usize>();
        let Snapshot {
            screen,
            store,
            _memory: mut snapshot_memory,
            ..
        } = self.snapshot;
        drop(screen);
        drop(store);
        ensure!(
            snapshot_memory.try_resize(picture_bytes),
            "Presentation frame exceeds reservation"
        );
        let picture = Arc::new(Picture {
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
            picture,
        })
    }
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
    use starkit::terminal_graphics::{
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
    fn delta_keeps_cursor_updates_and_resets_when_host_generation_changes() {
        let budget = Budget::new(1);
        let base = Picture {
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
