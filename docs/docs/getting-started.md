---
sidebar_position: 2
---

# Getting Started

Hosted builds:

- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)
- Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)

Clone the repository normally:

```sh
git clone https://github.com/neelsani/openipc-rs
cd openipc-rs
```

## Test The Core

```sh
cargo test -p openipc-core
```

## Build The WASM SDK

```sh
npm --prefix crates/openipc-web run build
```

The generated package is written to `crates/openipc-web/pkg`. It is not checked
into git.

## Run OpenIPC Station In The Browser

```sh
cd apps/openipc-station
npm install
npm run dev
```

Build the deployable browser/WebUSB version:

```sh
npm run build
```

The output goes to `apps/openipc-station/dist`. CI deploys that build from
`master`.

## Run OpenIPC Station As A Desktop App

```sh
cd apps/openipc-station
npm install
npm run desktop:dev
```

The desktop app uses the same React UI as the browser build, but USB receive and
transmit run through native Rust instead of browser WebUSB.

## Build This Documentation Site

```sh
cd docs
npm install
npm run build
```
