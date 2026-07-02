//! macOS VideoToolbox decoder backend.

mod callback;
mod codecs;
mod decoder;
mod ffi;
mod session;
mod surface;
#[cfg(test)]
mod tests;
mod texture;

pub use decoder::MacOsDecoder;
pub use surface::{MacOsMappedPlane, MacOsVideoFrame};
pub use texture::{MetalPlaneDescriptor, MetalPlaneFormat};
