//! Windows decoder backed by Media Foundation and D3D11 output textures.

mod d3d;
mod decoder;
mod mft;
mod runtime;
mod surface;
#[cfg(test)]
mod tests;

pub use decoder::WindowsDecoder;
pub use surface::{WindowsNv12Frame, WindowsVideoFrame};
