use std::sync::Arc;

use cros_codecs::video_frame::VideoFrame;

use crate::{DecodedSurface, FrameDimensions, PixelFormat, VideoError};

use super::frame::VaFrame;

/// Retained Linux VA-API output backed by a GBM DMA buffer.
pub struct LinuxVideoFrame {
    frame: Arc<VaFrame>,
    dimensions: FrameDimensions,
}

impl std::fmt::Debug for LinuxVideoFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LinuxVideoFrame")
            .field("dimensions", &self.dimensions)
            .field("fourcc", &self.frame.fourcc())
            .field("plane_pitches", &self.frame.get_plane_pitch())
            .finish()
    }
}

impl LinuxVideoFrame {
    pub(crate) fn new(frame: Arc<VaFrame>, dimensions: FrameDimensions) -> Self {
        Self { frame, dimensions }
    }

    /// Native DRM FourCC of the DMA-backed output surface.
    pub fn drm_fourcc(&self) -> u32 {
        self.frame.fourcc().into()
    }

    /// Number of bytes between rows for each image plane.
    pub fn plane_pitches(&self) -> Vec<usize> {
        self.frame.get_plane_pitch()
    }

    /// Allocated byte size of each image plane.
    pub fn plane_sizes(&self) -> Vec<usize> {
        self.frame.get_plane_size()
    }

    /// Temporarily map the DMA buffer for CPU reads.
    ///
    /// Rendering code should prefer native DMA-buffer import. Mapping is
    /// intended for diagnostics, screenshots, and software fallback paths.
    pub fn with_mapped_planes<R>(&self, read: impl FnOnce(&[&[u8]]) -> R) -> Result<R, VideoError> {
        let mapping = self.frame.map().map_err(|message| VideoError::Backend {
            backend: "vaapi",
            operation: "map DMA output surface",
            message,
        })?;
        let planes = mapping.get();
        Ok(read(&planes))
    }
}

impl DecodedSurface for LinuxVideoFrame {
    fn dimensions(&self) -> FrameDimensions {
        self.dimensions
    }

    fn pixel_format(&self) -> PixelFormat {
        if self.frame.fourcc().to_string() == "NV12" {
            PixelFormat::Nv12VideoRange
        } else {
            PixelFormat::Native(self.drm_fourcc())
        }
    }
}
