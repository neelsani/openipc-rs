---
sidebar_position: 5
---

# Rust Library Usage

Use `openipc-rs` as Rust crates when you want to build your own receiver,
diagnostic tool, desktop app, recorder, streamer, or hardware validation
utility.

## Dependencies

From crates.io:

```toml
[dependencies]
openipc-core = "0.1"
openipc-rtl88xx = "0.1"
```

From git:

```toml
[dependencies]
openipc-core = { git = "https://github.com/neelsani/openipc-rs", package = "openipc-core" }
openipc-rtl88xx = { git = "https://github.com/neelsani/openipc-rs", package = "openipc-rtl88xx" }
```

The hardware and WASM crates use the published WebUSB-capable `nusb-webusb`
package while importing it as `nusb`:

```toml
nusb = { package = "nusb-webusb", version = "0.2.3" }
```

## Library Boundaries

- `openipc-core` is pure protocol logic. It can process bytes from files, USB,
  tests, or another transport.
- `openipc-rtl88xx` owns Realtek USB device access and monitor-mode setup.
- `openipc-native` is a CLI and thin re-export crate.
- `openipc-web` is for building the WASM/npm package. Browser apps normally use
  `@openipc-rs/web` from npm instead.

## Parse A Realtek RX Transfer

```rust
use openipc_core::parse_rx_aggregate;
use openipc_core::realtek::RxPacketType;

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
    WfbKeypair,
};
use openipc_core::realtek::RxPacketType;

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

## Understand Pipeline Events

`ReceiverPipeline::push_80211_frame` returns a list because one input frame can
advance several layers:

```rust
for event in pipeline.push_80211_frame(packet.data)? {
    match event {
        PipelineEvent::IgnoredFrame => {}
        PipelineEvent::SessionEstablished { epoch, fec_k, fec_n } => {
            eprintln!("session epoch={epoch} fec={fec_k}/{fec_n}");
        }
        PipelineEvent::WfbPayload { packet_seq, len } => {
            eprintln!("recovered WFB payload seq={packet_seq} len={len}");
        }
        PipelineEvent::RtpPacket { payload, .. } => {
            // Mirror RTP, inspect packet timing, or ignore it.
            println!("rtp bytes={}", payload.len());
        }
        PipelineEvent::VideoFrame(frame) => {
            // Complete encoded access unit for a decoder or file writer.
            println!("annex-b frame bytes={}", frame.data.len());
        }
    }
}
```

`RtpPacket` and `VideoFrame` are not competing states. The pipeline emits
`RtpPacket` when a recovered payload parses as RTP, then may emit `VideoFrame`
from the same call if that RTP packet completes a H.264/H.265 access unit.

## Read Raw Payload Bytes

OpenIPC/WFB uses separate radio ports. Video is port `0x00`; the ground-station
MAVLink downlink observed in aviateur and PixelPilot is port `0x10`; data-style
payloads may use another port such as `0x20`. Use `PayloadPipeline` when you
only want recovered bytes and want your own app to decide how to parse them:

```rust
use openipc_core::{
    ChannelId, FrameLayout, PayloadPipeline, PayloadPipelineEvent, RadioPort,
    WfbKeypair,
};

fn build_payload_pipeline(
    link_id: u32,
    port: RadioPort,
    keypair_bytes: &[u8],
) -> Result<PayloadPipeline, Box<dyn std::error::Error>> {
    let keypair = WfbKeypair::from_bytes(keypair_bytes)?;
    Ok(PayloadPipeline::with_keypair(
        ChannelId::from_link_port(link_id, port),
        FrameLayout::WithFcs,
        keypair,
        0,
    )?)
}

fn handle_wifi_frame(
    pipeline: &mut PayloadPipeline,
    frame: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    for event in pipeline.push_80211_frame(frame)? {
        if let PayloadPipelineEvent::Payload(payload) = event {
            // payload.data is raw recovered bytes for the configured radio port.
            // payload.packet_seq carries the recovered WFB packet sequence.
            // Parse, store, or inspect it in your own application layer.
            println!("telemetry bytes: {}", payload.data.len());
        }
    }
    Ok(())
}
```

`RadioPort::MavlinkRx` is just a named convenience for the observed OpenIPC
MAVLink downlink port. You can also use `RadioPort::DataRx` or
`RadioPort::Custom(n)`. `openipc-rs` deliberately does not parse MAVLink, MSP,
CRSF, or arbitrary vendor protocols in the core crate. The boundary is recovered
bytes plus packet sequence metadata, so another crate or process can decide how
to interpret them.

## Open The Native Realtek Driver

```rust
use std::time::Duration;

use openipc_core::{
    ChannelId, FrameLayout, PipelineEvent, ReceiverPipeline, WfbKeypair,
};
use openipc_core::realtek::RxPacketType;
use openipc_rtl88xx::{
    ChannelWidth, DriverOptions, MonitorOptions, RadioConfig, RealtekDevice,
    DEFAULT_RX_TRANSFER_SIZE,
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

## Configure Hardware Bring-Up

Most apps can use `RealtekDevice::open_first(DriverOptions::default())` and
`initialize_monitor(...)`. When you need more control, use the option structs
directly:

```rust
use openipc_rtl88xx::{
    ChannelWidth, DriverOptions, Firmware8814Mode, MonitorOptions, RadioConfig,
    RealtekDevice,
};

fn open_specific_adapter() -> Result<(), Box<dyn std::error::Error>> {
    let device = RealtekDevice::open_first(DriverOptions {
        target_vendor_id: Some(0x0bda),
        target_product_id: Some(0x8813),
        tx_endpoint_override: None,
        skip_reset: false,
        initialize_hardware: true,
    })?;

    device.initialize_monitor_with_options(
        RadioConfig {
            channel: 161,
            channel_width: ChannelWidth::Mhz20,
            channel_offset: 0,
        },
        MonitorOptions {
            accept_bad_fcs: false,
            skip_tx_power: false,
            force_iqk: false,
            disable_iqk: false,
            firmware_8814_mode: Firmware8814Mode::Kernel,
            firmware_8814_chunk: None,
        },
    )?;

    Ok(())
}
```

Diagnostics such as thermal status, false-alarm counters, PHYDM watchdog ticks,
IQK, and power tracking are explicit APIs. The driver does not spawn its own
polling threads; schedule those reads from your app loop when you need them.

## Build Adaptive-Link Feedback

The adaptive-link pieces live in `openipc-core`; the actual send operation comes
from the driver.

```rust
use openipc_core::{AdaptiveLinkSender, WfbTxKeypair};

fn make_sender(key_bytes: &[u8]) -> Result<AdaptiveLinkSender, Box<dyn std::error::Error>> {
    let keypair = WfbTxKeypair::from_bytes(key_bytes)?;
    Ok(AdaptiveLinkSender::new(
        openipc_core::channel::DEFAULT_LINK_ID,
        keypair,
        0,
        1,
        5,
    )?)
}
```

The receiver loop records RSSI/SNR and FEC counters, then periodically asks the
sender for an encrypted WFB uplink packet. Native and browser code use the same
packet builder.
