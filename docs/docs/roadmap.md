---
sidebar_position: 14
---

# Roadmap

This is a practical validation list, not a promise that every item will land in
the next release.

## Core Protocol

- Done: Realtek RX aggregate parsing.
- Done: OpenIPC/WFB frame filtering.
- Done: WFB session/data decryption.
- Done: Reed-Solomon FEC recovery.
- Done: RTP H.264/H.265 depacketization.
- Still useful: add captured hardware fixtures and cross-check them against
  aviateur/openipc-zig.

## Realtek Driver

- Done: register helpers around vendor request `0x05`.
- Done: endpoint discovery and OUT endpoint selection.
- Done: VID/PID targeting and bulk-OUT endpoint override for native and
  WebUSB paths.
- Done: firmware/table data checked into the Rust driver crate.
- Done: RTL8812/RTL8821 bring-up paths.
- Done: RTL8814 reserved-page/DDMA firmware path.
- Done: TX descriptor support for adaptive-link feedback.
- Done: runtime TX-mode/radiotap parsing for legacy, HT, and VHT transmit.
- Done: multi-transfer bulk-IN receive loops for native and desktop station
  paths, plus a batch WebUSB read API.
- Done: C2H packet handling, RTL8814 TX-status parsing, and optional
  corrupted-FCS packet retention for diagnostics.
- Done: thermal, false-alarm counters, RTL8814 queue-depth, BB-register, and
  BB-dbgport diagnostics.
- Done: EFUSE-backed MAC/RFE parsing and RFE-aware table loading.
- Done: EFUSE TX-power PG parsing, per-rate TXAGC writes, optional 8812A
  by-rate offsets, and regulatory limit table support.
- Done: devourer-style RTL8812/RTL8821/RTL8814 band-switch and RFE pinmux
  programming.
- Done: RTL8812 thermal power tracking, RTL8812 IQK, RTL8814 IQK, and the
  monitor-mode PHYDM DIG watchdog.
- Done: Jaguar3 RTL8812CU/EU and RTL8822CU/EU detection, firmware/tables,
  descriptors, narrowband and 40/80 MHz channel setup, 40-in-80 TX, EFUSE/RFE handling, DACK/IQK,
  RTL8822E TXGAPK/thermal tracking, coex maintenance, and clean shutdown.
- Done: Jaguar2 RTL8812BU/RTL8822BU cold-start firmware, MAC/USB, EFUSE/RFE,
  BB/RF tables, IQK/DIG, RX, and TX paths.
- Done: Jaguar1/2/3 explicit sounding, beamforming report detection, CSI tone
  masks, and NBI notch controls on native and WebUSB.
- Done: devourer-compatible runtime switches for skipping TX power, forcing or
  disabling IQK/TXGAPK, selecting the RTL8814 firmware path/chunk size, and
  testing the legacy RTL8814 TX descriptor shape.
- Done: diagnostics exposed as explicit tick/read APIs rather than hidden
  library pollers.
- Still needed: hardware smoke tests and trace comparisons per chip family.

## Native And Desktop

- Done: native receive loop.
- Done: RTP-over-UDP mirror.
- Done: Annex-B output.
- Done: native adaptive-link uplink.
- Done: Nebulus native/Android renderer with platform decoders and latest-frame
  presentation.
- Done: Nebulus profiles, editable HUD, preflight, automatic native recovery,
  idle native/WebUSB channel survey, and sanitized support bundles.
- Done: packet-level multiple-adapter receive diversity on desktop, Android,
  and WebUSB with one shared WFB/FEC/RTP pipeline.
- Done: legacy Tauri desktop station using the native Rust/nusb backend.

## Browser

- Done: WebUSB permission prompt in JavaScript.
- Done: Rust/WASM `nusb` WebUSB receive/transmit object.
- Done: shared Realtek monitor initialization through WebUSB.
- Done: WebCodecs playback, recording, persisted settings, and link HUD.
- Still useful: fallback playback path for browsers without WebCodecs support
  for the active codec.

## Release And Distribution

- Done: crates.io metadata and crate READMEs.
- Done: generated npm package metadata for `@openipc-rs/web`.
- Done: GitHub Actions validation, Cloudflare Pages deploys, npm trusted
  publishing, crates.io publishing, and Nebulus desktop/Android release jobs.
- Still useful: signed and notarized desktop releases.
- Still useful: a hardware test matrix that records adapter model, chip family,
  OS, browser, cold-start result, RX result, and adaptive-link TX result.
