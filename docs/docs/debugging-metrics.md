---
sidebar_position: 10
---

# Debugging And Metrics

The station app tracks client-side metrics so receiver performance can be
debugged without guessing.

## Useful Signals

- USB transfer count, bytes, and errors.
- Realtek aggregate parse count and rejected packets.
- WFB session updates, decrypted packets, recovered fragments, and lost
  fragments.
- RTP packets and extracted Annex-B frames.
- WebCodecs decoder name, codec string, resolution, decode errors, and render
  FPS.
- Bitrate and frame-rate estimates.
- Adaptive-link RSSI, SNR, score, FEC changes, and IDR request state.

## Bottleneck Strategy

When video is not smooth, compare the stage counters in order:

1. USB bytes arriving.
2. Realtek packets parsed.
3. WFB packets decrypted.
4. RTP packets emitted.
5. Annex-B frames extracted.
6. WebCodecs frames decoded.
7. Canvas frames rendered.

The first stage that stops increasing usually identifies the bottleneck or
failure boundary.
