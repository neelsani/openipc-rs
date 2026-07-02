//! Android hardware decoding through NDK MediaCodec and AImageReader.

mod decoder;
mod session;
mod surface;

#[cfg(test)]
mod tests;

pub use decoder::AndroidDecoder;
pub use surface::{AndroidImagePlane, AndroidVideoFrame};
