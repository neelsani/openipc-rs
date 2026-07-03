# openipc-rs

Rust libraries and apps for OpenIPC FPV ground stations.

The repo contains the shared Rust packet pipeline, Realtek USB WiFi driver,
adaptive-link feedback, a WASM/WebUSB SDK, and Nebulus, the primary ground
station for desktop, Android, and the browser.

Nebulus supports packet-level receive diversity: several USB radios can feed
one low-latency WFB/FEC/RTP pipeline while the first valid packet copy wins.

## Repository

```text
crates/openipc-core       WFB, RTP, FEC, crypto, video and raw payload pipeline
crates/openipc-rtl88xx    Realtek rtl88xx USB WiFi driver
crates/openipc-video      Cross-platform low-latency H.264/H.265 decoding
crates/openipc-web        wasm-bindgen package for browser/WebUSB apps
apps/openipc-cli          Native command-line utilities
apps/wfb-rs               WFB-style Rust command-line tools
apps/nebulus              Main egui ground station for desktop, Android, and WebUSB
apps/openipc-station      Older React/Vite and Tauri station implementation
plugins/tauri-plugin-openipc-usb
                          Android USB and VPN permission bridge used by Station
docs                      Docusaurus documentation site
scripts                   cleanup helpers
```

## Packages

| Package           | Link                                                  | What it is                                                                                                                 |
| ----------------- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `openipc-core`    | [crates.io](https://crates.io/crates/openipc-core)    | Shared protocol code: WFB, FEC, RTP, crypto, video frame extraction, generic raw payload taps, adaptive-link packet logic. |
| `openipc-rtl88xx` | [crates.io](https://crates.io/crates/openipc-rtl88xx) | Realtek rtl88xx USB WiFi driver/HAL for monitor receive and adaptive-link transmit.                                        |
| `openipc-video`   | [crates.io](https://crates.io/crates/openipc-video)   | Hardware H.264/H.265 decoding for macOS, Linux, Windows, Android, and WebAssembly/WebCodecs.                               |
| `openipc-web`     | [crates.io](https://crates.io/crates/openipc-web)     | Rust/WASM bindings for browser WebUSB applications.                                                                        |
| `wfb-rs`          | [crates.io](https://crates.io/crates/wfb-rs)          | WFB-style command-line tools backed by the Rust userland Realtek driver.                                                   |
| `nebulus`         | [crates.io](https://crates.io/crates/nebulus)         | Primary egui ground station for desktop, Android, and browser/WebUSB.                                                      |
| `@openipc-rs/web` | [npm](https://www.npmjs.com/package/@openipc-rs/web)  | Generated npm package from `openipc-web`, with WASM, JS glue, and TypeScript definitions.                                  |

## Quick Start

Public links:

- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)
- Nebulus web app: [nebulus.openipc-rs.neels.dev](https://nebulus.openipc-rs.neels.dev)
- Legacy Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)

Test the core:

```sh
cargo test -p openipc-core
```

Run Nebulus on desktop:

```sh
cargo run -p nebulus --bin nebulus --release
```

Run Nebulus in the browser:

```sh
cd apps/nebulus
trunk serve --release --open
```

Build Nebulus for Android:

```sh
./scripts/android-nebulus-dev.sh
```

The script starts or reuses an emulator, waits for Android to boot, selects the
matching Rust target, installs Nebulus, and follows its timestamped Logcat
output. For an explicit AVD or a clean boot:

```sh
./scripts/android-nebulus-dev.sh --avd openipc_pixel_8_api36 --cold-boot
```

To build an APK without starting an emulator:

```sh
rustup target add aarch64-linux-android
cargo install cargo-apk2 --version 1.3.11 --locked
cargo apk2 build -p nebulus --lib --target aarch64-linux-android
```

On macOS with Homebrew OpenJDK and the default Android SDK path, this is the
environment the Android build expects if auto-detection does not pick it up:

```sh
export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME=$HOME/Library/Android/sdk
export ANDROID_SDK_ROOT=$ANDROID_HOME
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
export NDK_HOME=$ANDROID_NDK_HOME
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/bin:$PATH"
```

Nebulus asks Android's `UsbManager` for USB permission, passes the opened file
descriptor to `nusb::Device::from_fd`, and keeps all radio operations in Rust.
Its JNI bridge also handles Android `VpnService` consent for the optional VPN
route.

Use the native CLI:

```sh
cargo run -p openipc-cli -- list-supported
cargo run -p openipc-cli -- probe
cargo run -p openipc-cli -- recv --key gs.key --rf-channel 36 --adaptive-link
cargo build -p wfb-rs # builds wfb_rx, wfb_tx, wfb_keygen, wfb_tx_cmd, wfb_tun, wfb_rtsp
```

Build the WASM SDK:

```sh
bun run --cwd crates/openipc-web build
```

Clean generated files:

```sh
sh scripts/clean-generated.sh
```

## Status

The Rust protocol pipeline, Realtek driver path, WebUSB/WASM bindings, Nebulus
platform decoding and UI, adaptive-link feedback, native VPN tunnel bridging,
and CI/release automation are implemented. The driver tracks newer devourer
behavior for TX modes, multi-transfer RX, RTL8814 firmware bring-up,
RTL8812CU/EU and RTL8822CU/EU Jaguar3 descriptors, firmware, tables, EFUSE,
RFE, calibration, per-rate TX power, PHYDM, power tracking, IQK, C2H/TX-status
packets, and clean shutdown. Driver diagnostics are explicit APIs that apps
schedule themselves; the library does not create hidden polling threads.

More live adapter testing is still needed: cold-plug runs, register-trace
comparison, and browser WebUSB behavior across platforms.

## Docs

Use the hosted docs for the full guide:

- [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

Run them locally:

```sh
cd docs
bun install
bun run start
```

- [Getting Started](docs/docs/getting-started.md)
- [Architecture](docs/docs/architecture.md)
- [Native](docs/docs/native.md)
- [Web And WASM](docs/docs/web-wasm.md)
- [WASM SDK Usage](docs/docs/wasm-sdk.md)
- [Desktop Tauri](docs/docs/desktop-tauri.md)
- [Nebulus](docs/docs/nebulus.md)
- [Receive Diversity](docs/docs/receive-diversity.md)
- [Adaptive Link](docs/docs/adaptive-link.md)
- [CI/CD](docs/docs/ci-cd.md)
- [Publishing](docs/docs/publishing.md)

## Release

```sh
cargo install cargo-release git-cliff
cargo release patch --workspace
cargo release patch --workspace --execute
```

Local release commands only bump, commit, tag, and push. GitHub Actions
publishes crates, the npm package, desktop bundles, and web/docs deploys.
`git-cliff` updates [CHANGELOG.md](CHANGELOG.md) during release.

## CI/CD

Useful repository secrets:

- `CARGO_REGISTRY_TOKEN` for crates.io releases
- npm trusted publishing for `@openipc-rs/web`
- `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` for Nebulus, legacy
  Station, and docs deploys

## License

MIT
