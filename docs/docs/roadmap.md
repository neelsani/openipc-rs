---
sidebar_position: 14
---

# Roadmap

## Core Protocol

- Done: Realtek RX aggregate parsing.
- Done: OpenIPC/WFB frame filtering.
- Done: WFB session/data decryption.
- Done: Reed-Solomon FEC recovery.
- Done: RTP H.264/H.265 depacketization.
- Still useful: add captured hardware fixtures and cross-check them against
  aviateur/openipc-zig.

## Realtek Driver

- Done: register helpers around vendor request `0x05`.
- Done: endpoint discovery and OUT endpoint selection.
- Done: firmware/table data checked into the Rust driver crate.
- Done: RTL8812/RTL8821 bring-up paths.
- Done: RTL8814 reserved-page/DDMA firmware path.
- Done: TX descriptor support for adaptive-link feedback.
- Still needed: EFUSE-backed MAC/RFE parsing.
- Still needed: hardware smoke tests and trace comparisons per chip family.

## Native And Desktop

- Done: native receive loop.
- Done: RTP-over-UDP mirror.
- Done: Annex-B output.
- Done: native adaptive-link uplink.
- Done: Tauri desktop station using native Rust/nusb backend.
- Still useful: dedicated low-latency native renderer outside the Tauri app.

## Browser

- Done: WebUSB permission prompt in JavaScript.
- Done: Rust/WASM `nusb` WebUSB receive/transmit object.
- Done: shared Realtek monitor initialization through WebUSB.
- Done: WebCodecs playback, recording, persisted settings, and link HUD.
- Still useful: fallback playback path for browsers without WebCodecs support
  for the active codec.
