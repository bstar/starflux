#!/usr/bin/env python3
"""Compare the native parser with the original vt100 archive in a private build.

Requires vt100-0.16.2.crate in Cargo's download cache (from the previous registry
build). No live terminal, tmux server, application or user data is accessed.
Results are a throughput microbenchmark, not end-to-end UI latency measurements.
"""
import argparse
import hashlib
import shutil
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--baseline-path',type=Path)
args=parser.parse_args()
root = Path(__file__).resolve().parents[1]
cargo_home = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')))
expected = '054ff75fb8fa83e609e685106df4faeffdf3a735d3c74ebce97ec557d5d36fd9'
archives = list((cargo_home / 'registry/cache').glob('*/vt100-0.16.2.crate'))
archive = next((p for p in archives if hashlib.sha256(p.read_bytes()).hexdigest() == expected), None)
if archive is None and args.baseline_path is None:
    raise SystemExit('A verified cached vt100-0.16.2.crate is required; run cargo info vt100@0.16.2 first.')
with tempfile.TemporaryDirectory(prefix='starflux-parser-bench-') as directory:
    temporary = Path(directory)
    baseline = temporary / 'vt100-0.16.2'
    if args.baseline_path is None:
        with tarfile.open(archive) as package:package.extractall(temporary, filter='data')
    else:
        shutil.copytree(args.baseline_path,baseline)
        digest=hashlib.sha256()
        for source in sorted((baseline/'src').glob('*.rs')):digest.update(source.name.encode()+source.read_bytes())
        print(json.dumps(dict(baseline_source=str(args.baseline_path),source_digest=digest.hexdigest())),flush=True)
    # Match shipping dependencies and profile; omit upstream development-only
    # dependencies so the benchmark introduces no extra registry requirements.
    (baseline / 'Cargo.toml').write_text('''[package]
name = "vt100-baseline"
version = "0.16.2"
edition = "2021"
[dependencies]
itoa = "1.0.15"
unicode-width = "0.2.1"
vte = "0.15.0"
memchr = "2.7.4"
''')
    (temporary / 'Cargo.toml').write_text('''[package]
name = "starflux-parser-bench"
version = "0.0.0"
edition = "2021"
[dependencies]
base = { package = "vt100-baseline", path = "vt100-0.16.2" }
current = { package = "vt100", path = ''' + json.dumps(str(root / 'vendor/vt100')) + ''' }
[profile.release]
opt-level = 2
overflow-checks = true
''')
    (temporary / 'src').mkdir()
    (temporary / 'src/main.rs').write_text(r'''
use std::{time::Instant, hint::black_box};
fn main() {
    for (name, line) in [
        ("ascii", "normal log output and a progress update 0123456789\r\n"),
        ("styled", "\x1b[31merror\x1b[0m \x1b[32mgreen\x1b[0m unicode 界é\r\n"),
    ] {
        let data = line.repeat(12000);
        let mut baseline = Vec::new();
        let mut updated = Vec::new();
        for _ in 0..7 {
            let time = Instant::now();
            let mut parser = base::Parser::new(40, 120, 1000);
            for _ in 0..4 {
                for bytes in data.as_bytes().chunks(16384) { parser.process(bytes); }
            }
            black_box(parser.screen().cursor_position());
            baseline.push(time.elapsed().as_secs_f64());
            let time = Instant::now();
            let mut parser = current::Parser::new(40, 120, 1000);
            for _ in 0..4 {
                for bytes in data.as_bytes().chunks(16384) { parser.process(bytes); }
            }
            black_box(parser.screen().cursor_position());
            updated.push(time.elapsed().as_secs_f64());
        }
        baseline.sort_by(f64::total_cmp);
        updated.sort_by(f64::total_cmp);
        println!("{{\"workload\":\"{}\",\"baseline_ms\":{:.3},\"current_ms\":{:.3},\"ratio\":{:.3}}}",
            name, baseline[3] * 1000., updated[3] * 1000., updated[3] / baseline[3]);
    }
}
''')
    env = dict(os.environ, CARGO_BUILD_JOBS='2')
    subprocess.run([str(root / 'scripts/dev'), 'run', '--release', '--offline',
                    '--manifest-path', str(temporary / 'Cargo.toml')], env=env, check=True)
