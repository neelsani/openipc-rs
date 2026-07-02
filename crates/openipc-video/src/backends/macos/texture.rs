use crate::PixelFormat;
use objc2_core_video::{
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetWidth,
    CVPixelBufferGetWidthOfPlane,
};

use super::MacOsVideoFrame;

/// Metal texture format required for one CoreVideo image plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetalPlaneFormat {
    /// One 8-bit normalized channel, used for NV12 luma.
    R8Unorm,
    /// Two 8-bit normalized channels, used for interleaved NV12 chroma.
    Rg8Unorm,
    /// Four 8-bit normalized BGRA channels.
    Bgra8Unorm,
}

/// Description used by an application-owned `CVMetalTextureCache`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetalPlaneDescriptor {
    /// CoreVideo plane index.
    pub plane_index: usize,
    /// Plane width in pixels.
    pub width: usize,
    /// Plane height in pixels.
    pub height: usize,
    /// Matching Metal texture format.
    pub format: MetalPlaneFormat,
}

impl MacOsVideoFrame {
    /// Describe the pixel-buffer planes that an egui/Metal renderer should import.
    pub fn metal_planes(&self) -> Vec<MetalPlaneDescriptor> {
        use crate::DecodedSurface;

        match self.pixel_format() {
            PixelFormat::Nv12VideoRange | PixelFormat::Nv12FullRange => vec![
                MetalPlaneDescriptor {
                    plane_index: 0,
                    width: CVPixelBufferGetWidthOfPlane(self.pixel_buffer(), 0),
                    height: CVPixelBufferGetHeightOfPlane(self.pixel_buffer(), 0),
                    format: MetalPlaneFormat::R8Unorm,
                },
                MetalPlaneDescriptor {
                    plane_index: 1,
                    width: CVPixelBufferGetWidthOfPlane(self.pixel_buffer(), 1),
                    height: CVPixelBufferGetHeightOfPlane(self.pixel_buffer(), 1),
                    format: MetalPlaneFormat::Rg8Unorm,
                },
            ],
            PixelFormat::Bgra8 => vec![MetalPlaneDescriptor {
                plane_index: 0,
                width: CVPixelBufferGetWidth(self.pixel_buffer()),
                height: CVPixelBufferGetHeight(self.pixel_buffer()),
                format: MetalPlaneFormat::Bgra8Unorm,
            }],
            PixelFormat::Native(_) => Vec::new(),
        }
    }
}
