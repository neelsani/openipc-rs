# openipc-rs

Rust receiver building blocks for OpenIPC FPV ground-station applications.

This repository is a new implementation, created beside the existing
`devourer`, `aviateur`, and `openipc-zig` projects. Those projects were used as
references for USB behavior, Realtek RX descriptors, WFB packet layout, RTP
handling, and browser constraints. The code here is not a direct translation.

## Current Shape

```text
openipc-rs/
  crates/openipc-core/    shared packet, WFB RX/TX, Realtek RX/TX, adaptive link, and RTP logic
  crates/openipc-rtl88xx/ shared async Rust Realtek rtl88xx USB/HAL driver
  crates/openipc-native/  native CLI utilities over the Rust driver
  crates/openipc-web/     wasm-bindgen wrapper around the shared core
  apps/openipc-station/   shared Vite React/TypeScript station UI for browser and Tauri desktop
  apps/openipc-station/src-tauri/
                           Tauri desktop shell and native USB receive backend
  docs/                   Docusaurus documentation site and Cloudflare Worker config
  scripts/                build, clean, and release-version helpers
```

The shared Rust protocol path now includes:

- Realtek 88xxAU RX aggregate parsing from 24-byte RX descriptors, including
  PHY-status RSSI/SNR extraction.
- Realtek monitor-injection USB TX descriptor construction from radiotap +
  802.11 packets.
- OpenIPC/WFB IEEE 802.11 frame filtering by mirrored `57:42:<channel_id>` MAC fields.
- WFB session-key handling with `crypto_box`-compatible NaCl box decryption.
- WFB data decryption with libsodium-compatible legacy 8-byte-nonce
  ChaCha20-Poly1305.
- Vandermonde Reed-Solomon FEC recovery compatible with the reference WFB
  implementation.
- RTP parsing and H.264/H.265 depacketization to Annex-B frames.
- WFB uplink session creation, legacy ChaCha20-Poly1305 data encryption,
  Reed-Solomon parity generation, and radiotap/802.11 packet wrapping for
  OpenIPC uplink radio ports.
- Aviateur-compatible adaptive-link feedback generation with rolling RSSI/SNR
  and FEC windows, `fec_change` bump/decay, IDR code updates, and UDP-over-IPv4
  payload wrapping.
- Native USB discovery/open/claim and bulk-IN streaming using the
  WebUSB-capable `nusb-webusb` package.
- Shared async Realtek HAL for native and browser/WebUSB: RTL8812/RTL8821
  firmware download, LLT/page setup, MAC/BB/RF table loading, monitor filters,
  channel/bandwidth setup, RX bulk reads, and TX bulk writes.
- WASM bindings that expose the same encrypted receiver/uplink pipeline and a
  Rust/nusb WebUSB receive/transmit object to JavaScript.

## Status

Implemented and testable:

- `openipc-core` protocol and parser unit tests.
- End-to-end WFB session/data decrypt tests.
- WFB FEC recovery tests.
- Native USB device listing, probing, descriptor-driven endpoint selection, and
  bulk-transfer receive loop.
- Realtek rtl88xx driver crate with shared async vendor-control register
  access, checked-in firmware/table data, 8812/8821 firmware download,
  post-firmware queue/RX aggregation setup, BB/RF table loading, monitor-mode
  filters, and RF channel selection for both native and WebUSB builds. The
  reference projects are not build dependencies.
- Native stream output as Annex-B video frames and optional RTP-over-UDP mirror.
- Native adaptive-link uplink on `recv --adaptive-link`: receives video, records
  Realtek RSSI/SNR and WFB FEC counters, periodically sends encrypted WFB
  feedback packets on radio port 160 through the Realtek bulk-OUT endpoint.
- Manual adaptive-link uplink TX power override through devourer-style 0..63
  per-rate TXAGC programming for RTL8812/RTL8821 and RTL8814, available from
  native `--alink-tx-power` and the browser uplink TX power control.
- Browser/WASM bindings for key loading, Realtek RX aggregate parsing, encrypted
  WFB processing, adaptive-link uplink packet generation, shared Realtek
  monitor initialization through WebUSB, bulk IN/OUT, and typed Annex-B frame
  objects with codec, WebCodecs codec string, keyframe, and RTP timestamp
  metadata.
- Vite React/TypeScript OpenIPC Station app with Tailwind/shadcn-style local UI
  primitives, an aviateur-like Wi-Fi/Settings control panel, WebUSB
  controls, key loading, runtime metrics, persisted settings, adaptive-link
  controls, WebCodecs H.264/H.265 playback, canvas-stream WebM recording, and a
  link-quality HUD. Advanced receiver/debug controls stay collapsed by default.
  It
  passes the browser-granted `UsbDevice` into the `nusb` WASM backend,
  reads bulk transfers from Rust, sends adaptive feedback through Rust/WebUSB
  when enabled, and feeds RX bytes into Rust/WASM video reconstruction. The
  WebCodecs path consumes the structured Rust/WASM frame metadata instead of
  rediscovering RTP/video properties in React.
- OpenIPC Station serves `apps/openipc-station/public/gs.key` as the default keypair. If a
  user-selected keypair has been saved in browser localStorage, that key wins;
  otherwise the app preloads `/gs.key` on startup.
- Tauri desktop support using the same React UI. The desktop backend runs the
  native Rust Realtek driver directly, emits Annex-B video batches and link
  metrics to the UI over Tauri events, and uses the same WebCodecs/canvas HUD
  path for display and recording. The browser runtime remains WASM + WebUSB.

Remaining hardware boundary:

- RTL8812/RTL8821 cold initialization is implemented in Rust but still needs
  live adapter/on-air validation. No supported adapter was visible in this
  environment during verification.
- RTL8814 cold firmware download is implemented through the shared Rust HAL
  using the devourer-style reserved-page/DDMA path, including the 3081 MCU boot
  handshake and 8814 queue/LLT setup. It still needs live RTL8814AU validation
  and register-trace comparison against devourer on real hardware.
- Browser cold-start support uses the same shared async Realtek HAL as native.
  It still needs live adapter/on-air validation, and browser behavior depends on
  what the `nusb` WebUSB backend can legally do through the WebUSB API.
- Browser video playback depends on the user's browser WebCodecs support for
  the incoming codec. H.264 is broadly available; H.265 support is browser and
  OS dependent.
- The browser UI mirrors aviateur's receiver controls where Web APIs permit it:
  device selection, channel list, 20/40 MHz channel width, keypair selection,
  fullscreen, dark mode, adaptive-link enablement, and 1..40 mW manual uplink
  TX power. Aviateur's raw UDP local listener and RTP forwarding settings are
  intentionally omitted from the browser UI because browser builds cannot bind
  or forward raw UDP; use native `openipc-native --rtp-udp` for that workflow.
- Manual adaptive-link uplink TX power override is implemented in the shared
  HAL, but still needs live adapter/on-air validation across chip families.
- Native CLI display is not bundled. The CLI exposes RTP over UDP and Annex-B
  output, which can be consumed by `ffplay`, GStreamer, or a ground-station UI.
  The Tauri desktop app is the bundled native UI path.

## Build And Test

Clone the repository normally:

```sh
git clone https://github.com/neelsani/openipc-rs
```

Core tests do not require USB hardware or external system libraries:

```sh
cd openipc-rs
cargo test -p openipc-core
```

List USB devices visible to `nusb`:

```sh
cargo run -p openipc-native -- list
cargo run -p openipc-native -- list-supported
```

Probe the first supported Realtek adapter:

```sh
cargo run -p openipc-native -- probe
OPENIPC_RS_SKIP_RESET=1 cargo run -p openipc-native -- probe
```

Parse a captured Realtek RX bulk transfer:

```sh
cargo run -p openipc-native -- parse-aggregate capture.bin
```

Decode a captured transfer through WFB decrypt/FEC/RTP and write Annex-B frames:

```sh
cargo run -p openipc-native -- decode-aggregate capture.bin --key gs.key --out video.annexb
```

Receive from the first supported adapter and expose RTP on localhost:

```sh
cargo run -p openipc-native -- recv --key gs.key --rf-channel 36 --rtp-udp 127.0.0.1:5600 --out video.annexb
```

Receive video and send adaptive-link feedback to the air unit:

```sh
cargo run -p openipc-native -- recv --key gs.key --rf-channel 36 --adaptive-link --out video.annexb
```

The adaptive uplink uses the same 64-byte key file by default and interprets it
as ground-station secret key + air-side public key for the TX direction. Use
`--alink-key` if your setup stores a separate uplink key file. `--alink-fec`
defaults to `1:5`, matching aviateur's feedback path.

The native `recv` command opens the adapter through `nusb`, claims interface 0,
discovers the bulk endpoints from descriptors, initializes supported Realtek
hardware for monitor-mode receive, keeps several 32 KiB reads pending, parses
Realtek aggregates, decrypts WFB, recovers FEC, and emits RTP/video events.
Use `--no-init` only for diagnostics with an adapter that is already streaming.

Run the browser app:

```sh
cd apps/openipc-station
npm install
npm run dev
```

`npm run dev` first runs `npm run build:wasm`, which generates
`crates/openipc-web/pkg` from Rust and then starts Vite. For a production web
build, use:

```sh
cd apps/openipc-station
npm run build
```

The production web build writes `apps/openipc-station/dist`. That directory is
the deployable browser/WebUSB OpenIPC Station app and includes the generated
Rust/WASM package. The app is configured as a Cloudflare Worker with static
assets:

```sh
cd apps/openipc-station
npm run deploy:worker
```

CI deploys the station Worker on pushes to `master` when
`CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` repository secrets are set.

Run the desktop app during development:

```sh
cd apps/openipc-station
npm install
npm run desktop:dev
```

Use `desktop:dev` to open the Tauri window. The `dev:desktop` script only
starts the desktop-mode Vite dev server and is intended for Tauri's
`beforeDevCommand`.

Build-check the desktop app without producing installers:

```sh
cd apps/openipc-station
npm run desktop:check
```

Build desktop bundles/installers:

```sh
cd apps/openipc-station
npm run desktop:build
```

`desktop:dev`, `desktop:check`, and `desktop:build` reuse the same Vite UI.
In desktop mode the UI detects Tauri, calls native Rust commands for device
listing/connection/start/stop, and receives encoded H.264/H.265 Annex-B frame
batches from the native backend. The backend opens the adapter through `nusb`,
initializes monitor mode through `openipc-rtl88xx`, runs the shared
WFB/RTP/adaptive-link pipeline, and sends those frames to the UI for WebCodecs
playback. On macOS, full DMG bundling may require the host packaging tools and
signing setup; `desktop:check` is the fast source-level verification path.

Run the documentation site locally:

```sh
cd docs
npm install
npm run start
```

Build the documentation site:

```sh
cd docs
npm run build
```

The docs site is a Docusaurus app configured as a Cloudflare Worker with static
assets. CI deploys it from `docs/build` on pushes to `master` when
`CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` repository secrets are set.

The WASM SDK build is owned by `crates/openipc-web/package.json`:

```sh
npm --prefix crates/openipc-web run build
```

That npm script reads the `wasm-bindgen` version from `Cargo.lock`, installs the
matching `wasm-bindgen-cli` into `.cargo-tools`, and generates the package with:

```sh
cargo build -p openipc-web --target wasm32-unknown-unknown --release
.cargo-tools/bin/wasm-bindgen target/wasm32-unknown-unknown/release/openipc_web.wasm --target web --out-dir crates/openipc-web/pkg --typescript
```

It then writes publishable npm metadata from
`crates/openipc-web/package.json`, `crates/openipc-web/README.md`, and the root
`LICENSE` into `crates/openipc-web/pkg`. That generated `pkg` directory is the
publishable npm package for `@openipc-rs/web`.

To force `wasm-pack` instead, set `OPENIPC_RS_USE_WASM_PACK=1`.

The repo includes `.cargo/config.toml` with
`--cfg=web_sys_unstable_apis` for `wasm32-unknown-unknown`; this is required
because WebUSB bindings in `web-sys` are still gated as unstable APIs.

Generated artifacts are intentionally not checked in. Recreate
`crates/openipc-web/pkg`, `target`, `.cargo-tools`, root-level npm package
tarballs (`*.tgz`), `apps/openipc-station/node_modules`,
`apps/openipc-station/dist`, `apps/openipc-station/.wrangler`,
`apps/openipc-station/src-tauri/gen`, `docs/node_modules`, `docs/.docusaurus`,
and `docs/build` with the build commands above. Commit source, lockfiles, Tauri
source/config, docs source, and build tooling; do not commit the compiled
`.wasm` unless you are publishing a separate release artifact or npm package
from CI. The workspace depends on the published `nusb-webusb` package through
Cargo's package alias mechanism:

```toml
nusb = { package = "nusb-webusb", version = "0.2.3" }
```

To inspect or publish the WASM SDK package after building:

```sh
npm pack --dry-run crates/openipc-web/pkg
npm publish crates/openipc-web/pkg --access public
```

CI publishes `@openipc-rs/web` with npm trusted publishing from
`.github/workflows/ci.yml`, so the release workflow uses GitHub OIDC instead of
a long-lived npm automation token.

## Versioning and Release Tags

This repo currently uses one lockstep SemVer version for the Rust crates, WASM
npm metadata, station app, Tauri shell, and docs site. Use `cargo release` with
the workspace `release.toml` to update the shared version, create the release
commit, and create the annotated Git tag:

```sh
cargo install cargo-release
```

```sh
cargo release patch --workspace
cargo release patch --workspace --execute --no-publish
```

The cargo-release configuration updates:

- `crates/*/Cargo.toml`
- `crates/openipc-web/package.json`
- `apps/openipc-station/package.json`
- `apps/openipc-station/src-tauri/Cargo.toml`
- `docs/package.json`

To preview without applying changes, or to push the tag after the release:

```sh
cargo release patch --workspace
cargo release patch --workspace --execute --no-publish --push
```

Tags are named with a leading `v`, such as `v0.2.0`. CI/CD publishing should be
triggered from these tags, not from every `master` push. Tag builds publish
crates.io packages, publish `openipc-web` to npm, and upload Tauri desktop
bundles to the GitHub Release for Linux x64/arm64, macOS Apple Silicon/Intel,
and Windows x64/arm64 when release auth is configured:

- `CARGO_REGISTRY_TOKEN`

For npm, configure trusted publishing for `@openipc-rs/web` on npmjs.com:

- Publisher: GitHub Actions
- Organization/user: `neelsani`
- Repository: `openipc-rs`
- Workflow filename: `ci.yml`
- Allowed action: `npm publish`

Cloudflare deploys from `master` still require:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

To remove generated artifacts and return the workspace to source-only state:

```sh
sh scripts/clean-generated.sh
# or, from apps/openipc-station:
npm run clean
```

## Design Notes

`openipc-core` owns the protocol-critical pieces: frame validation, WFB
session/data decryption, FEC recovery, RTP depacketization, and event emission.
USB, browser permissions, and video display stay at the edges.

`openipc-rtl88xx` is organized around one async Realtek HAL shared by native and
browser builds. USB transport, firmware download, table loading, MAC setup, RF
channel setup, and timing are split into small modules. Native keeps blocking
compatibility methods at the API edge; browser builds use the same async methods
through the `nusb` WebUSB backend.

`openipc-native` uses the `nusb-webusb` package. The reference driver uses
libusb directly; this project keeps the same conceptual operations, but exposes
them through safe Rust methods.

`openipc-web` uses the same `nusb-webusb` dependency. Browser JavaScript still
owns the user-gesture permission prompt (`navigator.usb.requestDevice`), then
passes the resulting `UsbDevice` to `WebUsbRealtekDevice.fromWebUsbDevice`.
From there Rust/nusb claims interface 0, discovers endpoints, runs the shared
Realtek monitor initialization, passes bulk-IN transfer bytes through the shared
receiver pipeline, returns typed `OpenIpcVideoFrame` objects to React, and
accepts adaptive-link radiotap packets for bulk-OUT transmit.

`apps/openipc-station/src-tauri` is the native desktop adapter for the same UI. It exposes
small Tauri commands for list/connect/start/stop, keeps USB and WFB/RTP work in
native Rust, and sends batched frame/metric events to React. This avoids relying
on WebUSB in a desktop WebView while preserving the browser app as a true
WASM/WebUSB build.

See [docs/docs/reference-notes.md](docs/docs/reference-notes.md) for the
exploration summary and [docs/docs/roadmap.md](docs/docs/roadmap.md) for the
next implementation milestones. The same content is available through the
Docusaurus site in `docs/`.
