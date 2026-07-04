---
sidebar_position: 2
---

# Getting Started

Start here to receive an OpenIPC stream with Nebulus. The repository also
contains reusable Rust crates and a WASM SDK for building another ground
station.

Public builds:

- Nebulus: [nebulus.openipc-rs.neels.dev](https://nebulus.openipc-rs.neels.dev)
- Downloads: [GitHub Releases](https://github.com/neelsani/openipc-rs/releases)
- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)
- Legacy Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)

## Choose A Build

| Goal                                     | Recommended path                                                                  |
| ---------------------------------------- | --------------------------------------------------------------------------------- |
| Try without installing                   | Open Nebulus in a Chromium-based browser with WebUSB.                             |
| Run on macOS, Linux, Windows, or Android | Download the matching Nebulus artifact from GitHub Releases.                      |
| Develop Nebulus                          | Clone the repository and use Cargo or Trunk.                                      |
| Build another app                        | Start with [Crates And Packages](./crates.md) or [WASM SDK Usage](./wasm-sdk.md). |

Clone the repository for source builds:

```sh
git clone https://github.com/neelsani/openipc-rs
cd openipc-rs
```

## Live Receiver Requirements

- A Realtek adapter listed by the [driver device table](./realtek-driver.md#supported-device-ids).
- An OpenIPC transmitter using the same RF channel, channel width, Link ID, and
  WFB key.
- The matching `gs.key`. Nebulus includes the normal OpenIPC default key, but a
  custom transmitter key must be loaded with **Open file**.
- Permission to claim the USB device on the host operating system.

Browser mode also requires HTTPS or `localhost`. WebUSB is browser-gated, so
the picker must be opened by **Start RX** for the first radio or **Add adapter**
for each additional diversity receiver.

For source builds, install Rust stable. Trunk is needed for the browser app.
Bun is only needed for the documentation site, legacy Station, and generated
npm SDK workflows.

## Receive A Stream

1. Connect the Realtek USB adapter directly to the host when possible.
2. Start Nebulus, select the adapter, or apply a saved receiver profile.
3. Set the RF channel and width to match the air unit. If the channel is
   unknown, use the idle **Scan channels** window. Leave channel offset and
   Link ID at their defaults unless the VTX was configured differently.
4. Load the matching `gs.key` if the transmitter does not use the default key.
5. Enable adaptive link, payload routes, or VPN only when the air-side setup
   supports them. They are not required for video reception.
6. Run **Preflight** to catch invalid keys, routes, and unavailable platform
   features before hardware initialization.
7. Press **Start RX** and accept the USB permission prompt if one appears.

For packet-level receive diversity, connect additional supported adapters,
choose one primary receiver, and enable the others under **Diversity
receivers**. All radios use the same RF settings and key. Browser users must
press **Add adapter** once per device before selecting it.

The healthy startup sequence is: adapter initialized, USB transfers arriving,
802.11 packets accepted, WFB session established, payload recovered, RTP
packets received, codec configuration found, keyframe received, and decoder
active. The **Diagnostics** tab reports each boundary separately.

Seeing “waiting for an IDR frame” means USB, radio, WFB, and RTP have already
produced encoded video, but the decoder has not yet received a complete random
access frame and its SPS/PPS or VPS/SPS/PPS configuration. If that state
persists, inspect RTP diagnostics, codec preference, keyframe interval, and
packet loss. See [Debugging And Metrics](./debugging-metrics.md).

## Run Nebulus On Desktop

```sh
cargo run -p nebulus --bin nebulus --release
```

Desktop Nebulus uses native `nusb`, a platform hardware decoder, and the same
Rust protocol pipeline and egui UI as Android and browser targets.

Linux requires access to the USB device and a VA-API render node. Windows must
bind the adapter to a user-space USB driver such as WinUSB. macOS may request
USB accessory permission. Decoder requirements are covered in
[Platform Video Decoding](./native-video.md).

The Windows release installer includes Wintun for the optional VPN feature. If
Nebulus was installed with Cargo, open **Settings → VPN / tunnel** and use
**Install Wintun**; the app downloads and verifies the official signed package.
Video reception and adaptive-link feedback work without Wintun.

## Run Nebulus In A Browser

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cd apps/nebulus
trunk serve --release --open
```

The browser build uses WebUSB, the shared Rust Realtek HAL and protocol stack,
WebCodecs, and the egui web renderer. Build static deployment files with:

```sh
trunk build --release
```

The output is `apps/nebulus/dist`. Serve it over HTTPS; opening `index.html`
from a `file:` URL cannot provide WebUSB.

## Build Nebulus For Android

Android uses `UsbManager` for discovery and permission, then passes the opened
file descriptor to `nusb`; USB control and bulk transfers still run through the
Rust driver.

```sh
./scripts/android-nebulus-dev.sh
```

That command starts or reuses an emulator, waits for it to boot, chooses the
correct Rust target for its ABI, installs Nebulus, and follows Logcat. To build
an APK without running it:

```sh
rustup target add aarch64-linux-android
cargo install cargo-apk2 --version 1.3.11 --locked
cargo apk2 build -p nebulus --lib --target aarch64-linux-android
```

The build needs Java 17, the Android SDK, and Android NDK 27.2. Install the APK
produced under `target/` with `adb install -r <path-to-apk>`.

## Development Checks

After installing the host platform dependencies, run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo check -p openipc-web --target wasm32-unknown-unknown
```

For a faster protocol-only smoke test:

```sh
cargo test -p openipc-core
```

## Build The WASM SDK

Nebulus does not need the generated npm package. Build this package only when
developing a JavaScript/TypeScript application against `openipc-web`:

```sh
bun run --cwd crates/openipc-web build
```

The generated package is written to `crates/openipc-web/pkg` and is not checked
into git. It contains the `.wasm`, JavaScript glue, TypeScript definitions, npm
metadata, README, and license.

## Run The Native CLI

List supported adapter IDs and probe connected hardware:

```sh
cargo run -p openipc-cli -- list-supported
cargo run -p openipc-cli -- probe
```

Receive video, write Annex-B frames, and send adaptive-link feedback:

```sh
cargo run -p openipc-cli -- recv \
  --key gs.key \
  --rf-channel 161 \
  --rf-width 20 \
  --adaptive-link \
  --out video.annexb
```

## Build This Documentation Site

```sh
cd docs
bun install
bun run build
```

The static site is written to `docs/build`.
