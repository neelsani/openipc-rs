---
sidebar_position: 4
---

# Crates And Packages

`openipc-rs` is split so applications can depend on the smallest useful layer.
The UI app uses all of these pieces, but library users often only need one or
two crates.

| Name                       | Published As                                          | Use It For                                                                                                                                                                                                           |
| -------------------------- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `openipc-core`             | [crates.io](https://crates.io/crates/openipc-core)    | Parsing Realtek RX aggregates, filtering OpenIPC/WFB frames, decrypting WFB packets, recovering FEC blocks, routing raw payloads, depacketizing RTP into Annex-B frames, and creating adaptive-link uplink payloads. |
| `openipc-rtl88xx`          | [crates.io](https://crates.io/crates/openipc-rtl88xx) | Opening supported Realtek USB WiFi adapters, running monitor-mode initialization, reading bulk-IN transfers, sending adaptive-link bulk-OUT packets, and setting TX power overrides.                                 |
| `openipc-web`              | [crates.io](https://crates.io/crates/openipc-web)     | Rust/WASM bindings. Downstream Rust users normally do not call this directly unless they are building the npm package from source.                                                                                   |
| `@openipc-rs/web`          | [npm](https://www.npmjs.com/package/@openipc-rs/web)  | Browser SDK generated from `openipc-web`: WASM, JavaScript glue, and TypeScript definitions for WebUSB apps.                                                                                                         |
| `openipc-cli`              | Not published                                         | Native command-line utilities under `apps/openipc-cli` for probes, capture decoding, receive-loop testing, Annex-B output, and optional RTP UDP mirroring.                                                           |
| `openipc-station`          | Not published                                         | The React/Vite station app and Tauri desktop shell under `apps/openipc-station`, including WebCodecs playback, route management, adaptive-link controls, and native VPN tunnel bridging where supported.             |
| `tauri-plugin-openipc-usb` | Not published                                         | Local Tauri plugin used by Station's Android build to request USB/VPN permission through Android APIs and hand file descriptors to the Rust backend.                                                                 |

## Choosing A Layer

Use `openipc-core` if you already have captured USB transfers, 802.11 frames, or
RTP packets and only need protocol reconstruction. `ReceiverRuntime` is the
normal receive helper: it owns route fanout, WFB session/FEC state, and the RTP
depacketizer for the configured video route. Use the lower-level
`PayloadPipeline` only when you want to stop exactly at recovered WFB payload
bytes. The crate does not parse MAVLink, MSP, CRSF, or other telemetry formats
for you.

Use `openipc-core` plus `openipc-rtl88xx` if you are writing a native Rust
receiver, recorder, diagnostic app, or hardware validation tool.

Use `@openipc-rs/web` if you are writing a browser app. The npm package owns the
WASM loading boundary and exposes TypeScript-friendly classes such as
`OpenIpcReceiver` and `WebUsbRealtekDevice`.

Use `openipc-cli` when you want the existing command-line probes or a reference
native receive loop. It is an app package, so libraries should depend on
`openipc-core` and `openipc-rtl88xx` instead.

`tauri-plugin-openipc-usb` is an app-support crate, not a public SDK. It exists
because Android apps cannot enumerate USB devices from the normal Linux sysfs
path that `nusb` uses on desktop. The plugin owns the small Kotlin layer that
asks Android for permission and opens the adapter; the driver still runs in Rust
after Station receives the file descriptor.

## Versioning

The repo uses one lockstep SemVer version across the Rust crates, the npm
package metadata, the station app, and the docs package. `cargo release` updates
the Rust manifests, including the local Tauri plugin, uses
`bun pm version --cwd ...` for JavaScript package versions, and refreshes
`bun.lock` files before it creates the `v*` tag. CI publishes the crates.io
packages and the npm package from that tag. Crates marked `publish = false`,
such as the desktop shell and Android plugin, are versioned with the workspace
but are not uploaded to crates.io.

## Dependency Notes

The workspace imports `nusb-webusb` as `nusb`:

```toml
nusb = { package = "nusb-webusb", version = "0.2.3" }
```

That keeps the code written against the normal `nusb` path while using the
WebUSB-capable fork until upstream WebUSB support lands in the main `nusb`
crate.
