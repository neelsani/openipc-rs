# openipc-core

Shared protocol code for `openipc-rs`.

This crate contains the parts of the OpenIPC receive path that do not need to
know whether bytes came from native USB, WebUSB, a capture file, or a test
fixture. It is the right dependency when you want to parse or reconstruct
OpenIPC video without taking a dependency on a specific USB frontend.

## What It Does

- Parse Realtek rtl88xx USB RX aggregates and 24-byte RX descriptors.
- Filter OpenIPC/WFB 802.11 frames by channel id and radio port.
- Handle WFB session packets, data decryption, and FEC recovery.
- Parse RTP and depacketize H.264/H.265 into Annex-B access units.
- Build adaptive-link feedback payloads and WFB uplink packets.
- Parse legacy/HT/VHT radiotap TX modes and build Realtek USB TX descriptors
  for monitor-injection frames.

## Basic Receive Shape

```rust
use openipc_core::{
    parse_rx_aggregate, ChannelId, FrameLayout, PipelineEvent, ReceiverPipeline,
    WfbKeypair,
};
use openipc_core::realtek::RxPacketType;

fn push_transfer(
    pipeline: &mut ReceiverPipeline,
    transfer: &[u8],
) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let mut frames = Vec::new();

    for packet in parse_rx_aggregate(transfer)? {
        if packet.attrib.pkt_rpt_type != RxPacketType::NormalRx {
            continue;
        }
        if packet.attrib.crc_err || packet.attrib.icv_err {
            continue;
        }

        for event in pipeline.push_80211_frame(packet.data)? {
            if let PipelineEvent::VideoFrame(frame) = event {
                frames.push(frame.data);
            }
        }
    }

    Ok(frames)
}

fn pipeline_from_keypair(keypair_bytes: &[u8]) -> Result<ReceiverPipeline, Box<dyn std::error::Error>> {
    let keypair = WfbKeypair::from_bytes(keypair_bytes)?;
    Ok(ReceiverPipeline::with_keypair(
        ChannelId::default_video(),
        FrameLayout::WithFcs,
        keypair,
        0,
    )?)
}
```

The returned frame data is still encoded video. Feed it to WebCodecs, a native
decoder, a file writer, or an RTP/Annex-B bridge depending on your application.

## Crate Boundaries

`openipc-core` intentionally has no USB device ownership. Pair it with:

- `openipc-rtl88xx` for native or WebUSB Realtek adapter IO.
- `openipc-native` for a CLI-style native receive loop.
- `openipc-web` or `@openipc-rs/web` for browser/WASM applications.

## Status

The protocol pipeline has unit tests for parser, crypto, FEC, RTP, and uplink
helpers. Live radio behavior still depends on the USB driver and adapter
validation in higher-level crates.
