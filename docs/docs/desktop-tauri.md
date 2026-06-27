---
sidebar_position: 6
---

# Desktop Tauri

The desktop app lives in `apps/openipc-station/src-tauri` and reuses the same
React UI as the browser station.

```sh
cd apps/openipc-station
npm run desktop:dev
```

`desktop:dev` starts Vite through Tauri's `beforeDevCommand`, then opens the
native window. The local Vite URL exists for the development WebView; the Tauri
window is the desktop app.

## Desktop Data Flow

```text
Tauri command
  -> native Rust backend
  -> nusb USB device
  -> openipc-rtl88xx Realtek HAL
  -> openipc-core receiver pipeline
  -> Tauri events with encoded Annex-B frames and metrics
  -> React WebCodecs playback
```

The desktop mode does not use browser WebUSB. It uses native `nusb` for USB
operations and sends encoded frame batches to the UI.

## Build

Check the source-level desktop build without bundling installers:

```sh
cd apps/openipc-station
npm run desktop:check
```

Build desktop bundles/installers:

```sh
cd apps/openipc-station
npm run desktop:build
```
