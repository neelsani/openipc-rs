---
sidebar_position: 5
---

# Rust Library Usage

Use `openipc-rs` as Rust crates when you want to build your own receiver,
diagnostic tool, desktop app, recorder, streamer, or hardware validation
utility.

## Dependency Shape

After the crates are published, downstream applications can depend on them from
crates.io:

```toml
[dependencies]
openipc-core = "0.1"
openipc-rtl88xx = "0.1"
```

During development before a crates.io release, use a Git dependency or local
path dependency on this repository:

```toml
[dependencies]
openipc-core = { git = "https://github.com/neelsani/openipc-rs", package = "openipc-core" }
```

The hardware and WASM crates use the published WebUSB-capable `nusb-webusb`
package while importing it as `nusb`:

```toml
nusb = { package = "nusb-webusb", version = "0.2.3" }
```

## Parse A Realtek RX Transfer

```rust
use openipc_core::{parse_rx_aggregate, RxPacketType};

fn inspect_transfer(transfer: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    for packet in parse_rx_aggregate(transfer)? {
        if packet.attrib.pkt_rpt_type != RxPacketType::NormalRx {
            continue;
        }
        if packet.attrib.crc_err || packet.attrib.icv_err {
            continue;
        }

        println!(
            "802.11 frame={} bytes seq={} rssi0={} snr0={}",
            packet.data.len(),
            packet.attrib.seq_num,
            packet.attrib.rssi[0],
            packet.attrib.snr[0],
        );
    }

    Ok(())
}
```

## Reconstruct Video Frames

```rust
use openipc_core::{
    parse_rx_aggregate, ChannelId, FrameLayout, PipelineEvent, ReceiverPipeline,
    RxPacketType, WfbKeypair,
};

fn build_pipeline(keypair_bytes: &[u8]) -> Result<ReceiverPipeline, Box<dyn std::error::Error>> {
    let keypair = WfbKeypair::from_bytes(keypair_bytes)?;
    let pipeline = ReceiverPipeline::with_keypair(
        ChannelId::default_video(),
        FrameLayout::WithFcs,
        keypair,
        0,
    )?;
    Ok(pipeline)
}

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
```

The returned frame bytes are encoded Annex-B H.264/H.265. Your application can
write them to a file, feed a native decoder, or forward RTP/Annex-B to another
player.

## Open The Native Realtek Driver

```rust
use std::time::Duration;

use openipc_core::{
    ChannelId, FrameLayout, PipelineEvent, ReceiverPipeline, RxPacketType, WfbKeypair,
};
use openipc_rtl88xx::{
    ChannelWidth, DriverOptions, RadioConfig, RealtekDevice, DEFAULT_RX_TRANSFER_SIZE,
};

fn receive_once(keypair_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let keypair = WfbKeypair::from_bytes(keypair_bytes)?;
    let mut pipeline = ReceiverPipeline::with_keypair(
        ChannelId::default_video(),
        FrameLayout::WithFcs,
        keypair,
        0,
    )?;

    let device = RealtekDevice::open_first(DriverOptions::default())?;
    let report = device.initialize_monitor(RadioConfig {
        channel: 36,
        channel_offset: 0,
        channel_width: ChannelWidth::Mhz20,
    })?;
    eprintln!("initialized {:?}", report);

    let mut bulk_in = device.bulk_in_endpoint()?;
    let buffer = bulk_in.allocate(DEFAULT_RX_TRANSFER_SIZE);
    let completion = bulk_in.transfer_blocking(buffer, Duration::from_millis(1000));
    completion.status?;

    for packet in device.parse_rx_transfer(&completion.buffer[..completion.actual_len])? {
        if packet.attrib.pkt_rpt_type != RxPacketType::NormalRx {
            continue;
        }
        for event in pipeline.push_80211_frame(packet.data)? {
            if let PipelineEvent::VideoFrame(frame) = event {
                println!("video frame: {} bytes", frame.data.len());
            }
        }
    }

    Ok(())
}
```

For a full receive loop with adaptive link, use `openipc-rs recv` as the
reference implementation and then extract the pieces you need.
