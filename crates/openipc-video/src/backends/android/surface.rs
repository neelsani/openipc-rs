use ndk::{
    hardware_buffer::{HardwareBuffer, HardwareBufferRef},
    media::image_reader::Image,
};

use crate::{DecodedSurface, FrameDimensions, PixelFormat, VideoError};

use super::session::android_error;

/// Retained Android decoder output backed by an `AImage` and `AHardwareBuffer`.
///
/// Keeping this value alive keeps the `AImageReader` buffer leased and prevents
/// MediaCodec from overwriting it while a renderer imports the hardware buffer.
pub struct AndroidVideoFrame {
    image: Image,
    hardware_buffer: HardwareBufferRef,
    dimensions: FrameDimensions,
    crop_origin: [usize; 2],
    timestamp_ns: i64,
    native_format: u32,
}

// AImage has exclusive ownership here and the NDK does not impose thread
// affinity on AImage access or deletion. AHardwareBuffer explicitly permits
// read locking from multiple threads. Nebulus transfers this value between
// threads but never accesses the same frame concurrently.
unsafe impl Send for AndroidVideoFrame {}

/// Read-only view of one Android `AImage` YUV plane.
#[derive(Debug, Clone, Copy)]
pub struct AndroidImagePlane<'a> {
    data: &'a [u8],
    row_stride: usize,
    pixel_stride: usize,
}

impl<'a> AndroidImagePlane<'a> {
    /// Raw bytes exposed by `AImage_getPlaneData`.
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Bytes between adjacent plane rows.
    pub const fn row_stride(&self) -> usize {
        self.row_stride
    }

    /// Bytes between adjacent samples in one row.
    pub const fn pixel_stride(&self) -> usize {
        self.pixel_stride
    }
}

impl std::fmt::Debug for AndroidVideoFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AndroidVideoFrame")
            .field("dimensions", &self.dimensions)
            .field("timestamp_ns", &self.timestamp_ns)
            .field("native_format", &self.native_format)
            .finish()
    }
}

impl AndroidVideoFrame {
    pub(crate) fn new(image: Image) -> Result<Self, VideoError> {
        let crop = image
            .crop_rect()
            .map_err(|error| android_error("AImage_getCropRect", error))?;
        let fallback_width = image
            .width()
            .map_err(|error| android_error("AImage_getWidth", error))?;
        let fallback_height = image
            .height()
            .map_err(|error| android_error("AImage_getHeight", error))?;
        let crop_width = crop.right.saturating_sub(crop.left);
        let crop_height = crop.bottom.saturating_sub(crop.top);
        let dimensions = FrameDimensions {
            width: u32::try_from(if crop_width > 0 {
                crop_width
            } else {
                fallback_width
            })
            .map_err(|_| invalid_dimensions())?,
            height: u32::try_from(if crop_height > 0 {
                crop_height
            } else {
                fallback_height
            })
            .map_err(|_| invalid_dimensions())?,
        };
        let timestamp_ns = image
            .timestamp()
            .map_err(|error| android_error("AImage_getTimestamp", error))?;
        let hardware_buffer = image
            .hardware_buffer()
            .map_err(|error| android_error("AImage_getHardwareBuffer", error))?
            .acquire();
        let native_format = i32::from(hardware_buffer.describe().format) as u32;
        Ok(Self {
            image,
            hardware_buffer,
            dimensions,
            crop_origin: [
                usize::try_from(crop.left.max(0)).map_err(|_| invalid_dimensions())?,
                usize::try_from(crop.top.max(0)).map_err(|_| invalid_dimensions())?,
            ],
            timestamp_ns,
            native_format,
        })
    }

    /// Borrow the hardware buffer for EGL, OpenGL ES, Vulkan, or wgpu import.
    pub fn hardware_buffer(&self) -> &HardwareBuffer {
        &self.hardware_buffer
    }

    /// Borrow the `AImage` lease that owns this decoder output.
    pub fn image(&self) -> &Image {
        &self.image
    }

    /// Surface timestamp assigned by MediaCodec, in nanoseconds.
    pub const fn timestamp_ns(&self) -> i64 {
        self.timestamp_ns
    }

    /// Android hardware-buffer format code.
    pub const fn native_format(&self) -> u32 {
        self.native_format
    }

    /// Top-left crop offset within the backing image, in luma pixels.
    pub const fn crop_origin(&self) -> [usize; 2] {
        self.crop_origin
    }

    /// Borrow all `AImage` planes for scoped CPU presentation.
    ///
    /// The decoder requests flexible YUV 4:2:0 output as three CPU-readable
    /// planes. GPU clients should import [`Self::hardware_buffer`] instead to
    /// avoid readback.
    pub fn with_mapped_planes<R>(
        &self,
        callback: impl FnOnce(&[AndroidImagePlane<'_>]) -> R,
    ) -> Result<R, VideoError> {
        let count = self
            .image
            .number_of_planes()
            .map_err(|error| android_error("AImage_getNumberOfPlanes", error))?;
        let mut planes = Vec::with_capacity(usize::try_from(count.max(0)).unwrap_or(0));
        for index in 0..count {
            let data = self
                .image
                .plane_data(index)
                .map_err(|error| android_error("AImage_getPlaneData", error))?;
            let row_stride = self
                .image
                .plane_row_stride(index)
                .map_err(|error| android_error("AImage_getPlaneRowStride", error))?;
            let pixel_stride = self
                .image
                .plane_pixel_stride(index)
                .map_err(|error| android_error("AImage_getPlanePixelStride", error))?;
            let row_stride = usize::try_from(row_stride).map_err(|_| invalid_plane_layout())?;
            let pixel_stride = usize::try_from(pixel_stride).map_err(|_| invalid_plane_layout())?;
            if row_stride == 0 || pixel_stride == 0 {
                return Err(invalid_plane_layout());
            }
            planes.push(AndroidImagePlane {
                data,
                row_stride,
                pixel_stride,
            });
        }
        Ok(callback(&planes))
    }
}

impl DecodedSurface for AndroidVideoFrame {
    fn dimensions(&self) -> FrameDimensions {
        self.dimensions
    }

    fn pixel_format(&self) -> PixelFormat {
        PixelFormat::Native(self.native_format)
    }
}

fn invalid_dimensions() -> VideoError {
    VideoError::Backend {
        backend: "mediacodec",
        operation: "read output dimensions",
        message: "AImage reported negative dimensions".to_owned(),
    }
}

fn invalid_plane_layout() -> VideoError {
    VideoError::Backend {
        backend: "mediacodec",
        operation: "map AImage planes",
        message: "AImage reported an invalid plane stride".to_owned(),
    }
}
