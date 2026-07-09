---
sidebar_position: 3
---

# Architecture

`openipc-rs` keeps protocol logic in shared Rust crates and pushes platform
APIs to the edges. The main design goal is that the browser and native paths do
not reimplement the OpenIPC packet stack in different languages.

```mermaid
flowchart LR
    subgraph Shared["Shared Rust crates"]
        Core["openipc-core<br/>WFB, FEC, RTP, Annex-B, raw payloads"]
        Rtl["openipc-rtl88xx<br/>Realtek USB HAL"]
        Video["openipc-video<br/>platform H.264/H.265 decode"]
        Uplink["openipc-uplink<br/>userspace IPv4/UDP/TCP and SSH"]
    end

    Nebulus["Nebulus<br/>egui native, Android, or WASM"] --> Rtl
    Native["Native CLI app<br/>openipc-cli"] --> Rtl
    Browser["Browser app<br/>WebUSB permission"] --> Wasm["openipc-web<br/>wasm-bindgen"]
    Wasm --> Rtl
    Rtl --> Core
    Core --> Uplink
    Uplink --> Rtl
    Core --> Output["Annex-B frames<br/>raw payload bytes<br/>metrics<br/>adaptive feedback"]
    Output --> Video
    Output --> Players["UDP/file output<br/>recording"]
    Video --> NativeSurface["CVPixelBuffer, DMA/GBM, D3D11<br/>AHardwareBuffer, browser VideoFrame"]
```

## Shared Rust Responsibilities

- Realtek RX aggregate parsing from 24-byte USB RX descriptors.
- First-valid-copy WFB selection across independent receive adapters without a
  comparison delay.
- OpenIPC/WFB 802.11 frame filtering.
- WFB session-key handling, data decryption, FEC recovery, and counters.
- RTP parsing and H.264/H.265 depacketization into Annex-B frames.
- Decoder configuration, bounded frame queues, and decoder statistics in
  `openipc-video`; the actual decoder and retained output surface remain
  platform-specific across desktop, Android, and WebAssembly.
- Generic recovered-payload taps for non-video WFB radio ports. The core crate
  returns bytes and packet sequence metadata; application crates decide whether
  those bytes are MAVLink, MSP, CRSF, IP, or something else.
- Adaptive-link quality windows and feedback packet construction.
- WFB tunnel framing, userspace IPv4/UDP/TCP, virtual async streams, SSH, and
  typed VTX control in `openipc-uplink`.
- WFB uplink encryption, FEC parity generation, radiotap headers, and 802.11
  wrapping.

## Platform Responsibilities

The shared crates do not try to hide every platform difference. They hide the
protocol details, then let each target own the APIs that make sense there.

### Native

- USB discovery, open, reset, claim, endpoint discovery, and bulk IO through
  `nusb`.
- Realtek TX descriptor construction for monitor-injection packets before USB
  bulk OUT.
- CLI output as Annex-B or RTP-over-UDP.
- Nebulus per-adapter bulk-IN workers feeding one protocol and decoder worker;
  a lower-priority bounded radio worker owns auxiliary bulk-OUT and Jaguar3
  maintenance.

### Browser

- JavaScript owns each WebUSB permission prompt because browsers require a user
  gesture. Rust can concurrently operate every adapter already authorized by
  the user.
- The granted `UsbDevice` is passed into Rust/WASM through `nusb-webusb`,
  imported as `nusb`.
- Rust/WASM initializes the Realtek adapter, performs bulk IN/OUT, and returns
  typed video frames and metrics to React.
- React uses WebCodecs for playback and canvas capture for recording. Rust/WASM
  applications may instead drive `openipc_video::WebDecoder` and receive the
  same browser `VideoFrame` handles in Rust.
- Nebulus drives `openipc_video::WebDecoder` directly and keeps WebUSB,
  protocol reconstruction, and WebCodecs orchestration in Rust/WASM.
- Its persistent bounded WebUSB OUT queue and separately cancellable Jaguar3
  maintenance task allow the receive future to continue while those promises
  are pending.
- Browser VTX control uses no browser socket. Rust/WASM feeds recovered tunnel
  packets into smoltcp; Russh consumes its virtual TCP stream, adaptive-link
  uses its UDP socket, and outbound IP packets return through WebUSB bulk OUT.

### Desktop Application

Nebulus's native worker submits
Annex-B access units to `openipc-video`, receives a retained platform decoder
surface, and hands the newest presentable frame to egui. That path avoids the
browser and WebView boundaries entirely.

## Copy Boundaries

Nebulus has no JavaScript frame callback. It keeps encoded video and decoder
control in Rust, coalesces retained native decoder surfaces, uploads NV12
planes to persistent GPU textures, and performs color conversion in a shader.
CPU RGBA conversion is only a compatibility fallback. Direct IOSurface,
DMA-BUF, and D3D11 imports could remove the remaining plane copy.

The receive worker returns each bulk-IN buffer to `nusb` as soon as the parser
and WFB runtime release their borrow. It then moves the completed Annex-B
access unit into the decoder before processing audio, UDP, VPN, adaptive-link,
or diagnostic output. This ordering keeps optional routes out of the video
critical path.

On Android MediaCodec renders into a SurfaceTexture-backed external GLES
texture; the UI boundary carries only presentation metadata and the paint
callback latches the newest image. In the browser, WebCodecs `VideoFrame` is uploaded directly to a
persistent WebGL texture; decoded pixels do not pass through a WASM byte array.

## Data Flow

```mermaid
flowchart TD
    A1["Realtek adapter A<br/>USB bulk IN"] --> B1["RX aggregate parser A"]
    A2["Realtek adapter B<br/>USB bulk IN"] --> B2["RX aggregate parser B"]
    B1 --> S["First-valid-copy selector"]
    B2 --> S
    S --> C["OpenIPC 802.11/WFB filter"]
    C --> D["WFB session/data decrypt"]
    D --> E["Reed-Solomon FEC recovery"]
    E --> P["ReceiverRuntime<br/>route fanout"]
    U["Native UDP<br/>one recovered RTP packet/datagram"] --> P
    P --> F["RTP depacketizer<br/>video route"]
    F --> G["Annex-B H.264/H.265 frames"]
    G --> H["WebCodecs, file output, or UDP mirror"]
    P --> J["raw telemetry/data bytes<br/>non-video channels"]
```

The UDP branch uses `with_direct_video_route` and `push_direct_payload`. It
bypasses the radio-specific stages above `ReceiverRuntime`, while retaining
route taps, RTP reordering, codec tracking, and H.264/H.265 depacketization.

Adaptive-link feedback flows the other direction:

```mermaid
flowchart TD
    A["RSSI/SNR/FEC windows"] --> B["adaptive-link feedback text"]
    B --> C["smoltcp IPv4/UDP"]
    T["Native OS TUN<br/>complete IP packet"] --> Q["UplinkEngine<br/>bounded priority queues"]
    C --> Q
    Q --> P["same-tick IP aggregation<br/>atomic FEC batch"]
    P --> D["WFB encrypt/FEC"]
    D --> E["radiotap + 802.11 frame"]
    E --> F["Realtek TX descriptor"]
    F --> G["USB bulk OUT"]
```

Nebulus uses one `UplinkEngine`, one `UserspaceNetwork`, and one WFB tunnel TX
session for adaptive feedback, internal VTX TCP/SSH traffic, and optional native
TUN traffic. The OS-built TUN packet bypasses smoltcp socket processing but
joins a lower-priority bounded queue before OpenIPC length framing. The engine
aggregates small IP packets, atomically admits complete session/data/FEC frame
groups, and retains failed frames for bounded retry until the native or WebUSB
sink reports a real completion. One transmitter per link and radio port avoids
competing session epochs on port `0xa0`.
