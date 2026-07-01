---
sidebar_position: 4
---

# Low-Latency Operation

OpenIPC Station uses bounded queues. USB reads stay posted, WFB/RTP data is
processed as soon as it is recoverable, and compressed frames go to WebCodecs
without a playback jitter buffer.

## Browser path

`WebUsbReceiverSession` owns the nusb WebUSB endpoint and keeps four bulk-IN
transfers in flight by default. Each completion is processed inside Rust/WASM
and its buffer is recycled immediately. Only completed video frames, selected
route payloads, and diagnostics cross into JavaScript.

This removes the raw USB transfer round trip through JavaScript. One final
WASM-to-JavaScript copy remains for compressed data supplied to WebCodecs;
decoded surfaces remain in the browser.

## Desktop path

The Tauri receiver keeps native USB and WFB processing on its worker thread.
Video frames use a raw Tauri channel with a compact binary header and Annex-B
payload. Counters and control information use the lower-rate JSON event. This
avoids base64 expansion and JSON serialization for video frames.

## Queue policy

If the WebCodecs decode queue grows beyond the station limit, the decoder is
reset and the receiver waits for the next IDR frame instead of displaying stale
footage. RTP reordering is disabled by default and should only be enabled when
the link actually delivers out-of-order packets.

Audio starts with a short scheduling cushion. Recording adds browser media
encoding work and can increase latency on slower machines.

## Source-side settings

For `wfb-rs`, use zero tunnel aggregation when interactive latency matters. WFB
primary fragments are emitted immediately; FEC parity is generated when a
source block completes. Smaller FEC blocks reduce recovery wait at the cost of
less parity efficiency.

The transmitter encoder remains part of the latency budget. Low-delay encoder
settings, no lookahead, no B-frames, short GOPs, and frequent IDR frames are
required for low glass-to-glass latency.

## Measuring latency

Diagnostics cover the local path:

```text
USB completion -> Realtek parse -> WFB/FEC -> RTP frame
    -> IPC/WASM boundary -> decoder queue -> decoded output -> canvas
```

These measurements identify receiver bottlenecks but are not a true
glass-to-glass number. That requires a transmitter capture timestamp or a
synchronized on-screen timestamp.
