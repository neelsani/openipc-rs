---
sidebar_position: 10
---

# Devourer Parity Audit

This page tracks the driver-level audit against the current `devourer` tree.
The goal is practical parity: `openipc-rs` should issue the same class of USB,
firmware, MAC, RF, RX, and TX operations while keeping a Rust-native API and
using `nusb` instead of libusb directly.

The reference commits used for this pass were:

```text
OpenIPC/devourer f542b06 Add RTL8812EU / RTL8822EU (rtl8822e) support (#124)
OpenIPC/devourer 55f0649 Split Jaguar1 + compile-time per-chip selection (#125)
OpenIPC/devourer 94d2fa9 Fix rtl8822c 2.4 GHz RX deafness (#138)
OpenIPC/devourer e926b47 Jaguar1 TX-power parity (#139)
OpenIPC/devourer 7cd094a Current audited master
```

## Audit Plan

The risky parts of this rewrite are the places where small byte/register
differences do not fail at compile time. This is the checklist used for each
chip family:

| Area                 | What to compare                                                                   | Rust location                                            | Failure mode                                   |
| -------------------- | --------------------------------------------------------------------------------- | -------------------------------------------------------- | ---------------------------------------------- |
| USB discovery        | VID/PID table, interface claim, endpoint selection, endpoint override             | `openipc-rtl88xx::SUPPORTED_DEVICES`, `RealtekDevice`    | wrong adapter, wrong bulk OUT endpoint, no TX  |
| Control transfer ABI | Realtek vendor request, register width, endian order                              | `async_driver.rs`, `device.rs`                           | reads look plausible but write wrong registers |
| Firmware load        | power-on state, chunking, reserved-page/DDMA flow, firmware-ready polls           | `async_firmware*.rs`, `async_jaguar3.rs`                 | warm-start works, cold-plug fails              |
| MAC setup            | queue/FIFO, DMA, RX engine, WMAC options                                          | `async_mac.rs`, `async_jaguar3.rs`                       | no bulk-IN frames or FIFO stalls               |
| EFUSE/RFE            | logical-map decoding, RFE pinmux/table choices, TX power data                     | `async_efuse.rs`, `async_tables.rs`, `async_tx_power.rs` | works on one dongle revision, fails on another |
| PHY/RF tables        | table data, conditional opcodes, pseudo-delay entries, write order                | `rtl_data.rs`, `data/*`, table loaders                   | no RX sensitivity, wrong band, unstable TX     |
| Channel/BW           | RF18 band bits, SCO, DFIR, 5/10 MHz reclock, 40/80 fallback behavior              | `async_radio.rs`, `async_jaguar3.rs`                     | tuned to the wrong channel or sample rate      |
| RX descriptors       | field offsets, packet/C2H split, drvinfo/shift offset, 8-byte aggregate alignment | `openipc-core::realtek`                                  | corrupted 802.11 frames or missed C2H reports  |
| TX descriptors       | radiotap RATE/MCS/VHT parsing, 5 GHz CCK clamp, descriptor checksum               | `openipc-rtl88xx::tx`                                    | bulk OUT succeeds but nothing goes on-air      |
| Runtime polling      | coex keepalive, thermal power tracking, PHYDM/watchdog hooks                      | app-owned RX loop plus explicit driver APIs              | sustained TX degrades or stops                 |
| Shutdown             | stop TRX, close RX filter, power-off sequence                                     | `shutdown_monitor*`                                      | adapter wedges until unplug/replug             |

## Current Mapping

```mermaid
flowchart TD
    A["devourer WiFiDriver"] --> B["RtlJaguarDevice / RtlJaguar3Device"]
    B --> C["RtlUsbAdapter libusb control/bulk"]
    B --> D["HAL firmware, MAC, RF, IQK, coex"]
    B --> E["FrameParser RX/TX descriptors"]

    A2["openipc-rs app"] --> B2["openipc-rtl88xx RealtekDevice"]
    B2 --> C2["nusb native / WebUSB transport"]
    B2 --> D2["Rust HAL modules"]
    E2["openipc-core realtek parser"] --> A2
    B2 --> E2
```

`openipc-rs` deliberately does not copy devourer's class layout. The important
boundaries are:

- `openipc-rtl88xx` owns USB, registers, firmware, RF, TX descriptor building,
  diagnostics, and explicit runtime hooks.
- `openipc-core` owns byte-level RX aggregate parsing plus WFB/RTP/FEC payload
  handling.
- apps own scheduling: receive loops, periodic diagnostics, WebUSB UI timing,
  and Tauri worker threads.

## Executed Checks

### Jaguar3 RTL8812CU/EU and RTL8822CU/EU

The current Rust driver includes the new devourer Jaguar3 work:

- RTL8822C PIDs `0bda:c812`, `0bda:c82c`, and `0bda:c82e`.
- RTL8822E PIDs `0bda:881c`, `0bda:a81a`, `0bda:e822`, and `0bda:a82a`, plus
  ambiguous `0bda:8812`, `0bda:881a`, and `0bda:881b` devices selected by the
  authoritative `SYS_CFG2` chip ID (`0x17`; RTL8822C is `0x13`).
- 24-byte Jaguar3 RX descriptor layout with packet length, CRC/ICV flags,
  driver-info size, shift size, RX rate, and C2H report bit.
- 48-byte Jaguar3 TX descriptor layout, including the 16-bit descriptor
  checksum algorithm from `cal_txdesc_chksum_8822c`.
- Firmware, MAC, USB, BB/AGC/RF, RFK, DACK, IQK, beamforming setup, monitor RX
  filters, TX path enable, WiFi-only coex setup, H2C keepalives, and thermal
  power/LCK tracking.
- 5 MHz and 10 MHz narrowband retiming on top of 20 MHz channel tuning.
- 40/80 MHz requests degrade to the 20 MHz path for Jaguar3, matching
  devourer's current behavior rather than pretending those modes are fully
  ported.
- TX power override writes the same flat TXAGC reference class used by
  devourer for monitor inject/adaptive-link experiments.
- Clean shutdown now mirrors devourer `Stop()`: halt TRX through `CR`, close
  `RCR`, then run the 8822C card-disable power sequence.
- RTL8822C channel changes now include the vendor 3-wire reset bracket, gated
  RXBB write, RF18 read-modify-write, per-band CCK/OFDM AGC tables, CCK RX-IQ
  control, and force-anapar writes. Devourer validated this sequence as the fix
  for zero CCA and no receive on 2.4 GHz. These writes are gated away from the
  separate RTL8822E channel path.

The RTL8822E-specific path additionally includes:

- the 199,928-byte NIC firmware and exact AGC, PHY, PHY-PG, radio A/B, and RFK
  tables from `f542b06`; a full-array comparison against devourer passes for all
  eight arrays,
- V1 physical EFUSE reads with software power-cut and burst mode, the two-byte
  `0x3X` packed-map format, thermal baselines, and per-channel/path TX power,
- RFE fallback 21 for unprogrammed bare modules, PA-bias trim, RFE 21-24 antenna
  switching, GPIO/pad pinmux, and band-specific TX scaling/shaping,
- chip-specific DACK, IQK, TXGAPK, DPK bypass, and 5 GHz thermal compensation,
- the 7-bit Jaguar3 TXAGC range (`0..=127`) instead of Jaguar1's `0..=63`.

Regression tests now lock several high-risk bytes:

- Jaguar3 RX descriptor field positions and payload offset after drvinfo/shift.
- C2H report detection through descriptor word2 bit 28.
- Jaguar3 TX descriptor field offsets.
- 8822C TX descriptor checksum recomputation.
- 5 GHz CCK-rate requests clamped to OFDM before descriptor encoding.
- RTL8822E chip-ID overrides for shared PIDs, EFUSE block decoding, 5 GHz
  channel groups, TX-power differential sign extension, TXGAPK gain arithmetic,
  and reference-data boundaries.

### Jaguar1 RTL8812AU / RTL8821AU / RTL8814AU

The Rust code tracks the devourer behavior that matters for OpenIPC use:

- supported Realtek/OEM VID/PID discovery,
- firmware load and MAC/RF bring-up,
- RFE-aware table selection,
- EFUSE TX power data,
- monitor filters and RX aggregate parsing,
- radiotap-driven TX descriptor building,
- RTL8814 firmware mode/chunk controls,
- RTL8812/RTL8814 IQK,
- RTL8812 power tracking,
- PHYDM false-alarm/DIG watchdog hooks,
- C2H and RTL8814 TX-status report surfacing.
- vendor-correct 5 GHz TX-power groups (`60..98` and `100..106`) and
  `EFUSE -> chip default -> generic default` fallback for unprogrammed base
  cells,
- optional RX-chain masking through `MonitorOptions::rx_path_mask`,
  `RealtekDevice::set_rx_path_mask[_async]`, and the WASM `setRxPathMask`
  binding.

The Rust crate keeps these as explicit APIs. The app decides whether they run
in a native worker thread, a Tauri command, a browser loop, or a Web Worker.
Devourer's timed `DEVOURER_RX_PATHS=mask:mask@milliseconds` mode is a
measurement harness, not hidden HAL state: Rust apps can schedule the explicit
mask setter on their existing worker or browser timer.

## Why App-Owned Polling

Devourer is a native process and can create background threads around libusb.
`openipc-rs` is also a library for browsers and Tauri. A hidden polling thread
inside the driver would not map cleanly to WebUSB and would make app shutdown
harder to reason about.

For Jaguar3, devourer's coex thread does two jobs:

1. drain firmware C2H reports from bulk-IN;
2. every roughly two seconds, re-apply 5 GHz coex, power tracking, and H2C
   heartbeats.

OpenIPC Station already keeps bulk-IN transfers posted in its RX loop. The app
also calls `run_jaguar3_coex_keepalive` and `tick_jaguar3_power_tracking` on a
two-second cadence. The latter dispatches the correct RTL8822C or RTL8822E
thermal algorithm. The driver exposes the hooks; the app owns scheduling.

## Test Strategy

No test can prove RF without hardware, but the repo should catch translation
drift early:

- unit tests for descriptor bit positions and checksums,
- parser tests for aggregate alignment, malformed lengths, C2H reports,
  C2H metadata offsets, bad-FCS flags, and PHY-status boundaries,
- firmware-header tests for the chip-family signatures and RTL8814 64-byte
  reserved-page header path,
- TX tests for chip-family descriptor selection, checksum calculation, VHT
  rate/PHY flags, 5 GHz CCK clamping, and payload-size rejection,
- radio-channel tests for the 40/80 MHz center-channel mappings used by
  devourer and aviateur,
- RTL8822C RF18 and AGC-selection fixtures from the hardware-validated 2.4 GHz
  fix,
- Jaguar1 PG-default and 5 GHz group-boundary tests for blank and partially
  programmed EFUSE maps,
- fake USB control-transport tests for retrying native register reads/writes
  after stalls or cancelled transfers while failing fast on disconnect,
- recovery-classifier tests for transient stalls/timeouts versus fatal USB
  errors,
- generated-table sanity tests for known lengths and boundary values, plus an
  audit-time full-array/hash comparison of every RTL8822E firmware/table value,
- protocol tests for WFB session/decrypt/FEC behavior,
- optional PixelPilot/zfex reference tests for FEC parity when the fixture path
  is available,
- real-device cold-plug runs for each supported chip family,
- register-trace comparison against devourer for cold start and channel switch,
- sustained RX/TX tests with adaptive-link enabled.

The hardware tests are still required before claiming a specific adapter model
is proven. Matching source code and byte-level tests greatly reduce risk, but
they do not replace checking real USB timing, EFUSE variants, and RF behavior.

## Remaining Validation Boundary

The implementation is standalone and does not link devourer. The current audit
also fixed Jaguar3 shutdown, EU AFE reset ordering, shared-PID chip dispatch,
and Jaguar3's full 7-bit TXAGC range. The remaining boundary is hardware proof:

- cold-plug RTL8812AU, RTL8821AU, RTL8814AU, RTL8812CU/EU, and RTL8822CU/EU
  runs,
- register traces for init, channel switch, and shutdown,
- sustained WebUSB receive,
- sustained native/WebUSB adaptive TX,
- adapter matrix across Linux, macOS, Windows, Android, and browser WebUSB.
