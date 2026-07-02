---
sidebar_position: 1
slug: /
---

# openipc-rs

`openipc-rs` is a Rust receiver stack for OpenIPC FPV ground stations. It is
meant to be useful in two ways:

- as reusable crates for people building their own OpenIPC tools, and
- as working React/Tauri and pure-Rust egui ground-station apps.

The project was built after studying `devourer`, `aviateur`, `openipc-zig`, the
standalone `adaptive-link` tools, and PixelPilot. Those projects are reference
material only; `openipc-rs` builds independently.

## Repository Shape

```text
openipc-rs/
  crates/openipc-core/        protocol, WFB, RTP, video, adaptive link
  crates/openipc-rtl88xx/     shared async Realtek rtl88xx USB/HAL driver
  crates/openipc-video/       platform-native and WebCodecs video decoding
  apps/openipc-cli/           native CLI utilities
  apps/wfb-rs/                WFB-ng-style Rust binaries over the userland driver
  crates/openipc-web/         wasm-bindgen SDK
  apps/openipc-station/       browser and Tauri station UI
  apps/nebulus/               egui station for desktop, Android, and browser
  plugins/tauri-plugin-openipc-usb/
                                Android USB permission plugin for Station
  docs/                       this Docusaurus site
  scripts/                    build, clean, release helpers
```

## What Runs Where

The receive path is intentionally Rust-heavy. Native and browser builds share
Realtek descriptor parsing, WFB session handling, packet decryption, FEC
recovery, RTP depacketization, Annex-B video framing, and adaptive-link packet
construction.

The platform boundary is kept at the edges:

- native apps open USB devices through `nusb`,
- browser apps ask JavaScript for WebUSB permission and then pass the granted
  `UsbDevice` into Rust/WASM,
- Android Tauri builds use a local `tauri-plugin-openipc-usb` plugin to request
  USB permission and pass an opened file descriptor to Rust,
- Nebulus Android builds perform the same permission/file-descriptor handoff
  through a small Rust JNI module and do not require Tauri,
- playback is handled by WebCodecs in the React app; Rust/WASM applications can
  also use the `openipc-video` WebCodecs backend directly,
- desktop builds use Tauri only as the application shell around the same React
  UI and native Rust backend.

Nebulus is the all-Rust application path: egui provides the UI and
`openipc-video` selects VideoToolbox, VA-API, Media Foundation, MediaCodec, or
WebCodecs for the current target.

The `wfb-rs` package is the native WFB-ng-style tool set. It rewrites the
receive/transmit/helper binary roles in Rust and uses the `openipc-rtl88xx`
userland Realtek driver for radio I/O, rather than relying on Linux kernel
monitor-mode interfaces. That keeps the main radio path usable on Linux,
macOS, and Windows, subject to each operating system's USB permission and driver
binding requirements.

## Current Status

The protocol pipeline, browser SDK, station UI, native CLI, adaptive-link
feedback path, and Realtek driver are implemented, including Jaguar3
RTL8812CU/EU and RTL8822CU/EU. The remaining work is mostly hardware
validation: comparing traces against known-good receivers, testing more
adapters, and proving cold-start behavior across chip families and operating
systems.
