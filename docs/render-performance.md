# Graphical render performance

Graphical and cells presentations both send native terminal cells to the parent terminal. Terminal output and input do not require rasterizing terminal glyphs in the graphical workspace.

Graphical chrome now uses separate surfaces for the sidebar, header and footer instead of allocating full-window surfaces underneath native terminals. Empty foreground overlays are omitted, and visible menus/dialogs use their visible bounds. Unchanged workspace controls reuse the previous scene during terminal traffic. The cache includes viewport, appearance, controls, activity, overlays and split geometry; changes rebuild the scene. Snapshot `graphical_render` reports scene builds, reuse and surface pixels.

Cropping preserves physical pixels and global hit targets. Fractional cell geometry, artwork over a terminal, or controls crossing a crop boundary retain the original surface path. Pixel comparisons cover small and large windows and a real context menu; the headless release gate also verifies scene reuse during input.

## CPU render measurement

On this workstation, at 1848×2080 pixels with 24 project rows and animated activity, the shipping CPU renderer measured:

| Chrome | Median render | p95 render | Surface pixels |
| --- | ---: | ---: | ---: |
| Previous full-window surfaces | 51.98 ms | 53.04 ms | 7,687,680 |
| Cropped surfaces | 13.41 ms | 13.96 ms | 857,024 |

This is about 3.9× faster chrome rendering and 89% fewer surface pixels. It measures CPU scene rendering, excluding terminal transport and hardware display latency. The benchmark keeps the old scene shape available only in its test fixture for reproducibility.

```sh
./scripts/dev test --release --locked graphical::tests::graphical_chrome_benchmark -- --ignored --nocapture
```

Results are written to `target/graphical-render-proof/chrome.json`.

## Local and SSH input measurement

A terminal fixture in disposable 1800×1800 Kitty windows produced these medians on 2026-10-28, with 15 measured inputs after warmup. Terminals then ran on the retired tmux backend; this benchmark has not been rerun on native terminals, because it needs an otherwise idle desktop:

| Presentation | Local input delivery | Local captured update | SSH input delivery | SSH captured update |
| --- | ---: | ---: | ---: | ---: |
| Cells | 0.44 ms | 37.96 ms | 0.55 ms | 38.28 ms |
| Graphical | 0.43 ms | 38.05 ms | 0.55 ms | 37.99 ms |

Input delivery runs from Kitty RPC injection to fixture receipt. Captured update includes the screenshot sampling delay (approximately 20 ms), so it is not a hardware input-to-photon measurement. Native terminal typing was already comparable before the chrome changes; the rendering improvement applies to graphical workspace controls. SSH uses an actual authenticated OpenSSH connection to a disposable loopback daemon, not a separate machine or a WAN. No existing SSH configuration is changed.

Run from the graphical desktop:

```sh
STARFLUX_LATENCY_PHASE=local STARFLUX_LATENCY_WIDTH=1800 STARFLUX_LATENCY_HEIGHT=1800 STARFLUX_LATENCY_SAMPLES=18 python3 scripts/bench-terminal-latency.py
STARFLUX_LATENCY_PHASE=ssh STARFLUX_LATENCY_KINDS=cells-ssh,graphical-ssh STARFLUX_LATENCY_WIDTH=1800 STARFLUX_LATENCY_HEIGHT=1800 STARFLUX_LATENCY_SAMPLES=18 python3 scripts/bench-terminal-latency.py
```

The default comparison uses the CPU renderer; set `STARFLUX_BENCH_RENDERER` to select another backend. JSON samples, screenshots and controller logs are saved under `target/terminal-latency-proof/`. The script uses private fake sessions, closes its test windows and restores the previously focused window.

## Input-to-frame latency

`scripts/test-session-manager.py --latency` measures, without a desktop, the
time from a typed line reaching the controller to the presenter receiving the
frame that shows its echo, with two panes and then eight. It runs in a private
environment with fake shells; add `--cells` or `--ssh`. It excludes the host
terminal's own drawing and display refresh. Measured on 2026-11-20 with native
terminals (40 samples each, after warmup):

| Mode | Two panes median / p95 | Eight panes median / p95 |
| --- | ---: | ---: |
| Graphical | 1.9 / 2.1 ms | 2.3 / 2.7 ms |
| Cells | 2.3 / 2.4 ms | 2.3 / 2.5 ms |
| SSH | 1.7 / 2.2 ms | 2.3 / 2.7 ms |

The retired tmux backend measured about 9 ms median and 10 ms p95 in every mode,
and native terminals measured 18 / 25 ms before these changes. Almost all of
that was waiting on timers, not work:

- The controller ticked every 8 ms and took native frames only on a tick, and
  the owner sends the next frame only after the previous one is acknowledged,
  so most keystrokes waited for two ticks. A finished frame now wakes the
  controller through STAR/CORE's `Controller::wake` (STAR/CORE 0.26).
- The connector between owner and view polled every 2 ms and handed frames to
  the converter only on its next pass. It now sleeps until the owner replies
  or a view has input, an acknowledgement or new settings.
- The owner started rendering output only on its next poll, which could wait
  its 16 ms idle timeout, and noticed finished renders only by polling every
  2 ms. It now starts rendering in the same turn and finished renders wake it.

Hardware input-to-photon latency needs the desktop benchmark below, which was
last run on 2026-10-28, before native terminals became the default.

To see where a keystroke's time goes, set `STARFLUX_LATENCY_TRACE` to a file
path before starting STAR/FLUX (or a test). Each process appends
`monotonic_ns pid stage` lines for input queued, request written, owner input,
PTY write and read, render start, frame queued, frame received, frame converted
and frame taken, plus controller ticks and scenes. The timestamps share the
system monotonic clock, so stages from the controller, connector and owner line
up. Unset, tracing costs one cached check per mark.

## Idle CPU

With one native terminal and a cells presenter attached, a private session
measured these idle totals over 30 seconds after settling, including CPU of
reaped helper processes:

| Build | Idle CPU (one core) |
| --- | ---: |
| Before | 5.6% |
| After | 1.8% |

Three changes account for it: native pane health checks are answered in the
controller instead of spawning a helper process every refresh; the git status
helper runs every 30 seconds instead of every 3 while no one is typing and no
terminal is producing output; and the controller polls every 100 ms while idle
(8 ms while active), which STAR/CORE 0.25.2 allows. Shared project sync also
slows down while idle; the frontend snapshot keeps its 500 ms cadence. Input still wakes the controller
immediately. This was measured on a loaded workstation without a desktop
window; idle CPU with the graphical presenter and the eight-pane latency target
still need desktop measurements.

## App Space

App Space now retains unchanged chrome and crops the background surfaces around
its live app canvas. It also omits empty terminal-grid spans behind that canvas.
At 2304×1444 with animated project activity, CPU chrome rendering measured
21.49 ms median / 22.18 ms p95 before, and 6.83 ms median / 7.11 ms p95 after.
The pixel comparison test verifies the visible chrome is unchanged.

```sh
./scripts/dev test --release --locked graphical::tests::app_chrome_benchmark -- --ignored --nocapture
```

The H.264 encoder now uses automatic threading, matching STAR/SCREEN in the
`starreel` repository, instead of limiting it to two threads. A separate,
unpaced GStreamer moving-ball workload at 2050×1262, BGRA→Y444, x264 ultrafast,
quantizer 18 measured 61.8 fps with two threads and 89.7 fps with automatic
threading on this workstation. These are encoder throughput measurements,
not VirtueMachine interaction latency or remote display measurements.

For a real GTK app, compositor, H.264 stream and authenticated loopback SSH:

```sh
python3 scripts/test-multiwindow.py --ssh --apps --video
```

This acceptance test creates a private animated app and verifies app input,
stream delivery and worker/session persistence. It writes delivery rate and
render/encoder metadata to `target/app-video-proof/ssh.json`. Its protocol
consumer returns presentation credits without a hardware display, so this
measures the server/SSH delivery path rather than hardware input-to-photon
latency. The existing full-HD video unit test separately decodes encoded pixels
and checks color edges. Snapshot `app_video_render` reports source dimensions,
quality, negotiated frame window, packets and returned presentation credits.

## Audio and periodic delivery stalls

Capable frontends now receive app audio through CORE's PCM-v2 player, matching
STAR/SCREEN's remote preview path. The older fixed-stereo protocol is retained
only when `audio_pcm_v2` is absent. The host's private 48 kHz stereo i16 capture
is converted exactly to normalized F32 samples for transport. The PCM-v2 player
has a half-second ring reserve; the legacy player uses a 100 ms ring. These are
buffer capacities, not a claim of a fixed added playback delay.

App video uses cyclic intra refresh and a 24 Mbit/s high-quality rate limit
(12 Mbit/s for the older lower-resolution profile), reducing refresh bursts on
the SSH stream shared with audio. The initial high-quality profile retains full
resolution and 4:4:4 color; the full-HD decoded color-edge test still passes.
Packet-size and delivery-gap statistics appear in `app_video_render`, while
`audio.playback_protocol` identifies the negotiated audio player.

```sh
python3 scripts/test-multiwindow.py --ssh --apps --video --audio
python3 scripts/test-multiwindow.py --ssh --apps --video --audio --legacy-audio
```

The private test sends a continuous tone and an animated desktop together. It
checks the audio sample format, non-silence, eight seconds of delivery, video
continuity, and a maximum steady audio arrival gap below 80 ms. No audio is
played to desktop speakers by this protocol consumer. Actual remote-device
underruns and WAN jitter require observing the user's presenting frontend.

While App Space is visible, project activity markers use their reduced-motion
appearance. Status and project changes still update, but decorative waves do not
keep submitting chrome over the live app stream. A read-only observation of the
running VirtueMachine desktop delivered about 60 fps on the host; after this
change its chrome produced three scenes over twelve seconds (four scene builds
and 111 retained-scene hits). This measures the actual app's host path, not Mac
presentation latency. Last-stream timing and dimensions now remain inspectable
after returning to terminal space; `app_video_render.active` identifies whether
the stream is currently open.

The graphical drafting grid and its outer workspace margins are restored.
Terminal panes still fill their own bounds, and cells mode continues to use its
full terminal area without that graphical background.

## Viewer-limited streams

App video now adapts to measured frontend backpressure. A reported decoder
capability alone does not guarantee native 4:4:4 presentation can sustain an
interactive frame rate. After at least 60 delivery intervals, a sustained rate
below 45 fps and at least six encoded-frame credit waits over 20 ms in the last
three seconds trigger a decoder session change:

1. Start with native-resolution 4:4:4 on capable frontends.
2. Switch to native-resolution 4:2:0 when the viewer is consistently behind.

The app desktop is bounded to 1920×1080 and resized to the available App Space
when smaller. Its image is centered at one display pixel per source pixel;
larger workspaces do not stretch it. A smaller simultaneous viewer crops the
shared desktop instead of resampling it. The encoded preview and input
coordinates use those same native pixels. Presentation pressure
does not reduce resolution. Format changes issue a new video session;
interdependent access units are never skipped. Fast frontends retain the original
chroma. The 4:2:0 fallback can soften fine colored edges, while preserving the
full-resolution brightness detail. The previous 1600×900 fallback made game
art and text visibly soft when enlarged and has been removed.

`app_video_render` exposes `responsive_stage` (0–1), `chroma`, `encoded_size`,
`delivery_fps` and `max_frontend_credit_wait_ms`. Delivery rate reflects the
credit-paced packet path; it is not a hardware display-rate measurement.

```sh
python3 scripts/test-multiwindow.py --ssh --apps --video --audio --slow-video
```

The protocol test simulates slow presentation only for 4:4:4 access units and
checks the real H.264 session switches to 4:2:0 while app input, continuous audio
and retained workers remain usable. The ordinary video test exercises a fast
frontend without that delay.

## Canvas-drawn custom cursors

VirtueMachine draws its cursor bitmap into the game canvas with
`drawCustomCursorOnContext` / `drawCustomCursorLayer` in
`modern/client-web/app.ts`, and sets the browser cursor to `none` when those
bitmaps are available. Its visible pointer therefore includes the entire
input, app redraw, capture, encode, network, decode and presentation path.
A steady 60 fps stream does not establish low pointer latency.

The cursor stays in the game canvas. STARFLUX now keeps raw frames replaceable
while the viewer has no presentation credit, rather than encoding an extra
frame that becomes stale while waiting. Encoding begins when there is room
in the negotiated presentation window. Every encoded access unit is still
delivered in order; only unencoded frames are coalesced. This removes an extra
frame queue without changing game input, rendering or cursor artwork.

The optional acceptance consumer actually decodes H.264 and locates a cursor
painted into a private GTK app's canvas. It measures pointer injection through
SSH, app receipt/redraw, compositor capture, encoding, SSH return and decoding.
It reports app input receipt separately and can add presentation backpressure:

```sh
python3 scripts/test-multiwindow.py --ssh --apps --video --pointer-latency
python3 scripts/test-multiwindow.py --ssh --apps --video --pointer-latency --presentation-delay-ms 40
```

The decoder uses GStreamer from `target/run-env.sh` via system Python's GI
bindings. These are loopback SSH measurements; they exclude physical display
latency and do not establish the user's Mac input-to-photon latency. No input
is sent to VirtueMachine or any existing app.

With a 40 ms simulated presentation delay, twelve measured cursor updates
after warmup improved from 124.33 ms median / 144.73 ms maximum to 63.87 ms
median / 124.36 ms maximum. Both runs used native 2018×1238 4:2:0 video.
Under that pressure delivery averaged 40.31 fps before and 35.67 fps after:
the change favors a fresh cursor image over an extra queued frame. The normal
consumer still sustained 59.97 fps. A separate slow-video/audio SSH test
delivered 400 continuous PCM blocks with a maximum arrival gap of 24.39 ms.
These results are from the private software consumer, not the user's device.

With the 1080p desktop cap, the same SSH decoder test sustained 59.99 fps at
1920×1080 in 4:4:4. It asserts matching source, encoded and displayed dimensions,
and checks a second smaller frontend crops the shared desktop without scaling
or replacing its worker. The twelve cursor samples measured 69.31 ms median /
70.25 ms maximum; continuous PCM delivery had a 25.28 ms maximum arrival gap.

STAR/SCREEN's compositor currently ignores cursor visibility changes and uses
its own hide-desktop-cursor setting instead. A visible local system pointer
can make that comparison feel more responsive; that alone does not establish
lower latency for VirtueMachine's canvas-drawn pointer.
