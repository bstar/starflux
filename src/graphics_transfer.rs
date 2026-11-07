//! Kitty direct-transfer assembly and decoding, independent of the presenter.
//!
//! No source replies, filesystem reads, placement mutations or capability
//! advertisement happen here. A backend must own the assembler for one exact
//! terminal generation. Decode completed transmissions on a bounded worker,
//! then commit only if that generation is still current.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;

const MAX_HEADER: usize = 1024;
const MAX_CHUNK: usize = 4096;
const MAX_TRANSMISSION: usize = 16 * 1024 * 1024;
const MAX_RGBA: usize = 32 * 1024 * 1024;
const MAX_DIMENSION: u32 = 8192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    TooLarge,
    Unsupported,
    Busy,
    Timeout,
}
impl Error {
    pub fn message(self) -> &'static str {
        match self {
            Self::Invalid => "EINVAL:Invalid graphics data",
            Self::TooLarge => "E2BIG:Graphics resource limit",
            Self::Unsupported => "ENOTSUP:Unsupported graphics operation",
            Self::Busy => "EBUSY:Graphics decoder busy",
            Self::Timeout => "ETIMEDOUT:Graphics upload timed out",
        }
    }
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Default)]
pub struct Controls(BTreeMap<u8, String>);
impl Controls {
    pub fn from_apc(apc: &[u8]) -> Result<Self> {
        let body = apc.strip_prefix(b"G").ok_or(Error::Invalid)?;
        Self::parse(body.split(|byte| *byte == b';').next().unwrap_or_default())
    }
    pub fn is_continuation(&self) -> bool {
        self.0.keys().all(|key| matches!(key, b'm' | b'q'))
    }
    pub fn inherit_quiet(&mut self, continuation: &Self) -> Result<()> {
        let quiet = continuation.number(b'q', 0)?;
        if quiet > 2 {
            return Err(Error::Invalid);
        }
        if let Some(value) = continuation.0.get(&b'q') {
            self.0.insert(b'q', value.clone());
        }
        Ok(())
    }
    fn parse(header: &[u8]) -> Result<Self> {
        if header.len() > MAX_HEADER {
            return Err(Error::TooLarge);
        }
        let mut keys = BTreeMap::new();
        if !header.is_empty() {
            for pair in header.split(|b| *b == b',') {
                if pair.len() < 3 || !pair[0].is_ascii_alphabetic() || pair[1] != b'=' {
                    return Err(Error::Invalid);
                }
                let value = &pair[2..];
                if value.len() > 11
                    || !value
                        .iter()
                        .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
                {
                    return Err(Error::Invalid);
                }
                if keys
                    .insert(
                        pair[0],
                        String::from_utf8(value.to_vec()).map_err(|_| Error::Invalid)?,
                    )
                    .is_some()
                {
                    return Err(Error::Invalid);
                }
            }
        }
        Ok(Self(keys))
    }
    pub fn has(&self, key: u8) -> bool {
        self.0.contains_key(&key)
    }
    pub fn signed_number(&self, key: u8, default: i32) -> Result<i32> {
        self.0
            .get(&key)
            .map_or(Ok(default), |s| s.parse().map_err(|_| Error::Invalid))
    }
    pub fn character(&self, key: u8, default: u8) -> Result<u8> {
        match self.0.get(&key) {
            None => Ok(default),
            Some(s) if s.len() == 1 => Ok(s.as_bytes()[0]),
            _ => Err(Error::Invalid),
        }
    }
    pub fn number(&self, key: u8, default: u32) -> Result<u32> {
        self.0.get(&key).map_or(Ok(default), |s| {
            if !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::Invalid);
            }
            s.parse().map_err(|_| Error::Invalid)
        })
    }
}

#[derive(Debug)]
pub struct Transmission {
    pub controls: Controls,
    pub cursor: (u16, u16),
    bytes: Vec<u8>,
}
#[derive(Default)]
pub struct Assembler {
    pending: Option<Transmission>,
}
impl Assembler {
    /// Called for CAN/SUB, malformed APC, terminal reset, output gap or a source
    /// generation change. Incomplete pixels must never become a retained image.
    pub fn cancel(&mut self) {
        self.pending = None;
    }
    pub fn push(&mut self, apc: &[u8], cursor: (u16, u16)) -> Result<Option<Transmission>> {
        if apc.first() != Some(&b'G') {
            return Ok(None);
        }
        let result = self.append(&apc[1..], cursor);
        if result.is_err() {
            self.cancel();
        }
        result
    }
    fn append(&mut self, data: &[u8], cursor: (u16, u16)) -> Result<Option<Transmission>> {
        let boundary = data.iter().position(|b| *b == b';').unwrap_or(data.len());
        let mut controls = Controls::parse(&data[..boundary])?;
        let encoded = data.get(boundary + 1..).unwrap_or_default();
        let more = controls.number(b'm', 0)?;
        if more > 1 || controls.number(b'q', 0)? > 2 {
            return Err(Error::Invalid);
        }
        // Deletion explicitly cancels a pending upload. It is a complete new
        // command, not a malformed continuation or a reason to retain pixels.
        if matches!(controls.character(b'a', b't')?, b'd' | b'p') {
            if controls.character(b'a', b't')? == b'd' {
                self.cancel();
            }
            if more != 0 || !encoded.is_empty() {
                return Err(Error::Invalid);
            }
        }
        if encoded.len() > MAX_CHUNK {
            return Err(Error::TooLarge);
        }
        if more == 1 && !encoded.len().is_multiple_of(4) {
            return Err(Error::Invalid);
        }
        if let Some(pending) = &mut self.pending {
            if controls.0.keys().any(|k| !matches!(k, b'm' | b'q')) {
                return Err(Error::Invalid);
            }
            if let Some(quiet) = controls.0.remove(&b'q') {
                pending.controls.0.insert(b'q', quiet);
            }
        } else {
            self.pending = Some(Transmission {
                controls,
                cursor,
                bytes: Vec::new(),
            });
        }
        let pending = self.pending.as_mut().ok_or(Error::Invalid)?;
        let decoded = STANDARD.decode(encoded).map_err(|_| Error::Invalid)?;
        if pending.bytes.len().saturating_add(decoded.len()) > MAX_TRANSMISSION {
            return Err(Error::TooLarge);
        }
        pending.bytes.extend_from_slice(&decoded);
        // The placement origin belongs to the final chunk, not the first one.
        pending.cursor = cursor;
        Ok(if more == 1 { None } else { self.pending.take() })
    }
}

#[derive(Debug, PartialEq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
fn rgba_size(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        return Err(Error::Invalid);
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::TooLarge);
    }
    let size = u64::from(width) * u64::from(height) * 4;
    if size > MAX_RGBA as u64 {
        return Err(Error::TooLarge);
    }
    Ok(size as usize)
}
impl Transmission {
    pub fn allocated_bytes(&self) -> usize {
        self.bytes.capacity()
    }
    pub fn decode_image(&self) -> Result<Pixels> {
        if !matches!(self.controls.character(b'a', b't')?, b't' | b'T' | b'q')
            || self.controls.character(b't', b'd')? != b'd'
        {
            return Err(Error::Unsupported);
        }
        if self.controls.has(b'o') {
            if self.controls.character(b'o', b'z')? != b'z' {
                return Err(Error::Unsupported);
            }
            return self.decompress()?.decode_image();
        }
        let format = self.controls.number(b'f', 32)?;
        if format == 100 {
            return self.decode_png();
        }
        let channels = match format {
            24 => 3,
            32 => 4,
            _ => return Err(Error::Unsupported),
        };
        let width = self.controls.number(b's', 0)?;
        let height = self.controls.number(b'v', 0)?;
        let size = rgba_size(width, height)?;
        if self.bytes.len() != size / 4 * channels {
            return Err(Error::Invalid);
        }
        let rgba = if channels == 4 {
            self.bytes.clone()
        } else {
            let mut rgba = Vec::with_capacity(size);
            for rgb in self.bytes.as_chunks::<3>().0 {
                rgba.extend_from_slice(rgb);
                rgba.push(255);
            }
            rgba
        };
        Ok(Pixels {
            width,
            height,
            rgba,
        })
    }
    fn decode_png(&self) -> Result<Pixels> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(&self.bytes));
        decoder.set_limits(png::Limits { bytes: MAX_RGBA });
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|_| Error::Invalid)?;
        if reader.info().animation_control.is_some() {
            return Err(Error::Unsupported);
        }
        let width = reader.info().width;
        let height = reader.info().height;
        let size = rgba_size(width, height)?;
        if reader.output_buffer_size() > size {
            return Err(Error::TooLarge);
        }
        let mut bytes = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut bytes).map_err(|_| Error::Invalid)?;
        let mut rgba = Vec::with_capacity(size);
        match info.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(&bytes[..info.buffer_size()]),
            png::ColorType::Rgb => {
                for rgb in bytes[..info.buffer_size()].as_chunks::<3>().0 {
                    rgba.extend_from_slice(rgb);
                    rgba.push(255);
                }
            }
            png::ColorType::Grayscale => {
                for gray in &bytes[..info.buffer_size()] {
                    rgba.extend_from_slice(&[*gray, *gray, *gray, 255]);
                }
            }
            png::ColorType::GrayscaleAlpha => {
                for pair in bytes[..info.buffer_size()].as_chunks::<2>().0 {
                    rgba.extend_from_slice(&[pair[0], pair[0], pair[0], pair[1]]);
                }
            }
            _ => return Err(Error::Invalid),
        }
        if rgba.len() != size {
            return Err(Error::Invalid);
        }
        Ok(Pixels {
            width,
            height,
            rgba,
        })
    }
    fn decompress(&self) -> Result<Self> {
        use std::io::Read;
        let format = self.controls.number(b'f', 32)?;
        let limit = match format {
            24 | 32 => {
                rgba_size(
                    self.controls.number(b's', 0)?,
                    self.controls.number(b'v', 0)?,
                )? / 4
                    * if format == 24 { 3 } else { 4 }
            }
            100 => {
                let size = self.controls.number(b'S', 0)? as usize;
                if size == 0 {
                    return Err(Error::Invalid);
                }
                if size > MAX_TRANSMISSION {
                    return Err(Error::TooLarge);
                }
                size
            }
            _ => return Err(Error::Unsupported),
        };
        let mut decoder = flate2::bufread::ZlibDecoder::new(self.bytes.as_slice());
        let mut bytes = Vec::new();
        decoder
            .by_ref()
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Invalid)?;
        if bytes.len() > limit {
            return Err(Error::TooLarge);
        }
        if bytes.len() != limit || decoder.total_in() != self.bytes.len() as u64 {
            return Err(Error::Invalid);
        }
        let mut controls = self.controls.clone();
        controls.0.remove(&b'o');
        Ok(Self {
            controls,
            cursor: self.cursor,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transfer(header: &str, data: &[u8]) -> Transmission {
        Assembler::default()
            .push(
                format!("G{header};{}", STANDARD.encode(data)).as_bytes(),
                (1, 2),
            )
            .unwrap()
            .unwrap()
    }
    #[test]
    fn framed_graphics_never_publish_canceled_chunks_or_pollute_terminal_text() {
        #[derive(Default)]
        struct Receiver {
            transfer: Assembler,
            images: Vec<(Pixels, (u16, u16))>,
        }
        impl vt100::Callbacks for Receiver {
            fn unhandled_apc(&mut self, screen: &mut vt100::Screen, data: &[u8]) {
                if let Ok(Some(done)) = self.transfer.push(data, screen.cursor_position()) {
                    self.images
                        .push((done.decode_image().unwrap(), done.cursor));
                }
            }
            fn cancelled_apc(&mut self, _: &mut vt100::Screen) {
                self.transfer.cancel();
            }
        }
        let stream = b"A\x1b_Gf=24,s=2,v=1,m=1;AQID\x1b\\B\x1b_Gm=0;BAUG\x1b\\C\x1b_Gm=1;AAAA\x1b\\\x1b_Gdiscard\x18D\x1b_Gf=32,s=1,v=1;BwgJCg==\x1b\\E";
        for chunk in 1..=stream.len() {
            let mut p = vt100::Parser::new_with_callbacks(8, 40, 0, Receiver::default());
            for bytes in stream.chunks(chunk) {
                p.process(bytes);
            }
            assert_eq!(p.screen().contents(), "ABCDE");
            let images = &p.callbacks().images;
            assert_eq!(images.len(), 2);
            assert_eq!(images[0].0.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
            assert_eq!(images[0].1, (0, 2));
            assert_eq!(images[1].0.rgba, [7, 8, 9, 10]);
            assert_eq!(images[1].1, (0, 4));
        }
    }
    #[test]
    fn chunked_transfer_publishes_once_at_the_final_cursor() {
        let mut a = Assembler::default();
        assert!(
            a.push(b"Gi=4294967295,f=24,s=2,v=1,m=1;AQID", (0, 1))
                .unwrap()
                .is_none()
        );
        assert!(a.push(b"Gm=1;", (1, 1)).unwrap().is_none());
        let t = a.push(b"Gm=0,q=2;BAUG", (2, 3)).unwrap().unwrap();
        assert_eq!(t.cursor, (2, 3));
        assert_eq!(t.controls.number(b'i', 0).unwrap(), u32::MAX);
        assert_eq!(t.controls.number(b'q', 0).unwrap(), 2);
        assert_eq!(t.decode_image().unwrap().rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
        assert!(a.pending.is_none());
    }
    #[test]
    fn cancellation_and_errors_discard_all_partial_data() {
        for bad in [
            &b"Gm=3;AAAA"[..],
            b"Gm=0;====",
            b"Gm=0,i=1;AAAA",
            b"Gq=2,q=1;AAAA",
            b"Gm=1;A",
        ] {
            let mut a = Assembler::default();
            a.push(b"Gf=24,s=2,v=1,m=1;AQID", (0, 0)).unwrap();
            assert!(a.push(bad, (0, 0)).is_err());
            assert!(a.pending.is_none());
            assert_eq!(
                a.push(b"Gf=24,s=1,v=1;BAUG", (0, 0))
                    .unwrap()
                    .unwrap()
                    .decode_image()
                    .unwrap()
                    .rgba,
                [4, 5, 6, 255]
            );
        }
        let mut a = Assembler::default();
        a.push(b"Gm=1;AAAA", (0, 0)).unwrap();
        a.cancel();
        assert!(a.pending.is_none());
    }
    #[test]
    fn deletion_cancels_a_partial_upload_and_remains_a_complete_command() {
        let mut assembler = Assembler::default();
        assert!(
            assembler
                .push(b"Gi=1,f=24,s=2,v=1,m=1;AQID", (0, 0))
                .unwrap()
                .is_none()
        );
        let deleted = assembler.push(b"Ga=d,d=I,i=2", (1, 2)).unwrap().unwrap();
        assert_eq!(deleted.controls.character(b'a', b't').unwrap(), b'd');
        assert_eq!(deleted.controls.number(b'i', 0).unwrap(), 2);
        assert!(assembler.pending.is_none());
        let fresh = assembler
            .push(b"Gf=24,s=1,v=1;BAUG", (0, 0))
            .unwrap()
            .unwrap();
        assert_eq!(fresh.decode_image().unwrap().rgba, [4, 5, 6, 255]);
    }
    #[test]
    fn malformed_dimensions_payloads_and_large_transfers_are_bounded() {
        for header in [
            "f=24,s=0,v=1",
            "f=24,s=1,v=1",
            "f=32,s=-1,v=1",
            "f=32,s=4294967296,v=1",
            "f=32,s=4294967295,v=4294967295",
            "f=32,s=8192,v=8192",
        ] {
            assert!(transfer(header, &[1, 2, 3, 4]).decode_image().is_err());
        }
        let mut a = Assembler::default();
        assert_eq!(
            a.push(
                format!("Gm=1;{}", "A".repeat(MAX_CHUNK + 4)).as_bytes(),
                (0, 0)
            )
            .unwrap_err(),
            Error::TooLarge
        );
        a.pending = Some(Transmission {
            controls: Controls::default(),
            cursor: (0, 0),
            bytes: vec![0; MAX_TRANSMISSION],
        });
        assert_eq!(a.push(b"Gm=0;AAAA", (0, 0)).unwrap_err(), Error::TooLarge);
        assert!(a.pending.is_none());
    }
    #[test]
    fn files_shared_memory_and_unknown_formats_are_never_opened_or_decoded() {
        for header in ["t=f", "t=t", "t=s", "o=x", "a=f", "f=17"] {
            assert_eq!(
                transfer(header, b"/proc/self/environ").decode_image(),
                Err(Error::Unsupported)
            );
        }
    }
    fn compressed(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut writer = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        writer.write_all(bytes).unwrap();
        writer.finish().unwrap()
    }
    #[test]
    fn compressed_rgb_rgba_and_png_have_exact_bounded_output() {
        assert_eq!(
            transfer("f=32,s=1,v=1,o=z", &compressed(&[1, 2, 3, 4]))
                .decode_image()
                .unwrap()
                .rgba,
            [1, 2, 3, 4]
        );
        assert_eq!(
            transfer("f=24,s=1,v=1,o=z", &compressed(&[1, 2, 3]))
                .decode_image()
                .unwrap()
                .rgba,
            [1, 2, 3, 255]
        );
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[1, 2, 3, 4])
                .unwrap();
        }
        assert_eq!(
            transfer(&format!("f=100,o=z,S={}", bytes.len()), &compressed(&bytes))
                .decode_image()
                .unwrap()
                .rgba,
            [1, 2, 3, 4]
        );
        assert_eq!(
            transfer("f=100,o=z", &compressed(&bytes)).decode_image(),
            Err(Error::Invalid)
        );
        assert_eq!(
            transfer("f=100,o=z,S=4294967295", &compressed(&bytes)).decode_image(),
            Err(Error::TooLarge)
        );
        assert_eq!(
            transfer("f=32,s=1,v=1,o=z", &compressed(&[0; 16384])).decode_image(),
            Err(Error::TooLarge)
        );
        assert_eq!(
            transfer("f=32,s=1,v=1,o=z", &compressed(&[1, 2, 3])).decode_image(),
            Err(Error::Invalid)
        );
        let valid = compressed(&[1, 2, 3, 4]);
        let mut extra = valid.clone();
        extra.push(0);
        assert_eq!(
            transfer("f=32,s=1,v=1,o=z", &extra).decode_image(),
            Err(Error::Invalid)
        );
        for end in [0, valid.len() - 1] {
            assert_eq!(
                transfer("f=32,s=1,v=1,o=z", &valid[..end]).decode_image(),
                Err(Error::Invalid)
            );
        }
    }
    #[test]
    fn png_palette_transparency_gray_and_rgb_become_exact_rgba() {
        for (kind, data, expected) in [
            (png::ColorType::Rgba, vec![1, 2, 3, 4], vec![1, 2, 3, 4]),
            (png::ColorType::Rgb, vec![1, 2, 3], vec![1, 2, 3, 255]),
            (png::ColorType::Grayscale, vec![7], vec![7, 7, 7, 255]),
            (png::ColorType::GrayscaleAlpha, vec![7, 2], vec![7, 7, 7, 2]),
            (png::ColorType::Indexed, vec![0], vec![1, 2, 3, 4]),
        ] {
            let mut bytes = Vec::new();
            {
                let mut e = png::Encoder::new(&mut bytes, 1, 1);
                e.set_color(kind);
                e.set_depth(png::BitDepth::Eight);
                if kind == png::ColorType::Indexed {
                    e.set_palette(vec![1, 2, 3]);
                    e.set_trns(vec![4]);
                }
                e.write_header().unwrap().write_image_data(&data).unwrap();
            }
            assert_eq!(
                transfer("f=100", &bytes).decode_image().unwrap(),
                Pixels {
                    width: 1,
                    height: 1,
                    rgba: expected
                }
            );
            bytes.truncate(bytes.len() / 2);
            assert_eq!(
                transfer("f=100", &bytes).decode_image(),
                Err(Error::Invalid)
            );
        }
    }
}
