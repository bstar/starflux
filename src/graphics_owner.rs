//! Single-source parser/graphics/reply authority. The backend owns the PTY and
//! decoder pool; frontends only inspect this state. No PTY, process or presenter
//! is created here, and this engine alone does not advertise graphics support.
use crate::{
    graphics_decode::Completion,
    graphics_reply,
    graphics_requests::{self, Job, Requests},
    graphics_resources::Resources,
    graphics_store::{self, Generation, Geometry, Stamp, Store},
    graphics_stream::{Event, Gap, Passthrough},
    graphics_transfer::Error,
};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
const MAX_INPUT: usize = 64 * 1024;
const MAX_CHUNK: usize = 8192;
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fault {
    Protocol(Error),
    Requests(graphics_requests::Failure),
    Stream(Gap),
    InvalidCompletion,
    Sequence,
}
impl From<graphics_requests::Failure> for Fault {
    fn from(e: graphics_requests::Failure) -> Self {
        Self::Requests(e)
    }
}
struct Callbacks {
    requests: Requests,
    job: Option<Job>,
    fault: Option<Fault>,
    mutations: Vec<Mutation>,
    cell: [u16; 2],
    palette: [[u8; 3]; 2],
}
enum Mutation {
    Scroll {
        alternate: bool,
        top: u16,
        bottom: u16,
        rows: i32,
    },
    Clear {
        alternate: bool,
        kind: vt100::ScreenClear,
    },
}
impl Callbacks {
    fn response(&mut self, bytes: Vec<u8>) {
        if let Err(error) = self.requests.ready(bytes) {
            self.fault = Some(error.into());
        }
    }
}
impl vt100::Callbacks for Callbacks {
    const TRACK_SCROLL: bool = true;
    fn scrolled(&mut self, screen: &mut vt100::Screen, top: u16, bottom: u16, rows: i32) {
        let alternate = screen.alternate_screen();
        if let Some(Mutation::Scroll {
            alternate: a,
            top: t,
            bottom: b,
            rows: r,
        }) = self.mutations.last_mut()
            && *a == alternate
            && *t == top
            && *b == bottom
            && r.signum() == rows.signum()
        {
            *r = r.saturating_add(rows);
        } else {
            self.mutations.push(Mutation::Scroll {
                alternate,
                top,
                bottom,
                rows,
            });
        }
    }
    fn screen_cleared(&mut self, screen: &mut vt100::Screen, kind: vt100::ScreenClear) {
        self.mutations.push(Mutation::Clear {
            alternate: screen.alternate_screen(),
            kind,
        });
    }
    fn unhandled_apc(&mut self, screen: &mut vt100::Screen, bytes: &[u8]) {
        match self.requests.push(bytes, screen.cursor_position()) {
            Ok(Some(job)) => self.job = Some(job),
            Ok(None) | Err(graphics_requests::Failure::Protocol(_)) => (),
            Err(error) => self.fault = Some(error.into()),
        }
    }
    fn cancelled_apc(&mut self, _: &mut vt100::Screen) {
        if let Err(error @ graphics_requests::Failure::Queue(_)) = self.requests.cancel() {
            self.fault = Some(error.into());
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
        let p = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        match (i, j, c, p) {
            (None, None, 'c', _) => self.response(b"\x1b[?1;2c".to_vec()),
            (Some(b'>'), None, 'c', _) => self.response(b"\x1b[>0;1;0c".to_vec()),
            (None, None, 'n', 5) => self.response(b"\x1b[0n".to_vec()),
            (None, None, 'n', 6) => {
                let (row, col) = screen.cursor_position();
                self.response(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes());
            }
            (Some(b'?'), Some(b'$'), 'p', _) => {
                self.response(format!("\x1b[?{p};0$y").into_bytes())
            }
            (None, None, 't', 14) => {
                let (rows, columns) = screen.size();
                self.response(
                    format!(
                        "\x1b[4;{};{}t",
                        u32::from(rows) * u32::from(self.cell[1]),
                        u32::from(columns) * u32::from(self.cell[0])
                    )
                    .into_bytes(),
                );
            }
            (None, None, 't', 16) => {
                self.response(format!("\x1b[6;{};{}t", self.cell[1], self.cell[0]).into_bytes())
            }
            (None, None, 't', 18) => {
                let (rows, columns) = screen.size();
                self.response(format!("\x1b[8;{rows};{columns}t").into_bytes());
            }
            _ => (),
        }
    }
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        if params.get(1) != Some(&b"?".as_slice()) {
            return;
        }
        let index = match params.first().copied() {
            Some(b"10") => 0,
            Some(b"11") => 1,
            _ => return,
        };
        let [r, g, b] = self.palette[index];
        self.response(
            format!(
                "\x1b]{};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x1b\\",
                10 + index
            )
            .into_bytes(),
        );
    }
}
struct Pending {
    tag: u64,
    stamp: Stamp,
    geometry: Geometry,
    job: Option<Job>,
}
pub struct Owner {
    parser: vt100::Parser<Callbacks>,
    store: Store,
    generation: Generation,
    geometry: Geometry,
    stream: Passthrough,
    input: VecDeque<u8>,
    escape: bool,
    pending: Option<Pending>,
    sequence: u64,
    fault: Option<Fault>,
}
impl Owner {
    pub fn new(
        generation: Generation,
        geometry: Geometry,
        resources: Resources,
    ) -> Result<Self, Fault> {
        Self::validate_geometry(geometry)?;
        let callbacks = Callbacks {
            requests: Requests::with_resources(generation.clone(), resources.clone()),
            job: None,
            fault: None,
            mutations: Vec::new(),
            cell: geometry.cell,
            palette: [[220, 215, 186], [31, 31, 40]],
        };
        Ok(Self {
            parser: vt100::Parser::new_with_callbacks(
                geometry.rows,
                geometry.columns,
                1000,
                callbacks,
            ),
            store: Store::with_budget(generation.clone(), resources.retained),
            generation,
            geometry,
            stream: Passthrough::default(),
            input: VecDeque::new(),
            escape: false,
            pending: None,
            sequence: 0,
            fault: None,
        })
    }
    fn validate_geometry(geometry: Geometry) -> Result<(), Fault> {
        if geometry.rows == 0
            || geometry.rows > 192
            || geometry.columns == 0
            || geometry.columns > 512
            || geometry.cell.iter().any(|n| *n == 0 || *n > 256)
        {
            return Err(Fault::Protocol(Error::Invalid));
        }
        Ok(())
    }
    fn check(&self) -> Result<(), Fault> {
        self.fault.clone().map_or(Ok(()), Err)
    }
    /// All-or-nothing input admission. On false the caller retains these bytes
    /// and continues servicing user input, replies and decoder completions.
    pub fn ingest(&mut self, bytes: &[u8]) -> Result<bool, Fault> {
        self.check()?;
        if bytes.len() > MAX_CHUNK {
            return Err(Fault::Protocol(Error::TooLarge));
        }
        // Each passthrough layer may flush a saved prefix along with this read.
        if self.input.len() + bytes.len() + 64 > MAX_INPUT {
            return Ok(false);
        }
        let mut gap = None;
        self.stream.feed(bytes, |event| match event {
            Event::Bytes(data) => self.input.extend(data.iter().copied()),
            Event::Gap(value) => gap = Some(value),
        });
        if let Some(gap) = gap {
            return self.fail(Fault::Stream(gap));
        }
        Ok(true)
    }
    fn fail<T>(&mut self, fault: Fault) -> Result<T, Fault> {
        self.input.clear();
        self.pending = None;
        self.parser.callbacks_mut().job = None;
        self.parser
            .callbacks_mut()
            .requests
            .reset(self.generation.clone());
        self.store.reset(self.generation.clone());
        self.fault = Some(fault.clone());
        Err(fault)
    }
    /// Parse in bounded turns. An APC terminator is a commit barrier: following
    /// bytes wait for decode/store/cursor completion, while the caller can still
    /// route ordinary keyboard input to its retained source.
    pub fn pump(&mut self, budget: usize, now: Instant) -> Result<usize, Fault> {
        self.check()?;
        if let Err(error @ graphics_requests::Failure::Queue(_)) =
            self.parser.callbacks_mut().requests.expire(now)
        {
            return self.fail(error.into());
        }
        let mut processed = 0;
        let budget = budget.min(MAX_INPUT);
        while self.pending.is_none() && processed < budget && !self.input.is_empty() {
            // One command can cancel a partial request and reserve a new reply.
            // Full replies pause output until drained or partial expiry, never
            // dropping a query or blocking the caller's separate input path.
            if self.parser.callbacks().requests.remaining() < 2 {
                break;
            }
            let bytes = self.input.as_slices().0;
            let limit = bytes.len().min(budget - processed);
            let length = if self.escape {
                1
            } else if bytes[0] == 0x1b {
                limit.min(2)
            } else {
                bytes[..limit]
                    .iter()
                    .position(|b| *b == 0x1b)
                    .unwrap_or(limit)
            };
            self.escape = bytes[length - 1] == 0x1b;
            let alternate = self.parser.screen().alternate_screen();
            self.parser.process(&bytes[..length]);
            self.input.drain(..length);
            processed += length;
            if let Some(error) = self.parser.callbacks_mut().fault.take() {
                return self.fail(error);
            }
            let mut mutations = std::mem::take(&mut self.parser.callbacks_mut().mutations);
            for mutation in mutations.drain(..) {
                match mutation {
                    Mutation::Scroll {
                        alternate,
                        top,
                        bottom,
                        rows,
                    } => {
                        if let Err(error) = self.store.scroll_region(
                            alternate,
                            self.geometry,
                            top,
                            bottom,
                            rows,
                            1000,
                        ) {
                            return self.fail(Fault::Protocol(error));
                        }
                    }
                    Mutation::Clear { alternate, kind } => {
                        if let Err(error @ graphics_requests::Failure::Queue(_)) =
                            self.parser.callbacks_mut().requests.cancel()
                        {
                            return self.fail(error.into());
                        }
                        match kind {
                            vt100::ScreenClear::Reset => {
                                self.store.clear(false);
                                self.store.clear(true);
                            }
                            vt100::ScreenClear::Alternate => self.store.clear(true),
                            vt100::ScreenClear::Display => {
                                self.store.erase_visible(alternate, self.geometry.rows)
                            }
                        }
                    }
                }
            }
            self.parser.callbacks_mut().mutations = mutations;
            if !alternate && self.parser.screen().alternate_screen() {
                self.store.clear(true);
            }
            if let Some(job) = self.parser.callbacks_mut().job.take() {
                self.sequence = self.sequence.checked_add(1).ok_or(Fault::Sequence)?;
                let tag = NEXT_JOB
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                    .map_err(|_| Fault::Sequence)?;
                let pending = Pending {
                    tag,
                    stamp: self
                        .store
                        .stamp(self.sequence, self.parser.screen().alternate_screen()),
                    geometry: self.geometry,
                    job: None,
                };
                let action = job.transmission.controls.character(b'a', b't');
                if matches!(action, Ok(b't' | b'T' | b'q')) {
                    self.pending = Some(Pending {
                        job: Some(job),
                        ..pending
                    });
                } else {
                    self.commit(pending, job, Ok(None))?;
                }
            }
        }
        Ok(processed)
    }
    /// Dispatch through the backend's shared pool. A denied submission must be
    /// completed with EBUSY using the returned job, so later queries can drain.
    pub fn take_job(&mut self) -> Option<(u64, Job)> {
        let pending = self.pending.as_mut()?;
        Some((pending.tag, pending.job.take()?))
    }
    pub fn complete(&mut self, completion: Completion) -> Result<bool, Fault> {
        self.check()?;
        if self
            .pending
            .as_ref()
            .is_none_or(|p| p.tag != completion.tag)
        {
            return Ok(false);
        }
        if !self.parser.callbacks().requests.pending(&completion.job) {
            return Err(Fault::InvalidCompletion);
        }
        let pending = self.pending.take().unwrap();
        let decoded = completion.result.map(|decoded| Some(decoded.pixels));
        self.commit(pending, completion.job, decoded)?;
        Ok(true)
    }
    fn commit(
        &mut self,
        pending: Pending,
        job: Job,
        decoded: Result<Option<crate::graphics_transfer::Pixels>, Error>,
    ) -> Result<(), Fault> {
        let result = self
            .store
            .apply(&pending.stamp, &job.transmission, decoded, pending.geometry);
        let reply = match result {
            Ok(outcome) => {
                if let Some([columns, rows]) = outcome.cursor_delta {
                    // Outside the screen/scroll area positioning is protocol-
                    // defined as implementation-dependent. Clamp through the
                    // parser's relative cursor operations without touching the
                    // source PTY, saved cursor, attributes or input modes.
                    let columns = columns.min(u32::from(u16::MAX));
                    let rows = rows.min(u32::from(u16::MAX));
                    self.parser
                        .process(format!("\x1b[{rows}B\x1b[{columns}C").as_bytes());
                }
                Ok(())
            }
            Err(graphics_store::Failure::Protocol(error)) => {
                Err(graphics_reply::Failure::Protocol(error))
            }
            Err(graphics_store::Failure::Missing) => Err(graphics_reply::Failure::Missing),
            Err(graphics_store::Failure::Stale | graphics_store::Failure::OutOfOrder) => {
                return self.fail(Fault::Sequence);
            }
        };
        if let Err(error @ graphics_requests::Failure::Queue(_)) =
            self.parser.callbacks_mut().requests.complete(&job, reply)
        {
            return self.fail(error.into());
        }
        Ok(())
    }
    pub fn drain_replies(&mut self, limit: usize) -> Vec<u8> {
        self.parser
            .callbacks_mut()
            .requests
            .drain(limit.min(MAX_CHUNK))
    }
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }
    pub fn store(&self) -> &Store {
        &self.store
    }
    pub fn set_palette(&mut self, palette: [[u8; 3]; 2]) {
        self.parser.callbacks_mut().palette = palette;
    }
    pub fn buffered(&self) -> usize {
        self.input.len()
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    /// The backend applies the kernel PTY resize only after this accepts it.
    /// A resize cannot overtake an earlier parsed graphics/cursor operation.
    pub fn resize(&mut self, geometry: Geometry) -> Result<bool, Fault> {
        self.check()?;
        Self::validate_geometry(geometry)?;
        if self.pending.is_some() || !self.input.is_empty() {
            return Ok(false);
        }
        self.parser
            .screen_mut()
            .set_size(geometry.rows, geometry.columns);
        self.geometry = geometry;
        self.parser.callbacks_mut().cell = geometry.cell;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics_decode::Pool;
    use std::time::Duration;
    fn generation() -> Generation {
        Generation {
            host: "private-engine".into(),
            boot: uuid::Uuid::new_v4(),
            pid: 2,
            start_ticks: 1,
            epoch: uuid::Uuid::new_v4(),
        }
    }
    fn geometry() -> Geometry {
        Geometry {
            cell: [8, 16],
            rows: 24,
            columns: 80,
        }
    }
    fn setup() -> (Owner, Pool, Resources) {
        let resources = Resources::new(64 * 1024 * 1024, 512 * 1024 * 1024);
        (
            Owner::new(generation(), geometry(), resources.clone()).unwrap(),
            Pool::new(resources.clone()),
            resources,
        )
    }
    fn decode(owner: &mut Owner, pool: &Pool) {
        let (tag, job) = owner.take_job().unwrap();
        assert!(pool.try_submit(tag, job).is_ok());
        assert!(
            owner
                .complete(pool.receive(Duration::from_secs(5)).unwrap())
                .unwrap()
        );
    }
    const IMAGE: &[u8] = b"\x1b_Ga=T,f=24,s=1,v=1,i=71,c=2,r=2;ESIz\x1b\\";
    #[test]
    fn source_palette_and_size_queries_share_graphics_reply_order() {
        let (mut owner, pool, _) = setup();
        owner.set_palette([[1, 2, 3], [4, 5, 6]]);
        parse(
            &mut owner,
            &[
                IMAGE,
                b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[14t\x1b[16t\x1b[18t",
            ]
            .concat(),
        );
        assert!(owner.drain_replies(8192).is_empty());
        decode(&mut owner, &pool);
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        assert_eq!(owner.drain_replies(8192),b"\x1b_Gi=71;OK\x1b\\\x1b]10;rgb:0101/0202/0303\x1b\\\x1b]11;rgb:0404/0505/0606\x1b\\\x1b[4;384;640t\x1b[6;16;8t\x1b[8;24;80t");
        assert!(
            owner
                .resize(Geometry {
                    rows: 10,
                    columns: 20,
                    cell: [10, 20]
                })
                .unwrap()
        );
        parse(&mut owner, b"\x1b[14t\x1b[16t\x1b[18t");
        assert_eq!(
            owner.drain_replies(8192),
            b"\x1b[4;200;200t\x1b[6;20;10t\x1b[8;10;20t"
        );
    }
    fn parse(owner: &mut Owner, bytes: &[u8]) {
        owner.ingest(bytes).unwrap();
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
    }
    #[test]
    fn full_screen_scroll_preserves_signed_image_history_and_visible_raster() {
        let (mut owner, pool, resources) = setup();
        parse(&mut owner, &[b"\x1b[2;3H".as_slice(), IMAGE].concat());
        decode(&mut owner, &pool);
        parse(&mut owner, b"\x1b[24;1H\n");
        assert_eq!(owner.store().placements(false).next().unwrap().row(), 0);
        parse(&mut owner, b"\n\n");
        let placement = owner.store().placements(false).next().unwrap();
        assert_eq!(placement.row(), -2);
        let view = crate::graphics_paint::View {
            origin: [0, -32],
            width: 640,
            height: 384,
            cell: [8, 16],
            alternate: false,
        };
        let raster = crate::graphics_paint::compose(owner.store(), view, &[], &resources).unwrap();
        assert_eq!(
            raster.sample(crate::graphics_paint::Plane::AboveText, 16, 8),
            [17, 34, 51, 255]
        );
        drop(raster);
        parse(&mut owner, b"\x1b[65535S");
        // A single request scrolls no more than one actual page.
        assert_eq!(owner.store().placements(false).next().unwrap().row(), -26);
        for _ in 0..42 {
            parse(&mut owner, b"\x1b[65535S");
        }
        assert_eq!(owner.store().placements(false).count(), 0);
        assert_eq!(resources.retained.used(), 4); // Unplaced data remains reusable.
    }
    #[test]
    fn margin_scroll_clips_pixels_and_reverse_scroll_cannot_resurrect_rows() {
        let resources = Resources::new(64 * 1024 * 1024, 512 * 1024 * 1024);
        let mut owner = Owner::new(
            generation(),
            Geometry {
                rows: 6,
                columns: 4,
                cell: [1, 2],
            },
            resources.clone(),
        )
        .unwrap();
        let pool = Pool::new(resources.clone());
        parse(
            &mut owner,
            b"\x1b[3;1H\x1b_Ga=T,f=24,s=1,v=4,c=1,r=2,C=1,i=1;/wAAAP8AAAD/////\x1b\\",
        );
        decode(&mut owner, &pool);
        parse(&mut owner, b"\x1b[2;5r\x1b[2S");
        assert_eq!(owner.store().placements(false).next().unwrap().row(), 0);
        let view = crate::graphics_paint::View {
            origin: [0, 0],
            width: 4,
            height: 12,
            cell: [1, 2],
            alternate: false,
        };
        let raster = crate::graphics_paint::compose(owner.store(), view, &[], &resources).unwrap();
        assert_eq!(
            raster.sample(crate::graphics_paint::Plane::AboveText, 0, 0),
            [0; 4]
        );
        assert_eq!(
            raster.sample(crate::graphics_paint::Plane::AboveText, 0, 2),
            [0, 0, 255, 255]
        );
        assert_eq!(
            raster.sample(crate::graphics_paint::Plane::AboveText, 0, 3),
            [255; 4]
        );
        drop(raster);
        parse(&mut owner, b"\x1b[T");
        let raster = crate::graphics_paint::compose(owner.store(), view, &[], &resources).unwrap();
        assert_eq!(
            raster.sample(crate::graphics_paint::Plane::AboveText, 0, 2),
            [0; 4]
        );
        assert_eq!(
            raster.sample(crate::graphics_paint::Plane::AboveText, 0, 4),
            [0, 0, 255, 255]
        );
        assert_eq!(
            raster.sample(crate::graphics_paint::Plane::AboveText, 0, 5),
            [255; 4]
        );
    }
    #[test]
    fn margin_crossing_images_and_line_edits_outside_page_stay_unchanged() {
        let resources = Resources::new(64 * 1024 * 1024, 512 * 1024 * 1024);
        let mut owner = Owner::new(
            generation(),
            Geometry {
                rows: 6,
                columns: 4,
                cell: [1, 2],
            },
            resources.clone(),
        )
        .unwrap();
        let pool = Pool::new(resources);
        parse(
            &mut owner,
            b"\x1b_Ga=T,f=24,s=1,v=4,c=1,r=2,C=1,i=1;/wAAAP8AAAD/////\x1b\\",
        );
        decode(&mut owner, &pool);
        parse(&mut owner, b"\x1b[6;1HZ\x1b[2;5r\x1b[S");
        assert_eq!(owner.store().placements(false).next().unwrap().row(), 0);
        parse(&mut owner, b"\x1b[6;1H\x1b[65535L\x1b[65535M");
        assert_eq!(owner.screen().cell(5, 0).unwrap().contents(), "Z");
        assert_eq!(owner.store().placements(false).next().unwrap().row(), 0);
    }
    #[test]
    fn inserted_deleted_wrapped_and_reverse_index_rows_move_images_with_text() {
        let resources = Resources::new(64 * 1024 * 1024, 512 * 1024 * 1024);
        let mut owner = Owner::new(
            generation(),
            Geometry {
                rows: 6,
                columns: 4,
                cell: [1, 2],
            },
            resources.clone(),
        )
        .unwrap();
        let pool = Pool::new(resources);
        parse(
            &mut owner,
            b"\x1b[3;1H\x1b_Ga=T,f=24,s=1,v=4,c=1,r=2,C=1,i=1;/wAAAP8AAAD/////\x1b\\",
        );
        decode(&mut owner, &pool);
        parse(&mut owner, b"\x1b[2;5r\x1b[3;1H\x1b[L");
        assert_eq!(owner.store().placements(false).next().unwrap().row(), 3);
        parse(&mut owner, b"\x1b[M");
        assert_eq!(owner.store().placements(false).next().unwrap().row(), 2);
        parse(&mut owner, b"\x1b[2;1H\x1bM");
        assert_eq!(owner.store().placements(false).next().unwrap().row(), 3);
        parse(&mut owner, b"\x1b[5;4HXY");
        assert_eq!(owner.store().placements(false).next().unwrap().row(), 2);
        assert_eq!(owner.screen().cell(4, 0).unwrap().contents(), "Y");
    }
    #[test]
    fn display_clear_preserves_assets_and_reset_cancels_partial_uploads() {
        let (mut owner, pool, resources) = setup();
        parse(&mut owner, IMAGE);
        decode(&mut owner, &pool);
        owner.drain_replies(8192);
        parse(&mut owner, b"\x1b[2J");
        assert_eq!(owner.store().placements(false).count(), 0);
        assert_eq!(resources.retained.used(), 4);
        parse(&mut owner, b"\x1b_Ga=p,i=71,C=1;\x1b\\");
        assert_eq!(owner.store().placements(false).count(), 1);
        owner.drain_replies(8192);
        parse(
            &mut owner,
            b"\x1b_Ga=q,i=72,m=1;AAAA\x1b\\\x1bc\x1b_Gm=0;AAAA\x1b\\",
        );
        assert_eq!(owner.store().placements(false).count(), 0);
        assert_eq!(resources.retained.used(), 0);
        assert_eq!(resources.uploads.used(), 0);
        assert_eq!(
            owner.drain_replies(8192),
            b"\x1b_Gi=72;EINVAL:Invalid graphics data\x1b\\"
        );
        assert!(!owner.pending());
    }
    #[test]
    fn repeated_alternate_entry_clears_only_alternate_images() {
        let (mut owner, pool, resources) = setup();
        parse(&mut owner, IMAGE);
        decode(&mut owner, &pool);
        parse(&mut owner, b"\x1b[?1049h");
        parse(&mut owner, IMAGE);
        decode(&mut owner, &pool);
        assert_eq!(resources.retained.used(), 8);
        parse(&mut owner, b"\x1b[?1049h");
        assert_eq!(owner.store().placements(true).count(), 0);
        assert_eq!(owner.store().placements(false).count(), 1);
        assert_eq!(resources.retained.used(), 4);
        parse(&mut owner, b"\x1b[?1049l");
        assert!(!owner.screen().alternate_screen());
    }
    #[test]
    fn graphics_commit_precedes_following_text_cursor_queries_and_da() {
        let (mut owner, pool, resources) = setup();
        let wire = [b"\x1b[2;3H".as_slice(), IMAGE, b"\x1b[cX\x1b[6n"].concat();
        assert!(owner.ingest(&wire).unwrap());
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        assert!(owner.pending());
        assert!(owner.screen().contents().is_empty());
        assert!(owner.drain_replies(8192).is_empty());
        assert!(!owner.resize(geometry()).unwrap());
        decode(&mut owner, &pool);
        assert_eq!(owner.screen().cursor_position(), (3, 4));
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        assert_eq!(owner.screen().cell(3, 4).unwrap().contents(), "X");
        assert_eq!(
            owner.drain_replies(8192),
            b"\x1b_Gi=71;OK\x1b\\\x1b[?1;2c\x1b[4;6R"
        );
        assert_eq!(owner.store().placements(false).count(), 1);
        assert_eq!(resources.uploads.used(), 0);
        assert_eq!(resources.working.used(), 0);
        assert_eq!(resources.retained.used(), 4);
        drop(owner);
        assert_eq!(resources.retained.used(), 0);
    }
    #[test]
    fn quiet_decode_failure_releases_barrier_without_moving_cursor() {
        let (mut owner, pool, resources) = setup();
        owner
            .ingest(b"\x1b_Ga=T,i=72,f=24,s=2,v=1,q=2;AAAA\x1b\\Z\x1b[c")
            .unwrap();
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        decode(&mut owner, &pool);
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        assert_eq!(owner.screen().contents(), "Z");
        assert_eq!(owner.drain_replies(8192), b"\x1b[?1;2c");
        assert_eq!(resources.retained.used(), 0);
        assert_eq!(resources.uploads.used(), 0);
    }
    #[test]
    fn full_reply_queue_pauses_and_resumes_without_dropping_queries() {
        let (mut owner, _, _) = setup();
        let wire = b"\x1b[c".repeat(100);
        owner.ingest(&wire).unwrap();
        let mut received = Vec::new();
        for _ in 0..10 {
            owner.pump(MAX_INPUT, Instant::now()).unwrap();
            received.extend(owner.drain_replies(8192));
            if owner.buffered() == 0 {
                break;
            }
        }
        assert_eq!(owner.buffered(), 0);
        assert_eq!(received, b"\x1b[?1;2c".repeat(100));
    }
    #[test]
    fn stalled_partial_expiry_unblocks_full_queue_and_retains_later_text() {
        let (mut owner, _, resources) = setup();
        owner
            .ingest(
                &[
                    b"\x1b_Gi=73,a=q,m=1;AAAA".as_slice(),
                    b"\x1b\\",
                    &b"\x1b[c".repeat(100),
                    b"AFTER",
                ]
                .concat(),
            )
            .unwrap();
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        assert!(owner.buffered() > 0);
        assert!(owner.drain_replies(8192).is_empty());
        let later = Instant::now() + Duration::from_secs(31);
        owner.pump(MAX_INPUT, later).unwrap();
        let mut received = owner.drain_replies(8192);
        owner.pump(MAX_INPUT, later).unwrap();
        received.extend(owner.drain_replies(8192));
        assert!(received.starts_with(b"\x1b_Gi=73;ETIMEDOUT:Graphics upload timed out\x1b\\"));
        assert!(received.ends_with(&b"\x1b[?1;2c".repeat(100)));
        assert_eq!(owner.screen().contents(), "AFTER");
        assert_eq!(resources.uploads.used(), 0);
    }
    #[test]
    fn malformed_passthrough_invalidates_pending_work_and_releases_retained_state() {
        let (mut owner, pool, resources) = setup();
        owner.ingest(IMAGE).unwrap();
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        let (tag, job) = owner.take_job().unwrap();
        assert!(pool.try_submit(tag, job).is_ok());
        assert!(matches!(
            owner.ingest(b"\x1bPtmux;\x1bBAD"),
            Err(Fault::Stream(Gap::Malformed))
        ));
        assert!(
            owner
                .complete(pool.receive(Duration::from_secs(5)).unwrap())
                .is_err()
        );
        assert_eq!(resources.uploads.used(), 0);
        assert_eq!(resources.working.used(), 0);
        assert_eq!(resources.retained.used(), 0);
        assert!(owner.drain_replies(8192).is_empty());
    }
    #[test]
    fn old_completion_cannot_commit_into_a_replacement_source() {
        let (mut old, pool, resources) = setup();
        old.ingest(IMAGE).unwrap();
        old.pump(MAX_INPUT, Instant::now()).unwrap();
        let (tag, job) = old.take_job().unwrap();
        assert!(pool.try_submit(tag, job).is_ok());
        drop(old);
        let mut new = Owner::new(generation(), geometry(), resources.clone()).unwrap();
        assert!(
            !new.complete(pool.receive(Duration::from_secs(5)).unwrap())
                .unwrap()
        );
        assert_eq!(new.store().placements(false).count(), 0);
        assert!(new.drain_replies(8192).is_empty());
        assert_eq!(resources.uploads.used(), 0);
        assert_eq!(resources.working.used(), 0);
    }
    #[test]
    fn input_admission_is_bounded_and_rejected_bytes_can_be_retried() {
        let (mut owner, pool, _) = setup();
        owner.ingest(IMAGE).unwrap();
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        let chunk = vec![b'x'; 8192];
        let mut count = 0;
        while owner.ingest(&chunk).unwrap() {
            count += 1;
        }
        assert!(count > 0 && owner.buffered() < MAX_INPUT);
        assert_eq!(owner.pump(MAX_INPUT, Instant::now()).unwrap(), 0);
        decode(&mut owner, &pool);
        owner.pump(MAX_INPUT, Instant::now()).unwrap();
        assert!(owner.ingest(&chunk).unwrap());
        assert!(owner.ingest(&vec![b'x'; MAX_CHUNK + 1]).is_err());
    }
    #[test]
    fn split_escape_terminator_is_a_barrier_even_at_every_input_boundary() {
        for split in 0..=IMAGE.len() {
            let (mut owner, pool, _) = setup();
            owner.ingest(&IMAGE[..split]).unwrap();
            owner.pump(MAX_INPUT, Instant::now()).unwrap();
            owner.ingest(&[&IMAGE[split..], b"X"].concat()).unwrap();
            owner.pump(MAX_INPUT, Instant::now()).unwrap();
            assert!(!owner.screen().contents().contains('X'));
            decode(&mut owner, &pool);
            owner.pump(MAX_INPUT, Instant::now()).unwrap();
            assert_eq!(owner.screen().cell(2, 2).unwrap().contents(), "X");
        }
    }
}
