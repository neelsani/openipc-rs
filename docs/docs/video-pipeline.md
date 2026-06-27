---
sidebar_position: 7
---

# Video Pipeline

OpenIPC video arrives over WiFi as WFB data carrying RTP. `openipc-rs` turns
those packets into encoded video frames.

```mermaid
flowchart LR
    Air["Air unit"] --> Wifi["802.11 monitor frame"]
    Wifi --> Wfb["WFB packet"]
    Wfb --> Fec["FEC block"]
    Fec --> Rtp["RTP packet"]
    Rtp --> Video["H.264/H.265 Annex-B"]
    Video --> Decode["WebCodecs or native decoder"]
```

## Receive Path

1. USB bulk-IN returns a Realtek RX aggregate.
2. The Realtek parser splits the aggregate into 802.11 packets and extracts
   descriptor metadata such as RSSI, SNR, sequence number, and flags.
3. The OpenIPC filter checks mirrored `57:42:<channel_id>` MAC fields and radio
   ports.
4. WFB session packets update the data-decryption session key.
5. WFB data packets decrypt into primary and parity FEC fragments.
6. Reed-Solomon recovery repairs missing primary fragments where possible.
7. Primary fragments emit RTP packets.
8. RTP H.264/H.265 depacketization emits Annex-B frames.

## Annex-B Frames

Annex-B is the byte-stream form of H.264/H.265 where NAL units are separated by
start codes such as `00 00 00 01`. This is a convenient boundary for WebCodecs,
file output, and native player integration because the protocol stack can
deliver complete encoded access units without decoding pixels itself.

## Decode And Render

The station app decodes with WebCodecs where the browser or WebView supports
the codec string returned by Rust. H.264 is broadly available; H.265 depends on
browser and operating-system support.
