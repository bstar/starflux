//! Opt-in stage timestamps for diagnosing keystroke latency. Set
//! `STARFLUX_LATENCY_TRACE` to a file path; every STAR/FLUX process on the
//! host appends `monotonic_ns pid stage` lines to it. Unset, it costs one
//! cached check.
use std::{io::Write, sync::OnceLock};

static PATH: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();

pub fn mark(stage: &str) {
    let Some(path) = PATH
        .get_or_init(|| std::env::var_os("STARFLUX_LATENCY_TRACE").map(Into::into))
        .as_ref()
    else {
        return;
    };
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    let ns = now.tv_sec as u128 * 1_000_000_000 + now.tv_nsec as u128;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        let _ = writeln!(file, "{ns} {} {stage}", std::process::id());
    }
}
