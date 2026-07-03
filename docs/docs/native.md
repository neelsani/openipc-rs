---
sidebar_position: 4
---

# Native

Native command-line tools are split into two app packages:

- `apps/openipc-cli` builds the general `openipc-rs` helper for adapter probes,
  capture decoding, and OpenIPC receive-loop testing.
- `apps/wfb-rs` builds WFB-style binaries for receive, transmit, key
  generation, command control, tunneling, and simple RTSP proxying.

For embedding the crates directly in your own Rust application, see
[Rust Library Usage](./rust-library.md).

The `openipc-rs` and `wfb_rx` command-line receivers currently open one radio.
The Nebulus app supports packet-level receive diversity across multiple
adapters on desktop, Android, and WebUSB; see
[Receive Diversity](./receive-diversity.md).

## What It Is For

- listing and probing USB adapters,
- decoding captured Realtek RX transfers,
- receiving live OpenIPC video,
- writing Annex-B H.264/H.265 output,
- mirroring recovered RTP to UDP for compatibility testing,
- exercising adaptive-link feedback without the station UI,
- running WFB-style userland tools over the Rust Realtek driver.

The WFB-style tools are not pcap/PF_PACKET drop-in replacements for upstream
`wfb-ng`. `wfb_rx` and `wfb_tx` open a supported Realtek USB adapter directly
through `nusb` and `openipc-rtl88xx`.

## Rust WFB-ng-Style Binaries

`wfb-rs` is a Rust rewrite of the WFB-ng binary roles that are useful for
OpenIPC FPV bring-up:

- `wfb_rx` replaces the receive-side aggregator path with
  Realtek USB bulk-IN reads, Rust RX descriptor parsing, WFB session/FEC
  recovery, decryption, and UDP payload output.
- `wfb_tx` replaces the transmit-side UDP-to-radio path with Rust WFB packet
  creation, FEC, radiotap/802.11 header construction, Realtek TX descriptor
  creation, and USB bulk-OUT injection. On Jaguar3 it also drains firmware C2H
  reports and runs the two-second coex/thermal maintenance cadence needed for
  sustained transmit-only operation.
- `wfb_keygen`, `wfb_tx_cmd`, `wfb_tun`, and `wfb_rtsp` cover the supporting
  key, control, tunnel, and RTP/RTSP helper roles.

The important architectural difference from upstream WFB-ng is the radio
boundary. Upstream WFB-ng normally expects a WiFi adapter that has already been
configured by the operating-system driver and then talks through Linux monitor
mode interfaces such as pcap/PF_PACKET. `wfb-rs` instead talks directly to
supported Realtek USB adapters through the Rust `openipc-rtl88xx` userland
driver. That means the same Rust code owns monitor initialization, RX aggregate
parsing, TX descriptor generation, and frame injection.

Because the radio path uses `nusb` instead of Linux-only monitor interfaces,
the main RX/TX tools are designed to run on Linux, macOS, and Windows. Platform
USB permissions and driver binding still matter: Linux may need udev rules,
Windows needs a user-space USB-compatible driver binding, and macOS may show
permission prompts. `wfb_tun` is the exception in this package today because it
uses a Unix TUN interface.

This is not a binding to upstream WFB-ng and it does not link against
`devourer`. The implementation is written in Rust on top of `openipc-core` and
`openipc-rtl88xx`, with `nusb` providing the cross-platform USB transport.
Some helper roles are intentionally smaller than upstream, so check the parity
table below before relying on a specific WFB-ng flag or mode.

## Binaries

```sh
cargo build -p openipc-cli
cargo build -p wfb-rs
```

| Binary       | Purpose                                                       |
| ------------ | ------------------------------------------------------------- |
| `openipc-rs` | General probe, capture decode, and video receive helper.      |
| `wfb_keygen` | Generate WFB-compatible `drone.key` and `gs.key`.             |
| `wfb_rx`     | Realtek USB RX to recovered WFB payload UDP output.           |
| `wfb_tx`     | UDP input to WFB/FEC/radiotap/Realtek USB frame injection.    |
| `wfb_tx_cmd` | Control a running `wfb_tx` FEC/radio settings over UDP.       |
| `wfb_tun`    | Length-prefixed WFB tunnel UDP/TUN bridge on Unix.            |
| `wfb_rtsp`   | Minimal RTSP/RTP UDP proxy for local H.264/H.265 RTP streams. |

## List Devices

```sh
cargo run -p openipc-cli -- list
cargo run -p openipc-cli -- list-supported
```

## Probe A Realtek Adapter

```sh
cargo run -p openipc-cli -- probe
OPENIPC_RS_SKIP_RESET=1 cargo run -p openipc-cli -- probe
```

`probe` claims the first supported adapter, reads chip information, and prints
the selected bulk endpoints. It does not run full monitor-mode initialization.

## Decode Captures

Parse a captured Realtek RX bulk transfer:

```sh
cargo run -p openipc-cli -- parse-aggregate capture.bin
```

Decode a captured transfer through WFB/FEC/RTP and write Annex-B video:

```sh
cargo run -p openipc-cli -- decode-aggregate capture.bin --key gs.key --out video.annexb
```

Use this path when debugging protocol changes. It lets you test parser, WFB,
FEC, and RTP behavior without live USB timing in the loop.

## Receive Live Video

```sh
cargo run -p openipc-cli -- recv \
  --key gs.key \
  --rf-channel 161 \
  --rf-width 20 \
  --rtp-udp 127.0.0.1:5600 \
  --out video.annexb
```

Important receive options:

| Option                | Meaning                                                                                              |
| --------------------- | ---------------------------------------------------------------------------------------------------- |
| `--key <gs.key>`      | WFB keypair file. Required for encrypted streams.                                                    |
| `--channel-id <id>`   | OpenIPC/WFB channel id as decimal or `0x` hex. Defaults to the OpenIPC link id and video radio port. |
| `--epoch <n>`         | Minimum accepted WFB session epoch.                                                                  |
| `--rf-channel <n>`    | WiFi channel used for monitor mode.                                                                  |
| `--rf-width WIDTH`    | Channel width: `20`, `40`, or `80`.                                                                  |
| `--rf-offset <n>`     | Secondary-channel offset.                                                                            |
| `--rx-urbs <n>`       | Number of pending USB bulk-IN reads.                                                                 |
| `--max-transfers <n>` | Stop after a fixed number of USB transfers. Useful for repeatable tests.                             |
| `--no-init`           | Skip Realtek hardware initialization. Useful only when an adapter is already configured.             |

## Adaptive Link

```sh
cargo run -p openipc-cli -- recv \
  --key gs.key \
  --rf-channel 161 \
  --adaptive-link \
  --alink-tx-power 20 \
  --out video.annexb
```

The adaptive uplink uses the same 64-byte key file by default and interprets it
as ground-station secret key plus air-side public key for the TX direction. Use
`--alink-key` for a separate uplink key file.

`--alink-tx-power` is a manual Realtek TXAGC override for the feedback uplink.
Jaguar1 accepts `0..=63`; Jaguar3 accepts `0..=127`.
Adaptive link itself sends quality information to the air unit; it does not mean
the ground station automatically chooses RF power on its own.

## WFB-Style Payload RX/TX

Generate key files:

```sh
cargo run -p wfb-rs --bin wfb_keygen
```

Receive raw WFB payloads on the default video radio port and forward them to
UDP:

```sh
cargo run -p wfb-rs --bin wfb_rx -- \
  -K gs.key \
  -i 7669206 \
  -p 0 \
  -c 127.0.0.1 \
  -u 5600 \
  --rf-channel 161
```

Transmit UDP payloads over the adapter:

```sh
cargo run -p wfb-rs --bin wfb_tx -- \
  -K drone.key \
  -i 7669206 \
  -p 0 \
  -u 5600 \
  -k 8 \
  -n 12 \
  -J 10 \
  -E 5000 \
  --rf-channel 161 \
  -C 7000
```

Change a running transmitter:

```sh
cargo run -p wfb-rs --bin wfb_tx_cmd -- 7000 get_radio
cargo run -p wfb-rs --bin wfb_tx_cmd -- 7000 set_fec -k 4 -n 8
```

Bridge tunnel payloads to a TUN device:

```sh
sudo target/debug/wfb_tun -t wfb-tun -a 10.5.0.2/24 -l 5800 -c 127.0.0.1 -u 5801
```

Expose recovered RTP packets as a simple RTSP stream:

```sh
target/debug/wfb_rtsp -P 5600 -p 8554 -u /wfb h264
```

The Rust `wfb_rtsp` helper is intentionally smaller than upstream WFB-ng's
GStreamer wrapper: it forwards RTP from UDP to the RTSP client's selected RTP
port. It does not depayload, jitter-buffer, or repacketize.

`wfb_keygen` currently implements random key generation. The original
password-derived mode uses libsodium Argon2i and is intentionally not faked
with a different derivation.

For the full upstream-option parity table, including unsupported and remaining
no-op flags for each WFB-style binary, see `apps/wfb-rs/README.md`.

## USB Permissions

USB access is OS-specific. On Linux you may need udev rules or to run with
permissions that allow claiming the adapter. On Windows the device must be using
a driver stack that exposes it to user-space USB APIs. On macOS the OS may show
extra permission prompts. The Rust code uses `nusb`; the operating-system USB
policy still applies.
