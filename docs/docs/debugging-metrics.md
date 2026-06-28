---
sidebar_position: 10
---

# Debugging And Metrics

The station app tracks client-side metrics so receiver performance can be
debugged without guessing. The goal is to answer one question quickly: where did
the frame stop moving?

## Useful Signals

- USB transfer count, bytes, and errors.
- Realtek aggregate parse count and rejected packets.
- WFB session updates, decrypted packets, recovered fragments, and lost
  fragments.
- RTP packets and extracted Annex-B frames.
- Raw payload count and recovered telemetry/data bytes for the configured
  non-video radio port.
- WebCodecs decoder name, codec string, resolution, decode errors, and render
  FPS.
- WebCodecs capability probe: `VideoDecoder`, `EncodedVideoChunk`, H.264, H.265,
  secure context, and WebView user agent.
- Bitrate and frame-rate estimates.
- Adaptive-link RSSI, SNR, score, FEC changes, and IDR request state.

## Stage Timings

The station records timing around the main boundaries:

| Stage            | Meaning                                                                  |
| ---------------- | ------------------------------------------------------------------------ |
| USB read         | Time spent waiting for the next bulk transfer.                           |
| Realtek parse    | Time to split a USB aggregate into packet descriptors and 802.11 frames. |
| OpenIPC pipeline | WFB filtering, decrypt, FEC, RTP, and Annex-B extraction.                |
| Decode enqueue   | Time spent preparing and submitting `EncodedVideoChunk` objects.         |
| Decode to render | Time from encoded input to WebCodecs output.                             |
| Canvas render    | Time to draw the decoded `VideoFrame`.                                   |
| Adaptive TX      | Time spent building and sending feedback packets.                        |

Use these numbers together with counters. A long USB read may simply mean there
is no traffic. A growing decoder queue usually means WebCodecs cannot keep up.
Increasing FEC loss means the RF stream is arriving damaged before the decoder
ever sees it.

## Bottleneck Strategy

When video is not smooth, compare the stage counters in order:

1. USB bytes arriving.
2. Realtek packets parsed.
3. WFB packets decrypted.
4. Raw payload counts increasing if telemetry or data-channel traffic is
   expected.
5. RTP packets emitted.
6. Annex-B frames extracted.
7. Decoder capability probe reports the needed WebCodecs API and codec support.
8. WebCodecs frames decoded.
9. Canvas frames rendered.

The first stage that stops increasing usually identifies the bottleneck or
failure boundary.

## Common Patterns

| Symptom                              | Likely Boundary                                                                                                                                |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| No USB bytes                         | Device permission, driver claim, endpoint discovery, channel setup, or no RF traffic.                                                          |
| USB bytes but no accepted packets    | Realtek descriptor parsing, CRC/ICV drops, wrong channel, or unsupported descriptor variant.                                                   |
| Accepted packets but no WFB payloads | Wrong channel id, wrong radio port, or frame layout mismatch.                                                                                  |
| Video works but no raw payloads      | Air unit is not sending telemetry/data on the expected port, that channel's session packet has not arrived, or the link id/key does not match. |
| WFB payloads but no RTP              | Bad key, missing session packet, epoch filter, or unrecoverable FEC loss.                                                                      |
| RTP but no video frames              | Codec packetization issue or waiting for a keyframe/access unit.                                                                               |
| Video frames but black output        | WebCodecs unsupported codec/config, decoder reset, or no keyframe yet.                                                                         |
| Good decode FPS but low render FPS   | Canvas/rendering path or recording overhead.                                                                                                   |

## Logs

Keep logs enabled when validating a new adapter. The useful sequence is:

1. WASM or desktop runtime ready.
2. Device opened and interface claimed.
3. Realtek monitor initialization report.
4. RX transfer counters increasing.
5. WFB session accepted.
6. Annex-B frames emitted.
7. Decoder configured and rendering.
