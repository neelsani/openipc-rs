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

The video fullscreen button has a desktop-specific path. Browser builds use the
element Fullscreen API on `#video-region`. Tauri builds call the native window
fullscreen API and apply a video-only overlay class, because embedded WebViews
do not consistently support element fullscreen. The canvas and OSD stay inside
the video region in both modes.

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

## Android

Tauri can build the station for Android, but Android USB access is different
from Linux/macOS/Windows desktop USB access.

`nusb::list_devices()` is not the supported Android app-sandbox path today.
Android apps should discover devices through Android's own USB APIs:

1. Use `UsbManager` to list attached devices and match one of the Realtek
   VID/PID pairs.
2. Request user permission for that `UsbDevice`.
3. Open a `UsbDeviceConnection`.
4. Read `UsbDeviceConnection.fileDescriptor`.
5. Call the Tauri command `openipc_connect_from_fd` with that fd plus the usual
   channel settings.

OpenIPC Station includes this bridge as the local Tauri plugin
`plugins/tauri-plugin-openipc-usb`. The plugin owns the Android library project,
Kotlin `UsbManager` code, manifest USB-host feature, Rust command wrappers, and
Tauri permissions. Tauri includes it during Android dev/build like any other
mobile plugin; no generated `src-tauri/gen/android` source edits are required.

```sh
cd apps/openipc-station
bun run android:init
bun run android:dev
```

The Rust command duplicates the descriptor with `dup(2)`, then wraps the
duplicate with `nusb::Device::from_fd`. Android/Kotlin keeps the original
`UsbDeviceConnection` open until React has called `openipc_connect_from_fd`;
then React calls the Android close command for the original handle. Rust owns
only the duplicate after that.

The command shape is:

```ts
await tauriConnectFromFd({
  fd,
  vendorId: 0x0bda,
  productId: 0x8812,
  product: "RTL8812AU",
  channel: 161,
  channelWidthMhz: 20,
  channelOffset: 0,
  skipReset: true,
});
```

A minimal Android-side bridge looks like this conceptually:

```kotlin
val manager = getSystemService(Context.USB_SERVICE) as UsbManager
val device = manager.deviceList.values.first { usbDevice ->
    usbDevice.vendorId == 0x0bda && usbDevice.productId == 0x8812
}

// Request permission first in real code, then:
val connection = manager.openDevice(device)
val fd = connection.fileDescriptor
```

The shared Rust receive path after `openipc_connect_from_fd` is the same Realtek
HAL, OpenIPC/WFB/RTP pipeline, adaptive-link feedback path, and WebCodecs UI
used by desktop Tauri.

On Android, the UI calls `plugin:openipc-usb|list_devices` for attached
adapters. `openipc_list_devices` remains a compatibility fallback and returns
the supported Realtek IDs rather than live Android enumeration.

## Signing

The current Tauri config sets macOS `signingIdentity` to `"-"`, which produces
ad-hoc signed macOS bundles. That is useful for local and CI builds, but it is
not Apple Developer ID signing or notarization. Users may still see OS security
warnings until real signing and notarization are configured.
