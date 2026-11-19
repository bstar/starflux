//! Lossless native image planes. Text remains owned by the presenting terminal.
use crate::{
    graphics_paint::{Plane, Raster},
    graphics_resources::{Lease, Resources},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Write},
    sync::Arc,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub below: Option<String>,
    pub above: Option<String>,
}
impl Wire {
    pub fn bytes(&self) -> usize {
        self.below.as_ref().map_or(0, String::len) + self.above.as_ref().map_or(0, String::len)
    }
}
pub struct Encoded {
    pub wire: Wire,
    _memory: Lease,
}
struct Limited(Vec<u8>);
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > 2 * 1024 * 1024 {
            return Err(io::Error::other("Native image planes exceed encoded limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn png(bytes: &[u8], width: u32, height: u32) -> Result<Option<String>> {
    if bytes.is_empty() || bytes.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        return Ok(None);
    }
    let mut output = Limited(Vec::new());
    let mut encoder = png::Encoder::new(&mut output, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(bytes)?;
    writer.finish()?;
    Ok(Some(STANDARD.encode(output.0)))
}
pub fn encode(
    screen: &vt100::Screen,
    raster: &Raster,
    old: Option<&Arc<Encoded>>,
    resources: &Resources,
) -> Result<Arc<Encoded>> {
    let width = raster.view.width;
    let height = raster.view.height;
    ensure!(
        width <= 8192 && height <= 8192,
        "Native plane dimensions exceed host limit"
    );
    let count = usize::try_from(u64::from(width) * u64::from(height) * 4)?;
    let _working = resources
        .presentation
        .reserve(count + 16 * 1024 * 1024)
        .context("Native plane encoding is busy")?;
    let mut below = if raster.layers.iter().any(|l| l.plane != Plane::AboveText) {
        vec![0; count]
    } else {
        Vec::new()
    };
    for layer in &raster.layers {
        if layer.plane == Plane::AboveText {
            continue;
        }
        for (n, pixel) in layer.rgba.as_chunks::<4>().0.iter().enumerate() {
            if layer.plane == Plane::UnderBackground {
                let row = (n / width as usize) / usize::from(raster.view.cell[1]);
                let col = (n % width as usize) / usize::from(raster.view.cell[0]);
                if screen
                    .cell(row as u16, col as u16)
                    .is_none_or(|c| c.bgcolor() != vt100::Color::Default || c.inverse())
                {
                    continue;
                }
            }
            crate::graphics_paint::over(&mut below[n * 4..n * 4 + 4], *pixel);
        }
    }
    let above = raster
        .layers
        .iter()
        .find(|l| l.plane == Plane::AboveText)
        .map_or(&[][..], |l| l.rgba.as_slice());
    let mut hash = Sha256::new();
    hash.update(width.to_le_bytes());
    hash.update(height.to_le_bytes());
    hash.update(&below);
    hash.update(above);
    let id = format!("{:x}", hash.finalize());
    if let Some(old) = old.filter(|old| old.wire.id == id) {
        return Ok(old.clone());
    }
    let wire = Wire {
        id,
        width,
        height,
        below: png(&below, width, height)?,
        above: png(above, width, height)?,
    };
    ensure!(
        wire.bytes() <= 3 * 1024 * 1024,
        "Native image planes exceed wire budget"
    );
    let memory = resources
        .presentation
        .reserve(wire.bytes() * 2 + 4096)
        .context("Retained native planes are busy")?;
    Ok(Arc::new(Encoded {
        wire,
        _memory: memory,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossless_planes_mask_explicit_backgrounds_and_reuse_unchanged_pixels() {
        use crate::{
            graphics_paint::{View, compose},
            graphics_store::{Generation, Geometry, Store},
            graphics_transfer::Assembler,
        };
        let resources = Resources::new(0, 32 * 1024 * 1024);
        let mut store = Store::new(Generation {
            host: "private-planes".into(),
            boot: uuid::Uuid::new_v4(),
            pid: 2,
            start_ticks: 1,
            epoch: uuid::Uuid::new_v4(),
        });
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let mut assembler = Assembler::default();
        let transmission = assembler
            .push(
                format!(
                    "Ga=T,f=32,s=2,v=2,i=1,z=-2147483648;{}",
                    STANDARD.encode(rgba)
                )
                .as_bytes(),
                (0, 0),
            )
            .unwrap()
            .unwrap();
        store
            .apply(
                &store.stamp(1, false),
                &transmission,
                transmission.decode_image().map(Some),
                Geometry {
                    cell: [1, 2],
                    rows: 1,
                    columns: 2,
                },
            )
            .unwrap();
        let raster = compose(
            &store,
            View {
                origin: [0, 0],
                width: 2,
                height: 2,
                cell: [1, 2],
                alternate: false,
            },
            &[],
            &resources,
        )
        .unwrap();
        let mut parser = vt100::Parser::new(1, 2, 0);
        parser.process(b"a\x1b[44mb");
        let encoded = encode(parser.screen(), &raster, None, &resources).unwrap();
        let bytes = STANDARD
            .decode(encoded.wire.below.as_ref().unwrap())
            .unwrap();
        let image = starcore::image::load_from_memory(&bytes)
            .unwrap()
            .to_rgba8();
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(0, 1).0, [0, 0, 255, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [0, 0, 0, 0]);
        assert!(encoded.wire.above.is_none());
        let reused = encode(parser.screen(), &raster, Some(&encoded), &resources).unwrap();
        assert!(Arc::ptr_eq(&encoded, &reused));
    }
}
