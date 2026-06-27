---
sidebar_position: 4
---

# Native

The native CLI lives in `crates/openipc-native`.

For embedding the crates directly in your own Rust application, see
[Rust Library Usage](./rust-library.md).

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

## Decode Captures

Parse a captured Realtek RX bulk transfer:

```sh
cargo run -p openipc-native -- parse-aggregate capture.bin
```

Decode a captured transfer through WFB/FEC/RTP and write Annex-B video:

```sh
cargo run -p openipc-native -- decode-aggregate capture.bin --key gs.key --out video.annexb
```

## Receive Live Video

```sh
cargo run -p openipc-native -- recv \
  --key gs.key \
  --rf-channel 36 \
  --rtp-udp 127.0.0.1:5600 \
  --out video.annexb
```

## Adaptive Link

```sh
cargo run -p openipc-native -- recv \
  --key gs.key \
  --rf-channel 36 \
  --adaptive-link \
  --out video.annexb
```

The adaptive uplink uses the same 64-byte key file by default and interprets it
as ground-station secret key plus air-side public key for the TX direction. Use
`--alink-key` for a separate uplink key file.
