---
sidebar_position: 2
---

# Getting Started

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

Deploy that build to Cloudflare Workers:

```sh
npm run deploy:worker
```

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
