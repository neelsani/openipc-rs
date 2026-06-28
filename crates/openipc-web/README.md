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
- Recovered raw payload bytes for the default MAVLink downlink convenience tap
  via `mavlinkPayloads`
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
const receiver = OpenIpcReceiver.withKeypair(
  channelId,
  keypairBytes,
  minimumEpoch,
);

await radio.initializeMonitorWithOptions(
  channel,
  channelWidthMhz,
  channelOffset,
  false,
);

while (running) {
  const transfers = await radio.readRxTransfers(32768, 4);
  for (const transfer of transfers) {
    const batch = receiver.pushRxTransferProfiledWithOptions(transfer, false);
    for (const frame of batch.frames) {
      // frame.data is encoded H.264/H.265 Annex-B data.
      // Feed it into WebCodecs as an EncodedVideoChunk.
    }

    for (const payload of batch.mavlinkPayloads) {
      // payload.data is raw recovered bytes from the OpenIPC MAVLink RX port.
      // The SDK does not parse or forward MAVLink for you.
      console.log(payload.channelId.toString(16), payload.data.byteLength);
    }
  }
}
```

The `mavlinkPayloads` field is named for the default OpenIPC downlink port that
the browser SDK watches. The underlying Rust core is generic: `PayloadPipeline`
can recover bytes from `RadioPort::MavlinkRx`, `RadioPort::DataRx`, or
`RadioPort::Custom(n)`. Parse MAVLink, MSP, CRSF, IP, or vendor data in your app
layer.

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

const fa = JSON.parse(await radio.readFalseAlarmCounters());
await radio.runIqk(channel);

const dig = new WebUsbPhydmWatchdog();
const digReport = JSON.parse(await dig.tick(radio));

const pwr = new WebUsbPowerTracking8812();
await pwr.init(radio);
const pwrReport = JSON.parse(await pwr.tick(radio, channel, channelWidthMhz));
```

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
