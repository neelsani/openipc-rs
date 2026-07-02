//! Low-latency platform video decoding for OpenIPC applications.
//!
//! The crate accepts complete Annex-B access units, keeps codec parsing and
//! backpressure behavior platform-neutral, and delegates actual decoding to a
//! platform decoder. H.264 and H.265 are supported by the shared API.

#![deny(unsafe_op_in_unsafe_fn)]

/// Platform-neutral decoder inputs, outputs, options, capabilities, and errors.
pub mod api;
/// Platform decoder implementations and their retained surface types.
pub mod backends;
/// Annex-B parsing and H.264/H.265 configuration tracking.
pub mod codecs;
/// Small queue, mailbox, and RTP timestamp utilities for decode runtimes.
pub mod runtime;

pub use api::{
    CodecCapability, CodecConfig, CodecStreamInfo, DecodedFrame, DecodedSurface,
    DecoderCapabilities, DecoderOptions, DecoderStats, EncodedAccessUnit, FrameDimensions,
    PixelFormat, SubmitOutcome, VideoCodec, VideoDecoder, VideoError, VideoTimestamp,
};
pub use codecs::{CodecConfigTracker, ConfigUpdate, H264Config, H265Config};

#[cfg(target_os = "macos")]
pub use backends::macos::{MacOsDecoder, MacOsVideoFrame};
/// Decoder selected for the current native operating system.
#[cfg(target_os = "macos")]
pub type PlatformDecoder = MacOsDecoder;

#[cfg(target_os = "linux")]
pub use backends::linux::{LinuxDecoder, LinuxVideoFrame};
/// Decoder selected for the current native operating system.
#[cfg(target_os = "linux")]
pub type PlatformDecoder = LinuxDecoder;

#[cfg(target_os = "windows")]
pub use backends::windows::{WindowsDecoder, WindowsNv12Frame, WindowsVideoFrame};
/// Decoder selected for the current native operating system.
#[cfg(target_os = "windows")]
pub type PlatformDecoder = WindowsDecoder;

#[cfg(target_os = "android")]
pub use backends::android::{AndroidDecoder, AndroidImagePlane, AndroidVideoFrame};
/// Decoder selected for the current native operating system.
#[cfg(target_os = "android")]
pub type PlatformDecoder = AndroidDecoder;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use backends::web::{WebDecoder, WebVideoFrame};
/// Decoder selected for a browser/WebAssembly build.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type PlatformDecoder = WebDecoder;
