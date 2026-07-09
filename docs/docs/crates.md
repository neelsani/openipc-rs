---
sidebar_position: 4
---

# Crates And Packages

`openipc-rs` is split so applications can depend on the smallest useful layer.
The UI app uses all of these pieces, but library users often only need one or
two crates.

| Name              | Published As                                                                                                         | Use It For                                                                                                                                                                                                                               |
| ----------------- | -------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `openipc-core`    | [crates.io](https://crates.io/crates/openipc-core)                                                                   | Parsing Realtek RX aggregates, combining duplicate packets from multiple radios, decrypting WFB packets, recovering FEC blocks, routing raw payloads, depacketizing RTP into Annex-B frames, and creating adaptive-link uplink payloads. |
| `openipc-rtl88xx` | [crates.io](https://crates.io/crates/openipc-rtl88xx)                                                                | Realtek Jaguar1/2/3 userspace USB HAL: cold initialization, monitor RX, injection, wide/narrow channels, TX power, sounding, CSI/NBI controls, and diagnostics.                                                                          |
| `openipc-uplink`  | [crates.io](https://crates.io/crates/openipc-uplink)                                                                 | Userspace IPv4/UDP/TCP, bounded priority/aggregation scheduling, completion-aware WFB TX retries, virtual async TCP streams, WASM-compatible SSH, config transfer, and typed controls for existing OpenIPC VTX firmware.                    |
| `openipc-video`   | [crates.io](https://crates.io/crates/openipc-video)                                                                  | Turning Annex-B H.264/H.265 access units into retained decoder surfaces through VideoToolbox, VA-API, Media Foundation/D3D11, Android MediaCodec, or browser WebCodecs.                                                                  |
| `openipc-web`     | [crates.io](https://crates.io/crates/openipc-web)                                                                    | Rust/WASM bindings. Downstream Rust users normally do not call this directly unless they are building the npm package from source.                                                                                                       |
| `@openipc-rs/web` | [npm](https://www.npmjs.com/package/@openipc-rs/web)                                                                 | Browser SDK generated from `openipc-web`: WASM, JavaScript glue, and TypeScript definitions for WebUSB apps.                                                                                                                             |
| `openipc-cli`     | Not published                                                                                                        | Native command-line utilities under `apps/openipc-cli` for probes, capture decoding, receive-loop testing, Annex-B output, and RTP UDP mirroring.                                                                                        |
| `wfb-rs`          | [crates.io](https://crates.io/crates/wfb-rs)                                                                         | WFB-style command-line tools including `wfb_rx`, `wfb_tx`, `wfb_keygen`, `wfb_tx_cmd`, `wfb_tun`, and `wfb_rtsp` over the Rust userland driver.                                                                                          |
| `nebulus`         | [crates.io](https://crates.io/crates/nebulus) and [GitHub Releases](https://github.com/neelsani/openipc-rs/releases) | The primary pure-Rust egui station for desktop, Android, and browsers, using the platform decoder crate and a shared receiver UI/runtime.                                                                                                |

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
`DiversityCombiner` can merge valid copies from several adapters before one
shared `ReceiverRuntime`; see [Receive Diversity](./receive-diversity.md).

Add `openipc-uplink` when the application must reach the VTX through WFB tunnel
ports `0x20`/`0xa0`. It does not require a platform TCP socket or TUN device, so
the same smoltcp UDP/TCP and SSH path works in native, Android, and browser
builds.

Add `openipc-video` when an app also needs local playback. It consumes
the `DepacketizedFrame` values emitted by `openipc-core` and returns retained
CoreVideo, DMA-backed VA-API, D3D11, Android hardware-buffer, or browser
`VideoFrame` surfaces. Decoded output uses a latest-frame mailbox so a slow
renderer does not build a queue of stale FPV frames.

Use `@openipc-rs/web` if you are writing a browser app. The npm package owns the
WASM loading boundary and exposes TypeScript-friendly classes such as
`OpenIpcReceiver` and `WebUsbRealtekDevice`.

Use `openipc-cli` when you want the existing command-line probes or a reference
native receive loop. It is an app package, so libraries should depend on
`openipc-core` and `openipc-rtl88xx` instead.

Use `wfb-rs` when you specifically want WFB-ng-shaped command-line tools backed
by the Rust userland Realtek driver. The binaries are Rust rewrites of the
receive, transmit, key, control, tunnel, and RTSP helper roles: they are not
wrappers around upstream WFB-ng. In particular, `wfb_rx` and `wfb_tx` do not
use Linux pcap/PF_PACKET monitor interfaces. They open supported Realtek USB
adapters directly through `nusb` and `openipc-rtl88xx`, so the main radio tools
are intended for Linux, macOS, and Windows. `wfb_tun` is Unix-only because it
depends on a TUN interface.

Use Nebulus when you want the complete ground station or a Rust-native
application reference. Its desktop and Android builds run the USB receiver and
decoder on worker threads. Its WASM build keeps WebUSB/WFB recovery in the app
WASM and instantiates the internal `nebulus-decode-worker` binary target as
isolated RTP and WebCodecs workers connected by a direct `MessageChannel`. Only
a latest transferable `VideoFrame` returns to egui for presentation. Settings,
routes, metrics, recording, adaptive link, VPN controls where supported, and the
egui UI are shared.

## Versioning

The repo uses one lockstep SemVer version across the Rust crates, the npm
package metadata, Nebulus, and the docs package. `cargo release` updates the
Rust manifests, uses `bun pm version --cwd ...` for JavaScript package
versions, and refreshes `bun.lock` files before it creates the `v*` tag. CI
publishes the crates.io packages and the npm package from that tag. Internal
workspace packages marked `publish = false` are versioned but not uploaded to
crates.io.

## Dependency Notes

The workspace imports `nusb-webusb` as `nusb`:

```toml
nusb = { package = "nusb-webusb", version = "0.2.3" }
```

That keeps the code written against the normal `nusb` path while using the
WebUSB-capable fork until upstream WebUSB support lands in the main `nusb`
crate.
