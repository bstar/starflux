//! Generation-bound retained images. The owner applies decoded commands in
//! source order; this model never sends input or replies to a source.
use crate::graphics_resources::{Budget, Lease, Resources};
use crate::graphics_transfer::{Controls, Error, Pixels, Transmission};
use std::{collections::VecDeque, ops::Deref, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Generation {
    pub host: String,
    pub boot: uuid::Uuid,
    pub pid: u32,
    pub start_ticks: u64,
    pub epoch: uuid::Uuid,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Placement {
    pub image: u64,
    pub id: u32,
    pub alternate: bool,
    pub origin: (u16, u16),
    pub source: [u32; 4],
    pub offset: [u32; 2],
    pub inset: [u32; 2],
    pub pixels: [u32; 2],
    pub cells: [u32; 2],
    pub z: i32,
    requested_cells: [u32; 2],
    row_shift: i32,
    clip_rows: Option<[i32; 2]>,
}
impl Placement {
    pub fn row(&self) -> i32 {
        i32::from(self.origin.0).saturating_add(self.row_shift)
    }
    pub fn vertical_clip(&self) -> Option<[i32; 2]> {
        self.clip_rows
    }
    /// Resolve a frontend's geometry without changing source-owned placement
    /// metadata or the cursor delta computed for the source terminal.
    pub fn at_cell(&self, cell: [u16; 2]) -> Result<Self, Error> {
        let extent = fit(
            [self.source[2], self.source[3]],
            self.offset,
            self.requested_cells,
            cell,
        )?;
        let mut placement = self.clone();
        placement.pixels = extent.pixels;
        placement.inset = extent.inset;
        placement.cells = extent.cells;
        Ok(placement)
    }
}
#[derive(Clone)]
struct Image {
    key: u64,
    id: u32,
    alternate: bool,
    pixels: Arc<ImageData>,
}
struct ImageData {
    pixels: Pixels,
    retained: Lease,
}
impl Deref for ImageData {
    type Target = Pixels;
    fn deref(&self) -> &Pixels {
        &self.pixels
    }
}
#[derive(Clone, Copy)]
struct Limits {
    bytes: usize,
    images: usize,
    placements: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 64 * 1024 * 1024,
            images: 128,
            placements: 1024,
        }
    }
}
#[derive(Clone)]
pub struct Stamp {
    generation: Generation,
    sequence: u64,
    alternate: bool,
    buffer_epoch: uuid::Uuid,
}
#[derive(Clone, Copy)]
pub struct Geometry {
    pub cell: [u16; 2],
    pub rows: u16,
    pub columns: u16,
}
#[derive(Clone)]
pub struct Store {
    generation: Generation,
    sequence: u64,
    key: u64,
    images: VecDeque<Image>,
    placements: Vec<Placement>,
    bytes: usize,
    limits: Limits,
    buffer_epochs: [uuid::Uuid; 2],
    retained: Budget,
}
#[derive(Debug, Eq, PartialEq)]
pub enum Failure {
    Stale,
    OutOfOrder,
    Protocol(Error),
    Missing,
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Protocol(error)
    }
}
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    pub placement: Option<Placement>,
    pub cursor_delta: Option<[u32; 2]>,
}
impl Store {
    /// Follow text scrolling, retaining full-screen primary history within the
    /// supplied bound. Margin scrolls affect only wholly contained placements.
    /// A translated clip is intersected with the page, so reversing direction
    /// cannot resurrect image rows that have already been discarded.
    pub fn scroll_region(
        &mut self,
        alternate: bool,
        geometry: Geometry,
        top: u16,
        bottom: u16,
        rows: i32,
        history: u16,
    ) -> Result<(), Error> {
        if top >= bottom || bottom > geometry.rows {
            return Err(Error::Invalid);
        }
        if rows == 0 {
            return Ok(());
        }
        let heights = self
            .placements
            .iter()
            .map(|p| p.at_cell(geometry.cell).map(|p| p.cells[1]))
            .collect::<Result<Vec<_>, _>>()?;
        let full = top == 0 && bottom == geometry.rows;
        let keep_history = full && !alternate && rows < 0;
        let lower = if keep_history {
            -i32::from(history)
        } else {
            i32::from(top)
        };
        let upper = i32::from(bottom);
        let mut index = 0;
        self.placements.retain_mut(|p| {
            let height = heights[index] as i32;
            index += 1;
            if p.alternate != alternate {
                return true;
            }
            let start = p.row();
            let end = start.saturating_add(height);
            let [clip_top, clip_bottom] = p.clip_rows.unwrap_or([start, end]);
            let visible_top = start.max(clip_top);
            let visible_bottom = end.min(clip_bottom);
            let included = if full {
                visible_top < upper && visible_bottom > lower
            } else {
                visible_top >= i32::from(top) && visible_bottom <= upper
            };
            if !included {
                return true;
            }
            p.row_shift = p.row_shift.saturating_add(rows);
            let start = visible_top.saturating_add(rows).max(lower);
            let end = visible_bottom.saturating_add(rows).min(upper);
            if start >= end {
                return false;
            }
            p.clip_rows = Some([start, end]);
            true
        });
        Ok(())
    }
    pub fn erase_visible(&mut self, alternate: bool, rows: u16) {
        self.placements.retain(|p| {
            if p.alternate != alternate {
                return true;
            }
            let top = p.row();
            let bottom = top.saturating_add(p.cells[1] as i32);
            let [lo, hi] = p.clip_rows.unwrap_or([top, bottom]);
            top.max(lo) >= i32::from(rows) || bottom.min(hi) <= 0
        });
        self.buffer_epochs[usize::from(alternate)] = uuid::Uuid::new_v4();
    }
    pub fn new(generation: Generation) -> Self {
        Self::with_budget(generation, Resources::shared().retained)
    }
    pub fn with_budget(generation: Generation, retained: Budget) -> Self {
        Self {
            generation,
            sequence: 0,
            key: 0,
            images: VecDeque::new(),
            placements: Vec::new(),
            bytes: 0,
            limits: Limits::default(),
            buffer_epochs: [uuid::Uuid::new_v4(), uuid::Uuid::new_v4()],
            retained,
        }
    }
    pub fn reset(&mut self, generation: Generation) {
        self.generation = generation;
        self.sequence = 0;
        self.clear(false);
        self.clear(true);
    }
    pub fn stamp(&self, sequence: u64, alternate: bool) -> Stamp {
        Stamp {
            generation: self.generation.clone(),
            sequence,
            alternate,
            buffer_epoch: self.buffer_epochs[usize::from(alternate)],
        }
    }
    pub fn clear(&mut self, alternate: bool) {
        self.buffer_epochs[usize::from(alternate)] = uuid::Uuid::new_v4();
        let keys = self
            .images
            .iter()
            .filter(|i| i.alternate == alternate)
            .map(|i| i.key)
            .collect::<Vec<_>>();
        for key in keys {
            self.remove(key);
        }
    }
    fn remove(&mut self, key: u64) {
        if let Some(index) = self.images.iter().position(|i| i.key == key) {
            let image = self.images.remove(index).unwrap();
            self.bytes -= image.pixels.rgba.capacity();
            self.placements.retain(|p| p.image != key);
        }
    }
    fn image(&self, alternate: bool, id: u32) -> Option<&Image> {
        self.images
            .iter()
            .find(|i| id != 0 && i.id == id && i.alternate == alternate)
    }
    pub fn pixels(&self, key: u64) -> Option<&Pixels> {
        self.images
            .iter()
            .find(|i| i.key == key)
            .map(|i| &i.pixels.pixels)
    }
    pub fn protocol_id(&self, key: u64) -> Option<u32> {
        self.images
            .iter()
            .find(|image| image.key == key)
            .map(|image| image.id)
    }
    pub fn placements(&self, alternate: bool) -> impl Iterator<Item = &Placement> {
        self.placements
            .iter()
            .filter(move |p| p.alternate == alternate)
    }
    pub fn usage(&self) -> [usize; 3] {
        [self.bytes, self.images.len(), self.placements.len()]
    }
    /// Every parsed command (including a failed decode) occupies a sequence.
    /// A bounded owner queue must hold later results until preceding jobs finish.
    /// Out-of-order/stale work does not mutate state or consume the sequence.
    pub fn apply(
        &mut self,
        stamp: &Stamp,
        transmission: &Transmission,
        decoded: Result<Option<Pixels>, Error>,
        geometry: Geometry,
    ) -> Result<Outcome, Failure> {
        let sequence = stamp.sequence;
        let alternate = stamp.alternate;
        if stamp.generation != self.generation {
            return Err(Failure::Stale);
        }
        if sequence <= self.sequence {
            return Err(Failure::Stale);
        }
        if self.sequence.checked_add(1) != Some(sequence) {
            return Err(Failure::OutOfOrder);
        }
        self.sequence = sequence;
        if stamp.buffer_epoch != self.buffer_epochs[usize::from(alternate)] {
            return Err(Failure::Stale);
        }
        if geometry.rows == 0 || geometry.columns == 0 {
            return Err(Error::Invalid.into());
        }
        let cell = geometry.cell;
        let pixels = decoded?;
        let controls = &transmission.controls;
        let id = controls.number(b'i', 0)?;
        if controls.has(b'I') || controls.has(b'U') || controls.has(b'P') || controls.has(b'Q') {
            return Err(Error::Unsupported.into());
        }
        match controls.character(b'a', b't')? {
            b'q' => {
                Self::validate_pixels(pixels.as_ref().ok_or(Error::Invalid)?)?;
                Ok(Outcome::default())
            }
            b't' | b'T' => {
                let pixels = pixels.ok_or(Error::Invalid)?;
                Self::validate_pixels(&pixels)?;
                let key = self.key.checked_add(1).ok_or(Error::TooLarge)?;
                let place = if controls.character(b'a', b't')? == b'T' {
                    Some(placement(
                        controls,
                        transmission.cursor,
                        cell,
                        &pixels,
                        key,
                        alternate,
                    )?)
                } else {
                    None
                };
                let old = self.image(alternate, id).map(|i| i.key);
                let removed = old.map_or(0, |key| {
                    self.placements.iter().filter(|p| p.image == key).count()
                });
                let allocation = pixels.rgba.capacity();
                if allocation > self.limits.bytes
                    || self.limits.images == 0
                    || (place.is_some()
                        && self.placements.len() - removed >= self.limits.placements)
                {
                    return Err(Error::TooLarge.into());
                }
                // Reserve before eviction/replacement. A denied aggregate
                // admission must preserve every valid stored image/placement.
                // Reuse the old image's charge atomically so equal-size
                // replacement succeeds even when the shared budget is full.
                let retained = if let Some(key) = old {
                    let image = self.images.iter_mut().find(|i| i.key == key).unwrap();
                    if let Some(data) = Arc::get_mut(&mut image.pixels) {
                        if !data.retained.try_resize(allocation) {
                            return Err(Error::Busy.into());
                        }
                        std::mem::replace(&mut data.retained, self.retained.reserve(0).unwrap())
                    } else {
                        // A renderer still owns the old immutable pixels. Keep
                        // that allocation charged until its last snapshot drops.
                        self.retained.reserve(allocation).ok_or(Error::Busy)?
                    }
                } else {
                    self.retained.reserve(allocation).ok_or(Error::Busy)?
                };
                if let Some(old) = old {
                    self.remove(old);
                }
                while self.bytes + allocation > self.limits.bytes
                    || self.images.len() >= self.limits.images
                {
                    self.remove(self.images.front().ok_or(Error::TooLarge)?.key);
                }
                self.key = key;
                self.bytes += allocation;
                self.images.push_back(Image {
                    key,
                    id,
                    alternate,
                    pixels: Arc::new(ImageData { pixels, retained }),
                });
                self.finish_placement(place, controls)
            }
            b'p' => {
                if pixels.is_some() {
                    return Err(Error::Invalid.into());
                }
                let image = self.image(alternate, id).ok_or(Failure::Missing)?;
                let place = placement(
                    controls,
                    transmission.cursor,
                    cell,
                    &image.pixels,
                    image.key,
                    alternate,
                )?;
                let replaces = place.id != 0
                    && self
                        .placements
                        .iter()
                        .any(|p| p.image == place.image && p.id == place.id);
                if !replaces && self.placements.len() >= self.limits.placements {
                    return Err(Error::TooLarge.into());
                }
                self.finish_placement(Some(place), controls)
            }
            b'd' => {
                if pixels.is_some() {
                    return Err(Error::Invalid.into());
                }
                let selector = controls.character(b'd', b'a')?;
                let placement_id = controls.number(b'p', 0)?;
                match selector {
                    b'a' | b'A' => {
                        let mut deleted = Vec::new();
                        self.placements.retain(|p| {
                            let visible = p.alternate == alternate
                                && p.row() < i32::from(geometry.rows)
                                && p.row().saturating_add(p.cells[1] as i32) > 0
                                && p.origin.1 < geometry.columns;
                            if visible {
                                deleted.push(p.image);
                            }
                            !visible
                        });
                        if selector == b'A' {
                            for key in deleted {
                                if !self.placements.iter().any(|p| p.image == key) {
                                    self.remove(key);
                                }
                            }
                        }
                    }
                    b'i' | b'I' => {
                        if let Some(key) = self.image(alternate, id).map(|i| i.key) {
                            self.placements.retain(|p| {
                                p.image != key || (placement_id != 0 && p.id != placement_id)
                            });
                            if selector == b'I' && !self.placements.iter().any(|p| p.image == key) {
                                self.remove(key);
                            }
                        }
                    }
                    _ => return Err(Error::Unsupported.into()),
                }
                Ok(Outcome::default())
            }
            _ => Err(Error::Unsupported.into()),
        }
    }
    fn validate_pixels(pixels: &Pixels) -> Result<(), Error> {
        if pixels.width == 0 || pixels.height == 0 {
            return Err(Error::Invalid);
        }
        if pixels.width > 8192
            || pixels.height > 8192
            || u64::from(pixels.width) * u64::from(pixels.height) * 4 != pixels.rgba.len() as u64
            || pixels.rgba.len() > 32 * 1024 * 1024
        {
            return Err(Error::TooLarge);
        }
        Ok(())
    }
    fn finish_placement(
        &mut self,
        placement: Option<Placement>,
        controls: &Controls,
    ) -> Result<Outcome, Failure> {
        let cursor_delta = placement
            .as_ref()
            .filter(|_| controls.number(b'C', 0) == Ok(0))
            .map(|p| p.cells);
        if let Some(p) = &placement {
            if p.id != 0 {
                self.placements
                    .retain(|old| old.image != p.image || old.id != p.id);
            }
            self.placements.push(p.clone());
        }
        Ok(Outcome {
            placement,
            cursor_delta,
        })
    }
}
fn placement(
    c: &Controls,
    origin: (u16, u16),
    cell: [u16; 2],
    pixels: &Pixels,
    image: u64,
    alternate: bool,
) -> Result<Placement, Error> {
    if cell.iter().any(|n| *n == 0 || *n > 256) || c.number(b'C', 0)? > 1 {
        return Err(Error::Invalid);
    }
    let offset = [c.number(b'X', 0)?, c.number(b'Y', 0)?];
    if offset[0] >= u32::from(cell[0]) || offset[1] >= u32::from(cell[1]) {
        return Err(Error::Invalid);
    }
    let x = c.number(b'x', 0)?;
    let y = c.number(b'y', 0)?;
    if x >= pixels.width || y >= pixels.height {
        return Err(Error::Invalid);
    }
    let w = c.number(b'w', pixels.width - x)?.min(pixels.width - x);
    let h = c.number(b'h', pixels.height - y)?.min(pixels.height - y);
    if w == 0 || h == 0 {
        return Err(Error::Invalid);
    }
    let cols = c.number(b'c', 0)?;
    let rows = c.number(b'r', 0)?;
    if cols > 4096 || rows > 4096 {
        return Err(Error::TooLarge);
    }
    let extent = fit([w, h], offset, [cols, rows], cell)?;
    Ok(Placement {
        image,
        id: if c.number(b'i', 0)? == 0 {
            0
        } else {
            c.number(b'p', 0)?
        },
        alternate,
        origin,
        source: [x, y, w, h],
        offset,
        inset: extent.inset,
        pixels: extent.pixels,
        cells: extent.cells,
        z: c.signed_number(b'z', 0)?,
        requested_cells: [cols, rows],
        row_shift: 0,
        clip_rows: None,
    })
}
struct Extent {
    pixels: [u32; 2],
    inset: [u32; 2],
    cells: [u32; 2],
}
fn fit(
    [w, h]: [u32; 2],
    offset: [u32; 2],
    [cols, rows]: [u32; 2],
    cell: [u16; 2],
) -> Result<Extent, Error> {
    if cell.iter().any(|n| *n == 0 || *n > 256) || w == 0 || h == 0 {
        return Err(Error::Invalid);
    }
    // Offsets are fixed pixel distances. They may exceed the new cell size
    // after a font reduction; never underflow an explicitly sized rectangle.
    let max_w = (cols * u32::from(cell[0])).saturating_sub(offset[0]).max(1);
    let max_h = (rows * u32::from(cell[1])).saturating_sub(offset[1]).max(1);
    let (width, height) = match (cols, rows) {
        (0, 0) => (w, h),
        (_, 0) => (max_w, ratio(max_w, h, w, true)?),
        (0, _) => (ratio(max_h, w, h, true)?, max_h),
        _ if u64::from(max_w) * u64::from(h) <= u64::from(max_h) * u64::from(w) => {
            (max_w, ratio(max_w, h, w, false)?)
        }
        _ => (ratio(max_h, w, h, false)?, max_h),
    };
    Ok(Extent {
        pixels: [width, height],
        inset: if cols != 0 && rows != 0 {
            [(max_w - width) / 2, (max_h - height) / 2]
        } else {
            [0, 0]
        },
        cells: [
            if cols == 0 {
                (width + offset[0]).div_ceil(u32::from(cell[0]))
            } else {
                cols
            },
            if rows == 0 {
                (height + offset[1]).div_ceil(u32::from(cell[1]))
            } else {
                rows
            },
        ],
    })
}

fn ratio(value: u32, numerator: u32, denominator: u32, ceil: bool) -> Result<u32, Error> {
    let product = u64::from(value) * u64::from(numerator);
    let result = if ceil {
        product.div_ceil(u64::from(denominator))
    } else {
        product / u64::from(denominator)
    };
    if result > 1_048_576 {
        return Err(Error::TooLarge);
    }
    Ok(result.max(1) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics_transfer::Assembler;
    use base64::{Engine, engine::general_purpose::STANDARD};
    #[test]
    fn shared_retained_budget_rejects_growth_without_losing_old_images() {
        let budget = Budget::new(8);
        let mut a = Store::with_budget(generation(), budget.clone());
        let mut b = Store::with_budget(generation(), budget.clone());
        let first = apply(&mut a, 1, false, &image("a=T,i=1,p=1", [255, 0, 0, 255]))
            .unwrap()
            .placement
            .unwrap();
        apply(&mut b, 1, true, &image("a=T,i=1", [0, 0, 255, 255])).unwrap();
        assert_eq!(budget.used(), 8);
        assert_eq!(
            apply(&mut a, 2, false, &image("a=T,i=2", [0; 4])),
            Err(Failure::Protocol(Error::Busy))
        );
        let larger = Assembler::default()
            .push(
                format!("Ga=T,f=32,s=2,v=1,i=1;{}", STANDARD.encode([255; 8])).as_bytes(),
                (0, 0),
            )
            .unwrap()
            .unwrap();
        let stamp = a.stamp(3, false);
        assert_eq!(
            a.apply(&stamp, &larger, larger.decode_image().map(Some), geometry()),
            Err(Failure::Protocol(Error::Busy))
        );
        assert_eq!(budget.used(), 8);
        assert_eq!(a.pixels(first.image).unwrap().rgba, [255, 0, 0, 255]);
        assert_eq!(a.placements(false).next(), Some(&first));
        // A full shared budget still permits replacement with equal capacity.
        apply(&mut a, 4, false, &image("a=T,i=1", [0, 255, 0, 255])).unwrap();
        assert_eq!(budget.used(), 8);
        drop(b);
        assert_eq!(budget.used(), 4);
        let stamp = a.stamp(5, false);
        a.apply(&stamp, &larger, larger.decode_image().map(Some), geometry())
            .unwrap();
        assert_eq!(budget.used(), 8);
        apply(&mut a, 6, false, &command("a=d,d=I,i=1")).unwrap();
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn retained_accounting_uses_capacity_and_queries_do_not_reserve_storage() {
        let budget = Budget::new(7);
        let mut store = Store::with_budget(generation(), budget.clone());
        let t = image("a=T,i=1", [1, 2, 3, 4]);
        let mut rgba = Vec::with_capacity(8);
        rgba.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(
            store.apply(
                &store.stamp(1, false),
                &t,
                Ok(Some(Pixels {
                    width: 1,
                    height: 1,
                    rgba
                })),
                geometry()
            ),
            Err(Failure::Protocol(Error::Busy))
        );
        assert_eq!(budget.used(), 0);
        assert!(store.images.is_empty());
        let query = image("a=q,i=1", [1, 2, 3, 4]);
        apply(&mut store, 2, false, &query).unwrap();
        assert_eq!(budget.used(), 0);
        apply(&mut store, 3, false, &t).unwrap();
        assert_eq!(budget.used(), 4);
        // Rejected oversized storage capacity must not consume the shared limit.
        store.limits.bytes = 3;
        assert_eq!(
            apply(&mut store, 4, false, &t),
            Err(Failure::Protocol(Error::TooLarge))
        );
        assert_eq!(budget.used(), 4);
        store.reset(generation());
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn retained_charges_cover_both_buffers_and_survive_shared_owner_drop() {
        let budget = Budget::new(12);
        let mut a = Store::with_budget(generation(), budget.clone());
        let mut b = Store::with_budget(generation(), budget.clone());
        apply(&mut a, 1, false, &image("a=T,i=1", [255; 4])).unwrap();
        apply(&mut a, 2, true, &image("a=T,i=1", [255; 4])).unwrap();
        apply(&mut b, 1, false, &image("a=T,i=1", [255; 4])).unwrap();
        assert_eq!(budget.used(), 12);
        a.clear(true);
        assert_eq!(budget.used(), 8);
        drop(a);
        assert_eq!(budget.used(), 4);
        assert_eq!(b.placements(false).count(), 1);
        drop(b);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn render_snapshots_pin_old_pixels_without_double_charging_or_mutation() {
        let budget = Budget::new(8);
        let mut store = Store::with_budget(generation(), budget.clone());
        apply(&mut store, 1, false, &image("a=T,i=1", [1, 2, 3, 4])).unwrap();
        let old = store.clone();
        let key = old.placements(false).next().unwrap().image;
        assert_eq!(budget.used(), 4);
        apply(&mut store, 2, false, &image("a=T,i=1", [5, 6, 7, 8])).unwrap();
        assert_eq!(budget.used(), 8);
        assert_eq!(old.pixels(key).unwrap().rgba, [1, 2, 3, 4]);
        let new = store.clone();
        let new_key = new.placements(false).next().unwrap().image;
        assert_eq!(new.pixels(new_key).unwrap().rgba, [5, 6, 7, 8]);
        store.reset(generation());
        assert_eq!(budget.used(), 8);
        drop(old);
        assert_eq!(budget.used(), 4);
        drop(new);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn snapshot_at_capacity_rejects_replacement_without_destroying_live_image() {
        let budget = Budget::new(4);
        let mut store = Store::with_budget(generation(), budget.clone());
        apply(&mut store, 1, false, &image("a=T,i=1", [1, 2, 3, 4])).unwrap();
        let snapshot = store.clone();
        let key = snapshot.placements(false).next().unwrap().image;
        assert_eq!(
            apply(&mut store, 2, false, &image("a=T,i=1", [5, 6, 7, 8])),
            Err(Failure::Protocol(Error::Busy))
        );
        assert_eq!(store.pixels(key).unwrap().rgba, [1, 2, 3, 4]);
        drop(snapshot);
        apply(&mut store, 3, false, &image("a=T,i=1", [5, 6, 7, 8])).unwrap();
        assert_eq!(budget.used(), 4);
    }
    fn generation() -> Generation {
        Generation {
            host: "private-fixture".into(),
            boot: uuid::Uuid::new_v4(),
            pid: 123,
            start_ticks: 1,
            epoch: uuid::Uuid::new_v4(),
        }
    }
    fn command(header: &str) -> Transmission {
        Assembler::default()
            .push(format!("G{header};").as_bytes(), (1, 2))
            .unwrap()
            .unwrap()
    }
    fn image(header: &str, pixel: [u8; 4]) -> Transmission {
        Assembler::default()
            .push(
                format!("Gf=32,s=1,v=1,{header};{}", STANDARD.encode(pixel)).as_bytes(),
                (1, 2),
            )
            .unwrap()
            .unwrap()
    }
    fn geometry() -> Geometry {
        Geometry {
            cell: [8, 16],
            rows: 24,
            columns: 80,
        }
    }
    fn apply(
        store: &mut Store,
        sequence: u64,
        alternate: bool,
        t: &Transmission,
    ) -> Result<Outcome, Failure> {
        let decoded = if matches!(
            t.controls.character(b'a', b't').unwrap(),
            b't' | b'T' | b'q'
        ) {
            t.decode_image().map(Some)
        } else {
            Ok(None)
        };
        store.apply(&store.stamp(sequence, alternate), t, decoded, geometry())
    }
    #[test]
    fn replace_query_and_failed_replacement_are_atomic() {
        let mut store = Store::new(generation());
        let first = apply(&mut store, 1, false, &image("a=T,i=1,p=1", [1, 2, 3, 4]))
            .unwrap()
            .placement
            .unwrap();
        assert_eq!(store.pixels(first.image).unwrap().rgba, [1, 2, 3, 4]);
        apply(&mut store, 2, false, &image("a=q,i=1", [5, 6, 7, 8])).unwrap();
        assert_eq!(store.pixels(first.image).unwrap().rgba, [1, 2, 3, 4]);
        assert_eq!(store.placements(false).count(), 1);
        assert_eq!(
            apply(&mut store, 3, false, &image("a=T,i=1,w=0", [5, 6, 7, 8])),
            Err(Failure::Protocol(Error::Invalid))
        );
        assert_eq!(store.pixels(first.image).unwrap().rgba, [1, 2, 3, 4]);
        apply(&mut store, 4, false, &image("a=t,i=1", [5, 6, 7, 8])).unwrap();
        assert!(store.pixels(first.image).is_none());
        assert_eq!(store.placements(false).count(), 0);
        assert_eq!(store.bytes, 4);
        assert_eq!(store.images.len(), 1);
    }
    #[test]
    fn placements_replace_by_named_pair_and_anonymous_images_remain_distinct() {
        let mut store = Store::new(generation());
        let first = apply(&mut store, 1, false, &image("a=T,i=1,p=5", [1, 2, 3, 4])).unwrap();
        assert_eq!(first.cursor_delta, Some([1, 1]));
        let second = apply(&mut store, 2, false, &command("a=p,i=1,p=5,c=4,C=1")).unwrap();
        assert!(second.cursor_delta.is_none());
        assert_eq!(second.placement.unwrap().pixels, [32, 32]);
        assert_eq!(store.placements(false).count(), 1);
        apply(&mut store, 3, false, &command("a=p,i=1")).unwrap();
        apply(&mut store, 4, false, &command("a=p,i=1")).unwrap();
        assert_eq!(store.placements(false).count(), 3);
        let a = apply(&mut store, 5, false, &image("a=T,p=9", [1, 2, 3, 4]))
            .unwrap()
            .placement
            .unwrap();
        let b = apply(&mut store, 6, false, &image("a=T,p=9", [1, 2, 3, 4]))
            .unwrap()
            .placement
            .unwrap();
        assert_ne!(a.image, b.image);
        assert_eq!(a.id, 0);
        assert_eq!(b.id, 0);
        assert_eq!(
            apply(&mut store, 7, false, &command("a=p,i=0")),
            Err(Failure::Missing)
        );
    }
    #[test]
    fn source_identity_order_and_buffer_clear_reject_late_workers() {
        let old = generation();
        let mut store = Store::new(old);
        let t = image("a=T,i=1", [1, 2, 3, 4]);
        assert_eq!(apply(&mut store, 2, false, &t), Err(Failure::OutOfOrder));
        assert_eq!(store.sequence, 0);
        let pending = store.stamp(1, true);
        store.clear(true);
        assert_eq!(
            store.apply(&pending, &t, t.decode_image().map(Some), geometry()),
            Err(Failure::Stale)
        );
        assert_eq!(store.sequence, 1);
        apply(&mut store, 2, true, &t).unwrap();
        let late = store.stamp(3, true);
        let mut new = store.generation.clone();
        new.epoch = uuid::Uuid::new_v4();
        store.reset(new);
        assert_eq!(
            store.apply(&late, &t, t.decode_image().map(Some), geometry()),
            Err(Failure::Stale)
        );
        assert_eq!(store.sequence, 0);
        assert!(store.images.is_empty());
        assert!(store.placements.is_empty());
        assert_eq!(store.bytes, 0);
        apply(&mut store, 1, false, &t).unwrap();
    }
    #[test]
    fn quotas_cover_both_buffers_and_eviction_removes_dangling_placements() {
        let mut store = Store::new(generation());
        store.limits = Limits {
            bytes: 8,
            images: 2,
            placements: 8,
        };
        let a = apply(&mut store, 1, false, &image("a=T,i=1", [1, 2, 3, 4]))
            .unwrap()
            .placement
            .unwrap();
        let b = apply(&mut store, 2, true, &image("a=T,i=1", [5, 6, 7, 8]))
            .unwrap()
            .placement
            .unwrap();
        assert_ne!(a.image, b.image);
        apply(&mut store, 3, false, &image("a=T,i=2", [1, 2, 3, 4])).unwrap();
        assert!(store.pixels(a.image).is_none());
        assert!(store.pixels(b.image).is_some());
        assert_eq!(store.placements(false).count(), 1);
        assert_eq!(store.placements(true).count(), 1);
        assert_eq!(store.bytes, 8);
        assert_eq!(store.images.len(), 2);
        store.limits.placements = 2;
        assert_eq!(
            apply(&mut store, 4, false, &command("a=p,i=2")),
            Err(Failure::Protocol(Error::TooLarge))
        );
        assert_eq!(store.bytes, 8);
        store.limits.bytes = 0;
        assert_eq!(
            apply(&mut store, 5, false, &image("a=T,i=2", [5, 6, 7, 8])),
            Err(Failure::Protocol(Error::TooLarge))
        );
        assert_eq!(store.bytes, 8);
        assert!(store.pixels(b.image).is_some());
    }
    #[test]
    fn deletion_preserves_other_placements_buffers_and_undisplayed_data() {
        let mut store = Store::new(generation());
        let first = apply(&mut store, 1, false, &image("a=T,i=1,p=7", [1, 2, 3, 4]))
            .unwrap()
            .placement
            .unwrap();
        apply(&mut store, 2, false, &command("a=p,i=1,p=8")).unwrap();
        apply(&mut store, 3, false, &command("a=d,d=I,i=1,p=7")).unwrap();
        assert!(store.pixels(first.image).is_some());
        assert_eq!(store.placements(false).count(), 1);
        apply(&mut store, 4, false, &command("a=d,d=i,i=1,p=8")).unwrap();
        assert_eq!(store.placements(false).count(), 0);
        assert!(store.pixels(first.image).is_some());
        apply(&mut store, 5, false, &command("a=p,i=1")).unwrap();
        let alt = apply(&mut store, 6, true, &image("a=T,i=1", [1, 2, 3, 4]))
            .unwrap()
            .placement
            .unwrap();
        apply(&mut store, 7, false, &image("a=t,i=2", [1, 2, 3, 4])).unwrap();
        apply(&mut store, 8, false, &command("a=d,d=A")).unwrap();
        assert!(store.pixels(first.image).is_none());
        assert!(store.pixels(alt.image).is_some());
        assert!(store.image(false, 2).is_some());
        assert_eq!(store.placements(true).count(), 1);
        assert_eq!(
            apply(&mut store, 9, false, &command("a=d,d=z")),
            Err(Failure::Protocol(Error::Unsupported))
        );
        assert_eq!(store.placements(true).count(), 1);
    }
    #[test]
    fn fit_crop_offsets_z_and_extreme_aspect_ratios_are_checked() {
        let pixels = Pixels {
            width: 16,
            height: 8,
            rgba: vec![0; 16 * 8 * 4],
        };
        let c = command("a=p,i=1,x=2,y=1,w=999,h=999,c=4,r=4,X=1,Y=2,z=-2147483648");
        let p = placement(&c.controls, c.cursor, [8, 16], &pixels, 1, false).unwrap();
        assert_eq!(p.source, [2, 1, 14, 7]);
        assert_eq!(p.offset, [1, 2]);
        assert_eq!(p.pixels, [31, 15]);
        assert_eq!(p.z, i32::MIN);
        assert_eq!(p.cells, [4, 4]);
        assert_eq!(p.inset, [0, 23]);
        for h in [
            "x=16",
            "y=8",
            "w=0",
            "C=2",
            "X=8",
            "r=4294967295",
            "z=-2147483649",
            "c=4294967295",
        ] {
            let c = command(&format!("a=p,i=1,{h}"));
            assert!(placement(&c.controls, c.cursor, [8, 16], &pixels, 1, false).is_err());
        }
        let wide = Pixels {
            width: 8192,
            height: 1,
            rgba: vec![0; 8192 * 4],
        };
        let c = command("a=p,i=1,r=4096");
        assert_eq!(
            placement(&c.controls, c.cursor, [256, 256], &wide, 1, false),
            Err(Error::TooLarge)
        );
    }
    #[test]
    fn failed_decode_consumes_order_but_keeps_valid_pixels() {
        let mut store = Store::new(generation());
        let t = image("a=T,i=1", [1, 2, 3, 4]);
        let p = apply(&mut store, 1, false, &t).unwrap().placement.unwrap();
        assert_eq!(
            store.apply(&store.stamp(2, false), &t, Err(Error::Invalid), geometry()),
            Err(Failure::Protocol(Error::Invalid))
        );
        assert!(store.pixels(p.image).is_some());
        apply(&mut store, 3, false, &command("a=p,i=1")).unwrap();
    }
    #[test]
    fn visible_delete_leaves_offscreen_placements_and_invalid_cells_are_atomic() {
        let mut store = Store::new(generation());
        let mut t = image("a=T,i=1", [1, 2, 3, 4]);
        t.cursor = (25, 2);
        let offscreen = apply(&mut store, 1, false, &t).unwrap().placement.unwrap();
        let visible = apply(&mut store, 2, false, &image("a=T,i=2", [1, 2, 3, 4]))
            .unwrap()
            .placement
            .unwrap();
        apply(&mut store, 3, false, &command("a=d,d=A")).unwrap();
        assert!(store.pixels(offscreen.image).is_some());
        assert!(store.pixels(visible.image).is_none());
        assert_eq!(store.placements(false).count(), 1);
        let invalid = Geometry {
            cell: [0, 16],
            rows: 24,
            columns: 80,
        };
        assert_eq!(
            store.apply(
                &store.stamp(4, false),
                &t,
                t.decode_image().map(Some),
                invalid
            ),
            Err(Failure::Protocol(Error::Invalid))
        );
        assert!(store.pixels(offscreen.image).is_some());
        assert_eq!(store.placements(false).count(), 1);
    }
}
