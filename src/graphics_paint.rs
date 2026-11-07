//! Read-only composition of retained images. A view gets three independent
//! alpha planes so text/background ordering need not be flattened incorrectly.
use crate::{
    graphics_resources::{Lease, Resources},
    graphics_store::{Placement, Store},
    graphics_transfer::Error,
};
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_WORK: u64 = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Plane {
    UnderBackground,
    UnderText,
    AboveText,
}
impl Plane {
    fn of(z: i32) -> Self {
        if z < i32::MIN / 2 {
            Self::UnderBackground
        } else if z < 0 {
            Self::UnderText
        } else {
            Self::AboveText
        }
    }
    fn index(self) -> usize {
        match self {
            Self::UnderBackground => 0,
            Self::UnderText => 1,
            Self::AboveText => 2,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy)]
pub struct View {
    pub origin: [i32; 2],
    pub width: u32,
    pub height: u32,
    pub cell: [u16; 2],
    pub alternate: bool,
}
pub struct Layer {
    pub plane: Plane,
    pub rgba: Vec<u8>,
}
pub struct Raster {
    pub view: View,
    pub layers: Vec<Layer>,
    _memory: Lease,
}
impl Raster {
    pub fn sample(&self, plane: Plane, x: u32, y: u32) -> [u8; 4] {
        if x >= self.view.width || y >= self.view.height {
            return [0; 4];
        }
        let Some(layer) = self.layers.iter().find(|layer| layer.plane == plane) else {
            return [0; 4];
        };
        let offset = ((u64::from(y) * u64::from(self.view.width) + u64::from(x)) * 4) as usize;
        layer.rgba[offset..offset + 4].try_into().unwrap()
    }
    fn average(&self, plane: Plane, region: [u32; 4]) -> [u8; 4] {
        let Some(layer) = self.layers.iter().find(|layer| layer.plane == plane) else {
            return [0; 4];
        };
        let [x, y, width, height] = region;
        let mut alpha = 0_u64;
        let mut colors = [0_u64; 3];
        let mut count = 0_u64;
        for row in y..y.saturating_add(height).min(self.view.height) {
            for col in x..x.saturating_add(width).min(self.view.width) {
                let offset =
                    ((u64::from(row) * u64::from(self.view.width) + u64::from(col)) * 4) as usize;
                let pixel = &layer.rgba[offset..offset + 4];
                alpha += u64::from(pixel[3]);
                count += 1;
                for index in 0..3 {
                    colors[index] += u64::from(pixel[index]) * u64::from(pixel[3]);
                }
            }
        }
        if alpha == 0 || count == 0 {
            return [0; 4];
        }
        [
            (colors[0].saturating_add(alpha / 2) / alpha) as u8,
            (colors[1].saturating_add(alpha / 2) / alpha) as u8,
            (colors[2].saturating_add(alpha / 2) / alpha) as u8,
            ((alpha + count / 2) / count) as u8,
        ]
    }
}
struct Bounds {
    left: i64,
    top: i64,
    clip: [u32; 4],
}
fn bounds(p: &Placement, view: View) -> Option<Bounds> {
    let left = i64::from(p.origin.1) * i64::from(view.cell[0])
        + i64::from(p.offset[0])
        + i64::from(p.inset[0])
        - i64::from(view.origin[0]);
    let top = i64::from(p.row()) * i64::from(view.cell[1])
        + i64::from(p.offset[1])
        + i64::from(p.inset[1])
        - i64::from(view.origin[1]);
    let x0 = left.max(0).min(i64::from(view.width));
    let clip = p.vertical_clip().map(|[a, b]| {
        [
            i64::from(a) * i64::from(view.cell[1]) - i64::from(view.origin[1]),
            i64::from(b) * i64::from(view.cell[1]) - i64::from(view.origin[1]),
        ]
    });
    let y0 = top
        .max(clip.map_or(i64::MIN, |r| r[0]))
        .max(0)
        .min(i64::from(view.height));
    let x1 = (left + i64::from(p.pixels[0]))
        .max(0)
        .min(i64::from(view.width));
    let y1 = (top + i64::from(p.pixels[1]))
        .min(clip.map_or(i64::MAX, |r| r[1]))
        .max(0)
        .min(i64::from(view.height));
    (x1 > x0 && y1 > y0).then_some(Bounds {
        left,
        top,
        clip: [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32],
    })
}
fn clip(rect: Rect, view: View) -> [u32; 4] {
    let x0 = i64::from(rect.x).max(0).min(i64::from(view.width));
    let y0 = i64::from(rect.y).max(0).min(i64::from(view.height));
    let x1 = (i64::from(rect.x) + i64::from(rect.width))
        .max(x0)
        .min(i64::from(view.width));
    let y1 = (i64::from(rect.y) + i64::from(rect.height))
        .max(y0)
        .min(i64::from(view.height));
    [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32]
}
fn area(rect: [u32; 4]) -> u64 {
    u64::from(rect[2]) * u64::from(rect[3])
}
pub fn compose(
    store: &Store,
    view: View,
    masks: &[Rect],
    resources: &Resources,
) -> Result<Raster, Error> {
    let pixels = u64::from(view.width) * u64::from(view.height);
    if view.width == 0 || view.height == 0 || view.cell.iter().any(|n| *n == 0 || *n > 256) {
        return Err(Error::Invalid);
    }
    if pixels > MAX_PIXELS || masks.len() > 64 {
        return Err(Error::TooLarge);
    }
    let mut placements = Vec::new();
    for p in store.placements(view.alternate) {
        let placement = p.at_cell(view.cell)?;
        if let Some(bounds) = bounds(&placement, view) {
            placements.push((placement, bounds));
        }
    }
    if placements.is_empty() {
        return Ok(Raster {
            view,
            layers: Vec::new(),
            _memory: resources.working.reserve(0).ok_or(Error::Busy)?,
        });
    }
    placements.sort_by_key(|(p, _)| (p.z, store.protocol_id(p.image).unwrap_or(0)));
    let mut needed = [false; 3];
    let mut work = 0_u64;
    for (p, b) in &placements {
        needed[Plane::of(p.z).index()] = true;
        work += area(b.clip);
    }
    let mask_regions = masks
        .iter()
        .map(|mask| clip(*mask, view))
        .collect::<Vec<_>>();
    work += mask_regions.iter().map(|r| area(*r)).sum::<u64>();
    let bytes =
        pixels * (4 * needed.iter().filter(|n| **n).count() as u64 + u64::from(!masks.is_empty()));
    if bytes > MAX_BYTES || work > MAX_WORK {
        return Err(Error::TooLarge);
    }
    let mut memory = resources
        .working
        .reserve(bytes as usize)
        .ok_or(Error::Busy)?;
    let mut layers = Vec::new();
    for plane in [Plane::UnderBackground, Plane::UnderText, Plane::AboveText] {
        if needed[plane.index()] {
            layers.push(Layer {
                plane,
                rgba: vec![0; (pixels * 4) as usize],
            });
        }
    }
    let mut masked = if masks.is_empty() {
        Vec::new()
    } else {
        vec![0; pixels as usize]
    };
    for [x, y, width, height] in mask_regions {
        for row in y..y + height {
            let start = (u64::from(row) * u64::from(view.width) + u64::from(x)) as usize;
            masked[start..start + width as usize].fill(1);
        }
    }
    let mut painted = [false; 3];
    for (placement, b) in placements {
        let plane = Plane::of(placement.z);
        let output = layers
            .iter_mut()
            .find(|layer| layer.plane == plane)
            .unwrap();
        let input = store.pixels(placement.image).ok_or(Error::Invalid)?;
        let [x, y, width, height] = b.clip;
        let [sx, sy, sw, sh] = placement.source;
        for row in y..y + height {
            let source_y = sy
                + ((row as i64 - b.top) as u64 * u64::from(sh) / u64::from(placement.pixels[1]))
                    as u32;
            let target_start = (u64::from(row) * u64::from(view.width) + u64::from(x)) as usize;
            if !painted[plane.index()]
                && sw == placement.pixels[0]
                && sh == placement.pixels[1]
                && (masked.is_empty()
                    || masked[target_start..target_start + width as usize]
                        .iter()
                        .all(|m| *m == 0))
            {
                let source_x = sx + (i64::from(x) - b.left) as u32;
                let from = ((u64::from(source_y) * u64::from(input.width) + u64::from(source_x))
                    * 4) as usize;
                output.rgba[target_start * 4..(target_start + width as usize) * 4]
                    .copy_from_slice(&input.rgba[from..from + width as usize * 4]);
                continue;
            }
            for col in x..x + width {
                let target = (u64::from(row) * u64::from(view.width) + u64::from(col)) as usize;
                if !masked.is_empty() && masked[target] != 0 {
                    continue;
                }
                let source_x = sx
                    + ((i64::from(col) - b.left) as u64 * u64::from(sw)
                        / u64::from(placement.pixels[0])) as u32;
                let source = ((u64::from(source_y) * u64::from(input.width) + u64::from(source_x))
                    * 4) as usize;
                over(
                    &mut output.rgba[target * 4..target * 4 + 4],
                    input.rgba[source..source + 4].try_into().unwrap(),
                );
            }
        }
        painted[plane.index()] = true;
    }
    drop(masked);
    memory.shrink_to(layers.iter().map(|layer| layer.rgba.capacity()).sum());
    Ok(Raster {
        view,
        layers,
        _memory: memory,
    })
}
fn over(destination: &mut [u8], source: [u8; 4]) {
    let sa = u32::from(source[3]);
    if sa == 0 {
        return;
    }
    if sa == 255 {
        destination.copy_from_slice(&source);
        return;
    }
    let da = u32::from(destination[3]);
    let alpha = sa * 255 + da * (255 - sa);
    for index in 0..3 {
        destination[index] = ((u32::from(source[index]) * sa * 255
            + u32::from(destination[index]) * da * (255 - sa)
            + alpha / 2)
            / alpha) as u8;
    }
    destination[3] = ((alpha + 127) / 255) as u8;
}
pub struct CellView {
    pub stream: String,
    pub generation: u64,
    pub rect: starkit::terminal_graphics::protocol::Rect,
    pub focused: bool,
}
fn background(mut rgb: [u8; 3], pixel: [u8; 4]) -> [u8; 3] {
    let mut rgba = [rgb[0], rgb[1], rgb[2], 255];
    over(&mut rgba, pixel);
    rgb.copy_from_slice(&rgba[..3]);
    rgb
}
fn halves(
    raster: &Raster,
    col: u16,
    row: u16,
    columns: u16,
    default_background: bool,
    bg: [u8; 3],
) -> ([[u8; 3]; 2], bool) {
    let [cw, ch] = raster.view.cell;
    let top = u32::from(row) * u32::from(ch);
    let x = u32::from(col) * u32::from(cw);
    let split = u32::from(ch).div_ceil(2);
    let regions = [
        [x, top, u32::from(columns) * u32::from(cw), split],
        [
            x,
            top + split,
            u32::from(columns) * u32::from(cw),
            u32::from(ch) - split,
        ],
    ];
    let mut colors = [bg; 2];
    let mut above = false;
    for (index, region) in regions.into_iter().enumerate() {
        if default_background {
            colors[index] = background(
                colors[index],
                raster.average(Plane::UnderBackground, region),
            );
        }
        colors[index] = background(colors[index], raster.average(Plane::UnderText, region));
        let front = raster.average(Plane::AboveText, region);
        above |= front[3] != 0;
        colors[index] = background(colors[index], front);
    }
    // A one-pixel source row has no distinct lower sample.
    if ch == 1 {
        colors[1] = colors[0];
    }
    (colors, above)
}
/// Half-block fallback is an approximation; the terminal screen is immutable.
/// A positive image covering part of a wide glyph replaces its whole cluster,
/// preventing dangling continuation cells. History/copy still use source text.
pub fn cells(
    screen: &vt100::Screen,
    raster: &Raster,
    view: CellView,
    color: impl Fn(vt100::Color, bool) -> [u8; 3],
) -> Result<starkit::terminal_graphics::terminal_cells::Frame, Error> {
    use starkit::terminal_graphics::terminal_cells::{Frame, Row, Run};
    let rect = view.rect;
    let [cw, ch] = raster.view.cell;
    let (rows, cols) = screen.size();
    if raster.view.origin != [0, 0]
        || rect.width == 0
        || rect.height == 0
        || rect.width > 512
        || rect.height > 192
        || rect.width > cols
        || rect.height > rows
        || u32::from(rect.width) * u32::from(cw) != raster.view.width
        || u32::from(rect.height) * u32::from(ch) != raster.view.height
    {
        return Err(Error::Invalid);
    }
    let append = |runs: &mut Vec<Run>, run: Run| {
        if let Some(last) = runs.last_mut().filter(|last| {
            last.foreground == run.foreground
                && last.background == run.background
                && last.attributes == run.attributes
        }) {
            last.text.push_str(&run.text);
            last.columns += run.columns;
        } else {
            runs.push(run);
        }
    };
    let mut output = Vec::with_capacity(usize::from(rect.height));
    for row in 0..rect.height {
        let mut runs = Vec::new();
        let mut col = 0;
        while col < rect.width {
            let cell = screen.cell(row, col).ok_or(Error::Invalid)?;
            let span = (if cell.is_wide() { 2 } else { 1 }).min(rect.width - col);
            let text = if !cell.has_contents() || (cell.is_wide() && span == 1) {
                " ".to_string()
            } else {
                cell.contents().to_string()
            };
            let original_fg = color(cell.fgcolor(), true);
            let original_bg = color(cell.bgcolor(), false);
            let (fg, bg) = if cell.inverse() {
                (original_bg, original_fg)
            } else {
                (original_fg, original_bg)
            };
            let attributes = u8::from(cell.bold())
                | (u8::from(cell.dim()) << 1)
                | (u8::from(cell.italic()) << 2)
                | (u8::from(cell.underline()) << 3)
                | (u8::from(cell.inverse()) << 4);
            let default_bg = cell.bgcolor() == vt100::Color::Default && !cell.inverse();
            let (colors, above) = halves(raster, col, row, span, default_bg, bg);
            if above || text == " " {
                for column in col..col + span {
                    let (halves, _) = halves(raster, column, row, 1, default_bg, bg);
                    if halves == [bg; 2] && !above {
                        append(
                            &mut runs,
                            Run {
                                text: " ".into(),
                                columns: 1,
                                foreground: original_fg,
                                background: original_bg,
                                attributes,
                            },
                        );
                    } else {
                        append(
                            &mut runs,
                            Run {
                                text: if halves[0] == halves[1] {
                                    " ".into()
                                } else {
                                    "▀".into()
                                },
                                columns: 1,
                                foreground: halves[0],
                                background: halves[1],
                                attributes: 0,
                            },
                        );
                    }
                }
            } else if colors == [bg; 2] {
                append(
                    &mut runs,
                    Run {
                        text,
                        columns: span,
                        foreground: original_fg,
                        background: original_bg,
                        attributes,
                    },
                );
            } else {
                let mixed = std::array::from_fn(|i| {
                    (u16::from(colors[0][i]) + u16::from(colors[1][i])).div_ceil(2) as u8
                });
                append(
                    &mut runs,
                    Run {
                        text,
                        columns: span,
                        foreground: fg,
                        background: mixed,
                        attributes: attributes & !16,
                    },
                );
            }
            col += span;
        }
        output.push(Row { index: row, runs });
    }
    let (row, col) = screen.cursor_position();
    let cursor = (view.focused
        && !screen.hide_cursor()
        && screen.scrollback() == 0
        && row < rect.height
        && col < rect.width)
        .then_some([col, row]);
    let frame = Frame {
        stream: view.stream,
        generation: view.generation,
        rect,
        complete: true,
        rows: output,
        cursor,
    };
    frame.validate().map_err(|_| Error::Invalid)?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics_store::{Generation, Geometry};
    use crate::graphics_transfer::Assembler;
    use base64::{Engine, engine::general_purpose::STANDARD};

    struct World {
        store: Store,
        sequence: u64,
    }
    impl World {
        fn new() -> Self {
            Self {
                store: Store::new(Generation {
                    host: "private-render-test".into(),
                    boot: uuid::Uuid::new_v4(),
                    pid: 2,
                    start_ticks: 1,
                    epoch: uuid::Uuid::new_v4(),
                }),
                sequence: 0,
            }
        }
        fn put(
            &mut self,
            header: &str,
            size: [u32; 2],
            rgba: &[u8],
            cursor: (u16, u16),
            alt: bool,
        ) {
            let t = Assembler::default()
                .push(
                    format!(
                        "Ga=T,f=32,s={},v={},{header};{}",
                        size[0],
                        size[1],
                        STANDARD.encode(rgba)
                    )
                    .as_bytes(),
                    cursor,
                )
                .unwrap()
                .unwrap();
            self.sequence += 1;
            self.store
                .apply(
                    &self.store.stamp(self.sequence, alt),
                    &t,
                    t.decode_image().map(Some),
                    Geometry {
                        cell: [1, 2],
                        rows: 8,
                        columns: 8,
                    },
                )
                .unwrap();
        }
        fn raster(&self, width: u32, height: u32) -> Raster {
            compose(
                &self.store,
                view(width, height),
                &[],
                &Resources::new(0, 1024 * 1024),
            )
            .unwrap()
        }
    }
    fn view(width: u32, height: u32) -> View {
        View {
            origin: [0, 0],
            width,
            height,
            cell: [1, 2],
            alternate: false,
        }
    }
    fn palette(c: vt100::Color, fg: bool) -> [u8; 3] {
        match c {
            vt100::Color::Rgb(r, g, b) => [r, g, b],
            vt100::Color::Idx(4) => [0, 0, 255],
            _ if fg => [255; 3],
            _ => [0; 3],
        }
    }
    fn fallback(
        screen: &vt100::Screen,
        raster: &Raster,
    ) -> starkit::terminal_graphics::terminal_cells::Frame {
        cells(
            screen,
            raster,
            CellView {
                stream: "private".into(),
                generation: 1,
                rect: starkit::terminal_graphics::protocol::Rect {
                    x: 0,
                    y: 0,
                    width: screen.size().1,
                    height: screen.size().0,
                },
                focused: true,
            },
            palette,
        )
        .unwrap()
    }
    #[test]
    fn native_crop_and_viewport_clipping_preserve_source_pixels() {
        let mut w = World::new();
        w.put(
            "i=1,x=1,w=3",
            [4, 1],
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
            (0, 0),
            false,
        );
        let mut v = view(2, 1);
        v.origin = [1, 0];
        let r = compose(&w.store, v, &[], &Resources::new(0, 1024)).unwrap();
        assert_eq!(r.sample(Plane::AboveText, 0, 0), [0, 0, 255, 255]);
        assert_eq!(r.sample(Plane::AboveText, 1, 0), [255; 4]);
        assert_eq!(r.sample(Plane::AboveText, 2, 0), [0; 4]);
    }
    #[test]
    fn z_order_uses_protocol_ids_and_preserves_alpha_planes() {
        let mut w = World::new();
        w.put("i=90", [1, 1], &[255, 0, 0, 255], (0, 0), false);
        w.put("i=1", [1, 1], &[0, 0, 255, 128], (0, 0), false);
        assert_eq!(
            w.raster(1, 1).sample(Plane::AboveText, 0, 0),
            [255, 0, 0, 255]
        );
        w.put("i=100", [1, 1], &[0, 0, 255, 128], (0, 0), false);
        w.put("i=2,z=-1", [1, 1], &[0, 255, 0, 128], (0, 0), false);
        w.put(
            "i=3,z=-2147483648",
            [1, 1],
            &[255, 0, 0, 255],
            (0, 0),
            false,
        );
        let r = w.raster(1, 1);
        assert_eq!(r.sample(Plane::AboveText, 0, 0), [127, 0, 128, 255]);
        assert_eq!(r.sample(Plane::UnderText, 0, 0), [0, 255, 0, 128]);
        assert_eq!(r.sample(Plane::UnderBackground, 0, 0), [255, 0, 0, 255]);
    }
    #[test]
    fn popup_masks_and_raster_drop_release_working_memory() {
        let mut w = World::new();
        for (id, z) in [(1, 0), (2, -1), (3, i32::MIN)] {
            w.put(&format!("i={id},z={z}"), [2, 2], &[255; 16], (0, 0), false);
        }
        let resources = Resources::new(0, 100);
        let r = compose(
            &w.store,
            view(2, 2),
            &[Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 2,
            }],
            &resources,
        )
        .unwrap();
        assert_eq!(resources.working.used(), 48);
        for plane in [Plane::AboveText, Plane::UnderText, Plane::UnderBackground] {
            assert_eq!(r.sample(plane, 0, 1), [0; 4]);
            assert_eq!(r.sample(plane, 1, 1), [255; 4]);
        }
        drop(r);
        assert_eq!(resources.working.used(), 0);
        let empty = compose(&World::new().store, view(2, 2), &[], &Resources::new(0, 0)).unwrap();
        assert!(empty.layers.is_empty());
    }
    #[test]
    fn alternate_buffers_and_view_origins_are_independent() {
        let mut w = World::new();
        w.put("i=1", [1, 1], &[255, 0, 0, 255], (1, 1), false);
        w.put("i=1", [1, 1], &[0, 0, 255, 255], (1, 1), true);
        let mut v = view(1, 1);
        v.origin = [1, 2];
        let a = compose(&w.store, v, &[], &Resources::new(0, 100)).unwrap();
        v.alternate = true;
        v.cell = [2, 4];
        v.origin = [2, 4];
        let b = compose(&w.store, v, &[], &Resources::new(0, 100)).unwrap();
        assert_eq!(a.sample(Plane::AboveText, 0, 0), [255, 0, 0, 255]);
        assert_eq!(b.sample(Plane::AboveText, 0, 0), [0, 0, 255, 255]);
    }
    #[test]
    fn resources_and_dimensions_are_checked_before_painting() {
        let mut w = World::new();
        w.put("i=1", [1, 1], &[255; 4], (0, 0), false);
        let resources = Resources::new(0, 0);
        assert!(matches!(
            compose(&w.store, view(1, 1), &[], &resources),
            Err(Error::Busy)
        ));
        assert!(matches!(
            compose(&w.store, view(u32::MAX, u32::MAX), &[], &resources),
            Err(Error::TooLarge)
        ));
        assert_eq!(resources.working.used(), 0);
        // KIT's grid bound must be checked before visiting cells or allocating runs.
        let parser = vt100::Parser::new(1, 513, 0);
        let raster = compose(
            &World::new().store,
            view(513, 2),
            &[],
            &Resources::new(0, 0),
        )
        .unwrap();
        assert!(
            cells(
                parser.screen(),
                &raster,
                CellView {
                    stream: "private".into(),
                    generation: 1,
                    rect: starkit::terminal_graphics::protocol::Rect {
                        x: 0,
                        y: 0,
                        width: 513,
                        height: 1
                    },
                    focused: false
                },
                palette
            )
            .is_err()
        );
    }
    #[test]
    fn fallback_replaces_wide_cluster_without_mutating_history() {
        let mut w = World::new();
        w.put(
            "i=1",
            [1, 2],
            &[255, 0, 0, 255, 0, 0, 255, 255],
            (0, 1),
            false,
        );
        let mut parser = vt100::Parser::new(1, 2, 10);
        parser.process("界".as_bytes());
        let before = parser.screen().state_formatted();
        let frame = fallback(parser.screen(), &w.raster(2, 2));
        frame.validate().unwrap();
        assert_eq!(
            frame.rows[0]
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
            " ▀"
        );
        let last = frame.rows[0].runs.last().unwrap();
        assert_eq!(last.foreground, [255, 0, 0]);
        assert_eq!(last.background, [0, 0, 255]);
        assert_eq!(parser.screen().state_formatted(), before);
        assert_eq!(parser.screen().contents(), "界");
    }
    #[test]
    fn deep_images_respect_nondefault_background_and_preserve_text() {
        let mut w = World::new();
        w.put(
            "i=1,z=-2147483648",
            [2, 2],
            &[255, 0, 0, 255].repeat(4),
            (0, 0),
            false,
        );
        let mut parser = vt100::Parser::new(1, 2, 0);
        parser.process(b"\x1b[44mA\x1b[49mB");
        let frame = fallback(parser.screen(), &w.raster(2, 2));
        assert_eq!(frame.rows[0].runs[0].text, "A");
        assert_eq!(frame.rows[0].runs[0].background, [0, 0, 255]);
        assert_eq!(frame.rows[0].runs[1].text, "B");
        assert_eq!(frame.rows[0].runs[1].background, [255, 0, 0]);
    }
    #[test]
    fn transparent_images_preserve_combining_text_and_attributes() {
        let mut w = World::new();
        w.put("i=1", [1, 2], &[255, 0, 0, 0, 0, 0, 255, 0], (0, 0), false);
        let mut parser = vt100::Parser::new(1, 2, 0);
        parser.process("\x1b[1mé".as_bytes());
        let frame = fallback(parser.screen(), &w.raster(2, 2));
        assert!(frame.rows[0].runs[0].text.starts_with("é"));
        assert_eq!(frame.rows[0].runs[0].attributes, 1);
    }
    #[test]
    fn cell_sized_fit_tracks_each_view_without_changing_source_placement() {
        let mut w = World::new();
        let row = [
            255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255,
        ];
        w.put("i=1,c=2,r=2", [4, 2], &row.repeat(2), (1, 1), false);
        let original = w.store.placements(false).next().unwrap().clone();
        let mut v = view(16, 16);
        v.cell = [4, 4];
        let raster = compose(&w.store, v, &[], &Resources::new(0, 4096)).unwrap();
        assert_eq!(raster.sample(Plane::AboveText, 4, 5), [0; 4]);
        assert_eq!(raster.sample(Plane::AboveText, 4, 6), [255, 0, 0, 255]);
        assert_eq!(raster.sample(Plane::AboveText, 11, 9), [0, 255, 0, 255]);
        assert_eq!(raster.sample(Plane::AboveText, 12, 9), [0; 4]);
        let small = w.raster(8, 8);
        assert_eq!(small.sample(Plane::AboveText, 1, 3), [255, 0, 0, 255]);
        assert_eq!(small.sample(Plane::AboveText, 2, 3), [0, 255, 0, 255]);
        assert_eq!(*w.store.placements(false).next().unwrap(), original);
    }
    #[test]
    fn natural_images_move_with_cells_but_keep_native_pixel_dimensions() {
        let mut w = World::new();
        w.put(
            "i=1",
            [2, 1],
            &[255, 0, 0, 255, 0, 0, 255, 255],
            (1, 1),
            false,
        );
        let mut v = view(10, 10);
        v.cell = [4, 4];
        let raster = compose(&w.store, v, &[], &Resources::new(0, 4096)).unwrap();
        assert_eq!(raster.sample(Plane::AboveText, 4, 4), [255, 0, 0, 255]);
        assert_eq!(raster.sample(Plane::AboveText, 5, 4), [0, 0, 255, 255]);
        assert_eq!(raster.sample(Plane::AboveText, 6, 4), [0; 4]);
        assert_eq!(raster.sample(Plane::AboveText, 4, 5), [0; 4]);
    }
    #[test]
    fn one_axis_fit_preserves_aspect_and_explicit_cursor_geometry() {
        let mut columns = World::new();
        columns.put("i=1,c=3", [2, 1], &[255; 8], (0, 0), false);
        let source = columns.store.placements(false).next().unwrap();
        let source_before = source.clone();
        let frontend = source.at_cell([4, 3]).unwrap();
        assert_eq!(frontend.pixels, [12, 6]);
        assert_eq!(frontend.cells, [3, 2]);
        assert_eq!(*source, source_before);
        let mut rows = World::new();
        rows.put("i=1,r=3", [2, 1], &[255; 8], (0, 0), false);
        let frontend = rows
            .store
            .placements(false)
            .next()
            .unwrap()
            .at_cell([4, 3])
            .unwrap();
        assert_eq!(frontend.pixels, [18, 9]);
        assert_eq!(frontend.cells, [5, 3]);
    }
    #[test]
    fn font_reduction_cannot_underflow_pixel_offsets_or_mutate_retained_data() {
        let mut w = World::new();
        w.put("i=1,r=1,Y=1", [1, 1], &[255; 4], (0, 0), false);
        let source = w.store.placements(false).next().unwrap().clone();
        let reduced = source.at_cell([1, 1]).unwrap();
        assert_eq!(reduced.pixels, [1, 1]);
        assert_eq!(reduced.offset, [0, 1]);
        assert!(source.at_cell([0, 1]).is_err());
        assert!(source.at_cell([1, 257]).is_err());
        assert_eq!(*w.store.placements(false).next().unwrap(), source);
    }
}
