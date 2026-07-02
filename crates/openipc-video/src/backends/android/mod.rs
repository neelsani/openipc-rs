//! Android hardware decoding through NDK MediaCodec and AImageReader.

mod decoder;
mod session;
mod surface;
mod surface_decoder;
mod surface_session;

#[cfg(test)]
mod tests;

pub use decoder::AndroidDecoder;
pub use surface::{AndroidImagePlane, AndroidVideoFrame};
pub use surface_decoder::{AndroidPresentedFrame, AndroidSurfaceDecoder};
