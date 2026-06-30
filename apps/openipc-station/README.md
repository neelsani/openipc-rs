# OpenIPC Station

OpenIPC Station is the shared React/Vite UI for the browser/WebUSB build and
the Tauri desktop build.

Hosted app: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)

## Browser Development

```sh
bun install
bun run dev
```

`bun run dev` builds the Rust/WASM package first, then starts Vite.
Browser mode uses WebUSB. The Connect button opens the browser device picker,
then the Rust/WASM SDK claims the adapter, initializes monitor mode, reads USB
bulk transfers, and returns encoded video frames plus metrics to React.

## Production Web Build

```sh
bun run build
```

The static build is written to `dist`. It includes the generated
`openipc-web` WASM package and runs as the browser/WebUSB version of OpenIPC
Station.

CI deploys the same `dist` output to Cloudflare on normal pushes to `master`.
Local development only needs the build and preview commands above.

## Desktop Development

```sh
bun run desktop:dev
```

Desktop mode opens a Tauri window and uses the native Rust/nusb backend instead
of browser WebUSB.

The terminal still shows a local Vite URL because Tauri loads that dev server
inside the WebView. Opening the URL in a normal browser is not the desktop app;
it will use WebUSB. The Tauri window uses native USB commands and receives
batches over Tauri events.

Video decode happens in the frontend through WebCodecs in both modes. The Rust
side delivers encoded Annex-B H.264/H.265 frames. The video fullscreen button is
video-region fullscreen in the browser and native-window fullscreen plus a
video-only overlay in the Tauri app, so the OSD stays visible in both modes.

## UI Responsibilities

- Device/channel/key selection and persistent settings.
- Start/stop receive, recording, decoder reset, and video fullscreen.
- WebCodecs playback and canvas recording. Opus RTP payload type 98 can be
  played from the main video RTP route, which is how the OpenIPC audio docs
  describe mixed video/audio.
- Link HUD, metrics graphs, latency diagnostics, and logs.
- Route manager for extra WFB payload outputs. Routes can inspect bytes, log a
  throttled summary, forward to UDP in native/Tauri mode, or play filtered audio
  RTP with WebCodecs `AudioDecoder`. The current implemented audio codec is
  Opus, with an Auto mode for the documented OpenIPC payload type 98 stream.
- Raw route counters and audio metrics. Protocol parsing beyond video/audio is
  intentionally left to app code or downstream integrations.

## Android/Tauri

Android uses the same Tauri app shell, but USB discovery is Android-owned. The
Rust backend cannot enumerate USB adapters from the app sandbox with `nusb`
today. This app uses the local `tauri-plugin-openipc-usb` plugin, which ships
the Kotlin `UsbManager` bridge as a normal Tauri mobile plugin instead of
modifying Tauri's generated Android project.

```sh
bun run android:init
bun run android:dev
bun run android:build
```

Local Android builds need Java, the Android SDK, and an NDK. On macOS with
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

If the NDK is missing, install the same packages CI uses:

```sh
sdkmanager --sdk_root="$ANDROID_HOME" --install \
  "platform-tools" \
  "platforms;android-36" \
  "build-tools;36.0.0" \
  "ndk;27.2.12479018"
```

The plugin uses Android `UsbManager` to list supported Realtek adapters, request
permission, open a `UsbDeviceConnection`, and pass its file descriptor to the
Rust `openipc_connect_from_fd` command. Rust duplicates that descriptor before
handing it to `nusb::Device::from_fd`; the frontend then asks the plugin to
close the original Android handle.
