---
sidebar_position: 7
---

# WASM SDK Usage

`openipc-web` is the browser SDK layer. It exposes the Rust receiver pipeline
and the WebUSB Realtek device wrapper to JavaScript.

## Install Or Build

When published:

```sh
bun add @openipc-rs/web
```

From this repository:

```sh
bun run --cwd crates/openipc-web build
```

The generated package is written to `crates/openipc-web/pkg`.

## Browser Receive Flow

```mermaid
sequenceDiagram
    participant UI as React UI
    participant USB as navigator.usb
    participant WASM as openipc-web WASM
    participant RTL as nusb WebUSB Realtek
    participant Codec as WebCodecs

    UI->>USB: requestDevice({ filters })
    USB-->>UI: UsbDevice
    UI->>WASM: WebUsbRealtekDevice.fromWebUsbDevice(device)
    WASM->>RTL: claim interface and discover endpoints
    UI->>WASM: initializeMonitor(channel, width, offset)
    WASM->>RTL: firmware, MAC, BB/RF, channel setup
    loop receive
        UI->>WASM: readRxTransfer(32768)
        WASM-->>UI: Uint8Array
        UI->>WASM: receiver.pushRxTransferProfiled(bytes)
        WASM-->>UI: frames and metrics
        UI->>Codec: EncodedVideoChunk
    end
```

## Minimal Receiver

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

const channelId = 7669206 << 8;
const keypairBytes = new Uint8Array(await (await fetch("/gs.key")).arrayBuffer());
const receiver = OpenIpcReceiver.withKeypair(channelId, keypairBytes, 0n);

await radio.initializeMonitor(36, 20, 0);

while (true) {
  const transfer = await radio.readRxTransfer(32768);
  const batch = receiver.pushRxTransferProfiled(transfer);

  for (const frame of batch.frames) {
    // frame.data is encoded Annex-B H.264/H.265.
    console.log(frame.codec, frame.data.byteLength, frame.isKeyFrame);
  }
}
```

`pushRxTransferProfiled` is usually the best entry point for applications. It
returns frames and counters from the same call, so the UI can update metrics
without replaying the transfer through another parser.

## WebCodecs Rendering

```ts
import type { OpenIpcVideoFrame } from "@openipc-rs/web";

const canvas = document.querySelector<HTMLCanvasElement>("#video")!;
const canvasContext = canvas.getContext("2d", { alpha: false })!;

let decoder: VideoDecoder | undefined;
let decoderKey = "";
let waitingForKeyframe = true;

async function configureDecoder(frame: OpenIpcVideoFrame) {
  const key = `${frame.codec}:${frame.codecString}`;
  if (decoder && decoderKey === key) {
    return true;
  }

  const config: VideoDecoderConfig =
    frame.codec === "h264"
      ? {
          codec: frame.codecString,
          avc: { format: "annexb" },
          hardwareAcceleration: "prefer-hardware",
          optimizeForLatency: true,
        }
      : {
          codec: frame.codecString,
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
    output: (videoFrame) => {
      const width = videoFrame.displayWidth || videoFrame.codedWidth;
      const height = videoFrame.displayHeight || videoFrame.codedHeight;
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
      canvasContext.drawImage(videoFrame, 0, 0, width, height);
      videoFrame.close();
    },
    error: () => {
      waitingForKeyframe = true;
    },
  });
  decoder.configure(support.config ?? config);
  decoderKey = key;
  waitingForKeyframe = true;
  return true;
}

async function decodeFrame(frame: OpenIpcVideoFrame) {
  if (!(await configureDecoder(frame))) {
    return;
  }
  if (waitingForKeyframe && !frame.isKeyFrame) {
    return;
  }

  waitingForKeyframe = false;
  decoder!.decode(
    new EncodedVideoChunk({
      type: frame.isKeyFrame ? "key" : "delta",
      timestamp: performance.now() * 1000,
      data: frame.data,
    }),
  );
}
```

The timestamp should be monotonic and in microseconds. The station app maps RTP
timestamps to a local microsecond clock so WebCodecs sees stable timing even
when frames arrive in bursts.

## Adaptive Link In Browser

```ts
import { OpenIpcAdaptiveLink } from "@openipc-rs/web";

const linkId = 7669206;
const adaptive = new OpenIpcAdaptiveLink(linkId, keypairBytes, 0n, 1, 5);

while (running) {
  const nowMs = Date.now();
  const transfer = await radio.readRxTransfer(32768);
  adaptive.recordRxTransfer(transfer, nowMs);

  const batch = receiver.pushRxTransferProfiled(transfer);
  adaptive.recordReceiverCounters(receiver, nowMs);

  for (const frame of batch.frames) {
    await decodeFrame(frame);
  }

  await adaptive.tickAndSend(radio, nowMs, 36);
}
```

`tickAndSend` only sends when the adaptive-link interval says a feedback packet
is due. Call it from the receive loop after updating counters.

## Browser Requirements

- HTTPS or `localhost`.
- WebUSB support.
- WebCodecs support for playback.
- A browser and operating system that allow access to the USB adapter.
- A supported RTL8812/RTL8814/RTL8821-class adapter.
