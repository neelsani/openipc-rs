---
sidebar_position: 7
---

# Video Pipeline

OpenIPC video arrives over WiFi as WFB data carrying RTP. `openipc-rs` turns
those packets into encoded video frames. It does not decode pixels in the core
pipeline.

```mermaid
flowchart LR
    Air["Air unit"] --> Wifi["802.11 monitor frame"]
    Wifi --> Wfb["WFB packet"]
    Wfb --> Fec["FEC block"]
    Fec --> Rtp["RTP packet"]
    Rtp --> Video["H.264/H.265 Annex-B"]
    Video --> Decode["WebCodecs or native decoder"]
    Fec --> Raw["Raw non-video payload bytes"]
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
7. Video-channel primary fragments emit RTP packets.
8. RTP H.264/H.265 depacketization emits Annex-B frames.

The pipeline emits events as it learns new information. Session packets update
the decryptor. Data packets may produce recovered RTP packets. RTP packets may
or may not complete a video access unit. Only completed access units are sent to
the video decoder.

This event stream is intentionally layered. An app can count WFB payloads,
mirror RTP, and write Annex-B frames from the same receive loop:

```rust
for event in pipeline.push_80211_frame(frame)? {
    match event {
        PipelineEvent::WfbPayload { .. } => counters.wfb += 1,
        PipelineEvent::RtpPacket { payload, .. } => mirror_rtp(&payload)?,
        PipelineEvent::VideoFrame(frame) => decoder.push(frame.data)?,
        _ => {}
    }
}
```

`RtpPacket` and `VideoFrame` can both appear for the same input frame because
the depacketizer may complete an access unit while processing that RTP packet.

Non-video WFB channels stop earlier. `PayloadPipeline` returns recovered payload
bytes after decryption and FEC, without treating them as RTP. Use it for
MAVLink, MSP, CRSF, data ports, or custom radio ports. The station currently
watches the observed OpenIPC MAVLink downlink port and exposes byte counts in
diagnostics. It does not parse MAVLink messages.

## Annex-B Frames

Annex-B is the byte-stream form of H.264/H.265 where NAL units are separated by
start codes such as `00 00 00 01`. This is a convenient boundary for WebCodecs,
file output, and native player integration because the protocol stack can
deliver complete encoded access units without decoding pixels itself.

In this project, an Annex-B frame means "one encoded access unit ready for a
decoder." It may contain multiple NAL units, such as parameter sets plus an IDR
slice. Rust marks keyframes so the UI can wait for a valid decoder entry point
after packet loss or decoder reset.

## Decode And Render

The station app decodes with WebCodecs where the browser or WebView supports
the codec string returned by Rust. H.264 is broadly available; H.265 depends on
browser and operating-system support.

The render path is:

```mermaid
flowchart LR
    Rust["Rust/WASM or native backend"] --> Frame["Annex-B frame + metadata"]
    Frame --> Chunk["EncodedVideoChunk"]
    Chunk --> Decoder["WebCodecs VideoDecoder"]
    Decoder --> Surface["VideoFrame"]
    Surface --> Canvas["Canvas render and recording"]
```

## Recording

The station records from the rendered canvas. That means the recording feature
captures what the decoder actually produced, not the raw RF stream. For protocol
debugging, use the native CLI to write Annex-B output or save USB captures.
