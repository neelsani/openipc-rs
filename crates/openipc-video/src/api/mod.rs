//! Stable platform-neutral types used to configure and drive video decoders.

mod capabilities;
mod config;
mod decoder;
mod error;
mod frame;
mod stats;
mod surface;

pub use capabilities::{CodecCapability, DecoderCapabilities};
pub use config::{CodecConfig, CodecStreamInfo, DecoderOptions, VideoCodec};
pub use decoder::{SubmitOutcome, VideoDecoder};
pub use error::VideoError;
pub use frame::{DecodedFrame, EncodedAccessUnit, VideoTimestamp};
pub use stats::DecoderStats;
pub use surface::{DecodedSurface, FrameDimensions, PixelFormat};
