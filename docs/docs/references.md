---
sidebar_position: 13
---

# References

These are the upstream projects and docs used as reference material while
designing `openipc-rs`.

## OpenIPC

- [OpenIPC documentation](https://docs.openipc.org/)
- [OpenIPC GitHub organization](https://github.com/OpenIPC)

## Receiver And Ground Station References

- [devourer](https://github.com/OpenIPC/devourer): Realtek 11ac userspace
  driver used as the primary hardware behavior reference.
- [aviateur](https://github.com/OpenIPC/aviateur): cross-platform OpenIPC FPV
  ground station used as the application and adaptive-link reference.
- [openipc-zig](https://github.com/neelsani/openipc-zig): browser/WebUSB and
  Zig/WASM experiment used as the browser feasibility reference.
- [PixelPilot](https://github.com/OpenIPC/PixelPilot): Android ground-station
  app used as a packaging and playback reference.
- [adaptive-link](https://github.com/OpenIPC/adaptive-link): standalone
  adaptive-link transmitter/receiver tools used to cross-check feedback format
  and behavior.

## Rust And Browser Dependencies

- [nusb docs.rs](https://docs.rs/nusb/latest/nusb/)
- [nusb-webusb docs.rs](https://docs.rs/nusb-webusb/latest/nusb/)
- [kevinmehall/nusb](https://github.com/kevinmehall/nusb)
- [WebUSB API](https://developer.mozilla.org/en-US/docs/Web/API/USB)
- [WebCodecs API](https://developer.mozilla.org/en-US/docs/Web/API/WebCodecs_API)

## Published Packages

- [openipc-core on crates.io](https://crates.io/crates/openipc-core)
- [openipc-rtl88xx on crates.io](https://crates.io/crates/openipc-rtl88xx)
- [openipc-web on crates.io](https://crates.io/crates/openipc-web)
- [@openipc-rs/web on npm](https://www.npmjs.com/package/@openipc-rs/web)
