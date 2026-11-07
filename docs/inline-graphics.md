# Inline terminal graphics implementation

This work is incomplete. Native terminals do not yet advertise Kitty graphics
support or show retained images. Existing App Space streaming is separate.

The native parser now frames APC strings in output order, delivering complete
APC bodies through callbacks. It bounds them to 8 KiB, distinguishes BEL from
the string terminator, and cancels malformed, overflowing or interrupted strings.
Cancellation preserves ordinary text and active OSC 8 links. DCS, SOS and PM
remain ignored terminal strings. No graphics payload becomes terminal text.

`src/graphics_transfer.rs` assembles Kitty direct transfers and decodes RGB,
RGBA and static PNG into RGBA. Intermediate chunks publish nothing; a completed
transfer carries the final chunk's cursor. Duplicate control keys, malformed
base64, invalid continuations and excessive dimensions/payloads fail and clear
partial transfers. Cancellation also clears partial data. PNG decoding expands
palette/transparency and grayscale, strips 16-bit samples, and rejects animation.
No filesystem or shared-memory image paths are opened. Zlib-compressed direct
RGB/RGBA/PNG transfers have bounded, exact-size decompression; compressed PNG
requires its declared byte size. Truncated streams, excess output and trailing
compressed data fail. Other transmission media remain unsupported.

Current per-transfer limits are a 1 KiB header, 4096 base64 bytes per chunk,
16 MiB assembled bytes, 8192 pixels per dimension and 32 MiB decoded RGBA.
`src/graphics_store.rs` retains images and placements across primary/alternate
buffers, with a shared per-terminal ceiling of 64 MiB, 128 images and 1024
placements. Replacement and deletion remove dangling placements. Failed
replacement preserves valid earlier data. Queries leave existing images intact.
Named placements replace by image/placement pair; anonymous placements remain
distinct. Cropping, aspect-preserving fit, letterboxing, pixel offsets, signed
z-order and requested cursor deltas are represented without a renderer.

The store checks host, boot, PID/start and terminal epoch. Decoded commands must
commit in source order; later commands wait rather than overwriting earlier
state. Resetting a buffer invalidates old decoder work. Invalid and failed
commands consume their ordered position without replacing valid images.

Retained stores now share a 512 MiB pixel-capacity budget per backend process.
Admission precedes replacement/eviction; denied growth preserves existing data,
and equal-capacity replacement works at the limit. Delete, eviction, reset and
drop release charges. This is not a quota across independent backend processes
or an accounting of total RSS. Neither module
sends source input, replies or capability advertisements. The decoder/store are
not wired into live terminal callbacks yet, and cursor deltas are not applied
to a terminal by the store.

Protocol reference: [Kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/).

`scripts/test-graphics-observer.py` and `--ssh` use private tmux servers and a
fake terminal process. A control client receives the original tmux-wrapped
graphics bytes, preserves geometry, and detaches without replacing the terminal
or breaking its input. SSH uses an authenticated private loopback daemon. This
proves byte transport, not image display or remote internet performance. The
observer uses `ignore-size`; tmux's `read-only` client flag was unsuitable because
it also rejected separate `send-keys` requests when this was the only client.
The observer itself sends no input. Production observer isolation/backpressure
and safe response ownership still need implementation.

`src/graphics_stream.rs` bounds control lines to 128 KiB, decodes octal output
for one typed pane ID, handles extended output and detects pause/continue,
malformed input, oversized records and disconnection. Command response blocks
are excluded from source output. Nested tmux passthrough is supported up to four
levels, with a 64 KiB wrapper bound. A framing fault stops publication until an
owner supplies a new verified baseline. No transport process is started by the
module itself.

The disposable `examples/graphics-stream-proof.rs` connects control
decoding, passthrough unwrapping, the native terminal parser, image decoding and
the retained store. Its private local/SSH transport acceptance verifies an exact
RGBA digest and retained placement from a real tmux control stream. It also
checks explicit observer pause/continue without blocking the fake source or
resizing/replacing it. This pixel-only proof decodes synchronously in its own
test process; it is not an installed application. Production
aggregate budgets, startup coverage and process ownership remain required.

Response ordering needs source ownership, too. tmux handles ordinary primary
device-attribute queries in its own parser; a separate control observer does not
automatically take ownership of those replies. Before advertising graphics,
verify query/attribute ordering at the source, including wrapped and unwrapped
queries and multiple frontends. A source-owned PTY adapter may be necessary to
intercept replies before tmux. This is an architectural inference from
[tmux's input parser](https://github.com/tmux/tmux/blob/master/input.c), not a
completed response adapter or a reason to restart existing terminals.

`src/graphics_reply.rs` provides a bounded
source response queue: 64 outstanding slots, at most 256 bytes per reply,
source-generation tickets, ordered draining and duplicate/stale completion
guards. A completed DA reply cannot pass an unresolved preceding graphics
query. Reply construction handles image/placement IDs, quiet mode, success and
supported errors. Image-number assignment remains unfinished.

`src/graphics_requests.rs` connects assembly to that queue.
It reserves order at the first chunk, keeps the initial request identity for
malformed continuations, honors subsequent quiet overrides, cancels partial
uploads, and gives deletion a new response identity. Source reset invalidates
old jobs. Unsupported reply identities complete an empty slot rather than
wedging all later queries. A full response queue refuses new uploads before
buffering them.

`src/graphics_resources.rs` accounts for in-flight uploads and decoder output.
Default shared limits per backend process are 128 MiB for uploads and 512 MiB
for decoder work/results. An upload reserves 32 MiB before allocation (covering
assembly growth), then shrinks its charge to the completed vector capacity.
Reservations are accounting entries; they do not preallocate those byte buffers.
Cancellation, reset, errors and job drop release that reservation. Placement
and deletion commands require no upload reservation. These are conservative
payload reservations, not a bound on the complete process's RSS.

`src/graphics_decode.rs` runs two workers, with bounded eight-entry input and
completion queues. Shared resources enforce two active decoders even across
multiple pools. Each decoder reserves 160 MiB of working capacity before
decoding and reduces the result's charge to its RGBA capacity after temporary
buffers have been freed. Saturation returns `EBUSY`; the request owner can
complete the error in order. Dropping a pool closes its queues and releases
queued/running/completed reservations without joining a worker on the caller.
Results retain their charge until consumed. Retained store data still has only
per-terminal limits; application-wide retained-image eviction/accounting
and source fairness remain unfinished.

The request owner expires stalled uploads after 30 seconds
without payload progress or five minutes total. Empty continuation chunks cannot
renew the idle deadline, while valid trickling data still reaches the total
deadline. Expiry releases the upload reservation and completes an ordered
`ETIMEDOUT` response, respecting quiet mode. Orphan continuation chunks cannot
recreate the expired upload. The owner must tick expiry while no bytes arrive;
the private PTY proof now does so. This has been promoted to canonical source.

`examples/graphics-owner-proof.rs` creates a private PTY and synthetic child,
then delays each query result until the following DA has completed internally.
The child verifies exact reply order for valid, invalid, quiet, unsupported-media
and chunked queries, including a DA between chunks, malformed base64 and canceled
APC. These sixteen direct/wrapped cases pass locally and when the private owner
executable is invoked through authenticated loopback SSH. Eighteen DA replies
are checked because the interleaved case contains two queries.
This owner proof now uses the asynchronous decoder pool and asserts that all
in-flight reservations are released after the replies are delivered.
The optional `--stalled-upload` adds an actual idle wait for both direct and
wrapped queries. Local and SSH checks verify expiry at the real idle deadline,
ordered continuation of DA responses, and reservation release. Deterministic
unit tests additionally cover the five-minute total deadline without sleeping.
This tests a source owner directly; it does not make tmux an ordered response
authority. Existing tmux sources and transferred brokers are not replaced by
the proof. Production integration still needs a verified source owner before
tmux can independently answer an unwrapped query.

`graphics_paint` composes retained pixels into three separate alpha planes:
below default backgrounds, below text, and above text. It clips crops and pane
viewports, orders images by z and protocol ID, masks popup regions and keeps
primary/alternate buffers separate. Natural-size images remain pixel aligned.
Only visible placements allocate planes. Shared working-memory reservations
cover plane/mask payloads before allocation and are released with the raster;
composition rejects more than 16 million viewport pixels, 64 million placement/
mask pixel operations or 128 MiB of payload. These are bounded payload budgets,
not an accounting of all process memory.

The cells fallback emits validated KIT grids with colored half blocks. Negative
images preserve source glyphs and respect nondefault terminal backgrounds;
positive images can replace covered glyphs. Wide glyphs are replaced as whole
clusters so no continuation cell is left dangling. Combining text and attributes
survive transparent images, and the source screen/history is immutable. This
fallback approximates pixels and does not preserve text under positive images.
The grid bounds are checked before visiting cells or allocating output runs.

`graphics-stream-proof` now sends private observed pixels through composition
and validates its cells frame, source-screen immutability and reservation release.
The observer test's `--composition` option asserts those results; with `--ssh`,
the composition executable also runs over SSH. An optional second executable
argument names an output directory for PNG planes and a cells JSON frame. These
are disposable proof artifacts, not installed presentation. The canonical gate
includes the composition checks. Actual host rendering still needs integration.
Explicit cell-sized placements now retain the requested rows/columns and resolve
fit/insets for each view's cell size without changing source cursor metadata.
Natural-size images keep their native pixel dimensions.

`graphics_owner` combines the parser, store and ordered request queue behind a
bounded source-output queue. Decode jobs are dispatched through the backend's
shared worker pool. Parsing stops at completed image commands until pixels,
placement and cursor movement commit; following text and terminal queries cannot
overtake them. The caller retains its independent user-input path. Source tags
and response tickets guard worker completions, and malformed passthrough requires
a verified replacement baseline. This component does not create a PTY, broker or
presenter and is not wired into the installed terminal backend.

The parser exposes opt-in scroll notifications and typed display/alternate/reset
clears. The owner moves images with linefeeds, wrapping, reverse index, explicit
scrolls and inserted/deleted lines. Signed offsets retain bounded primary-buffer
image history; page clipping follows margin scrolling, and reversing direction
cannot restore discarded rows. Placements crossing a margin stay outside that
page operation. A display clear removes visible placements while keeping reusable
image data; alternate-buffer clearing and terminal reset release their state.
Reset also cancels partial uploads. Requests for excessive line shifts are
clamped to the page; line edits outside its margins do nothing.

The optional `graphics-authority-proof --mutations` uses a private raw PTY to
check actual source-observed reply/cursor ordering and image pixels after a real
linefeed. It verifies reusable image data after a display clear, redisplay, and
released storage after reset. `test-graphics-observer.py --authority-executable
... --mutations` additionally runs this authority through SSH when requested.
These remain private component proofs without a live presenter. Source ownership,
scrollback UI/image archive behavior and production reconnect still need work.

Remaining work:

The native source ownership component now lives in `source_broker`. It owns
multiple new PTYs, one parser/store per source, a shared decoder pool and shared
graphics/input/reply budgets behind a private authenticated Unix socket. Its
JSON control messages do not enter terminal input. View connections can disappear
while the source continues producing output, replying to queries and retaining
images. Launch receipts are consumed durably before spawning; an exited or lost
source ID never implicitly restarts its saved command. Initialization/fsync runs
in a bounded worker pool, duplicate opens share one initialization, and PID-bound
close requests escalate asynchronously instead of blocking the service loop.

This component is not yet selected by the application's normal session backend.
Its helper `examples/native-source-broker.rs` is disposable acceptance plumbing,
not an installed daemon launcher or a replacement for existing tmux/transferred
owners. Run `scripts/test-source-broker.py --executable
target/release/examples/native-source-broker` locally and with `--ssh` to check
worker/view lifetimes, no-view image output, source identity, launch receipts,
independent views/palettes, large input, resize and nonblocking close. Both are
part of the release gate. SSH acceptance uses a private loopback daemon and a
detached worker, not a real remote project or internet-latency benchmark.

Normal backend discovery/adoption, durable project-slot references, native
history/snapshot/lifecycle adapters, streamed presentation and robust transport/
graphics-fault recovery still need integration. The private worker now renders immutable snapshots on bounded workers and can
push credit-controlled cells frames with acknowledged row deltas. Production
frontend adapters and sharp host pixel delivery still need integration. Verify
process-group cleanup and interrupted-creation supervision before enabling the
backend broadly. Host presentation, clipboard/title/notification events, image
archives and the broader protocol requirements below remain unfinished.

- A backend-owned observer/parser for each verified source generation, including
  native, imported and transferred sources, local/SSH, startup output and gaps.
- Integrate the bounded control decoder, passthrough and worker pool into the
  source owner, including timers, source fairness and aggregate retained-image
  budgets. A slow observer must never block the terminal or accumulate unbounded
  tmux output.
- Connect the retained store to verified source output and terminal mutations;
  finish scrollback/presentation and resize interactions, all deletion selectors, image numbers,
  alternate-screen/reset observation and source replacement. Add source-owned
  local media, placeholders, animation and other audited protocol requirements
  before claiming their support. Per-terminal quotas still need an aggregate
  application-wide budget and bounded decoder admission.
- One source response authority; query replies must precede following device
  attributes, without duplicates from multiple frontends. Do not advertise
  success until storage and both presentation paths actually work.
- Graphical composition without disturbing text/input or App Space streams,
  a usable cells fallback, independent multi-view geometry and hidden tabs.
- Bounded image archives and cold restoration tied to process/boot/epoch;
  previews must stay read-only and must not replay terminal callbacks.
- Real rendered acceptance locally and over SSH, malformed-stream/resource
  stress, reconnect/no-view output, multiple sources/frontends and App Space
  coexistence. Current parser/decoder tests are foundations for these checks.


The private worker's presentation transport supports `watch`, `ack` and
`unwatch`. A client supplies a UUID view identity, strictly increasing positive
view epoch, host viewport generation, focus, geometry and palette. It must reject
frames from an older epoch/source/generation. A changed watch cancels pending
rendering and removes unsent obsolete packets; a partially written JSON line
finishes with its original epoch. Each view has one frame awaiting acknowledgement;
only the exact epoch/sequence releases credit, after its packet has been sent.
The next frame captures the latest source state, skipping intermediate output.
The first frame is complete. Subsequent frames name their acknowledged
`base_sequence` and contain changed rows plus current cursor/input modes. Source
and view changes reset that base. An unwatch retains an epoch tombstone until
transport disconnect, so delayed requests cannot resurrect an obsolete view.
`source_closed` retires a view; `view_error` reports failed composition or a view
which no longer fits its resized source. Failures back off and repeated errors
are deduplicated. Natural exits publish updated input availability without
restarting a command.

Frame composition and bounded JSON encoding run on two workers with a four-job
queue. The broker admits at most eight pending renders and four per peer. A
separate 128 MiB presentation budget covers visible-grid snapshots, frame runs
and composition; a separate 64 MiB transport budget covers encoded packets.
Images share immutable retained allocations, which remain charged until the
last snapshot releases them. Capturing a view copies neither terminal history
nor its inactive buffer. A frame waiting for acknowledgement retains only the
cells frame, not image pixels or a whole terminal snapshot. The decoder's working
budget remains independent. These are bounded payload reservations, not a total
RSS guarantee. One-shot `cells` uses the same asynchronous renderer and retains
a half-closed caller until its response is ready.

Run the private fixture with `--streams` locally and over loopback SSH to check
slow-view input/query isolation, reconstructed row deltas, independent fonts and
palettes, stale acknowledgements, source switches, resize error recovery,
source-close/natural-exit events and released snapshot reservations. It runs only
fake programs in private namespaces. This verifies the component and transport;
it does not prove real host image presentation or WAN frame latency.


Compatible native-owner discovery, a token-injecting local/SSH stdio connector,
and durable pane launch/reference records are now implemented as an internal
component. Its socket survives loss of runtime state; pending launch outcomes
can be recovered read-only without rerunning saved commands. See
[`native-sources.md`](native-sources.md) for the contract and remaining normal
backend/host-presentation integration. The normal workspace still uses its
existing backends; none were migrated or restarted by this component work.

## Native graphical planes candidate

The isolated native-source candidate now has an actual lossless image path from
source-owned uploads to the presenting Kitty window. `graphics_planes.rs`
encodes view-sized PNG planes separately from native terminal text. It masks
under-background pixels where the source cell has an explicit background or
inverse attributes, combines negative planes in source order, and keeps the
above-text plane separate. Unchanged image payloads share their retained cache;
ordinary text changes do not require another PNG upload. Cells presentation
keeps the bounded half-block fallback.

`image-planes-v1` is an explicit retained-owner capability and `Watch.pixels`
defaults to false for existing protocol callers. A current STAR/FLUX local helper
adds `image_planes_v1` to its font report; graphical controllers request planes
only after that report. The immutable STAR/KIT pin is unchanged. Older helpers
keep their existing behavior. This helper negotiation currently depends on the
existing Kitty font-watcher report and is not a universal new KIT capability.

The display-side `graphics_frontend.rs` validates geometry and bounded inline
PNG data, allocates its own image IDs, and places the planes below/above native
text, beneath the existing graphical chrome. It accepts no filenames, shell
commands or arbitrary escapes. Replacements arrive before old placements are
retired. Scoped clears and helper shutdown delete only this presenter's images.
Clear operations flush before being acknowledged by the output service. The
ordering follows the [Kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/#display-images).

`graphics_delivery.rs` keeps the latest queued update per view, with fair view
ordering, a 16 MiB queue and 64 pending view identities. One additional request
can be in flight. Effect acknowledgements mean admission to this local queue,
not physical GPU completion. Media/control packets and keyboard input continue
through the relay while the output service applies an image. The font/probe
mutex is not held during presentation. Presenter failure closes the view
transport with an error while leaving retained source processes running.

Encoding has a 2 MiB PNG limit per plane and 3 MiB combined base64 payload limit,
within the existing 4 MiB source frame ceiling. View dimensions are bounded to
8192 per side and 16 Mi pixels, subject to the shared working budgets. The host
keeps at most 32 view records, 16 MiB encoded data and 64 MiB of decoded texture
extents. These are payload/working limits, not total process RSS/VRAM limits.
Incoming admission counts string/raw bytes separately from JSON structure so
large base64 strings do not monopolize the budget reserved for small JSON nodes.

Private graphical UI proofs validate one-pixel source image detail locally and
with both the presentation and source reached over SSH. They also cover delete,
review hide/restore, switching to an empty App Space and back, retained source
PIDs, styled bursts and connector recovery. Cells proofs ensure no graphical
extension is sent. The actual local relay/output proof captures Kitty commands,
PNG byte preservation, scoped replacement/deletion and shutdown cleanup. A
stalled-output proof checks unchanged media/control forwarding, keyboard input
and replacement of obsolete image requests.

A real Kitty screenshot proof runs only inside private Xvfb, using cached Nix
Mesa software rendering. It confirms below-text glyph visibility, above-text
glyph coverage, one-pixel geometry and deletion restoring native text. The PNG
and relay data are byte-exact. The llvmpipe framebuffer shows at most one 8-bit
level of edge/glyph rounding; the physical comparison explicitly allows that
measured error. Artifacts are in `target/native-planes-kitty-artifacts/` and
results in `target/native-planes-kitty.log`.

This remains a candidate, not an installed backend or completed graphics parity.
Remaining requirements include production source recovery/cleanup, compatible
observation of existing sources, buffer-switch/resize/load coverage, host
placement-loss recovery, larger/chunked image transport and constrained-network
media acceptance. File/shared-memory transfers, animation and Unicode
placeholder compatibility remain separate unsupported protocol work. Cold-start
screen archives still do not restore image pixels. The original parity goal
and its unchecked requirements remain active.

The final delivery implementation is asynchronous: the local relay admits an
image into a latest-per-view queue and keeps forwarding other packets while the
output service works. A private delayed-presenter test explicitly withholds the
image application ACK and verifies keyboard input, presented/control messages
and an audio/media packet still pass. It also verifies twenty queued updates
collapse to the latest pending image. This covers relay packet flow; sustained
playback on a constrained WAN remains separate acceptance work.

The offscreen proof preserves its actual Kitty screenshots (`baseline.png`,
`below-text.png`, `above-text.png`, `restored.png`) and numeric comparisons in
`target/native-planes-kitty-artifacts/`. The source and transmitted PNGs preserve
exact pixel values. The software-GPU framebuffer comparison allows only the
measured one-level edge/antialias rounding and checks unchanged pixel geometry.
