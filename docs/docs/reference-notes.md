---
sidebar_position: 13
---

# Reference Notes

These notes summarize what was learned from the reference projects.

## devourer

`devourer` is the native Realtek USB WiFi implementation. It owns the hardware
bring-up that matters for OpenIPC receive:

- firmware download,
- EFUSE/EEPROM and power sequencing,
- BB/RF tables,
- monitor mode,
- channel and bandwidth selection,
- TX descriptors,
- RX descriptor parsing.

The USB model is vendor-control register access plus bulk endpoints:

- request `0x05` for register reads and writes,
- interface 0 claim,
- descriptor-driven bulk IN and bulk OUT endpoint discovery,
- 32 KiB RX transfer buffers to avoid splitting full chip-side aggregates.

The Realtek RX aggregate format is shared Rust logic in `openipc-core`.

## aviateur

`aviateur` is the native OpenIPC ground station. It uses devourer for adapter
access, then handles WFB, RTP, adaptive-link feedback, and video playback.

Packet flow:

1. devourer emits parsed 802.11 frames.
2. OpenIPC/WFB frame checks validate `57:42:<channel_id>` MAC fields.
3. WFB session packets decrypt a session key.
4. WFB data packets decrypt into FEC fragments.
5. Primary fragments emit RTP packets.
6. RTP packets go to playback or optional UDP output.

`openipc-rs` mirrors the protocol behavior in shared Rust, while keeping UI,
USB permissions, and rendering at platform edges.

Aviateur uses native concurrency and UDP-style boundaries because it is a native
application composed from native receiver/playback pieces. The browser station
does not need to mirror that exact shape. In `openipc-rs`, JavaScript drives an
async receive loop, Rust/WASM processes each transfer, and WebCodecs owns
decode/render scheduling.

## openipc-zig

`openipc-zig` proves that browser/WebUSB OpenIPC receive is possible, even if
its implementation is not the desired long-term shape. It is useful for:

- browser permission flow,
- WebUSB constraints,
- WebCodecs playback reference,
- understanding how much hardware setup must still happen in browser builds.

`openipc-rs` keeps WebUSB as a transport adapter and puts the actual receiver
pipeline in Rust/WASM.

## PixelPilot

PixelPilot is useful as an Android reference for packaging a full ground-station
experience around an H.264/H.265 WFB feed. Its Android path wraps a USB file
descriptor with libusb, runs the devourer Realtek driver, and routes parsed
802.11 frames into one wfb-ng aggregator per radio port.

Observed PixelPilot channel map:

- video: port `0x00`, channel `0x7505d600`, recovered RTP goes to UDP `5600`,
- telemetry downlink: port `0x10`, channel `0x7505d610`, recovered bytes often
  carry MAVLink or MSP/OSD data,
- generic data/tunnel RX: port `0x20`, channel `0x7505d620`, recovered bytes go
  to UDP `8000`,
- audio RX in the wfb-ng audio profile: port `0x30`, channel `0x7505d630`,
- telemetry TX in wfb-ng profiles: port `0x90`, channel `0x7505d690`,
- generic tunnel/adaptive uplink TX: port `0xa0`, channel `0x7505d6a0`,
- audio TX in the wfb-ng audio profile: port `0xb0`, channel `0x7505d6b0`.

The OpenIPC firmware package under `firmware/general/package/wifibroadcast-ng`
matches this map. Its default `wfb.yaml` uses link id `7669206` (`0x7505d6`).
The `wifibroadcast` service starts video with `wfb_tx`'s default radio port
`0x00`, telemetry with `load_wfb 144 16 ...` (`0x90/0x10`), and tunnel/data with
`load_wfb 160 32 ...` (`0xa0/0x20`). Upstream `wfb-ng`'s `master.cfg` uses the
same ground-station stream map.

The station UI uses these names in its route radio-port selector. It derives
full channel ids from the active link id so route rows do not expose the long
decimal channel numbers.

The WFB receive behavior matters for compatibility:

- session packets are decrypted with the receiver secret key and transmitter
  public key from `gs.key`;
- session epoch must not move backwards;
- session channel id must match the aggregator's channel id;
- optional encrypted session TLV tags are allowed and ignored by the receiver;
- FEC is Reed-Solomon `VDM_RS`, with `1 <= k <= n < 256`;
- contiguous primary fragments are emitted immediately;
- missing primary fragments are recovered once enough primary/parity fragments
  arrive;
- whole missing blocks are skipped once later blocks prove the stream has moved
  on;
- per-packet decrypt or parse failures are treated as drops, not as receive-loop
  failures.

`openipc-rs` mirrors those protocol rules in `openipc-core` while keeping RTP
depacketization explicit. Apps can forward recovered video-port bytes as RTP or
feed them into `RtpDepacketizer` for Annex-B output.

PixelPilot is also a useful reminder that playback is a product feature, not
just a parser feature. Resolution, decoder status, render FPS, bitrate, and
error counters need to be visible when debugging field behavior.
