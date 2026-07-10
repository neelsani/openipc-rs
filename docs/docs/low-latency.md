---
sidebar_position: 4
---

# Low-Latency Operation

Nebulus is designed to display the newest decodable frame instead of preserving
every frame in a growing playback queue. USB, protocol recovery, decode, and
presentation each use bounded work so temporary overload drops stale output
rather than adding seconds of delay.

## Native And Android Path

The receiver worker keeps four USB bulk-IN transfers posted. It parses Realtek
aggregates, advances WFB/FEC state, depacketizes RTP, and submits complete access
units to `openipc-video`. The UI thread does not wait on USB or codec work.

Each completed transfer is returned to the endpoint immediately after WFB/RTP
processing stops borrowing its storage. Video submission then runs before VPN,
audio, UDP, adaptive-link, and diagnostic work. The encoded access-unit buffer
is moved into `openipc-video`; ordinary playback does not clone the H.264/H.265
frame. Recording is the exception because the muxer and decoder both need to
own the encoded sample.

Native adaptive-link/VPN bulk-OUT and Jaguar3 coexistence/power maintenance run
on a bounded background radio worker below the RX thread's scheduling
priority. A stalled transmitter can drop auxiliary packets, but it cannot hold
the receive loop. Browser adaptive feedback similarly uses one persistent
bounded WebUSB OUT endpoint and never awaits a normal TX completion in the RX
future. Jaguar3 browser maintenance runs as a separately cancellable local
task and is joined before monitor shutdown.

The route manager parses the 802.11 header once before handing a matched
forwarder payload to its WFB runtime. `openipc-video` then inspects Annex-B
configuration and keyframe state in one allocation-free NAL pass. On macOS,
the normal uniquely owned four-byte Annex-B buffer is converted to
VideoToolbox length prefixes in place; unusual shared or three-byte input uses
the general copying fallback.

The decoder uses a small platform-bounded input queue. Decoded output is a
single latest-frame slot: a newer native surface replaces an older surface that
egui has not presented. macOS imports VideoToolbox IOSurface planes directly
into Metal/wgpu textures. Linux and Windows upload only the newest NV12 surface
into persistent GPU textures and perform color conversion in a shader. Android
renders MediaCodec output directly into a SurfaceTexture external GLES texture,
avoiding decoded-plane mapping and per-frame texture upload.

Desktop wgpu presentation requests a non-vsynced surface with one frame of
surface latency. The exact present mode still depends on the graphics backend
and compositor, and tearing is possible. macOS marks render and receiver work
as user-interactive QoS. Windows raises the receiver/render thread priority.
Linux requests a negative nice value when permissions allow it. Android uses
urgent-display priority for rendering, PixelPilot's `-16` receive priority,
the fastest same-resolution display mode, a non-vsynced egui surface, and a
MediaCodec realtime/low-latency configuration.

Nebulus polls native decoder output at most 2 ms apart after submission begins,
independently of whether the next USB aggregate has completed. This prevents a
50 ms idle USB wait from capping a 60 FPS MediaCodec stream at 20 observed or
presented frames per second. Physical Android devices permit eight in-flight
frames for normal hardware pipeline depth. The Android SDK emulator permits
twelve because Goldfish's software codec advertises an eight-frame output
delay. These are decoder-work bounds, not playback queues; output remains
latest-only.

## Browser Path

Nebulus keeps WebUSB transfer buffers inside Rust/WASM and recycles each buffer
after parsing. WebUSB, WFB/FEC recovery, route selection, audio, and adaptive
link remain on the app executor. Recovered video RTP packets are packed into a
single transferable buffer per receive batch and sent to a Rust/WASM RTP
worker. Complete access units move over a direct `MessageChannel` to a second
worker that owns WebCodecs. Decoder stalls therefore cannot block RTP parsing
or WebUSB buffer recycling.

The RTP-to-decoder and decoder-input queues are bounded. If decode cannot keep
up, Nebulus drops complete dependent access units and resumes at a keyframe;
it never grows a latency backlog. The decoder transfers at most one retained
`VideoFrame` back per display refresh and replaces any older pending output.
The renderer uploads that frame directly to a WebGL texture, so decoded pixels
do not make a round-trip through a WASM byte array. Repaint requests remain
event-driven. The canvas requests a desynchronized WebGL2 context without
antialias, depth, stencil, or a preserved backbuffer; unsupported hints are
ignored by the browser.

## Queue Policy

- Four USB reads stay in flight, while at most two completed transfers per
  adapter wait for protocol processing.
- RTP reordering is disabled by default; enable it only for measured
  out-of-order delivery.
- Decoder input is bounded per platform. Android drops the newest access unit
  on transient MediaCodec pressure and continues immediately, accepting a
  possibly damaged picture over an extended blackout. A true codec stall still
  resets dependency state and resumes at a keyframe.
- Decoded output is latest-only.
- Runtime metrics and counters are emitted at 20 Hz. Worker output transfer is
  paced by animation frames and remains independent of that throttle.
- Native audio requests a 256-frame device buffer and caps queued PCM at 20 ms.
  Web Audio restarts near 5 ms and trims a schedule that exceeds 40 ms.

After decoder reset, playback waits for codec parameter sets and a new H.264 IDR
or H.265 random-access frame. A shorter transmitter keyframe interval reduces
recovery time. Android's transient-pressure path intentionally does not reset:
for FPV, a temporarily damaged frame is preferable to suppressing every frame
until the next IDR.

## Source-Side Settings

Receiver tuning cannot compensate for a high-latency encoder. For FPV, configure
the air-side encoder without B-frames or lookahead, use a short GOP, and emit
regular keyframes. Smaller WFB FEC blocks recover sooner but have less efficient
parity overhead; larger blocks improve coding efficiency at the cost of waiting
for more source fragments.

For `wfb-rs`, avoid tunnel aggregation when interactive latency matters. WFB
primary fragments are emitted immediately and parity is generated as a source
block completes.

## Measure The Right Boundary

Nebulus reports local processing stages:

```text
USB completion -> Realtek parse -> WFB/FEC -> RTP depacketize
    -> decoder submit -> decoded output -> GPU presentation
```

`USB completion to decode submit` is the video critical path. `Receive batch`
also includes route, recording, VPN, and maintenance work that runs after video
submission, so it should not be treated as display latency. `Decode to GPU
upload` includes time in the latest-only app event slot plus the platform upload
or SurfaceTexture latch.

Packet-level trace targets are sampled one in 128 by Nebulus's log sink. The
protocol counters are exact; sampling only prevents string formatting and
stderr/Logcat output from becoming the largest latency source in Very verbose
mode.

Use **Metrics** for link quality, post-FEC loss, repair rate, bitrate, delivered
FPS, and processing latency. Use **Diagnostics > Stage latency** to locate a
specific receiver bottleneck. These are not glass-to-glass measurements; true
camera-to-display latency requires a transmitter timestamp or synchronized
visual test.

Aviateur adds a local UDP handoff, FFmpeg demux, decoded-frame transfer, and a
small presentation queue. PixelPilot uses a direct MediaCodec surface on
Android. Nebulus avoids those handoffs and uses the same direct-surface class
of path as PixelPilot on Android, but performance claims still require the same
adapter, VTX stream, display mode, and a synchronized glass-to-glass test.

## Reproducible Microbenchmarks

Run the Rust protocol benchmark without hardware:

```sh
cargo bench -p openipc-core --bench dataplane --locked
```

To compare the same WFB 8/12 recovery workload with PixelPilot's vendored
`zfex.c`, keep a PixelPilot checkout next to `openipc-rs` and run:

```sh
./scripts/benchmark-reference-fec.sh
```

Pass another wfb-ng source directory as the first argument to test the zfex
revision used by Aviateur or another receiver. On an Apple Silicon development
machine, the July 2026 median results were:

| WFB recovery workload                   | `openipc-core` | PixelPilot zfex |
| --------------------------------------- | -------------: | --------------: |
| One missing fragment, 3996 bytes        |       0.812 us |        0.935 us |
| Four missing fragments, 3996 bytes each |       3.171 us |        3.349 us |

These figures isolate FEC math and memory handling. They are machine-specific
and do not establish glass-to-glass latency. Use the same adapter, encoded
stream, decoder, display mode, and high-speed-camera test for an end-to-end
comparison.
