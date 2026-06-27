---
sidebar_position: 6
---

# Desktop Tauri

The desktop app lives in `apps/openipc-station/src-tauri` and reuses the same
React UI as the browser station.

```sh
cd apps/openipc-station
bun run desktop:dev
```

`desktop:dev` starts Vite through Tauri's `beforeDevCommand`, then opens the
native window. The local Vite URL exists for the development WebView; the Tauri
window is the desktop app.

If you open the Vite URL directly in a normal browser while `desktop:dev` is
running, it behaves like the browser build and will use WebUSB. The Tauri window
uses the desktop runtime and native `nusb` backend.

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

Video decode still happens in the WebView through WebCodecs. Rust handles USB
and protocol reconstruction; the UI handles decoded frame lifecycle, rendering,
recording, and HUD updates.

## Build

Check the source-level desktop build without bundling installers:

```sh
cd apps/openipc-station
bun run desktop:check
```

Build desktop bundles/installers:

```sh
cd apps/openipc-station
bun run desktop:build
```

CI checks and releases desktop targets for Linux x64/arm64, macOS Apple
Silicon/Intel, and Windows x64/arm64.

## Signing

The current Tauri config sets macOS `signingIdentity` to `"-"`, which produces
ad-hoc signed macOS bundles. That is useful for local and CI builds, but it is
not Apple Developer ID signing or notarization. Users may still see OS security
warnings until real signing and notarization are configured.
