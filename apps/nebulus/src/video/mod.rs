#[cfg(not(target_arch = "wasm32"))]
use openipc_video::FrameDimensions;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod native_gpu;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub(crate) use native_gpu::NativeNv12Renderer as PlatformVideoRenderer;
#[cfg(target_os = "android")]
mod android_glow;
#[cfg(target_os = "android")]
pub(crate) use android_glow::AndroidGlowRenderer as PlatformVideoRenderer;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod web_glow;
#[cfg(not(target_arch = "wasm32"))]
use openipc_video::DecodedFrame;
#[cfg(target_os = "macos")]
use openipc_video::PlatformDecoder;
#[cfg(target_os = "macos")]
use openipc_video::VideoDecoder as _;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use web_glow::WebGlowRenderer as PlatformVideoRenderer;

/// CPU-presentable frame consumed by egui.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub(crate) struct PresentedFrame {
    pub(crate) dimensions: FrameDimensions,
    pub(crate) rgba: Vec<u8>,
    pub(crate) decode_latency_ms: f64,
}

/// Convert a target-native decoder surface into tightly packed RGBA pixels.
#[cfg(target_os = "macos")]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn present_frame(
    frame: DecodedFrame<openipc_video::MacOsVideoFrame>,
    decoder: &PlatformDecoder,
) -> Result<PresentedFrame, String> {
    let dimensions = frame.dimensions();
    let rgba = crate::video::platform::macos_rgba(&frame.surface)?;
    Ok(PresentedFrame {
        dimensions,
        rgba,
        decode_latency_ms: decoder.stats().last_decode_latency_us as f64 / 1000.0,
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn fallback_rgba(
    frame: &DecodedFrame<openipc_video::MacOsVideoFrame>,
) -> Result<Vec<u8>, String> {
    platform::macos_rgba(&frame.surface)
}

#[cfg(target_os = "linux")]
pub(crate) fn fallback_rgba(
    frame: &DecodedFrame<openipc_video::LinuxVideoFrame>,
) -> Result<Vec<u8>, String> {
    platform::linux_rgba(&frame.surface, frame.dimensions())
}

#[cfg(target_os = "windows")]
pub(crate) fn fallback_rgba(
    frame: &DecodedFrame<openipc_video::WindowsVideoFrame>,
) -> Result<Vec<u8>, String> {
    let mapped = frame
        .surface
        .copy_nv12()
        .map_err(|error| error.to_string())?;
    platform::windows_rgba(&mapped)
}

#[cfg(target_os = "android")]
pub(crate) fn fallback_rgba(
    _frame: &DecodedFrame<openipc_video::AndroidPresentedFrame>,
) -> Result<Vec<u8>, String> {
    Err("Android direct SurfaceTexture presentation failed".to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod platform;
