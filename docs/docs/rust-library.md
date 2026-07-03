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
openipc-video = "0.1"
```

From git:

```toml
[dependencies]
openipc-core = { git = "https://github.com/neelsani/openipc-rs", package = "openipc-core" }
openipc-rtl88xx = { git = "https://github.com/neelsani/openipc-rs", package = "openipc-rtl88xx" }
openipc-video = { git = "https://github.com/neelsani/openipc-rs", package = "openipc-video" }
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
- `openipc-video` consumes Annex-B access units and owns platform decode, frame
  backpressure, and retained decoded surfaces on desktop, Android, and web.
- `openipc-web` is for building the WASM/npm package. Browser apps normally use
  `@openipc-rs/web` from npm instead.
- `apps/openipc-cli` is a command-line app, not a library dependency.

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

`push_rx_transfer` uses the Jaguar1 layout by default. Hardware-facing apps
should use `push_rx_transfer_with_kind` and pass the descriptor kind reported by
`openipc-rtl88xx`, as shown in the native driver example below.

## Reconstruct Video Frames

```rust
use openipc_core::{
    ChannelId, FrameLayout, PayloadRouteId, ReceiverBatchOptions, ReceiverRuntime,
    WfbKeypair,
};

const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);

fn build_receiver(keypair_bytes: &[u8]) -> Result<ReceiverRuntime, Box<dyn std::error::Error>> {
    let keypair = WfbKeypair::from_bytes(keypair_bytes)?;
    Ok(ReceiverRuntime::with_keyed_video_route(
        FrameLayout::WithFcs,
        VIDEO_ROUTE,
        ChannelId::default_video(),
        0,
        keypair,
        0,
    )?)
}

fn push_transfer(
    receiver: &mut ReceiverRuntime,
    transfer: &[u8],
) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let batch = receiver.push_rx_transfer(transfer, &ReceiverBatchOptions::default())?;
    Ok(batch.frames.into_iter().map(|frame| frame.data).collect())
}
```

## Decode Video

`ReceiverRuntime` emits encoded frames. Move those frames directly into the
target decoder without converting them to base64:

```rust,no_run
use openipc_video::{PlatformDecoder, VideoDecoder};

# #[cfg(any(
#     target_os = "macos",
#     target_os = "linux",
#     target_os = "windows",
#     target_os = "android",
#     all(target_arch = "wasm32", target_os = "unknown"),
# ))]
fn decode(
    decoder: &mut PlatformDecoder,
    frame: openipc_core::DepacketizedFrame,
) -> Result<(), Box<dyn std::error::Error>> {
    decoder.submit(frame.into())?;
    if let Some(frame) = decoder.latest_frame() {
        let size = frame.dimensions();
        println!("decoded {}x{}", size.width, size.height);
    }
    Ok(())
}
```

The decoder automatically tracks H.264 SPS/PPS and H.265 VPS/SPS/PPS. See
[Platform Video Decoding](./native-video.md) for backpressure behavior and
surface rendering on each target.

The `DepacketizedFrame` values returned by `ReceiverRuntime` contain encoded
Annex-B H.264/H.265. Your application can write those encoded bytes to a file,
submit them to `openipc-video`, or use its raw RTP route for forwarding.

Treat a malformed Realtek aggregate as a transfer-level error. Treat a failed
WFB frame inside a valid aggregate as a packet drop and keep scanning, which is
how the working wfb-ng/PixelPilot receiver behaves.

## Combine Multiple Receive Adapters

Use one USB capture loop per adapter and one shared `ReceiverRuntime` for all
of them. `DiversityCombiner` identifies encrypted WFB session/data packets and
forwards the first valid copy immediately. Later copies are dropped before
decryption and FEC work. Because all unique fragments enter the same receiver,
fragments heard by different radios can recover one FEC block.

```rust
use openipc_core::{
    parse_rx_aggregate, DiversityCombiner, DiversitySourceId, FrameLayout,
    ReceiverBatchOptions, ReceiverRuntime,
};
use openipc_core::realtek::RxPacketType;

fn push_radio_transfer(
    receiver: &mut ReceiverRuntime,
    diversity: &mut DiversityCombiner,
    source: DiversitySourceId,
    transfer: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    for packet in parse_rx_aggregate(transfer)? {
        let valid = packet.attrib.pkt_rpt_type == RxPacketType::NormalRx
            && !packet.attrib.crc_err
            && !packet.attrib.icv_err;
        if !valid {
            continue;
        }

        let decision = diversity.observe_frame(
            source,
            packet.data,
            FrameLayout::WithFcs,
        );
        if decision.should_forward() {
            let _batch = receiver.push_80211_frame(
                packet.data,
                &ReceiverBatchOptions::default(),
            )?;
        }
    }
    Ok(())
}
```

Assign each open adapter a stable `DiversitySourceId`. Do not create one
receiver per radio: that would split WFB sessions and FEC fragments into
independent state machines and throw away the main benefit of diversity.
`DiversityCombiner::stats()` reports first-copy contributions and duplicate
counts per source. It is intentionally bypassed in Nebulus when only one radio
is active, so the normal path has no diversity hashing overhead.

## Compose Payload Recovery And RTP

`ReceiverRuntime` does two jobs for the configured video route: it recovers WFB
payload bytes, then feeds those bytes to the built-in RTP depacketizer. If your
app also needs raw RTP, add the video route to `raw_payload_routes`:

```rust
let batch = receiver.push_rx_transfer(
    transfer,
    &ReceiverBatchOptions {
        raw_payload_routes: vec![VIDEO_ROUTE],
        ..ReceiverBatchOptions::default()
    },
)?;

for rtp in batch.raw_payloads {
    // Forward to UDP, inspect timing, or store the RTP packet.
    println!("rtp bytes={}", rtp.data.len());
}

for frame in batch.frames {
    // Encoded Annex-B H.264/H.265 access unit.
    println!("annex-b frame bytes={}", frame.data.len());
}
```

For OpenIPC mixed audio, attach a second route id to the same video channel and
use an RTP payload tap. That copies only Opus RTP payload type 98 while the
video depacketizer still consumes the same recovered packets:

```rust
use openipc_core::{PayloadRouteId, ReceiverBatchOptions, RtpPayloadTap};
use openipc_core::rtp::RTP_PAYLOAD_TYPE_OPUS;

const AUDIO_ROUTE: PayloadRouteId = PayloadRouteId::new(3);

receiver.add_keyed_route(AUDIO_ROUTE, video_channel_id, 0, keypair, 0)?;

let batch = receiver.push_rx_transfer(
    transfer,
    &ReceiverBatchOptions {
        rtp_payload_taps: vec![RtpPayloadTap {
            route_id: AUDIO_ROUTE,
            payload_type: RTP_PAYLOAD_TYPE_OPUS,
        }],
        ..ReceiverBatchOptions::default()
    },
)?;

for packet in batch.raw_payloads {
    // packet.data is the original RTP packet with payload type 98.
}
```

The lower-level `PayloadPipeline` still exists for tools that want to stop
exactly at recovered WFB bytes:

```rust
use openipc_core::{PayloadPipelineEvent, RtpDepacketizer};

for event in pipeline.push_80211_frame(packet.data)? {
    if let PayloadPipelineEvent::Payload(payload) = event {
        if let Some(frame) = rtp.push(&payload.data)? {
            println!("annex-b frame bytes={}", frame.data.len());
        }
    }
}
```

`RtpDepacketizer` is separate on purpose. Apps that want RTP forwarding can use
the recovered payload bytes directly. Apps that want OpenIPC video frames feed
those same bytes into `RtpDepacketizer`, which emits a frame only when enough
RTP packets have arrived to complete an H.264/H.265 access unit. Fragmented
H.264/H.265 frames with RTP sequence gaps are dropped rather than emitted as
corrupted Annex-B.

## Route Multiple Payload Outputs

Use `ReceiverRuntime::add_keyed_route` when an app has more than one logical
output. Internally it uses `PayloadRouteManager`: one WFB runtime owns the
session key, decrypt/FEC state, and counters for a channel, then route IDs fan
recovered payloads out to the app's sinks.

```rust
use openipc_core::{
    ChannelId, FrameLayout, PayloadRouteId, RadioPort, ReceiverBatchOptions,
    ReceiverRuntime, WfbKeypair,
};

const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);
const TELEMETRY_ROUTE: PayloadRouteId = PayloadRouteId::new(2);

fn build_receiver(
    link_id: u32,
    keypair_bytes: &[u8],
) -> Result<ReceiverRuntime, Box<dyn std::error::Error>> {
    let keypair = WfbKeypair::from_bytes(keypair_bytes)?;
    let mut receiver = ReceiverRuntime::with_keyed_video_route(
        FrameLayout::WithFcs,
        VIDEO_ROUTE,
        ChannelId::from_link_port(link_id, RadioPort::Video),
        0,
        keypair,
        0,
    )?;
    receiver.add_keyed_route(
        TELEMETRY_ROUTE,
        ChannelId::from_link_port(link_id, RadioPort::TelemetryRx),
        0,
        keypair,
        0,
    )?;

    Ok(receiver)
}

fn push_transfer(
    receiver: &mut ReceiverRuntime,
    transfer: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let batch = receiver.push_rx_transfer(
        transfer,
        &ReceiverBatchOptions {
            raw_payload_routes: vec![TELEMETRY_ROUTE],
            ..ReceiverBatchOptions::default()
        },
    )?;

    for frame in batch.frames {
        println!("video frame bytes={}", frame.data.len());
    }
    for payload in batch.raw_payloads {
        println!("raw telemetry-port bytes={}", payload.data.len());
    }

    Ok(())
}
```

If two user-facing routes target the same channel, for example video display
and RTP forwarding on port `0x00`, register both route IDs against the same
channel/key slot. The manager will still keep one WFB runtime and return both
route IDs on each recovered payload.

## Read Raw Payload Bytes

OpenIPC/WFB uses separate radio ports. Video downlink is port `0x00`; telemetry
downlink is port `0x10`; tunnel/data downlink is port `0x20`. The station UI
exposes these as radio-port presets: video `0x00`, telemetry RX/TX `0x10/0x90`,
tunnel RX/TX `0x20/0xa0`, and optional audio profile RX/TX `0x30/0xb0`. Use
`PayloadRouteManager` for
multi-output apps, or a direct `PayloadPipeline` for a small single-channel
tool, when you only want recovered bytes and want your own app to decide how to
parse them:

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

`RadioPort::TelemetryRx` is the observed OpenIPC telemetry downlink port. That
payload may be MAVLink, MSP/OSD, or another router-specific format. You can also
use `RadioPort::TunnelRx`, `RadioPort::AudioRx`, or `RadioPort::Custom(n)`.
`RadioPort::AudioRx` is for separate wfb-ng audio profiles; the documented
OpenIPC Opus path may instead be mixed into the video route. `openipc-rs`
deliberately does not parse MAVLink, MSP, CRSF, or arbitrary vendor protocols in
the core crate. The boundary is recovered bytes plus packet sequence metadata,
so another crate or process can decide how to interpret them.

The WFB session parser accepts the fixed wfb-ng session fields and ignores
optional encrypted TLV tags. FEC recovery follows the usual wfb-ng behavior:
contiguous primary fragments are emitted early, missing primaries are recovered
when enough fragments arrive, and completely missing blocks are skipped once
later blocks are ready.

## PixelPilot Reference Test

`openipc-core` includes an ignored integration test that compares the Rust FEC
implementation with PixelPilot's vendored wfb-ng `zfex.c`. It builds a small C
harness at test runtime, asks PixelPilot/zfex for parity and recovered-fragment
vectors, then compares those bytes with Rust `FecCode`. It also feeds
PixelPilot-generated parity into a WFB-shaped `PlainAssembler` case.

Run it when you have PixelPilot checked out locally:

```sh
OPENIPC_PIXELPILOT_REF=/Users/neels/expir/openipc/PixelPilot \
  cargo test -p openipc-core --test pixelpilot_reference -- --ignored --nocapture
```

Normal `cargo test` compiles the test but does not run it, so CI and published
crates do not depend on PixelPilot or a C compiler.

## Open The Native Realtek Driver

```rust
use std::time::Duration;

use openipc_core::{
    ChannelId, FrameLayout, PayloadRouteId, ReceiverBatchOptions, ReceiverRuntime, WfbKeypair,
};
use openipc_rtl88xx::{
    ChannelWidth, DriverOptions, MonitorOptions, RadioConfig, RealtekDevice,
    DEFAULT_RX_TRANSFER_SIZE,
};

const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);

fn receive_once(keypair_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let keypair = WfbKeypair::from_bytes(keypair_bytes)?;
    let mut receiver = ReceiverRuntime::with_keyed_video_route(
        FrameLayout::WithFcs,
        VIDEO_ROUTE,
        ChannelId::default_video(),
        0,
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
    let descriptor_kind = device.rx_descriptor_kind();

    let mut bulk_in = device.bulk_in_endpoint()?;
    let buffer = bulk_in.allocate(DEFAULT_RX_TRANSFER_SIZE);
    let completion = bulk_in.transfer_blocking(buffer, Duration::from_millis(1000));
    completion.status?;

    let batch = receiver.push_rx_transfer_with_kind(
        &completion.buffer[..completion.actual_len],
        descriptor_kind,
        &ReceiverBatchOptions::default(),
    )?;
    for frame in batch.frames {
        println!("video frame: {} bytes", frame.data.len());
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
            skip_txgapk: false,
            firmware_8814_mode: Firmware8814Mode::Kernel,
            firmware_8814_chunk: None,
        },
    )?;

    Ok(())
}
```

When several identical adapters are attached, list them once and reopen each
one by its physical-path identity:

```rust
use openipc_rtl88xx::{list_supported_devices, DriverOptions, RealtekDevice};

fn open_all_adapters() -> Result<Vec<RealtekDevice>, Box<dyn std::error::Error>> {
    let mut devices = Vec::new();
    for summary in list_supported_devices()? {
        devices.push(RealtekDevice::open_by_id(
            &summary.stable_id(),
            DriverOptions::default(),
        )?);
    }
    Ok(devices)
}
```

The stable id includes the USB bus and physical port chain when the platform
provides them. Keep the returned devices separate through initialization and
bulk-IN capture, then feed their parsed valid frames through one
`DiversityCombiner` as shown above.

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
