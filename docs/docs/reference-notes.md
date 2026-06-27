---
sidebar_position: 13
---

# Reference Notes

These notes summarize what was learned from the reference projects.

## devourer

`devourer` is the native Realtek USB WiFi implementation. It owns the hardware
bring-up that matters for OpenIPC receive:

- firmware download,
- EFUSE/EEPROM and power sequencing,
- BB/RF tables,
- monitor mode,
- channel and bandwidth selection,
- TX descriptors,
- RX descriptor parsing.

The USB model is vendor-control register access plus bulk endpoints:

- request `0x05` for register reads and writes,
- interface 0 claim,
- descriptor-driven bulk IN and bulk OUT endpoint discovery,
- 32 KiB RX transfer buffers to avoid splitting full chip-side aggregates.

The Realtek RX aggregate format is shared Rust logic in `openipc-core`.

## aviateur

`aviateur` is the native OpenIPC ground station. It uses devourer for adapter
access, then handles WFB, RTP, adaptive-link feedback, and video playback.

Packet flow:

1. devourer emits parsed 802.11 frames.
2. OpenIPC/WFB frame checks validate `57:42:<channel_id>` MAC fields.
3. WFB session packets decrypt a session key.
4. WFB data packets decrypt into FEC fragments.
5. Primary fragments emit RTP packets.
6. RTP packets go to playback or optional UDP output.

`openipc-rs` mirrors the protocol behavior in shared Rust, while keeping UI,
USB permissions, and rendering at platform edges.

## openipc-zig

`openipc-zig` proves that browser/WebUSB OpenIPC receive is possible, even if
its implementation is not the desired long-term shape. It is useful for:

- browser permission flow,
- WebUSB constraints,
- WebCodecs playback reference,
- understanding how much hardware setup must still happen in browser builds.

`openipc-rs` keeps WebUSB as a transport adapter and puts the actual receiver
pipeline in Rust/WASM.

## PixelPilot

PixelPilot is useful as an Android reference for packaging a full ground-station
experience around an H.264/H.265 WFB feed. It helps validate expectations for
codec handling, latency, and UI-level receiver metrics, but it is not the
source of the Realtek USB driver path used here.
