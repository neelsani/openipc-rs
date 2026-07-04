# openipc-core

Shared protocol code for `openipc-rs`.

This crate contains the parts of the OpenIPC receive path that do not need to
know whether bytes came from native USB, WebUSB, a capture file, or a test
fixture. It is the right dependency when you want to parse or reconstruct
OpenIPC video without taking a dependency on a specific USB frontend.

## What It Does

- Parse Realtek rtl88xx USB RX aggregates and 24-byte RX descriptors.
- Select Jaguar1 or Jaguar3 RX descriptor layouts explicitly when processing
  transfers from a hardware driver.
- Filter OpenIPC/WFB 802.11 frames by channel id and radio port.
- Handle WFB session packets, optional session TLVs, data decryption, and FEC
  recovery.
- Expose recovered non-video WFB payload bytes from telemetry, tunnel/data,
  audio, or custom radio ports without parsing those application protocols.
- Parse RTP and depacketize H.264/H.265 into Annex-B access units.
- Expose RTP payload bytes for app-owned sinks such as UDP forwarding or Opus
  audio decoding.
- Build adaptive-link feedback payloads and WFB uplink packets.
- Build radiotap + 802.11 WFB transmit frames. Hardware crates add their own
  USB/driver descriptors before transmission.
- Select the first valid copy of each WFB packet when several independent
  receive adapters feed one protocol pipeline.

## Basic Receive Shape

```rust
use openipc_core::{
    ChannelId, FrameLayout, PayloadRouteId, ReceiverBatchOptions, ReceiverRuntime,
    WfbKeypair,
};

const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);

fn push_transfer(
    receiver: &mut ReceiverRuntime,
    transfer: &[u8],
) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let batch = receiver.push_rx_transfer(transfer, &ReceiverBatchOptions::default())?;
    Ok(batch.frames.into_iter().map(|frame| frame.data).collect())
}

fn receiver_from_keypair(keypair_bytes: &[u8]) -> Result<ReceiverRuntime, Box<dyn std::error::Error>> {
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
```

`push_rx_transfer` assumes the Jaguar1 descriptor layout for compatibility.
When bytes come from `openipc-rtl88xx`, call `push_rx_transfer_with_kind` with
`device.rx_descriptor_kind()` so RTL8812CU/EU and RTL8822CU/EU transfers use the
Jaguar3 layout.

The returned frame data is still encoded video. Feed it to WebCodecs, a native
decoder, a file writer, or an RTP/Annex-B bridge depending on your application.
Long-running receivers should treat per-frame WFB errors as dropped packets and
continue scanning the current USB aggregate.

## Multiple-Adapter Diversity

Use one `ReceiverRuntime` for every adapter tuned to the same channel. Put a
`DiversityCombiner` before it and forward only the first valid copy:

```rust
use openipc_core::{DiversityCombiner, DiversitySourceId, FrameLayout, RealtekRxPacket};

fn keep_first_copy<'a>(
    combiner: &mut DiversityCombiner,
    source: u16,
    packet: &RealtekRxPacket<'a>,
) -> bool {
    if packet.attrib.crc_err || packet.attrib.icv_err {
        return true; // Let ReceiverRuntime account for the descriptor drop.
    }
    combiner
        .observe_frame(
            DiversitySourceId::new(source),
            packet.data,
            FrameLayout::WithFcs,
        )
        .should_forward()
}
```

The combiner identifies encrypted data by channel, WFB session generation, and
data nonce. Session packets use their crypto-box nonce. It forwards the first
valid copy immediately and never waits for a stronger RSSI sample. Unique
fragments from every radio then share the normal WFB decryptor and FEC
assembler, so fragments split across adapters can recover one block. Filter
corrupt copies before the combiner so a later valid copy is still accepted.

`DiversityStats` reports first-copy wins and duplicates per source. Adapter
opening, concurrent USB polling, tuning, and failure handling stay at the
application or hardware-crate boundary.

## Payload Routes And RTP

`PayloadPipeline` is the lower-level channel-recovery state machine in
`openipc-core`. It emits recovered bytes after channel filtering, WFB session
handling, decryption, and FEC. It does not parse RTP, MAVLink, MSP, CRSF, IP,
vendor data, or any other application protocol.

Most apps should start with `ReceiverRuntime`. It wraps `PayloadRouteManager`
and the RTP depacketizer used for the configured video route. The route manager
keeps one runtime per `(channel_id, key_slot)` and attaches one or more route
IDs to that runtime. This avoids decrypting and FEC-recovering the same channel
twice when you want multiple outputs, such as local video display plus RTP
forwarding.

For OpenIPC video, configure one video route. Recovered payloads on that route
are treated as RTP and are turned into Annex-B H.264/H.265 frames. If you also
want RTP forwarding, ask `ReceiverRuntime` to tap the same route as raw payload
bytes:

```rust
use openipc_core::{PayloadRouteId, ReceiverBatchOptions, ReceiverRuntime};

const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);

fn receive_with_rtp_tap(
    receiver: &mut ReceiverRuntime,
    transfer: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let batch = receiver.push_rx_transfer(
        transfer,
        &ReceiverBatchOptions {
            raw_payload_routes: vec![VIDEO_ROUTE],
            ..ReceiverBatchOptions::default()
        },
    )?;

    for rtp in batch.raw_payloads {
        println!("RTP bytes: {}", rtp.data.len());
    }
    for frame in batch.frames {
        println!("Annex-B frame bytes: {}", frame.data.len());
    }
    Ok(())
}
```

Applications that already have recovered RTP, such as a native UDP listener,
can enter after WFB without maintaining a separate depacketizer:

```rust
use openipc_core::{
    ChannelId, FrameLayout, PayloadRouteId, ReceiverBatchOptions, ReceiverRuntime,
};

fn handle_udp_rtp(rtp_datagram: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut receiver = ReceiverRuntime::with_direct_video_route(
        FrameLayout::WithFcs,
        PayloadRouteId::new(1),
        ChannelId::default_video(),
        0,
    );
    let batch = receiver.push_direct_payload(
        receiver.video_runtime(),
        1,
        rtp_datagram,
        &ReceiverBatchOptions::default(),
    )?;
    for frame in batch.frames {
        println!("decoded access unit input: {} bytes", frame.data.len());
    }
    Ok(())
}
```

`with_direct_video_route` bypasses only 802.11, WFB decryption, and FEC. Route
fanout, optional RTP reordering, codec configuration tracking, and H.264/H.265
depacketization are unchanged. The older `with_mock_video_route` and
`push_mock_payload` names remain aliases for development code.

Add another route for a non-video WFB channel when you want recovered telemetry,
IP/VPN, or custom bytes:

```rust
use openipc_core::{
    ChannelId, PayloadRouteId, RadioPort, ReceiverBatchOptions, ReceiverRuntime,
    WfbKeypair,
};

const TELEMETRY_ROUTE: PayloadRouteId = PayloadRouteId::new(2);

fn add_telemetry_route(
    receiver: &mut ReceiverRuntime,
    keypair_bytes: &[u8],
    port: RadioPort,
) -> Result<(), Box<dyn std::error::Error>> {
    let keypair = WfbKeypair::from_bytes(keypair_bytes)?;
    receiver.add_keyed_route(
        TELEMETRY_ROUTE,
        ChannelId::from_link_port(7669206, port),
        0,
        keypair,
        0,
    )?;
    Ok(())
}

fn handle_transfer(
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
    for payload in batch.raw_payloads {
        println!(
            "route={} channel=0x{:08x} bytes={}",
            payload.route_id.raw(),
            payload.channel_id.raw(),
            payload.data.len()
        );
    }
    Ok(())
}
```

`RadioPort::TelemetryRx` is OpenIPC's observed telemetry downlink port. That
stream may contain MAVLink, MSP/OSD, or another router-specific payload. Use
`RadioPort::TunnelRx`, `RadioPort::AudioRx`, or `RadioPort::Custom(n)` for other
payload channels. `openipc-core` does not parse MAVLink or any other telemetry
protocol. Applications can parse, display, record, or inspect those bytes later.

Audio follows the same rule, with one extra helper. OpenIPC-documented audio is
usually Opus RTP payload type 98 mixed into the main video RTP route. Use
`ReceiverBatchOptions::rtp_payload_taps` to copy only that payload type while
the video depacketizer keeps consuming the same route. Custom wfb-ng profiles
can also carry audio on a separate route such as `RadioPort::AudioRx`.
`openipc-core` still does not own audio playback; it only provides recovered RTP
packet bytes and metadata.

`RtpDepacketizer` is also conservative about fragmented video. If a fragmented
H.264/H.265 RTP access unit has a sequence gap, the partial frame is dropped and
the depacketizer waits for the next clean fragment start.

## Crate Boundaries

## Logging

The crate emits diagnostics through the standard [`log`](https://docs.rs/log)
facade and never installs a logger. Applications choose the subscriber. Session
changes are `info`, FEC recovery and rejected RTP are `debug`, failures are
`warn`, and per-packet WFB/RTP details are `trace`.

`openipc-core` intentionally has no USB device ownership. Pair it with:

- `openipc-rtl88xx` for native or WebUSB Realtek adapter IO.
- `openipc-web` or `@openipc-rs/web` for browser/WASM applications.
- `apps/openipc-cli` for CLI-style native receive-loop examples.

## Status

The protocol pipeline has unit tests for parser, crypto, FEC, RTP, and uplink
helpers. Live radio behavior still depends on the USB driver and adapter
validation in higher-level crates.

An optional PixelPilot/wfb-ng reference test can compare Rust FEC output against
PixelPilot's vendored `zfex.c` implementation:

```sh
OPENIPC_PIXELPILOT_REF=/path/to/PixelPilot \
  cargo test -p openipc-core --test pixelpilot_reference -- --ignored --nocapture
```

The test builds a temporary C harness, generates PixelPilot parity/recovery
vectors, and compares them with `FecCode` plus a WFB-shaped
`PlainAssembler` recovery case. It is ignored by default so normal CI does not
depend on a PixelPilot checkout or a C compiler.
