---
sidebar_position: 9
---

# Nebulus

Nebulus is the workspace's pure-Rust ground station. It is built with egui and
targets macOS, Linux, Windows, Android, and browsers from one application
crate. Use it when you want a native Rust UI or a compact reference showing how
the driver, protocol, and decoder crates fit together.

Nebulus is the project's primary ground station. It provides the low-latency
video receive path, adaptive link, configurable payload routes, Opus playback,
encoded recording, native VPN bridging, diagnostics, and a portable all-Rust
UI. The older React/Tauri OpenIPC Station remains in the repository as an
alternative implementation. New desktop and Android release artifacts, and the
hosted app at [nebulus.openipc-rs.neels.dev](https://nebulus.openipc-rs.neels.dev),
are built from Nebulus. The older React app remains available at its own legacy
URL.

The header always shows the package version. CI builds also embed the current
tag and short commit hash from the same `OPENIPC_*` metadata used by Station.

## Architecture

```mermaid
flowchart LR
    USB["Realtek USB adapter"] --> RTL["openipc-rtl88xx<br/>monitor mode and RX aggregates"]
    RTL --> Core["openipc-core<br/>WFB, crypto, FEC, RTP"]
    Core --> Video["openipc-video<br/>platform H.264/H.265 decoder"]
    Core --> Routes["route fanout<br/>inspect, log, UDP, audio"]
    Core --> Record["keyframe-aligned<br/>MP4 recorder"]
    Core --> Tun["native TUN<br/>RX 0x20 / TX 0xa0"]
    Routes --> Opus["ropus<br/>Opus to PCM"]
    Opus --> Audio["CPAL or Web Audio"]
    Video --> Latest["single latest-frame slot"]
    Latest --> Egui["Nebulus egui renderer"]
    Core --> Link["adaptive-link quality and feedback"]
    Link --> RTL
```

The desktop and Android app keep the blocking USB receive loop on a dedicated
worker thread. It initializes the radio, keeps four bulk-IN transfers in
flight, parses each Realtek aggregate, advances the receiver state machine,
submits complete access units to the decoder, and sends compact state updates
to egui. The UI thread never waits on USB or codec work.

The browser follows the same stages on the browser's local async executor.
WebUSB and WebCodecs objects cannot cross Rust threads, so Nebulus polls them
without a Web Worker. Every asynchronous completion requests an egui repaint;
the UI does not busy-loop while idle.

## Platform Boundaries

| Target  | USB access                                 | Video decode                   | Audio output   |
| ------- | ------------------------------------------ | ------------------------------ | -------------- |
| macOS   | `nusb`                                     | VideoToolbox                   | CPAL/CoreAudio |
| Linux   | `nusb`                                     | VA-API through `cros-codecs`   | CPAL/ALSA      |
| Windows | `nusb`                                     | Media Foundation and D3D11     | CPAL/WASAPI    |
| Android | `UsbManager`, then `nusb::Device::from_fd` | NDK MediaCodec and ImageReader | CPAL/AAudio    |
| Browser | `nusb-webusb` / WebUSB                     | WebCodecs                      | Web Audio      |

Android's JNI bridge only handles discovery, permission, and opening the USB
file descriptor. Radio control transfers and streaming transfers are still
performed by the Rust driver.

## Desktop

From the repository root:

```sh
cargo run -p nebulus --bin nebulus-desktop --release
```

After a release is published to crates.io, a source install is also available:

```sh
cargo install nebulus
nebulus-desktop
```

Prebuilt archives and app bundles are available from
[GitHub Releases](https://github.com/neelsani/openipc-rs/releases). Linux and
Windows archives contain the executable directly; macOS releases contain an
ad-hoc-signed `.app` bundle. Platform security warnings are expected until
notarization and code signing are configured.

### System Tray

macOS and Windows builds install a Nebulus tray icon. It appears in the macOS
menu bar or the Windows notification area; Windows may move it under the
overflow arrow. The menu provides:

- **Show Nebulus** and **Hide Nebulus**,
- **Start RX** or **Stop RX**, synchronized with receiver state,
- **Enable VPN on next start**, available while the receiver is stopped,
- **Open VPN Settings**, which restores the window and selects the VPN panel,
- **Quit Nebulus**.

VPN cannot be enabled in the middle of an active receiver session because its
WFB tunnel routes and native TUN interface are constructed during startup. Stop
RX, enable VPN from either the tray or VPN panel, then start RX again. Linux
does not currently build the tray integration, avoiding additional
AppIndicator/GTK runtime dependencies.

Select a supported adapter, set the radio channel and width to match the VTX,
confirm the WFB key, and press **Start RX**. The default OpenIPC `gs.key` is
embedded. **Open file** uses the native desktop dialog, browser picker, or
Android Storage Access Framework to load another key. The key is never text
editable. Dropping a `gs.key` file on the window remains available where the
platform supports file drops. Channel, offset, and Link ID use bounded sliders
with individual buttons that restore OpenIPC defaults.

Once monitor initialization succeeds, the top of Settings shows the connected
receiver's actual USB VID:PID, probed chip family, RF path layout, cut revision,
USB connection speed, bulk endpoints, cold/warm initialization result, firmware
download status, and active RF and video-channel configuration. These values
come from the opened device and `InitReport`, not the selector's family hint.
Nebulus clears the summary on stop or failure so it never looks like a stale
device is still connected.

The Linux decoder requires VA-API development packages. See
[Platform Video Decoding](./native-video.md#linux-va-api) for the package list
and render-node override.

## Browser

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd apps/nebulus
trunk serve --release --open
```

WebUSB requires localhost or HTTPS. Pressing **Start RX** directly opens the
device picker inside the click handler, preserving the browser's required user
gesture. The selected device is initialized into monitor mode by the same Rust
HAL used by native targets.

To build static deployment files:

```sh
cd apps/nebulus
trunk build --release
```

Serve the generated `dist/` directory over HTTPS. Do not open `index.html`
directly from disk; WebUSB is unavailable from a `file:` origin.

Run `trunk serve` without `--release` to expose the development-only
**Codec mock** button. The same button is available from a debug native build
started with `cargo run -p nebulus --bin nebulus-desktop`. It loops an embedded,
pre-recorded 1920x1080 H.264 stream with 48 kHz Opus audio. Rust packetizes and
interleaves both tracks as RTP. Video runs through the normal depacketizer and
production decode/presentation path; audio runs through the configured
mixed-audio route, Opus decoder, volume control, and output queue. WASM uses
WebCodecs only for video decoding; the mock does not use an encoder. It requires
no USB adapter and is omitted from release builds.

## Android

Nebulus uses Android NativeActivity and declares USB-host support through
Cargo APK metadata.

```sh
./scripts/android-nebulus-dev.sh
```

The helper discovers Java, the Android SDK, and the newest installed NDK. It
starts the first available AVD or reuses a running emulator, waits for boot,
maps the emulator ABI to the correct Rust target, installs Nebulus, and follows
timestamped Logcat output. Select an AVD or force a clean boot with:

```sh
./scripts/android-nebulus-dev.sh --avd openipc_pixel_8_api36 --cold-boot
```

For an APK-only build:

```sh
rustup target add aarch64-linux-android
cargo install cargo-apk2 --locked
cargo apk2 build -p nebulus --lib --target aarch64-linux-android
```

On first start, Android displays its USB permission prompt after the user
starts the receiver. The app keeps the `UsbDeviceConnection` alive for the
whole receiver session and gives a duplicated descriptor to `nusb`, avoiding a
second Java/Kotlin data path.

The Android entrypoint installs Nebulus's shared Rust logger, so driver and
application messages are available in standard application output and the
in-app Logs tab.

The default build uses Android's debug key. A distribution build additionally
needs a release keystore configured through
`[package.metadata.android.signing.release]`; do not commit keystore passwords
to the repository.

## Latency Behavior

Nebulus favors current video over complete playback:

- Four USB reads remain in flight to avoid endpoint starvation.
- WFB FEC and optional RTP reorder happen before decode.
- Decoder work is capped at three access units in flight.
- Decoded output is a single-slot latest-frame mailbox.
- Runtime events coalesce pending video to one frame and merge pending batch
  counters, so a slow UI cannot build a decoded-frame queue.
- egui presents only the newest output available after a receive batch.
- The receiver thread calls `Context::request_repaint()` when new state or a
  frame is ready.

On macOS, Linux, and Windows, the receiver hands retained native decoder
surfaces to the UI through a latest-only event slot. Stale surfaces are
dropped before presentation work begins. The UI uploads the newest frame's Y
and UV planes into persistent `R8Unorm` and `Rg8Unorm` wgpu textures and
converts them in a GPU fragment shader. This reduces a 1080p upload from about
8.3 MB of RGBA to 3.1 MB of NV12 and removes per-pixel CPU color conversion.

VideoToolbox exposes mapped NV12 planes on macOS. Linux maps the selected
VA-API DMA surface only after coalescing. Windows retains the Media Foundation
D3D11 texture through coalescing, reuses one resolution-matched staging
texture for readback, and then uploads NV12. The CPU RGBA presenter remains a
failure fallback. Stable wgpu does not currently expose portable IOSurface,
DMA-BUF, or D3D11 texture import; those imports are the remaining route to a
fully zero-copy presentation path.

Android also queues retained decoder outputs rather than converted pixels.
Only the newest MediaCodec `AImage` is mapped. Its Y/U/V planes are packed into
reused buffers, uploaded to persistent `R8Unorm` textures, and converted in a
GPU shader. Contiguous planes use row copies; interleaved chroma honors the
reported pixel stride. CPU RGBA conversion is only the failure fallback.

Browser builds retain the WebCodecs `VideoFrame` through the latest-only
event queue and use WebGL's native `VideoFrame` texture upload. There is no
`copyTo(RGBA)`, JavaScript pixel array, or decoded-frame copy across the WASM
boundary. The persistent texture is updated in place when resolution is
unchanged.

## Payload Routes And Audio

The Routes tab configures application outputs without changing protocol
parsing in `openipc-core`. A route has a stable numeric ID, a radio port under
the current Link ID, and one action:

| Action      | Behavior                                                               |
| ----------- | ---------------------------------------------------------------------- |
| Inspect     | Counts recovered payloads and bytes without parsing them.              |
| Log         | Adds a rate-limited size, sequence, and hexadecimal preview to Logs.   |
| Audio       | Selects an RTP payload type, decodes Opus with `ropus`, and plays PCM. |
| UDP forward | Sends the unchanged recovered payload to a native UDP destination.     |

UDP is unavailable in browsers and cannot be enabled there. The default routes
match Station: telemetry on `0x10`, mixed RTP audio on video port `0x00` using
payload type 98, and a disabled data route on `0x20`. A separate transmitter
audio profile can instead be selected with audio port `0x30`.

Routes using the same channel and key slot share one `PayloadPipeline`. Mixed
audio therefore shares video's WFB session, decryption, and FEC state; only the
matching RTP payload is copied into the audio action. Route topology, ports,
actions, and codec settings are locked while receiving and apply on the next
start. Output volume remains adjustable during reception and updates every
active audio route on the next packet on native, Android, and Web builds.

## Diagnostics

The Metrics tab keeps six operational signals over a rolling window: best-path
link score, unrecoverable post-FEC loss, the percentage of damaged primary
packets repaired by FEC, encoded video bitrate, delivered video FPS, and local
receive-through-decode processing latency. Loss and FEC percentages use deltas
from each sampling interval rather than lifetime counters. RSSI/SNR remain in
the video OSD and audio queue/counter details remain with route diagnostics.
Plots disable dragging, zooming, wheel navigation, and double-click reset; their
bounds follow the newest retained samples.
Diagnostics is divided into four views:

- **Pipeline health** follows USB initialization, 802.11 parsing, WFB recovery,
  RTP arrival, codec configuration, decoding, audio, and VPN state.
- **RTP** exposes payload/NAL type, sequence and timestamp, codec parameter-set
  state, malformed and unsupported packets, fragment gaps, config-wait drops,
  and reorder-buffer counters.
- **Stage latency** keeps rolling last, average, p95, maximum, and sample count
  values for USB wait, Realtek parsing, WFB/RTP, routes, decoder submission,
  hardware decode, and the complete receive batch.
- **Environment** reports target OS and architecture, runtime, renderer, USB
  API, media backend, H.264/H.265 availability and acceleration status, native
  surface support, logical processors, browser user agent where applicable,
  and the maximum resolution/FPS observed in the current session. Platform
  decoder APIs do not expose a reliable global maximum on every target, so
  observed limits are labeled as such.

The Logs tab owns capture verbosity: Low, Normal, High, or Very verbose. It also
has an independent minimum-level display filter and target/message search. Logs
remain bounded to avoid memory growth.

## GUI Settings

The GUI tab keeps appearance controls separate from receiver and codec
configuration. Settings apply immediately and persist through eframe storage:

- **Theme** selects Catppuccin Latte, Frappé, Macchiato, or Mocha. Latte is the
  light palette; the other three are dark palettes.
- **Interface scale** adjusts the complete interface from 75% to 150% in 5%
  increments without changing decoded video resolution.
- **Link telemetry overlay** controls the in-video RSSI/SNR/loss/FEC strip.
- **Controls panel visible** hides or restores the side/bottom controls. The
  header's **Controls** button always remains available to restore it.
- **Reset GUI settings** restores Macchiato, 100% scale, the telemetry overlay,
  and a visible controls panel.

These options are shared by desktop, Android, and browser builds. They do not
change radio initialization, WFB processing, decoder selection, or recording
output.

## Recording

Nebulus records the original encoded H.264/H.265 access units before decode and
muxes them into an `.mp4` file. It does not decode and re-encode the picture, so
recording does not reduce quality or add an encoder to the receive path. The
recorder arms immediately and begins at the next keyframe. Codec parameter sets
and dimensions are read from that access unit, while RTP timestamps supply the
MP4 sample timing.

The first enabled audio route is also recorded when it carries Opus RTP. The
recorder strips the RTP header and writes each raw Opus packet to the MP4 audio
track using Opus's fixed 48 kHz RTP clock and the route's channel count. Video
and audio each retain their own RTP timing; recording begins both timelines at the first media
captured after the keyframe because the receiver has no RTCP clock mapping.

Native muxing runs on a bounded worker so filesystem and container work stay out
of the USB/decode loop. Browser recordings are assembled and downloaded when
recording stops. Both targets cap retained encoded media at 512 MiB. Each
depacketized video access unit becomes exactly one MP4 sample, preserving
multi-slice pictures and RTP's 90 kHz timing.

## VPN / TUN

On macOS, Linux, Windows, and Android, the VPN tab can create a native
layer-three interface at `10.5.0.3/24` when RX starts. Downlink payloads recovered on radio
port `0x20` are length-decoded and written to TUN. Uplink IP packets are
length-prefixed, passed through `WfbTransmitter`, and injected by
`openipc-rtl88xx` on radio port `0xa0`. The transmitter refreshes its WFB
session packet once per second and drains at most 32 queued packets per receive
iteration to keep video work bounded.

VPN is unavailable in browser builds because browsers cannot create an OS
network interface or send arbitrary UDP/IP packets. Android uses a small
`VpnService` solely for user consent and TUN creation. Its descriptor is
duplicated into `rust-tun`; packet transport, WFB wrapping, and Realtek
injection remain in Rust.

Malformed USB aggregates and recoverable bulk-transfer failures are logged and
skipped. A stalled endpoint is cleared before reads resume. A disconnect or
fatal initialization/decode error moves the app to the failed state instead of
leaving the UI stuck in a connecting state.

## Validate

```sh
cargo test -p nebulus --all-targets
cargo clippy -p nebulus --all-targets --no-deps -- -D warnings
cargo check -p nebulus --target wasm32-unknown-unknown
cargo check -p nebulus --target aarch64-linux-android --lib
```

Cross-compilation validates target APIs. Actual radio initialization, codec
selection, pixel output, and adaptive-link transmission still need a supported
adapter and VTX for end-to-end validation.
