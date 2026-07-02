# Nebulus

Nebulus is a pure-Rust OpenIPC FPV ground station built with
[egui](https://github.com/emilk/egui). It opens a supported Realtek USB WiFi
adapter, reconstructs WFB video, decodes H.264 or H.265 with the operating
system's video API, and always presents the newest decoded frame.

It shares the same Rust application, protocol pipeline, settings, metrics, and
UI across desktop, Android, and the browser. Only USB access and video-surface
presentation are target-specific.

## Run On Desktop

```sh
cargo run -p nebulus --bin nebulus-desktop --release
```

On Linux, install the VA-API build dependencies listed in the
[`openipc-video` README](../../crates/openipc-video/README.md). The app uses
`nusb` directly; it does not need the Tauri backend or the devourer library.

## Run In A Browser

Install [Trunk](https://trunkrs.dev), then serve the app from a secure context
or localhost:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd apps/nebulus
trunk serve --release --open
```

Press **Start RX** to open the browser's WebUSB device picker. Browser builds
use the same Rust Realtek initialization and WFB/FEC/RTP pipeline as native
builds. WebCodecs performs H.264/H.265 decoding and WebGL uploads the retained
browser `VideoFrame` directly, without copying decoded pixels through WASM.

Create deployable files with:

```sh
cd apps/nebulus
trunk build --release
```

The output is written to `apps/nebulus/dist/` and is intentionally ignored by
Git.

## Build For Android

Nebulus uses `NativeActivity`; it does not need a Kotlin application shell.
Install the Android SDK, NDK, and a Rust APK packager, then build the library
target:

```sh
rustup target add aarch64-linux-android
cargo install cargo-apk2 --locked
cargo apk2 build -p nebulus --lib --target aarch64-linux-android
```

The manifest metadata requests `android.hardware.usb.host`. At runtime the
Rust JNI bridge uses Android `UsbManager` to request permission and open the
adapter, duplicates its file descriptor, and hands that descriptor to
`nusb::Device::from_fd`. All later USB control and bulk transfers still run
through `nusb` and `openipc-rtl88xx`.

Rust `log` output is registered with Android logcat under the `Nebulus` tag.

That command creates an installable APK with the normal Android debug key. Add
`[package.metadata.android.signing.release]` keystore settings outside source
control before adding `--release` for distribution builds.

## Data Path

```text
USB bulk IN
  -> openipc-rtl88xx RX descriptor parsing
  -> openipc-core 802.11 filtering, WFB crypto, FEC, RTP depacketizing
  -> openipc-video platform H.264/H.265 decoder
  -> newest decoded frame
  -> platform GPU presenter
```

Desktop and Android run USB, protocol, and decode work on a dedicated Rust
worker thread. The egui event loop only updates state and uploads the newest
presentable frame. The browser keeps WebUSB and WebCodecs on its local async
executor because browser handles are not `Send`. Rust/WASM submits compressed
access units directly to the browser WebCodecs API; application-written
JavaScript callbacks are not part of the receive path.

Enabled payload routes share the receiver's WFB runtimes whenever they use the
same channel and key slot. The default mixed-audio route therefore taps Opus
RTP payload type 98 from the video channel without decrypting or FEC-decoding
the packet twice. Opus decoding uses the pure-Rust `ropus` implementation.
CPAL feeds native and Android audio devices; browser builds schedule PCM with
Web Audio. Output volume can be adjusted while the receiver is running and is
applied to every active audio route without restarting RX.

Pending frame events use a one-frame replacement slot, while pending batch
metrics are merged. Rendering stalls therefore drop old pictures instead of
growing a delayed playback queue.

The default Metrics view focuses on six operational signals: best-path link
score, unrecoverable post-FEC loss, FEC recovery percentage, encoded video
bitrate, delivered video FPS, and local receive-through-decode processing
latency. Loss and recovery use deltas from each sampling window rather than
lifetime counters, so old link damage does not distort the current graph.

On macOS, Linux, and Windows, Nebulus keeps decoder-native frames in a
latest-only queue, uploads NV12 Y and UV planes into persistent wgpu textures,
and converts to RGB in the GPU shader. This avoids CPU color conversion and
reduces a 1080p upload from about 8.3 MB of RGBA to 3.1 MB of NV12. Linux maps
the newest VA-API DMA surface and Windows reads the newest D3D11 surface only
after stale frames have been discarded. Direct IOSurface, DMA-BUF, and D3D11
texture import remain optional future zero-copy optimizations.

Android coalesces retained MediaCodec `AImage` outputs before mapping the
selected frame. Reused packed Y/U/V buffers feed persistent wgpu textures and
the GPU performs color conversion. The browser keeps WebCodecs `VideoFrame`
objects inside Rust/WASM and uploads them directly into a persistent WebGL
texture; decoded pixel arrays never cross the WASM boundary.

## Included Controls

- Supported-adapter discovery and refresh
- RF channel, width, offset, link ID, epoch, and USB transfer size
- Built-in default `gs.key`, native file picker, and key-file drop
- Optional RTP reorder buffer
- Adaptive-link quality tracking, uplink feedback, and TX power override
- H.264/H.265 playback, video-only fullscreen, and link OSD
- Keyframe-aligned H.264/H.265 Annex-B recording without re-encoding
- Live bitrate, receive/decode/render FPS, RSSI, loss, and latency plots
- Pipeline-health, RTP, per-stage latency, and environment diagnostics
- Configurable inspect, rate-limited log, audio, and UDP payload routes
- Opus playback with volume, queue depth, and decoder/error metrics
- Native OpenIPC VPN/TUN bridging on macOS, Linux, Windows, and Android
- Catppuccin Macchiato theme and persisted receiver settings

UDP forwarding and VPN/TUN are native-only. Their controls are unavailable in
browser builds. Android requests `VpnService` consent and passes the resulting
TUN file descriptor into the same Rust bridge used by desktop targets.

Recording writes the original encoded access units instead of re-encoding
decoded pictures. It waits for an H.264/H.265 keyframe before writing, so the
result begins at a valid random-access point. Desktop builds stream to the
selected `.annexb`, `.h264`, or `.h265` file. Browser builds retain up to 512
MiB and download the matching elementary stream when recording stops. Audio is
not muxed into this elementary-stream format.

The VPN tab bridges recovered IP packets from radio port `0x20` into a native
L3 interface at `10.5.0.3/24`. Packets read from that interface are encrypted,
FEC-wrapped, injected through the userland Realtek driver, and transmitted on
radio port `0xa0`. Linux may require elevated network-device permissions;
Windows uses Wintun through `rust-tun`; Android uses its system `VpnService`.

Debug native and WASM builds also show **Codec mock**. It loops embedded,
pre-recorded 1920x1080 H.264 and 48 kHz Opus fixtures, packetizes both tracks
as RTP, and interleaves them on their media clocks. Native debug builds can
start it automatically for profiling:

```sh
NEBULUS_CODEC_MOCK=1 cargo run -p nebulus --bin nebulus-desktop
```

Video passes through the normal RTP depacketizer and `openipc-video`; audio passes through
the configured mixed-audio RTP tap, `ropus`, and the normal output queue. The
native build uses its platform video decoder and WASM uses WebCodecs decoding;
neither mock uses an encoder. Release builds omit the button and mock assets.

## Validate

```sh
cargo fmt --all --check
cargo clippy -p nebulus --all-targets --no-deps -- -D warnings
cargo test -p nebulus --all-targets
cargo check -p nebulus --target wasm32-unknown-unknown
cargo check -p nebulus --target aarch64-linux-android --lib
```

## License

MIT
