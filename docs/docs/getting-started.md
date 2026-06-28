---
sidebar_position: 2
---

# Getting Started

Start here if you want to run the station, build the WASM package, or check the
Rust crates locally.

Public builds:

- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)
- Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)

Clone the repository normally:

```sh
git clone https://github.com/neelsani/openipc-rs
cd openipc-rs
```

## Prerequisites

- Rust stable with `cargo`.
- Bun 1.3.14 or newer for JavaScript dependencies, scripts, and npm package
  publishing.
- A supported Realtek rtl88xx USB WiFi adapter for live receive.
- A ground-station key file such as `gs.key` for encrypted OpenIPC streams.

For browser mode you also need a secure context: HTTPS or `localhost`. WebUSB is
browser-gated, so the device picker must be opened from a user click.

## Check The Rust Workspace

Run the same basic checks you will see in CI:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo check -p openipc-web --target wasm32-unknown-unknown
```

For a faster first smoke test:

```sh
cargo test -p openipc-core
```

## Build The WASM SDK

```sh
bun run --cwd crates/openipc-web build
```

The generated package is written to `crates/openipc-web/pkg`. It is not checked
into git. The generated package contains the `.wasm`, JavaScript glue,
TypeScript definitions, npm metadata, README, and license file.

## Run The Browser Station

```sh
cd apps/openipc-station
bun install
bun run dev
```

Open the Vite URL printed by Bun. In browser mode the Connect button uses
`navigator.usb.requestDevice`, then Rust/WASM takes over the Realtek
initialization and receive loop.

Build the deployable browser/WebUSB version:

```sh
bun run build
```

The output goes to `apps/openipc-station/dist`. CI deploys that build from
`master`.

## Run The Desktop Station

```sh
cd apps/openipc-station
bun install
bun run desktop:dev
```

The desktop app uses the same React UI as the browser build, but USB receive and
transmit run through native Rust instead of browser WebUSB.

`desktop:dev` starts a local Vite server because the Tauri WebView loads the app
from Vite during development. Seeing `http://127.0.0.1:5173/` in the terminal is
normal; the Tauri window is the desktop app.

## Run The Native CLI

List adapters:

```sh
cargo run -p openipc-native -- list-supported
```

Probe the first supported adapter without monitor initialization:

```sh
cargo run -p openipc-native -- probe
```

Receive video, write Annex-B frames, and send adaptive-link feedback:

```sh
cargo run -p openipc-native -- recv \
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
