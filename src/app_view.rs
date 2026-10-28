//! App Space uses native pixels, bounded to a 1080p desktop.
use crate::compositor::Frame;
use starkit::{content_view::ContentView, native_surface::PixelRect};
use std::sync::Arc;

pub const MAX_WIDTH: u32 = 1920;
pub const MAX_HEIGHT: u32 = 1080;

pub fn canvas(bounds: PixelRect) -> PixelRect {
    let width = (u32::from(bounds.width).min(MAX_WIDTH) / 2 * 2).max(1) as u16;
    let height = (u32::from(bounds.height).min(MAX_HEIGHT) / 2 * 2).max(1) as u16;
    PixelRect::new(
        bounds.x + bounds.width.saturating_sub(width) / 2,
        bounds.y + bounds.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub fn view(source: [u32; 2], bounds: PixelRect) -> ContentView {
    // A shared desktop may be larger than this viewer, or still resizing.
    // Crop its upper-left pixels instead of fitting/stretching the image.
    let width = source[0].min(u32::from(bounds.width));
    let height = source[1].min(u32::from(bounds.height));
    ContentView::new(
        [width, height],
        PixelRect::new(
            bounds.x + (bounds.width - width as u16) / 2,
            bounds.y + (bounds.height - height as u16) / 2,
            width as u16,
            height as u16,
        ),
    )
    .expect("Nonempty app viewport")
}

pub fn crop(frame: Frame, bounds: PixelRect) -> Frame {
    let width = frame.width.min(u32::from(bounds.width));
    let height = frame.height.min(u32::from(bounds.height));
    if width == frame.width && height == frame.height {
        return frame;
    }
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for row in frame
        .pixels
        .chunks_exact(frame.width as usize * 4)
        .take(height as usize)
    {
        pixels.extend_from_slice(&row[..width as usize * 4]);
    }
    Frame {
        pixels: Arc::new(pixels),
        width,
        height,
        time: frame.time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_and_small_views_use_one_display_pixel_per_source_pixel() {
        for (bounds, expected) in [
            (PixelRect::new(300, 100, 3000, 1800), [1920, 1080]),
            (PixelRect::new(300, 100, 999, 701), [998, 700]),
        ] {
            let canvas = canvas(bounds);
            assert_eq!(
                [u32::from(canvas.width), u32::from(canvas.height)],
                expected
            );
            for source in [[1920, 1080], [1280, 720], [2214, 1432]] {
                let view = view(source, canvas);
                let [x, y, width, height] = view.fitted();
                assert_eq!([width, height], view.source.map(f64::from));
                assert_eq!(view.to_content([x + 20., y + 30.], false), Some([20., 30.]));
                assert!(x >= f64::from(bounds.x) && y >= f64::from(bounds.y));
            }
        }
    }
    #[test]
    fn a_smaller_view_crops_shared_pixels_without_resampling() {
        let pixels = (0..24).collect::<Vec<u8>>();
        let frame = Frame {
            pixels: Arc::new(pixels.clone()),
            width: 3,
            height: 2,
            time: std::time::Instant::now(),
        };
        let full = crop(frame.clone(), PixelRect::new(0, 0, 3, 2));
        assert!(Arc::ptr_eq(&full.pixels, &frame.pixels));
        let cropped = crop(frame, PixelRect::new(0, 0, 2, 2));
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(*cropped.pixels, [&pixels[..8], &pixels[12..20]].concat());
    }
}
