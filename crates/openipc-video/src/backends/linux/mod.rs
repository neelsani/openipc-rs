//! Linux VA-API decoder backed by `cros-codecs` and DMA surfaces.

mod decoder;
mod device;
mod frame;
mod session;
mod surface;
#[cfg(test)]
mod tests;

pub use decoder::LinuxDecoder;
pub use surface::LinuxVideoFrame;
