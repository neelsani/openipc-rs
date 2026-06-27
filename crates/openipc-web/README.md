# @openipc-rs/web

WebAssembly and WebUSB bindings for the `openipc-rs` receiver stack.

This package is the browser SDK layer. It exposes the Rust OpenIPC transport
pipeline to JavaScript:

- Realtek USB RX transfer parsing
- WFB session/decryption/FEC handling
- RTP depacketization
- H.264/H.265 Annex-B frame output for WebCodecs
- Adaptive-link feedback helpers
- WebUSB Realtek device access

It does not include a UI or video renderer. Applications are expected to feed
the encoded frames into WebCodecs, MSE, a worker pipeline, or another renderer.

## Install

```sh
npm install @openipc-rs/web
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
const receiver = OpenIpcReceiver.withKeypair(channelId, keypairBytes, minimumEpoch);

await radio.initializeMonitor(channel, channelWidthMhz, channelOffset);

while (running) {
  const transfer = await radio.readRxTransfer(32768);
  const batch = receiver.pushRxTransferProfiled(transfer);
  for (const frame of batch.frames) {
    // frame.data is encoded H.264/H.265 Annex-B data.
    // Feed it into WebCodecs as an EncodedVideoChunk.
  }
}
```

## WebCodecs Rendering

The Rust/WASM side outputs compressed H.264/H.265 frames. Your app should pass
those frames to WebCodecs and render the decoded `VideoFrame` objects.

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
      ? { codec, avc: { format: "annexb" }, hardwareAcceleration: "prefer-hardware", optimizeForLatency: true }
      : { codec, hevc: { format: "annexb" }, hardwareAcceleration: "prefer-hardware", optimizeForLatency: true };

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
const receiver = OpenIpcReceiver.withKeypair(channelId, keypairBytes, minimumEpoch);

await radio.initializeMonitor(channel, channelWidthMhz, channelOffset);

while (running) {
  const transfer = await radio.readRxTransfer(32768);
  const batch = receiver.pushRxTransferProfiled(transfer);
  for (const frame of batch.frames) {
    await decodeOpenIpcFrame(frame);
  }
}
```

## Browser Requirements

- HTTPS or localhost secure context
- WebUSB support
- WebCodecs support for playback in typical browser apps
- A supported Realtek 802.11ac USB adapter

## Build From Source

From the repository root:

```sh
npm --prefix crates/openipc-web run build
```

The build generates the publishable package in:

```text
crates/openipc-web/pkg
```

Generated files are not committed to Git.
