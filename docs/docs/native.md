---
sidebar_position: 4
---

# Native

The native CLI lives in `crates/openipc-native` and builds a binary named
`openipc-rs`.

For embedding the crates directly in your own Rust application, see
[Rust Library Usage](./rust-library.md).

## What It Is For

- listing and probing USB adapters,
- decoding captured Realtek RX transfers,
- receiving live OpenIPC video,
- writing Annex-B H.264/H.265 output,
- mirroring recovered RTP to UDP for compatibility testing,
- exercising adaptive-link feedback without the station UI.

## List Devices

```sh
cargo run -p openipc-native -- list
cargo run -p openipc-native -- list-supported
```

## Probe A Realtek Adapter

```sh
cargo run -p openipc-native -- probe
OPENIPC_RS_SKIP_RESET=1 cargo run -p openipc-native -- probe
```

`probe` claims the first supported adapter, reads chip information, and prints
the selected bulk endpoints. It does not run full monitor-mode initialization.

## Decode Captures

Parse a captured Realtek RX bulk transfer:

```sh
cargo run -p openipc-native -- parse-aggregate capture.bin
```

Decode a captured transfer through WFB/FEC/RTP and write Annex-B video:

```sh
cargo run -p openipc-native -- decode-aggregate capture.bin --key gs.key --out video.annexb
```

Use this path when debugging protocol changes. It lets you test parser, WFB,
FEC, and RTP behavior without live USB timing in the loop.

## Receive Live Video

```sh
cargo run -p openipc-native -- recv \
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
cargo run -p openipc-native -- recv \
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
Adaptive link itself sends quality information to the air unit; it does not mean
the ground station automatically chooses RF power on its own.

## USB Permissions

USB access is OS-specific. On Linux you may need udev rules or to run with
permissions that allow claiming the adapter. On Windows the device must be using
a driver stack that exposes it to user-space USB APIs. On macOS the OS may show
extra permission prompts. The Rust code uses `nusb`; the operating-system USB
policy still applies.
