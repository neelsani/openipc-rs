# Nebulus

Nebulus is a pure-Rust OpenIPC FPV ground station built with
[egui](https://github.com/emilk/egui). It opens one or more supported Realtek
USB WiFi adapters, reconstructs WFB video, decodes H.264 or H.265 with the
operating system's video API, and always presents the newest decoded frame.
Native builds can instead receive an already-recovered RTP stream from UDP.

It shares the same Rust application, protocol pipeline, settings, metrics, and
UI across desktop, Android, and the browser. USB access, native UDP sockets,
and video-surface presentation are target-specific.

Nebulus is the primary ground station distributed by this repository. Tagged
releases include Linux x64/arm64 executables, macOS Apple Silicon/Intel disk
images, Windows x64/arm64 installers, and one universal Android APK. The hosted
browser build is available at
[nebulus.openipc-rs.neels.dev](https://nebulus.openipc-rs.neels.dev).
The source package is published at
[crates.io/crates/nebulus](https://crates.io/crates/nebulus).

## Run On Desktop

```sh
cargo run -p nebulus --bin nebulus --release
```

Or install the published package from crates.io:

```sh
cargo install nebulus
nebulus
```

On Linux, install the VA-API build dependencies listed in the
[`openipc-video` README](../../crates/openipc-video/README.md). The app uses
`nusb` directly and does not depend on the devourer library.

macOS and Windows builds add a Nebulus system-tray icon. Its menu can show or
hide the window, start or stop RX, enable VPN for the next receiver start, open
the full VPN panel, and quit. macOS displays it in the menu bar; Windows uses
the notification area and may place it under the overflow arrow.

After monitor initialization succeeds, Settings shows a connected-receiver
summary with the actual USB VID:PID, probed Realtek family, RF path layout, cut
revision, USB speed, selected bulk endpoints, initialization result, firmware
download status, and active RF/Link ID configuration. The summary is cleared
when the receiver disconnects.

### Receive RTP From UDP

In **Settings → Receiver**, select **UDP RTP** and choose a local bind address
and port. The default is `0.0.0.0:5600`. Each UDP datagram must contain one
complete RTP packet. H.264 and H.265 payloads use the same reorder,
depacketizer, decoder, metrics, OSD, and MP4 recording path as USB reception;
Opus carried on the configured mixed-audio RTP payload type uses the normal
audio route.

UDP input is available on desktop and Android. It receives RTP after the radio
transport, so it intentionally bypasses Realtek initialization, 802.11/WFB
filtering, decryption, and FEC. Adaptive-link uplink, VPN/TUN, diversity,
channel scanning, and routes on non-video radio ports therefore require the
USB source. Browsers cannot bind arbitrary UDP sockets and continue to use
WebUSB.

The GUI tab contains presentation-only settings. It offers Catppuccin Latte,
Frappé, Macchiato, and Mocha themes, a persistent 75–150% interface scale,
an editable video OSD, control-panel visibility, and a one-click GUI reset.
Each indicator can be hidden or dragged to normalized video coordinates, so
one layout remains usable at different window sizes. Its icon, label, value,
status coloring, background, size, and opacity can be configured independently.
Supported indicators can optionally include a mini graph with a configurable
history window and dimensions; RSSI can optionally include signal bars. Graphs
and bars are off by default. Changes apply immediately on desktop, Android,
and the browser. OSD layouts have their own named profiles. **Duplicate** starts
a new layout from the current one, edits auto-save to the selected layout, and
the same OSD profile can be reused with any receiver profile.

Settings includes named receiver profiles. A profile snapshots the primary and
diversity adapters, radio, Link ID, keys, routes, telemetry policy, audio, VPN,
decoder choices, and a reference to a reusable OSD profile; GUI appearance
stays global. Use **Save current** after changing a profile. **Run preflight**
checks the selected adapters, keys, radio values, routes, decoder state, VPN,
and adaptive-link configuration before RX starts.

**Settings → Preset packs** installs and exports versioned community JSON packs.
Packs can contain OSD, theme, route, telemetry, and performance components, but
their schema cannot represent keys, USB identities, radio configuration, local
paths, or concrete UDP destinations. Installation shows a component preview;
versions are pinned and never update a receiver profile automatically. Packs
can be opened from a local file or HTTPS URL, and static registry indexes can be
browsed directly. GitHub blob links are accepted and checksum-pinned registry
entries are verified before preview. See the
[preset documentation](https://openipc-rs.neels.dev/docs/presets) and bundled
schema under `apps/nebulus/presets`.

**Scan channels** opens an idle-only survey. The Rust driver initializes the
adapter once, fast-retunes it across same-band channels, and reports traffic,
WFB frames, RSSI, SNR, EVM, observed bitrate, measured retune time, and whether
each hop used the fast path or a full fallback. The adapter is shut down after
the survey. A scan and normal RX never run at the same time.

## Run In A Browser

Install [Trunk](https://trunkrs.dev), then serve the app from a secure context
or localhost:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd apps/nebulus
trunk serve --release --open
```

Use **Add adapter** to authorize each WebUSB radio, select a primary and any
diversity receivers, then press **Start RX**. With no authorized selection,
**Start RX** opens the browser's WebUSB device picker for one adapter. Browser builds
use the same Rust Realtek initialization and WFB/FEC/RTP pipeline as native
builds. WebCodecs performs H.264/H.265 decoding and WebGL uploads the retained
browser `VideoFrame` directly, without copying decoded pixels through WASM.
Recovered RTP batches are transferred to a Rust/WASM RTP worker. Complete
access units then cross a direct `MessageChannel` to a separate WebCodecs
worker, so a slow decoder cannot stall RTP ingest. Both handoff queues are
bounded and discard dependent frames until a keyframe after overload. Only the
newest transferable `VideoFrame` crosses back for presentation. The Metrics
tab reports receive, decoder-output, and presentation rates separately.

The worker is the feature-gated `nebulus-decode-worker` binary inside this
same Cargo package, not another app or crate. Trunk enables it automatically;
normal native builds and `cargo install nebulus` only build the main binary.

Create deployable files with:

```sh
cd apps/nebulus
trunk build --release
```

The output is written to `apps/nebulus/dist/` and is intentionally ignored by
Git.

## Build For Android

Nebulus uses `NativeActivity`; it does not need a Kotlin application shell.
After installing the Android SDK, NDK, and `cargo-apk2`, run from the repository
root:

```sh
./scripts/android-nebulus-dev.sh
```

This starts or reuses an emulator, waits for boot completion, selects the Rust
target matching the AVD ABI, builds and installs Nebulus, and follows Logcat.
Use `--help` for AVD, release, cold-boot, and no-Logcat options.

To only build the library target:

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

Rust `log` output is mirrored to standard Android application output and the
in-app Logs tab.

Android settings use an app-private eframe RON file under the activity's
internal data directory. Receiver profiles, OSD profiles, the selected key,
routes, and GUI preferences therefore survive process death and device restart without
requiring storage permission. Support bundles use Android's document picker,
so the user chooses where the ZIP is written.

That command creates an installable APK with the normal Android debug key. Add
`[package.metadata.android.signing.release]` keystore settings outside source
control before adding `--release` for distribution builds.

## Data Path

```text
USB: bulk IN from each selected adapter
  -> openipc-rtl88xx RX descriptor parsing and optional diversity
  -> openipc-core 802.11 filtering, WFB crypto, and FEC

UDP: one already-recovered RTP packet per datagram

Either source -> openipc-core route fanout
     -> video RTP depacketizing -> openipc-video H.264/H.265 decoder
     -> telemetry route -> MAVLink/MSP/CRSF decoder -> video OSD
     -> audio route -> Opus decoder -> audio device
  -> newest decoded video frame -> platform GPU presenter
```

Desktop and Android keep one bulk-IN capture worker per adapter and one shared
protocol/decode worker. The egui event loop only updates state and uploads the
newest presentable frame. The browser keeps WebUSB and WFB recovery on the app
executor, then transfers recovered RTP batches to an RTP worker. A second
worker owns WebCodecs, and a direct `MessageChannel` carries complete access
units between them; application-written JavaScript callbacks are not part of
the receive path.

Enabled payload routes share the receiver's WFB runtimes whenever they use the
same channel and key slot. The default mixed-audio route therefore taps Opus
RTP payload type 98 from the video channel without decrypting or FEC-decoding
the packet twice. Opus decoding uses the pure-Rust `ropus` implementation.
CPAL feeds native and Android audio devices; browser builds schedule PCM with
Web Audio. Output volume can be adjusted while the receiver is running and is
applied to every active audio route without restarting RX.

The default telemetry route reads raw payloads from OpenIPC radio port `0x10`.
Its **Telemetry to OSD** action can auto-detect checksum-valid MAVLink, MSP, or
CRSF frames, or be pinned to one format. The radio port remains editable for
custom VTX layouts. Decoding belongs to Nebulus rather than `openipc-core`:
the shared core still returns protocol-neutral payload bytes, so applications
using the libraries can choose different telemetry parsers. Nebulus normalizes
common flight values into one OSD state and hides stale telemetry indicators
after three seconds by default.

MAVLink support uses the `mavlink` crate's generated Common dialect. The crate
provides current message layouts, enums, CRC extras, and MAVLink 2 payload
handling; Nebulus only keeps the bounded incremental framer needed for payloads
that may be split across WFB packets and maps selected messages into its
protocol-neutral OSD state.

The **Telemetry** tab shows the detected protocol, source identity, frame age,
and accepted/rejected/filtered counters. It also controls the stale-data
timeout, MAVLink system/component filters and signing policy, MSP version and
direction filters, and CRSF device-address filtering. MAVLink signing accepts a
32-byte binary key or a file containing 64 hexadecimal digits. **Verify signed**
authenticates signed packets while allowing unsigned traffic; **Require signed**
also rejects MAVLink 1, unsigned MAVLink 2, invalid signatures, stale signing
timestamps, and replays. The MAVLink key is separate from the WFB `gs.key`.

Pending frame events use a one-frame replacement slot. USB buffers are
re-armed before decode or route work, encoded frames move into the decoder
without a playback copy, and diagnostic batches are emitted at 20 Hz. Rendering
stalls therefore drop old pictures instead of growing a delayed playback queue.
Codec configuration and keyframe detection share one allocation-free Annex-B
scan. The macOS path also converts the normal uniquely owned Annex-B buffer to
VideoToolbox length prefixes in place.

Adaptive-link/VPN transmit and Jaguar3 maintenance do not run on the native RX
thread. Native and browser transmit share `UplinkEngine`: bounded control/TUN
queues, control-first scheduling, same-tick IP aggregation, atomic FEC-batch
admission, and completion-driven bounded retries. Browser Jaguar3 maintenance
runs as a separate local async task. Sustained TUN overload is rejected at its
queue boundary instead of delaying video or silently consuming control traffic.

The default Metrics view focuses on six operational signals: best-path link
score, unrecoverable post-FEC loss, FEC recovery percentage, encoded video
bitrate, delivered video FPS, and local receive-through-decode processing
latency. Loss and recovery use deltas from each sampling window rather than
lifetime counters, so old link damage does not distort the current graph.

On macOS, Linux, and Windows, Nebulus keeps decoder-native frames in a
latest-only queue and converts NV12 to RGB in the GPU shader. macOS imports
VideoToolbox IOSurface planes directly into Metal/wgpu without a per-frame
pixel upload. Linux maps the newest VA-API DMA surface and Windows reads the
newest D3D11 surface only after stale frames have been discarded, then uploads
NV12 into persistent textures. Direct DMA-BUF and D3D11 texture import remain
platform-specific future optimizations.

Android sends MediaCodec output directly to a `SurfaceTexture` backed by an
external OpenGL ES texture. The egui Glow paint callback latches the newest
decoder image, so decoded planes are never mapped or copied through Rust. The
browser keeps WebCodecs `VideoFrame`
objects inside Rust/WASM and uploads them directly into a persistent WebGL
texture; decoded pixel arrays never cross the WASM boundary.

Desktop builds request non-vsynced wgpu presentation with one frame of surface
latency. Android requests its fastest same-resolution display mode, disables
egui vsync, raises the receive thread priority, and configures MediaCodec for
low latency. Once decoding starts, the receiver polls platform output at most
2 ms apart instead of tying presentation to the cadence of USB completions.
Android allows eight MediaCodec frames in flight to accommodate normal pipeline
depth without becoming a playback queue. Nebulus continuously presents the
decoder's latest SurfaceTexture while receiving so worker-event coalescing
cannot cap Android rendering below the display cadence. Output remains
latest-only.
Native audio requests a 256-frame output buffer and keeps no more than 20 ms of
queued PCM.

## Included Controls

- Supported-adapter discovery and refresh
- Native direct H.264/H.265 RTP reception from a configurable UDP listener
- Packet-level receive diversity across multiple adapters of the same or mixed
  supported Realtek families
- RF channel, width, offset, link ID, epoch, and USB transfer size
- Built-in default `gs.key`, native file picker, and key-file drop
- Optional RTP reorder buffer
- Adaptive-link quality tracking, uplink feedback, and TX power override
- H.264/H.265 playback, video-only fullscreen, and a configurable video OSD
- Drag-and-drop OSD editor for link, battery, GPS, flight-mode, motion, attitude, and status indicators
- Named receiver profiles shared by desktop, Android, and browser builds
- Preflight validation and native automatic reconnect with bounded backoff
- Idle channel survey with per-channel WFB traffic and RSSI
- Keyframe-aligned H.264/H.265 MP4 recording without re-encoding
- Live bitrate, receive/decode/render FPS, RSSI, loss, and latency plots
- Pipeline-health, RTP, per-stage latency, and environment diagnostics
- Level-controlled library logging with target/text filtering and trace capture
- Configurable inspect, rate-limited log, telemetry-to-OSD, audio, and UDP payload routes
- Opus playback with volume, queue depth, and decoder/error metrics
- Native OpenIPC VPN/TUN bridging on macOS, Linux, Windows, and Android
- Catppuccin Macchiato theme and persisted receiver settings
- Sanitized ZIP support bundle with persistent driver initialization traces,
  first-receive milestones, parser histograms, platform state, and session logs

Direct UDP input, UDP forwarding, and VPN/TUN are native-only. Their controls
are unavailable in browser builds. Android requests `VpnService` consent and passes the resulting
TUN file descriptor into the same Rust bridge used by desktop targets.
Automatic receiver recovery is native-only because starting a WebUSB device
selection requires a browser user gesture. Native recovery begins only after a
receiver had reached Ready or Receiving; an initially invalid configuration
does not create a retry loop. Delays increase from one to eight seconds and
reset after 30 seconds of stable reception.

Recording writes the original encoded access units and the first enabled Opus
audio route into MP4 without re-encoding. It waits for an H.264/H.265 keyframe,
so the result begins at a valid random-access point. Video and audio timing come
from their RTP clocks. Native muxing runs on a bounded recorder worker; browser
recordings download when stopped. Both targets cap retained encoded media at
512 MiB. On desktop, **Record** never opens a file dialog: it writes a unique
timestamped MP4 to the folder selected under Settings → Recording. The default
is `Nebulus` inside the user's Videos directory, with Documents or the home
directory as fallbacks. Android uses app-owned storage without prompting.

The **VPN / tunnel** section under Settings bridges recovered IP packets from
radio port `0x20` into a native L3 interface at `10.5.0.3/24`. Packets read from
that interface enter the shared `UserspaceNetwork` raw-IP queue, which applies
OpenIPC tunnel framing before they are encrypted, FEC-wrapped, injected through
the userland Realtek driver, and transmitted on radio port `0xa0`. Linux may
require elevated network-device permissions;
Windows uses Wintun through `rust-tun`; Android uses its system `VpnService`.

The Windows release installer includes the matching `wintun.dll`. A
`cargo install nebulus` installation detects when the DLL is absent and shows
**Install Wintun** in Settings. Nebulus downloads the official signed 0.14.1
archive, verifies its published SHA-256, and installs the architecture-matched
DLL under `%LOCALAPPDATA%\Nebulus\wintun\0.14.1`. The installer runs outside
the receiver thread. Adaptive-link uses the same userspace network and WFB
transmitter but does not require Wintun or an enabled VPN route.

Debug native and WASM builds also show **H.264 mock** or **H.265 mock**, based
on the codec preference under Setup → Media. `Auto` selects H.265 to match the
usual OpenIPC stream. The adjacent arrow selects 720p, 1080p, or 4K and a
30/60/120/240 FPS RTP cadence. These are explicit development controls and are
identical on every target. The mock loops pre-recorded 60 FPS video
and 48 kHz Opus fixtures, packetizes both tracks as RTP, and interleaves them on
their media clocks. Higher selected rates replay access units faster to stress
the normal depacketizer and decoder path. The large video fixtures are excluded
from the crates.io package. Source builds read them locally; packaged desktop
and Android debug builds download the selected fixture once and cache it, while
the browser uses its HTTP cache. SHA-256 verification happens before playback.
Native debug builds can start it automatically for profiling:

```sh
NEBULUS_CODEC_MOCK=1 cargo run -p nebulus --bin nebulus
```

Video passes through the normal RTP depacketizer and `openipc-video`; audio passes through
the configured mixed-audio RTP tap, `ropus`, and the normal output queue. The
native build uses its platform video decoder and WASM uses WebCodecs decoding;
neither mock uses an encoder. Release builds omit the button and mock loader.

## Validate

```sh
cargo fmt --all --check
cargo clippy -p nebulus --all-targets --no-deps -- -D warnings
cargo test -p nebulus --all-targets
cargo check -p nebulus --target wasm32-unknown-unknown
cargo check -p nebulus --bin nebulus-decode-worker --features web-decode-worker --target wasm32-unknown-unknown
cargo check -p nebulus --target aarch64-linux-android --lib
```

## License

MIT
