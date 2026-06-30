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
  -> openipc-core ReceiverRuntime
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

## VPN Tunnel

Station has a dedicated VPN tab for the OpenIPC tunnel/data ports. It is
disabled by default and is intentionally separate from the payload route
builder. When enabled, Station listens for recovered WFB payloads on tunnel RX
port `0x20`, writes the contained IP packets into an OS VPN/TUN interface,
reads packets back from that interface, wraps them with OpenIPC's two-byte
tunnel length prefix, and sends them over tunnel TX port `0xa0`.

The native bridge uses the Rust `tun` crate. On desktop Unix builds it creates a
`10.5.0.3/24` interface and keeps it nonblocking so the existing receive loop
can service USB, adaptive-link feedback, and tunnel uplink without creating a
separate worker thread. Creating the interface may require root or network
administration privileges depending on the OS. After receive starts, the VPN
tab shows the actual interface name, local IP, and tunnel RX/TX ports reported
by the native backend.

On Windows, Station uses the same bridge through Wintun. The Windows target may
need Wintun installed or bundled next to the app, depending on how the final
installer is packaged.

Browser/WebUSB builds cannot create OS network interfaces, so the VPN tab is
shown as unavailable there. Android uses the local `tauri-plugin-openipc-usb`
plugin to request `VpnService` consent, create the VPN/TUN file descriptor, and
pass it to the Rust backend before receive starts.

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
Kotlin `UsbManager` code, `VpnService` bridge, manifest USB-host/VPN entries,
Rust command wrappers, and Tauri permissions. Tauri includes it during Android
dev/build like any other mobile plugin; no generated `src-tauri/gen/android`
source edits are required.

```sh
cd apps/openipc-station
bun run android:init
bun run android:dev
```

For local builds, Tauri needs Java, the Android SDK, and an NDK. On macOS with
Homebrew OpenJDK and the default Android SDK path, this is the environment
Tauri expects if auto-detection does not pick it up:

```sh
export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME=$HOME/Library/Android/sdk
export ANDROID_SDK_ROOT=$ANDROID_HOME
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
export NDK_HOME=$ANDROID_NDK_HOME
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/bin:$PATH"
```

If `bun tauri android init --ci` reports `Android NDK not found`, install the
same SDK packages used by CI:

```sh
sdkmanager --sdk_root="$ANDROID_HOME" --install \
  "platform-tools" \
  "platforms;android-36" \
  "build-tools;36.0.0" \
  "ndk;27.2.12479018"
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

On Android, the UI still calls the app-level `openipc_list_devices` command.
The Rust command delegates to the local `tauri-plugin-openipc-usb` plugin, and
the plugin does live `UsbManager` enumeration using filters generated from
`openipc-rtl88xx::SUPPORTED_DEVICES`.

## Signing

The current Tauri config sets macOS `signingIdentity` to `"-"`, which produces
ad-hoc signed macOS bundles. That is useful for local and CI builds, but it is
not Apple Developer ID signing or notarization. Users may still see OS security
warnings until real signing and notarization are configured.
