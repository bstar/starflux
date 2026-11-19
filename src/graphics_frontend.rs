//! Display-side Kitty image planes. Only validated inline PNG bytes are accepted;
//! no file, command, path, arbitrary escape, or caller-selected Kitty ID is used.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::{collections::BTreeMap, io::Write};
pub const META: &str = "starflux-image-planes-v1";
struct Slot {
    signature: String,
    ids: Vec<u32>,
    encoded: usize,
    decoded: usize,
}
pub struct Presenter {
    slots: BTreeMap<String, Slot>,
    next: u32,
    generation: u64,
}
impl Default for Presenter {
    fn default() -> Self {
        Self {
            slots: BTreeMap::new(),
            next: 0xe0000000,
            generation: 0,
        }
    }
}
fn numbers<const N: usize>(value: &Value) -> Result<[u32; N]> {
    let array = value.as_array().context("Missing native image geometry")?;
    ensure!(array.len() == N, "Invalid native image geometry");
    let mut result = [0; N];
    for (a, b) in result.iter_mut().zip(array) {
        *a = b
            .as_u64()
            .and_then(|n| n.try_into().ok())
            .context("Invalid native image coordinate")?;
    }
    Ok(result)
}
fn retire(ids: &[u32], out: &mut impl Write) -> Result<()> {
    for id in ids {
        write!(out, "\x1b_Ga=d,d=I,i={id},q=2;\x1b\\")?;
    }
    Ok(())
}
impl Presenter {
    pub fn clear(&mut self, out: &mut impl Write) -> Result<()> {
        for slot in self.slots.values() {
            retire(&slot.ids, out)?;
        }
        self.slots.clear();
        out.flush()?;
        Ok(())
    }
    pub fn apply(&mut self, value: &Value, out: &mut impl Write) -> Result<()> {
        let view = value["view"]
            .as_str()
            .context("Missing native image view")?;
        ensure!(
            !view.is_empty()
                && view.len() <= 96
                && view
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "Invalid native image view"
        );
        if value["op"] == "clear" {
            if let Some(old) = self.slots.remove(view) {
                retire(&old.ids, out)?;
                out.flush()?;
            }
            return Ok(());
        }
        ensure!(value["op"] == "planes", "Invalid native image operation");
        let generation = value["generation"]
            .as_u64()
            .filter(|n| *n > 0)
            .context("Invalid native image generation")?;
        if generation < self.generation {
            return Ok(());
        }
        let [x, y, columns, rows] = numbers::<4>(&value["rect"])?;
        let [cw, ch] = numbers::<2>(&value["cell"])?;
        ensure!(
            columns > 0
                && columns <= 512
                && rows > 0
                && rows <= 192
                && x.checked_add(columns).is_some_and(|n| n <= 4096)
                && y.checked_add(rows).is_some_and(|n| n <= 4096)
                && (1..=256).contains(&cw)
                && (1..=256).contains(&ch),
            "Native image placement exceeds limit"
        );
        let planes = &value["planes"];
        let width = planes["width"]
            .as_u64()
            .context("Missing native image width")?;
        let height = planes["height"]
            .as_u64()
            .context("Missing native image height")?;
        ensure!(
            width == u64::from(columns * cw)
                && height == u64::from(rows * ch)
                && width * height <= 16 * 1024 * 1024,
            "Native image dimensions disagree"
        );
        let mut images = Vec::new();
        let mut encoded = 0;
        let mut decoded = 0;
        for (field, z) in [("below", -1), ("above", 1)] {
            if planes[field].is_null() {
                continue;
            }
            let png = planes[field].as_str().context("Invalid native plane")?;
            ensure!(
                png.len() <= 3 * 1024 * 1024,
                "Native plane exceeds wire limit"
            );
            let bytes = STANDARD.decode(png)?;
            let dimensions = starcore::image::ImageReader::with_format(
                std::io::Cursor::new(&bytes),
                starcore::image::ImageFormat::Png,
            )
            .into_dimensions()?;
            ensure!(
                (u64::from(dimensions.0), u64::from(dimensions.1)) == (width, height),
                "Native plane PNG dimensions disagree"
            );
            encoded += png.len();
            decoded += (width * height * 4) as usize;
            images.push((png, z));
        }
        ensure!(
            encoded <= 3 * 1024 * 1024,
            "Native planes exceed wire limit"
        );
        let signature = value.to_string();
        if generation == self.generation
            && self
                .slots
                .get(view)
                .is_some_and(|s| s.signature == signature)
        {
            return Ok(());
        }
        ensure!(
            self.slots.contains_key(view) || self.slots.len() < 32,
            "Too many native image views"
        );
        ensure!(
            self.slots
                .iter()
                .filter(|(id, _)| id.as_str() != view)
                .map(|(_, s)| s.encoded)
                .sum::<usize>()
                + encoded
                <= 16 * 1024 * 1024,
            "Native image cache is busy"
        );
        ensure!(
            self.slots
                .iter()
                .filter(|(id, _)| id.as_str() != view)
                .map(|(_, s)| s.decoded)
                .sum::<usize>()
                + decoded
                <= 64 * 1024 * 1024,
            "Native image textures are busy"
        );
        // All validation precedes output. Replace placements before retiring old
        // IDs, under the same stdout lock used by the native text renderer.
        write!(out, "\x1b[?2026h\x1b7\x1b[{};{}H", y + 1, x + 1)?;
        if generation > self.generation {
            self.clear(out)?;
            self.generation = generation;
        }
        let mut ids = Vec::new();
        for (png, z) in images {
            self.next = self
                .next
                .checked_add(1)
                .context("Native image IDs exhausted")?;
            let id = self.next;
            let chunks = png.as_bytes().chunks(4096);
            let count = chunks.len();
            for (n, chunk) in chunks.enumerate() {
                let more = usize::from(n + 1 < count);
                if n == 0 {
                    write!(
                        out,
                        "\x1b_Ga=T,t=d,f=100,z={z},i={id},p=1,C=1,q=2,c={columns},r={rows},m={more};"
                    )?;
                } else {
                    write!(out, "\x1b_Gm={more};")?;
                }
                out.write_all(chunk)?;
                out.write_all(b"\x1b\\")?;
            }
            ids.push(id);
        }
        if let Some(old) = self.slots.remove(view) {
            retire(&old.ids, out)?;
        }
        self.slots.insert(
            view.into(),
            Slot {
                signature,
                ids,
                encoded,
                decoded,
            },
        );
        out.write_all(b"\x1b8\x1b[?2026l")?;
        out.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command() -> Value {
        let mut out = std::io::Cursor::new(Vec::new());
        starcore::image::RgbaImage::from_raw(2, 2, vec![255; 16])
            .unwrap()
            .write_to(&mut out, starcore::image::ImageFormat::Png)
            .unwrap();
        let png = STANDARD.encode(out.into_inner());
        serde_json::json!({"op":"planes","view":"private-pane","generation":1,"rect":[3,4,1,1],"cell":[2,2],"planes":{"id":"test","width":2,"height":2,"below":png,"above":png}})
    }
    #[test]
    fn native_planes_have_distinct_z_order_are_cached_and_delete_only_owned_ids() {
        let mut presenter = Presenter::default();
        let value = command();
        let mut bytes = Vec::new();
        presenter.apply(&value, &mut bytes).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("z=-1,") && text.contains("z=1,"));
        assert!(text.contains("\x1b[5;4H"));
        assert!(text.contains("f=100"));
        let mut unchanged = Vec::new();
        presenter.apply(&value, &mut unchanged).unwrap();
        assert!(unchanged.is_empty());
        presenter
            .apply(
                &serde_json::json!({"op":"clear","view":"private-pane"}),
                &mut unchanged,
            )
            .unwrap();
        let text = String::from_utf8(unchanged).unwrap();
        assert_eq!(text.matches("a=d,d=I,i=").count(), 2);
        assert!(!text.contains("d=A"));
    }
    #[test]
    fn malformed_pixels_geometry_and_escape_payloads_cannot_emit_any_bytes() {
        for (field, value) in [
            ("rect", serde_json::json!([u32::MAX, 4, 1, 1])),
            ("cell", serde_json::json!([0, 2])),
            ("generation", serde_json::json!(0)),
        ] {
            let mut frame = command();
            frame[field] = value;
            let mut bytes = Vec::new();
            assert!(Presenter::default().apply(&frame, &mut bytes).is_err());
            assert!(bytes.is_empty());
        }
        let mut frame = command();
        frame["planes"]["above"] = serde_json::json!("\x1b_Ga=d,d=A");
        let mut bytes = Vec::new();
        assert!(Presenter::default().apply(&frame, &mut bytes).is_err());
        assert!(bytes.is_empty());
        let mut frame = command();
        frame["planes"]["width"] = serde_json::json!(3);
        assert!(Presenter::default().apply(&frame, &mut bytes).is_err());
        assert!(bytes.is_empty());
    }
}
