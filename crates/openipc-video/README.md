# openipc-video

Low-latency H.264/H.265 decoding for OpenIPC applications without FFmpeg or
GStreamer.

`openipc-core` reconstructs complete Annex-B access units from RTP.
`openipc-video` accepts those access units, tracks parameter sets, waits for a
random-access frame, and returns the newest retained decoder surface.

| Target  | Decoder                  | Retained output                             |
| ------- | ------------------------ | ------------------------------------------- |
| macOS   | VideoToolbox             | IOSurface-backed `CVPixelBuffer`            |
| Linux   | `cros-codecs` + VA-API   | GBM/DMA-backed frame                        |
| Windows | Media Foundation + D3D11 | `ID3D11Texture2D` subresource               |
| Android | NDK MediaCodec           | `AImage` with an acquired `AHardwareBuffer` |
| Web     | WebCodecs                | browser `VideoFrame`                        |

Cargo only enables the dependency set for the current target.

The current Linux and Windows H.265 paths expose 8-bit Main/NV12 output.
Main10 needs a P010 surface path there. Android, macOS, and WebCodecs negotiate
their platform-native output, subject to the decoder available on the device.

## Decode Frames

```rust,no_run
use openipc_video::{PlatformDecoder, VideoDecoder};

# #[cfg(any(
#     target_os = "macos",
#     target_os = "linux",
#     target_os = "windows",
#     target_os = "android",
#     all(target_arch = "wasm32", target_os = "unknown"),
# ))]
# fn receive(frame: openipc_core::DepacketizedFrame) -> Result<(), Box<dyn std::error::Error>> {
let mut decoder = PlatformDecoder::new(Default::default())?;
decoder.submit(frame.into())?;

if let Some(decoded) = decoder.latest_frame() {
    let size = decoded.dimensions();
    println!("decoded {}x{}", size.width, size.height);
    // Import decoded.surface with the target renderer.
}
# Ok(())
# }
```

`submit` detects H.264 SPS/PPS and H.265 VPS/SPS/PPS in-band. A changed
configuration rebuilds the platform session and suppresses delta frames until
the next keyframe. The output mailbox holds one frame: if rendering falls
behind, a newer output replaces the stale one instead of increasing latency.

`DecoderOptions` controls the in-flight limit, low-latency preference, and
hardware preference. `DecoderStats` reports waits, dropped inputs, replaced
outputs, platform errors, queue depth, and submit-to-output latency.

Decoder configuration and backpressure are also emitted through the standard
[`log`](https://docs.rs/log) facade. The crate does not install a logger; the
embedding application controls filtering and output.

## Render The Surface

- macOS: import `MacOsVideoFrame::pixel_buffer()` through a
  `CVMetalTextureCache` owned by the renderer.
- Linux: inspect DRM FourCC and plane layout on `LinuxVideoFrame`; use
  `with_mapped_planes()` until `cros-codecs` exposes its DMA-BUF descriptors.
- Windows: use `WindowsVideoFrame::texture()` and `subresource_index()` with
  the D3D11 device returned by `WindowsDecoder::d3d_device()`. Renderers that
  cannot import D3D11 may call `WindowsVideoFrame::copy_nv12()`; its shared
  readback context reuses the staging texture across frames.
- Android: import `AndroidVideoFrame::hardware_buffer()` with EGL, Vulkan, or a
  renderer that supports `AHardwareBuffer`, or use `with_mapped_planes()` for
  portable CPU presentation. Keep the frame alive through draw.
- Web: borrow `WebVideoFrame::video_frame()` for canvas/WebGPU, or use
  `clone_video_frame()` when transferring ownership to JavaScript.

Desktop output surfaces implement `Send + Sync`. Browser frames are local to
their JavaScript executor, and Android image leases should stay on the decoder
or render thread selected by the app.

## Target Notes

Android requires API 26 or newer. The NDK API chooses the preferred decoder but
does not expose reliable hardware/software classification at that API level.
The backend requests a flexible YUV `AImageReader` surface with CPU-read and
GPU-sampled usage, falling back to CPU-readable usage when necessary, and
retains the matching hardware buffer.

The web backend requires WebCodecs in a secure context. Call
`WebDecoder::is_config_supported` once parameter sets are available to check
the exact profile, level, dimensions, and browser support. WebCodecs treats
hardware acceleration as a preference. `flush_async` awaits queued browser
work; the common synchronous `flush` closes it immediately.

Linux builds need VA-API, GBM, DRM, pkg-config, and Clang development files. On
Debian or Ubuntu:

```sh
sudo apt-get install clang libclang-dev libdrm-dev libgbm-dev libva-dev pkg-config
```

Set `OPENIPC_VAAPI_DEVICE=/dev/dri/renderD129` to select a DRM render node.

## Validate

```sh
cargo test -p openipc-video --all-targets
cargo clippy -p openipc-video --all-targets --no-deps -- -D warnings
cargo clippy -p openipc-video --target aarch64-linux-android --all-targets --no-deps -- -D warnings
cargo clippy -p openipc-video --target wasm32-unknown-unknown --all-targets --no-deps -- -D warnings
cargo run -p openipc-video --example inspect_annex_b -- h265 frame.h265
cargo run -p openipc-video --example decode_access_unit -- h265 frame.h265
```

The decode example needs a target-native decoder and a complete random-access
frame containing its parameter sets.

## License

MIT
