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

The decoder accepts at most three access units in flight. Decoded output is a
single latest-frame slot: a newer native surface replaces an older surface that
egui has not presented. Native targets upload NV12 or YUV planes to persistent
GPU textures and perform color conversion in a shader. CPU RGBA conversion is a
compatibility fallback.

## Browser Path

Nebulus keeps WebUSB transfer buffers inside Rust/WASM and recycles each buffer
after parsing. WFB, FEC, RTP, route selection, and decoder orchestration remain
in Rust. WebCodecs returns browser `VideoFrame` objects, and the renderer uploads
the newest frame directly to a WebGL texture. Decoded pixels do not make a
round-trip through a WASM byte array.

WebUSB and WebCodecs objects are local to the browser event loop, so the web
build uses an async local executor rather than a native worker thread. Repaint
requests are event-driven; Nebulus does not busy-loop while idle.

## Queue Policy

- Four USB reads stay in flight.
- RTP reordering is disabled by default; enable it only for measured
  out-of-order delivery.
- Decoder input is bounded to three access units.
- Decoded output is latest-only.
- Runtime metrics and counters are coalesced before the UI consumes them.
- Audio uses a short scheduling queue to absorb output-device jitter without
  coupling video timing to audio playback.

After decoder reset or overload, playback waits for codec parameter sets and a
new H.264 IDR or H.265 random-access frame. This avoids presenting corrupted
delta frames. A shorter transmitter keyframe interval reduces recovery time.

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

Use **Metrics** for link quality, post-FEC loss, repair rate, bitrate, delivered
FPS, and processing latency. Use **Diagnostics > Stage latency** to locate a
specific receiver bottleneck. These are not glass-to-glass measurements; true
camera-to-display latency requires a transmitter timestamp or synchronized
visual test.

The legacy React/Tauri Station has an additional encoded-frame IPC boundary and
decodes through WebCodecs in its WebView. It remains documented as an integration
example, but Nebulus is the primary low-latency path.
