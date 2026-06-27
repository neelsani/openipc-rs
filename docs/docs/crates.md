---
sidebar_position: 4
---

# Crates And Packages

`openipc-rs` is split so applications can depend on the smallest useful layer.
The UI app uses all of these pieces, but library users often only need one or
two crates.

| Name | Published As | Use It For |
| --- | --- | --- |
| `openipc-core` | [crates.io](https://crates.io/crates/openipc-core) | Parsing Realtek RX aggregates, filtering OpenIPC/WFB frames, decrypting WFB packets, recovering FEC blocks, extracting RTP, building Annex-B H.264/H.265 frames, and creating adaptive-link uplink payloads. |
| `openipc-rtl88xx` | [crates.io](https://crates.io/crates/openipc-rtl88xx) | Opening supported Realtek USB WiFi adapters, running monitor-mode initialization, reading bulk-IN transfers, sending adaptive-link bulk-OUT packets, and setting TX power overrides. |
| `openipc-native` | [crates.io](https://crates.io/crates/openipc-native) | CLI utilities and a thin native-facing library adapter. This is useful for probes, capture decoding, and reference receive loops. |
| `openipc-web` | [crates.io](https://crates.io/crates/openipc-web) | Rust/WASM bindings. Downstream Rust users normally do not call this directly unless they are building the npm package from source. |
| `@openipc-rs/web` | [npm](https://www.npmjs.com/package/@openipc-rs/web) | Browser SDK generated from `openipc-web`: WASM, JavaScript glue, and TypeScript definitions for WebUSB apps. |
| `openipc-station` | Not published | The React/Vite station app and Tauri desktop shell under `apps/openipc-station`. |

## Choosing A Layer

Use `openipc-core` if you already have captured USB transfers, 802.11 frames, or
RTP packets and only need protocol reconstruction.

Use `openipc-core` plus `openipc-rtl88xx` if you are writing a native Rust
receiver, recorder, diagnostic app, or hardware validation tool.

Use `@openipc-rs/web` if you are writing a browser app. The npm package owns the
WASM loading boundary and exposes TypeScript-friendly classes such as
`OpenIpcReceiver` and `WebUsbRealtekDevice`.

Use `openipc-native` when you want the existing CLI commands or a stable crate
name that re-exports the native driver API.

## Versioning

The repo uses one lockstep SemVer version across the Rust crates, the npm
package metadata, the station app, and the docs package. `cargo release` updates
the Rust manifests, uses `bun pm version --cwd ...` for JavaScript package
versions, and refreshes `bun.lock` files before it creates the `v*` tag. CI
publishes crates.io packages and the npm package from that tag.

## Dependency Notes

The workspace imports `nusb-webusb` as `nusb`:

```toml
nusb = { package = "nusb-webusb", version = "0.2.3" }
```

That keeps the code written against the normal `nusb` path while using the
WebUSB-capable fork until upstream WebUSB support lands in the main `nusb`
crate.
