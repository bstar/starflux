//! Disposable read-only transport acceptance executable. Not an installed app.
#[path = "../src/graphics_paint.rs"]
mod graphics_paint;
#[path = "../src/graphics_resources.rs"]
mod graphics_resources;
#[path = "../src/graphics_store.rs"]
mod graphics_store;
#[path = "../src/graphics_stream.rs"]
mod graphics_stream;
#[path = "../src/graphics_transfer.rs"]
mod graphics_transfer;
use graphics_stream::{Control, Event, Gap, Passthrough};
use sha2::{Digest, Sha256};
use std::io::Read;
struct Images {
    assembler: graphics_transfer::Assembler,
    store: graphics_store::Store,
    sequence: u64,
    images: usize,
    errors: usize,
    digest: Option<String>,
}
impl Images {
    fn new() -> Self {
        let generation = graphics_store::Generation {
            host: "isolated-proof".into(),
            boot: uuid::Uuid::new_v4(),
            pid: std::process::id(),
            start_ticks: 1,
            epoch: uuid::Uuid::new_v4(),
        };
        Self {
            assembler: Default::default(),
            store: graphics_store::Store::new(generation),
            sequence: 0,
            images: 0,
            errors: 0,
            digest: None,
        }
    }
}
impl vt100::Callbacks for Images {
    fn unhandled_apc(&mut self, screen: &mut vt100::Screen, data: &[u8]) {
        match self.assembler.push(data, screen.cursor_position()) {
            Ok(Some(t)) => {
                self.sequence += 1;
                let decoded = match t.controls.character(b'a', b't') {
                    Ok(b't' | b'T' | b'q') => t.decode_image().map(Some),
                    _ => Ok(None),
                };
                let stamp = self.store.stamp(self.sequence, screen.alternate_screen());
                let (rows, columns) = screen.size();
                match self.store.apply(
                    &stamp,
                    &t,
                    decoded,
                    graphics_store::Geometry {
                        cell: [8, 16],
                        rows,
                        columns,
                    },
                ) {
                    Ok(outcome) => {
                        if let Some(p) = outcome.placement {
                            self.images += 1;
                            self.digest = self
                                .store
                                .pixels(p.image)
                                .map(|p| format!("{:x}", Sha256::digest(&p.rgba)));
                        }
                    }
                    Err(_) => self.errors += 1,
                }
            }
            Ok(None) => (),
            Err(_) => self.errors += 1,
        }
    }
    fn cancelled_apc(&mut self, _: &mut vt100::Screen) {
        self.assembler.cancel();
        self.errors += 1;
    }
}
fn main() -> anyhow::Result<()> {
    let pane = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Expected private pane number"))?
        .parse::<u32>()?;
    let mut control = Control::new(pane);
    let mut passthrough = Passthrough::default();
    let mut parser = vt100::Parser::new_with_callbacks(27, 91, 0, Images::new());
    let mut gaps = 0;
    let mut input = std::io::stdin().lock();
    let mut buffer = [0; 8192];
    let mut total = 0;
    loop {
        let length = input.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        total += length;
        anyhow::ensure!(total <= 8 * 1024 * 1024, "Proof input exceeds its budget");
        control.feed(&buffer[..length], |event| match event {
            Event::Bytes(bytes) => passthrough.feed(bytes, |event| match event {
                Event::Bytes(bytes) => parser.process(bytes),
                Event::Gap(_) => gaps += 1,
            }),
            Event::Gap(Gap::Closed) => (),
            Event::Gap(_) => gaps += 1,
        });
    }
    control.finish(|event| {
        if let Event::Gap(gap) = event
            && gap != Gap::Closed
        {
            gaps += 1;
        }
    });
    passthrough.finish(|_| gaps += 1);
    let images = parser.callbacks();
    let resources = graphics_resources::Resources::new(0, 16 * 1024 * 1024);
    let (rows, columns) = parser.screen().size();
    let view = graphics_paint::View {
        origin: [0, 0],
        width: u32::from(columns) * 8,
        height: u32::from(rows) * 16,
        cell: [8, 16],
        alternate: parser.screen().alternate_screen(),
    };
    let raster = graphics_paint::compose(&images.store, view, &[], &resources)
        .map_err(|e| anyhow::anyhow!("Composition failed: {e:?}"))?;
    for p in images.store.placements(view.alternate) {
        let p = p
            .at_cell(view.cell)
            .map_err(|e| anyhow::anyhow!("View fit failed: {e:?}"))?;
        let x = u32::from(p.origin.1) * 8 + p.offset[0] + p.inset[0];
        let y = u32::from(p.origin.0) * 16 + p.offset[1] + p.inset[1];
        let source = images.store.pixels(p.image).unwrap();
        let offset = ((p.source[1] * source.width + p.source[0]) * 4) as usize;
        anyhow::ensure!(
            raster.sample(graphics_paint::Plane::AboveText, x, y)
                == source.rgba[offset..offset + 4],
            "Private source pixel changed during composition"
        );
    }
    let before = parser.screen().state_formatted();
    let frame = graphics_paint::cells(
        parser.screen(),
        &raster,
        graphics_paint::CellView {
            stream: "private-render-proof".into(),
            generation: 1,
            rect: starkit::terminal_graphics::protocol::Rect {
                x: 0,
                y: 0,
                width: columns,
                height: rows,
            },
            focused: true,
        },
        |color, foreground| match color {
            vt100::Color::Rgb(r, g, b) => [r, g, b],
            _ if foreground => [255; 3],
            _ => [0; 3],
        },
    )
    .map_err(|e| anyhow::anyhow!("Cells composition failed: {e:?}"))?;
    frame.validate()?;
    let mut other_view = view;
    other_view.cell = [12, 24];
    other_view.width = u32::from(columns) * 12;
    other_view.height = u32::from(rows) * 24;
    let other = graphics_paint::compose(&images.store, other_view, &[], &resources)
        .map_err(|e| anyhow::anyhow!("Independent composition failed: {e:?}"))?;
    for source_placement in images.store.placements(view.alternate) {
        let p = source_placement
            .at_cell(other_view.cell)
            .map_err(|e| anyhow::anyhow!("Independent fit failed: {e:?}"))?;
        let x = u32::from(p.origin.1) * 12 + p.offset[0] + p.inset[0];
        let y = u32::from(p.origin.0) * 24 + p.offset[1] + p.inset[1];
        let pixels = images.store.pixels(p.image).unwrap();
        let offset = ((p.source[1] * pixels.width + p.source[0]) * 4) as usize;
        anyhow::ensure!(
            other.sample(graphics_paint::Plane::AboveText, x, y) == pixels.rgba[offset..offset + 4],
            "Independent view changed source pixel"
        );
        if p.pixels != source_placement.pixels {
            anyhow::ensure!(
                other.sample(graphics_paint::Plane::AboveText, x + p.pixels[0] - 1, y)
                    == pixels.rgba[offset..offset + 4],
                "Resized private solid image did not fill its fitted extent"
            );
        }
    }
    graphics_paint::cells(
        parser.screen(),
        &other,
        graphics_paint::CellView {
            stream: "private-other-view".into(),
            generation: 2,
            rect: frame.rect,
            focused: false,
        },
        |c, fg| match c {
            vt100::Color::Rgb(r, g, b) => [r, g, b],
            _ if fg => [255; 3],
            _ => [0; 3],
        },
    )
    .map_err(|e| anyhow::anyhow!("Independent cells failed: {e:?}"))?
    .validate()?;
    drop(other);
    anyhow::ensure!(
        before == parser.screen().state_formatted(),
        "Composition changed source history"
    );
    if let Some(directory) = std::env::args().nth(2) {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory)?;
        for (index, layer) in raster.layers.iter().enumerate() {
            let mut encoder = png::Encoder::new(
                std::fs::File::create(directory.join(format!("plane-{index}.png")))?,
                view.width,
                view.height,
            );
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&layer.rgba)?;
        }
        std::fs::write(directory.join("cells.json"), serde_json::to_vec(&frame)?)?;
    }
    drop(raster);
    anyhow::ensure!(
        resources.working.used() == 0,
        "Composition retained a working reservation"
    );
    println!(
        "{}",
        serde_json::json!({"images":images.images,"placements":images.store.placements(false).count(),"rgba_sha256":images.digest,"errors":images.errors,"gaps":gaps,"text":parser.screen().contents(),"composition_verified":true,"independent_cell_fit":true,"cells_frame_valid":true,"source_history_unchanged":true,"composition_reservations_released":true})
    );
    anyhow::ensure!(
        images.errors == 0 && gaps == 0,
        "Private graphics stream was not complete"
    );
    Ok(())
}
