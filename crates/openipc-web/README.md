# openipc-web

Rust crate and generated npm package for browser/WASM OpenIPC applications.

The Rust crate is published as `openipc-web` on crates.io. Its build script
generates the npm package `@openipc-rs/web`, which contains the `.wasm`,
JavaScript glue, and TypeScript definitions used by browser apps.

This is the browser SDK layer. It exposes the Rust OpenIPC transport pipeline to
JavaScript:

- Realtek USB RX transfer parsing
- WFB session/decryption/FEC handling
- RTP depacketization
- H.264/H.265 Annex-B frame output for WebCodecs
- Recovered raw payload bytes for caller-selected route IDs
- Compatibility output for the default telemetry downlink tap via
  `mavlinkPayloads`
- Adaptive-link feedback helpers
- WebUSB Realtek device access
- Realtek diagnostics and calibration hooks: false-alarm counters, PHYDM DIG
  watchdog ticks, RTL8812 power tracking, RTL8812/RTL8814 IQK, C2H packets,
  RTL8814 TX-status reports, and optional bad-FCS packet retention

It does not include a UI or video renderer. Applications are expected to feed
the encoded frames into WebCodecs, MSE, a worker pipeline, or another renderer.

## Install

For browser applications, install the generated npm package:

```sh
bun add @openipc-rs/web
```

Rust workspace users can depend on the crate directly when building the WASM
package from source:

```toml
[dependencies]
openipc-web = "0.1"
```

## Basic Shape

```ts
import init, {
  OpenIpcReceiver,
  WebUsbRealtekDevice,
  supportedUsbFilters,
} from "@openipc-rs/web";

await init();

const filters = JSON.parse(supportedUsbFilters());
const usbDevice = await navigator.usb.requestDevice({ filters });
const radio = await WebUsbRealtekDevice.fromWebUsbDevice(usbDevice);
const telemetryChannelId = (channelId & 0xffffff00) | 0x10;
const receiver = OpenIpcReceiver.withKeypairOnly(
  channelId,
  keypairBytes,
  minimumEpoch,
);
receiver.addKeyedRoute(2, telemetryChannelId, keypairBytes, minimumEpoch);

const initReport = await radio.initializeMonitorWithOptions(
  channel,
  channelWidthMhz,
  channelOffset,
  false,
);
console.log(initReport.chip, initReport.status);

while (running) {
  const transfers = await radio.readRxTransfers(32768, 4);
  for (const transfer of transfers) {
    const batch = receiver.pushRxTransferProfiledWithRouteIds(
      transfer,
      false,
      new Uint32Array([2]),
    );
    for (const frame of batch.frames) {
      // frame.data is encoded H.264/H.265 Annex-B data.
      // Feed it into WebCodecs as an EncodedVideoChunk.
    }

    for (const payload of batch.rawPayloads) {
      // payload.data is raw recovered bytes for the requested route.
      // The SDK does not parse MAVLink/MSP/CRSF/IP/vendor data for you.
      console.log(payload.routeId, payload.channelId.toString(16), payload.data.byteLength);
    }
  }
}
```

`withKeypair(...)` is still available as a compatibility shortcut. It creates
the video route plus a default telemetry downlink route and returns that route's
bytes through both `rawPayloads` and the older `mavlinkPayloads` alias. New
apps should prefer `withKeypairOnly(...)`, `addKeyedRoute(...)`, and
`pushRxTransferProfiledWithRouteIds(...)` so the route list is explicit.

The underlying Rust core is generic: `ReceiverRuntime` uses route IDs for
configured WFB channels and can expose raw payload bytes for telemetry,
tunnel/data, RTP mirroring, Opus audio, or custom channel IDs. Parse MAVLink,
MSP, CRSF, IP, Opus, or vendor data in your app layer.

Use `fromWebUsbDeviceWithOptions(device, txEndpointOverride)` if a hardware
variant needs a specific bulk-OUT endpoint. Pass `-1` for the default endpoint
selection. For a custom VID/PID not in the built-in table, request the device
with your own WebUSB filter and call `fromWebUsbDeviceAdvanced(device, -1, vid,
pid)`.

Use `initializeMonitorAdvanced(...)` for bring-up experiments:

```ts
await radio.initializeMonitorAdvanced(
  channel,
  channelWidthMhz,
  channelOffset,
  false, // acceptBadFcs
  false, // skipTxPower
  false, // forceIqk
  false, // disableIqk
  "kernel", // RTL8814 firmware path: "kernel" or "rtw88"
  -1, // RTL8814 chunk override; -1 means default
);
```

## WebCodecs Rendering

The Rust/WASM side outputs compressed H.264/H.265 frames. Pass those frames to
WebCodecs and render the decoded `VideoFrame` objects.

```html
<canvas id="video"></canvas>
```

```ts
import init, {
  OpenIpcReceiver,
  WebUsbRealtekDevice,
  supportedUsbFilters,
  type OpenIpcVideoFrame,
} from "@openipc-rs/web";

const canvas = document.querySelector<HTMLCanvasElement>("#video")!;
const ctx = canvas.getContext("2d", { alpha: false })!;

let decoder: VideoDecoder | undefined;
let decoderKey = "";
let waitingForKeyframe = true;
let baseRtpTimestamp: number | undefined;
let baseTimestampUs = 0;

function timestampUs(rtpTimestamp: number): number {
  if (baseRtpTimestamp === undefined) {
    baseRtpTimestamp = rtpTimestamp >>> 0;
    baseTimestampUs = Math.round(performance.now() * 1000);
  }

  const delta = (rtpTimestamp >>> 0) - baseRtpTimestamp;
  return baseTimestampUs + Math.round((delta * 1_000_000) / 90_000);
}

function renderFrame(frame: VideoFrame) {
  try {
    const width = frame.displayWidth || frame.codedWidth;
    const height = frame.displayHeight || frame.codedHeight;
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    ctx.drawImage(frame, 0, 0, width, height);
  } finally {
    frame.close();
  }
}

async function ensureDecoder(frame: OpenIpcVideoFrame): Promise<boolean> {
  const codec = frame.codecString;
  const key = `${frame.codec}:${codec}`;
  if (decoder && decoderKey === key) {
    return true;
  }

  const config: VideoDecoderConfig =
    frame.codec === "h264"
      ? {
          codec,
          avc: { format: "annexb" },
          hardwareAcceleration: "prefer-hardware",
          optimizeForLatency: true,
        }
      : {
          codec,
          hevc: { format: "annexb" },
          hardwareAcceleration: "prefer-hardware",
          optimizeForLatency: true,
        };

  const support = await VideoDecoder.isConfigSupported(config);
  if (!support.supported) {
    return false;
  }

  decoder?.close();
  decoder = new VideoDecoder({
    output: renderFrame,
    error: (error) => {
      console.warn("VideoDecoder error", error);
      waitingForKeyframe = true;
    },
  });
  decoder.configure(support.config ?? config);
  decoderKey = key;
  waitingForKeyframe = true;
  return true;
}

async function decodeOpenIpcFrame(frame: OpenIpcVideoFrame) {
  if (!(await ensureDecoder(frame))) {
    return;
  }
  if (waitingForKeyframe && !frame.isKeyFrame) {
    return;
  }

  waitingForKeyframe = false;
  decoder!.decode(
    new EncodedVideoChunk({
      type: frame.isKeyFrame ? "key" : "delta",
      timestamp: timestampUs(frame.timestamp),
      data: frame.data,
    }),
  );
}

await init();

const filters = JSON.parse(supportedUsbFilters());
const usbDevice = await navigator.usb.requestDevice({ filters });
const radio = await WebUsbRealtekDevice.fromWebUsbDevice(usbDevice);
const receiver = OpenIpcReceiver.withKeypair(
  channelId,
  keypairBytes,
  minimumEpoch,
);

await radio.initializeMonitor(channel, channelWidthMhz, channelOffset);

while (running) {
  const transfers = await radio.readRxTransfers(32768, 4);
  for (const transfer of transfers) {
    const batch = receiver.pushRxTransferProfiled(transfer);
    for (const frame of batch.frames) {
      await decodeOpenIpcFrame(frame);
    }
  }
}
```

## Opus Audio

OpenIPC audio is commonly Opus RTP payload type 98 mixed into the main video RTP
route. Use the filtered RTP tap API to copy only those packets back to
JavaScript while Rust continues depacketizing H.264/H.265 video from the same
route. If you use a custom wfb-ng profile with a separate audio radio port,
register that route id and tap payload type 98 on that route instead.

Opus has no extra RTP depacketization step here: after the RTP header is
removed, the remaining bytes are the Opus payload for `EncodedAudioChunk`.
Use the Opus RTP clock rate of 48 kHz when converting RTP timestamps to
microseconds.

```ts
const AUDIO_ROUTE = 3;
const OPUS_PT = 98;
const audioChannelId = channelId;

const receiver = OpenIpcReceiver.withKeypairOnly(channelId, keypairBytes, 0n);
receiver.addKeyedRoute(AUDIO_ROUTE, audioChannelId, keypairBytes, 0n);

const audioDecoder = new AudioDecoder({
  output: (audioData) => {
    // Copy AudioData into an AudioBuffer or AudioWorklet for playback.
    audioData.close();
  },
  error: console.warn,
});
audioDecoder.configure({
  codec: "opus",
  sampleRate: 48_000,
  numberOfChannels: 1,
});

function parseRtp(packet: Uint8Array) {
  if (packet.length < 12 || packet[0] >> 6 !== 2) return null;
  const csrc = packet[0] & 0x0f;
  const pt = packet[1] & 0x7f;
  const timestamp =
    ((packet[4] << 24) | (packet[5] << 16) | (packet[6] << 8) | packet[7]) >>> 0;
  const offset = 12 + csrc * 4;
  return { pt, timestamp, payload: packet.subarray(offset) };
}

const batch = receiver.pushRxTransferProfiledWithRouteIdsAndRtpTaps(
  transfer,
  false,
  new Uint32Array([]),
  new Uint32Array([AUDIO_ROUTE]),
  new Uint8Array([OPUS_PT]),
);

for (const payload of batch.rawPayloads) {
  const rtp = parseRtp(payload.data);
  if (!rtp) continue;
  audioDecoder.decode(
    new EncodedAudioChunk({
      type: "key",
      timestamp: Math.round(performance.now() * 1000),
      data: rtp.payload,
    }),
  );
}
```

## Adaptive-Link Feedback

The browser SDK can also send the ground-station adaptive-link feedback path.
Use epoch milliseconds for the feedback clock, matching the station app.

```ts
import { OpenIpcAdaptiveLink } from "@openipc-rs/web";

const linkId = channelId >>> 8;
const adaptive = new OpenIpcAdaptiveLink(linkId, keypairBytes, 0n, 1, 5);

await radio.setTxPowerOverride(channel, uplinkTxPower);

while (running) {
  const nowMs = Date.now();
  const transfers = await radio.readRxTransfers(32768, 4);

  for (const transfer of transfers) {
    adaptive.recordRxTransfer(transfer, nowMs);

    const batch = receiver.pushRxTransferProfiled(transfer);
    adaptive.recordReceiverCounters(receiver, nowMs);

    for (const frame of batch.frames) {
      await decodeOpenIpcFrame(frame);
    }
  }

  await adaptive.tickAndSend(radio, nowMs, channel);
}
```

## Driver Diagnostics

Browser apps can call the same WebUSB driver hooks used by the station UI:

```ts
import { WebUsbPhydmWatchdog, WebUsbPowerTracking8812 } from "@openipc-rs/web";

const fa = await radio.readFalseAlarmCounters();
console.log("false alarms", fa.all, "CCA", fa.ccaAll);

const iqk = await radio.runIqk(channel);
console.log("IQK", iqk.chip, iqk.ran);

const dig = new WebUsbPhydmWatchdog();
const digReport = await dig.tick(radio);
console.log("DIG IGI", digReport.previousIgi, "->", digReport.currentIgi);

const pwr = new WebUsbPowerTracking8812();
await pwr.init(radio);
const pwrReport = await pwr.tick(radio, channel, channelWidthMhz);
console.log("power tracking applied", pwrReport.applied);
```

Initialization and diagnostic APIs return typed wasm-bindgen objects. The few
string-returning APIs that remain, such as `supportedUsbFilters()` and FEC
counter helpers, are JSON because their shapes are either browser API input or
debug/status snapshots.

The bad-FCS flag is intentionally explicit. Normal video receive should pass
`false`; diagnostics and experiments can pass `true` to keep corrupted RX
frames surfaced by the Realtek descriptor.

Diagnostics are explicit calls rather than background pollers. In a browser app,
schedule them with your own timer, animation frame, or worker if you need one.
That keeps WebUSB transfers predictable and avoids a library-created loop
competing with video RX/TX.

## Browser Requirements

- HTTPS or localhost secure context
- WebUSB support
- WebCodecs support for playback in typical browser apps
- A supported Realtek 802.11ac USB adapter

## Build From Source

From the repository root:

```sh
bun run --cwd crates/openipc-web build
```

The build generates the publishable package in:

```text
crates/openipc-web/pkg
```

Generated files are not committed to Git.
