// Adapted from STAR/SCREEN, MIT, Copyright (c) 2026 bstar.
mod common;
pub use common::*;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::run;
/// A main application and a right-hand stack, in recording-source pixels.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn tile_rects(count: usize, settings: &crate::project::Settings) -> Vec<[i32; 4]> {
    if count == 0 {
        return vec![];
    }
    let width = settings.width as i32;
    let height = settings.height as i32;
    if count == 1 {
        return vec![[0, 0, width, height]];
    }
    let main = width * 2 / 3;
    let mut result = vec![[0, 0, main - 4, height]];
    let stack = (count - 1).min(usize::from(u16::MAX));
    for index in 0..stack {
        let y = height * index as i32 / stack as i32;
        let end = height * (index + 1) as i32 / stack as i32;
        result.push([main + 4, y, width - main - 4, (end - y - 4).max(1)]);
    }
    result
}
