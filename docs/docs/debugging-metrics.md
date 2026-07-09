---
sidebar_position: 10
---

# Debugging And Metrics

Nebulus exposes the receive path as counters, health states, logs, RTP details,
and stage timings. Debug from left to right through the pipeline; the first stage
that stops advancing is normally the useful boundary.

## Pipeline Health

The health view follows this order:

1. USB adapter initialized.
2. USB transfers arriving.
3. Realtek RX descriptors and 802.11 frames parsed.
4. Frames accepted for the configured WFB channel.
5. WFB session established and payload recovered.
6. RTP packets arriving on the video route.
7. Codec parameter sets complete.
8. Encoded access units extracted.
9. Platform decoder active.
10. Optional audio and VPN routes healthy.

With **UDP RTP** selected, the first five USB/WFB rows are replaced by **UDP
listener initialized** and **UDP datagrams arriving**. RTP and every downstream
row retain the same meaning.

With receive diversity enabled, the same view includes a **Receive adapters**
section. Check each radio's online state, USB errors, queue drops, RSSI/SNR,
first-copy wins, and duplicates. Queue drops should remain zero. A secondary
with few wins can still be useful if those wins occur during primary antenna
shadowing.

A pending marker means that stage has not been observed in the current receiver
session. It is not automatically an error: audio and VPN remain pending when
those features are disabled.

## Metrics

The rolling graphs intentionally focus on six operational signals:

| Signal             | What it answers                                                   |
| ------------------ | ----------------------------------------------------------------- |
| Link score         | Is the best receive path healthy enough for the selected profile? |
| Post-FEC loss      | How much data remained unrecoverable after FEC?                   |
| FEC repair         | How much damaged primary traffic was reconstructed?               |
| Encoded bitrate    | Is video data arriving at the expected rate?                      |
| Delivered FPS      | Are complete access units reaching the decoder?                   |
| Processing latency | Is local receive, protocol, or decode work falling behind?        |

RSSI and SNR remain in the video OSD because they are useful while flying.
Audio packet rate and queue depth live with route diagnostics rather than the
main graph grid.

Direct UDP input has no RSSI, WFB loss, or FEC state. In that mode the graph
grid shows encoded bitrate, delivered FPS, decoded FPS, and local processing
latency instead of empty radio graphs; radio-only OSD values display as
unavailable.

## RTP Diagnostics

The RTP view reports sequence number, timestamp, payload type, codec/NAL type,
fragment gaps, malformed packets, unsupported packets, parameter-set state,
keyframe waits, damaged frames forwarded or dropped, and optional
reorder-buffer counters. Nebulus forwards guarded damaged frames to an already
synchronized decoder; a rising forwarded count paired with decoder errors
indicates loss beyond the decoder's error-concealment ability. After such an
error, the decoder waits for a clean keyframe.

For H.264, a decoder needs SPS, PPS, and an IDR. H.265 normally needs VPS, SPS,
PPS, and a BLA/IDR/CRA access unit. “Packets arriving, waiting for IDR” therefore
means the radio and WFB stages have succeeded; inspect whether parameter sets
were seen, whether fragmented NAL units have gaps, and whether the transmitter
is emitting periodic random-access frames.

RTP reorder is off by default. Turn it on only when diagnostics show sequence
reordering rather than simple packet loss. Reordering adds a bounded wait and
cannot recover packets that never arrived.

## Stage Timings

| Stage                           | Meaning                                                                            |
| ------------------------------- | ---------------------------------------------------------------------------------- |
| USB wait                        | Time until a bulk transfer completes; high idle values may simply mean no traffic. |
| Realtek parse                   | Splitting the USB aggregate and interpreting RX descriptors.                       |
| WFB/RTP                         | Filtering, session crypto, FEC, route fanout, and video depacketization.           |
| UDP socket wait                 | Native wait for the next RTP datagram; replaces USB wait for direct UDP input.     |
| RTP pipeline                    | Direct route fanout, optional reorder, and depacketization for UDP input.          |
| Decoder submit                  | Preparing and handing an access unit to the platform decoder.                      |
| USB completion to decode submit | Critical receive path before auxiliary route work.                                 |
| UDP datagram to decode submit   | Equivalent direct-UDP critical path.                                               |
| Hardware decode                 | Submit-to-output latency reported by `openipc-video`.                              |
| Decode to GPU upload            | Latest-frame event wait plus platform texture upload/latch.                        |
| Routes                          | Inspect/log/UDP/audio processing for recovered payload routes.                     |
| Receive batch                   | All local work, including work performed after video submission.                   |

The view keeps last, average, p95, maximum, and sample count. Compare latency
with queue and drop counters: a low average can hide periodic stalls, while a
high USB wait with no incoming traffic is normal.

## Common Failure Patterns

| Symptom                               | Check next                                                                                                        |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| No adapter listed                     | USB permission, cable/OTG support, device VID/PID, and OS driver binding.                                         |
| Adapter opens but no USB bytes        | RF channel/width, monitor initialization log, antenna/VTX power, and endpoint errors.                             |
| USB bytes but no accepted frames      | Descriptor family, CRC/ICV counters, Link ID, and configured channel ID.                                          |
| Accepted frames but no WFB payload    | Key mismatch, missing session packet, wrong radio port, or FEC/session errors.                                    |
| WFB payload but no RTP                | Video radio port or transmitter destination is wrong.                                                             |
| RTP but no access units               | Fragment gaps, unsupported packetization, malformed RTP, or reorder requirement.                                  |
| Access units but waiting for keyframe | Missing SPS/PPS or VPS/SPS/PPS, long GOP, or lost random-access frame.                                            |
| Decoder errors                        | Codec preference, unsupported H.265 profile/bit depth, invalid configuration, or platform backend failure.        |
| Good decode FPS but poor presentation | GPU upload/render path, window load, or output-surface conversion fallback.                                       |
| Audio packets but silence             | Wrong audio payload type, Opus settings, muted volume, suspended browser audio context, or output-device failure. |

## Logs And Environment

The Logs tab's **Capture** selector controls verbosity. Nebulus installs a
process-wide Rust `log` subscriber. Records emitted by
`openipc-core`, `openipc-rtl88xx`, `openipc-video`, Nebulus, and dependencies are
written to stderr/Logcat or the browser console and copied into the bounded Logs
tab. The tab can filter by minimum severity and search both target and message.

| Verbosity    | Captured levels                 | Intended use                                      |
| ------------ | ------------------------------- | ------------------------------------------------- |
| Low          | Warn, Error                     | Flight use when only actionable failures matter   |
| Normal       | Info, Warn, Error               | Everyday startup and session state                |
| High         | Debug and above                 | Adapter, WFB, FEC, RTP, and decoder investigation |
| Very verbose | Trace, Debug, Info, Warn, Error | Register, USB transfer, and per-packet tracing    |

Packet-level Trace records from RTP, WFB, and USB targets are sampled one in
128 before formatting. Counters and stage metrics still account for every
packet. The capture queue keeps at most 4,000 pending records and the visible
app history trims itself at 1,000 entries. A separate 10,000-record rolling
history feeds support bundles. Use the target search to isolate
`openipc_rtl88xx::register`, `openipc_rtl88xx::usb`, `openipc_core::wfb`,
`openipc_core::fec`, `openipc_core::rtp`, or `openipc_video`.

A useful startup trace contains device open, chip probe, firmware/EFUSE
initialization, monitor channel setup, WFB session acceptance, codec
configuration, keyframe acceptance, and decoder activation. Normal remains the
recommended setting for measurements; even sampled trace output adds work.

For “OpenIPC-WASM works but Nebulus does not,” export a support bundle after
the failure and compare boundaries in this order:

1. Open `driver_init.json`. Confirm raw `SYS_CFG`/`SYS_CFG2`, selected chip,
   descriptor kind, EFUSE RFE/board/RF paths, every successful init stage, and
   post-init `CR`, `RCR`, `RXFLTMAP2`, and `RXDMA_STATUS`.
2. In `report.json`, find the last entry in `receive_milestones_seconds`.
3. Check the adapter parser histogram and first descriptor/rejection samples.
   Non-zero trailing bytes can be normal padding; non-zero trailing _data_ plus
   invalid packet lengths usually points to a descriptor or alignment mismatch.
4. If matching 802.11 frames stop before a WFB session, verify link ID and key.
   If WFB payloads reach RTP, move to the RTP and codec-config counters instead
   of changing the radio driver.

The complete register trace is structured and unsampled in
`driver_init.json`; trace-level console logging remains sampled for latency.

The Environment view identifies target OS/architecture, renderer, USB API,
decoder backend, codec support, acceleration status where the platform exposes
it, logical processor count, browser user agent, and the largest resolution/FPS
observed in the current session. An observed maximum is not a hardware limit.
