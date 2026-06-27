---
sidebar_position: 1
slug: /
---

# openipc-rs

`openipc-rs` is a Rust receiver stack for OpenIPC FPV ground-station
applications. It is designed to be usable as:

- shared Rust crates for native applications,
- a native CLI and Tauri desktop station,
- a browser/WebAssembly SDK using WebUSB,
- a React-based OpenIPC Station application.

The project was built after studying `devourer`, `aviateur`, `openipc-zig`, the
standalone `adaptive-link` tools, and PixelPilot. Those projects are reference
material only; `openipc-rs` builds independently.

## Repository Shape

```text
openipc-rs/
  crates/openipc-core/        protocol, WFB, RTP, video, adaptive link
  crates/openipc-rtl88xx/     shared async Realtek rtl88xx USB/HAL driver
  crates/openipc-native/      native CLI utilities
  crates/openipc-web/         wasm-bindgen SDK
  apps/openipc-station/       browser and Tauri station UI
  docs/                       this Docusaurus site
  scripts/                    build, clean, release helpers
```

## Current Boundary

The implementation includes the full receive pipeline in Rust, browser and
desktop station apps, adaptive-link feedback generation, and shared Realtek
bring-up paths for native and WebUSB. Hardware support still needs live adapter
validation across each chip family before the support matrix should be treated
as final.
