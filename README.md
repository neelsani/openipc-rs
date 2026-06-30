# openipc-rs

Rust libraries and apps for OpenIPC FPV ground stations.

The repo contains the shared Rust packet pipeline, Realtek USB WiFi driver code,
adaptive-link feedback, a WASM/WebUSB SDK, and OpenIPC Station for browser and
desktop.

## Repository

```text
crates/openipc-core       WFB, RTP, FEC, crypto, video and raw payload pipeline
crates/openipc-rtl88xx    Realtek rtl88xx USB WiFi driver
crates/openipc-web        wasm-bindgen package for browser/WebUSB apps
apps/openipc-cli          Native command-line utilities
apps/openipc-station      React/Vite browser app and Tauri desktop app
plugins/tauri-plugin-openipc-usb
                          Android USB permission bridge used by Station
docs                      Docusaurus documentation site
scripts                   cleanup helpers
```

## Packages

| Package           | Link                                                  | What it is                                                                                                                 |
| ----------------- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `openipc-core`    | [crates.io](https://crates.io/crates/openipc-core)    | Shared protocol code: WFB, FEC, RTP, crypto, video frame extraction, generic raw payload taps, adaptive-link packet logic. |
| `openipc-rtl88xx` | [crates.io](https://crates.io/crates/openipc-rtl88xx) | Realtek rtl88xx USB WiFi driver/HAL for monitor receive and adaptive-link transmit.                                        |
| `openipc-web`     | [crates.io](https://crates.io/crates/openipc-web)     | Rust/WASM bindings for browser WebUSB applications.                                                                        |
| `@openipc-rs/web` | [npm](https://www.npmjs.com/package/@openipc-rs/web)  | Generated npm package from `openipc-web`, with WASM, JS glue, and TypeScript definitions.                                  |

## Quick Start

Public links:

- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)
- Station web app: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)

Test the core:

```sh
cargo test -p openipc-core
```

Run the browser/WebUSB station:

```sh
cd apps/openipc-station
bun install
bun run dev
```

Run the desktop station:

```sh
cd apps/openipc-station
bun install
bun run desktop:dev
```

Start the Tauri Android shell setup:

```sh
cd apps/openipc-station
bun run android:init
bun run android:dev
```

On macOS with Homebrew OpenJDK and the default Android SDK path, this is the
environment Tauri expects if auto-detection does not pick it up:

```sh
export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME=$HOME/Library/Android/sdk
export ANDROID_SDK_ROOT=$ANDROID_HOME
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
export NDK_HOME=$ANDROID_NDK_HOME
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/bin:$PATH"
```

Android USB discovery uses the local `tauri-plugin-openipc-usb` plugin. The
plugin asks for permission with Android `UsbManager`, opens the adapter, passes
the file descriptor to Rust, and the Rust backend continues through
`nusb::Device::from_fd`.

Use the native CLI:

```sh
cargo run -p openipc-cli -- list-supported
cargo run -p openipc-cli -- probe
cargo run -p openipc-cli -- recv --key gs.key --rf-channel 36 --adaptive-link
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

The Rust protocol pipeline, Realtek driver path, WebUSB/WASM bindings,
WebCodecs station UI, adaptive-link feedback, and CI/release automation are
implemented. The driver tracks newer devourer behavior for TX modes,
multi-transfer RX, RTL8814 firmware bring-up, EFUSE/per-rate TX power, PHYDM,
power tracking, IQK, C2H/TX-status packets, and hardware diagnostics. Driver
diagnostics are explicit APIs that apps schedule themselves; the library does
not create hidden polling threads.

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
- `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` for station/docs deploys

## License

MIT
