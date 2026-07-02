use std::ffi::c_void;

use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVReturnSuccess, CVPixelBuffer,
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};

use crate::{DecodedSurface, FrameDimensions, PixelFormat, VideoError};

/// Read-only view of one mapped CoreVideo pixel-buffer plane.
#[derive(Debug, Clone, Copy)]
pub struct MacOsMappedPlane<'a> {
    data: &'a [u8],
    stride: usize,
}

impl<'a> MacOsMappedPlane<'a> {
    /// Bytes in this plane, including any row padding.
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Number of bytes between adjacent rows.
    pub const fn stride(&self) -> usize {
        self.stride
    }
}

struct PixelBufferLock<'a>(&'a CVPixelBuffer);

impl Drop for PixelBufferLock<'_> {
    fn drop(&mut self) {
        // SAFETY: This guard is created only after a successful read-only lock,
        // and uses the same pixel buffer and flags when releasing it.
        let _ = unsafe { CVPixelBufferUnlockBaseAddress(self.0, CVPixelBufferLockFlags::ReadOnly) };
    }
}

/// Retained VideoToolbox output backed by a CoreVideo pixel buffer.
pub struct MacOsVideoFrame {
    pixel_buffer: CFRetained<CVPixelBuffer>,
    decode_info_flags: u32,
}

impl std::fmt::Debug for MacOsVideoFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MacOsVideoFrame")
            .field("pixel_buffer", &CFRetained::as_ptr(&self.pixel_buffer))
            .field("decode_info_flags", &self.decode_info_flags)
            .finish()
    }
}

// CoreVideo pixel buffers are reference-counted, may be retained across the
// VideoToolbox callback boundary, and are designed for cross-thread GPU and
// display handoff. This wrapper exposes only immutable metadata operations.
unsafe impl Send for MacOsVideoFrame {}
unsafe impl Sync for MacOsVideoFrame {}

impl MacOsVideoFrame {
    pub(crate) fn new(pixel_buffer: CFRetained<CVPixelBuffer>, decode_info_flags: u32) -> Self {
        Self {
            pixel_buffer,
            decode_info_flags,
        }
    }

    /// Borrow the retained CoreVideo pixel buffer.
    pub fn pixel_buffer(&self) -> &CVPixelBuffer {
        &self.pixel_buffer
    }

    /// VideoToolbox information flags reported for this output frame.
    pub const fn decode_info_flags(&self) -> u32 {
        self.decode_info_flags
    }

    /// Whether the output is IOSurface-backed for native GPU interop.
    pub fn is_io_surface_backed(&self) -> bool {
        unsafe extern "C-unwind" {
            fn CVPixelBufferGetIOSurface(pixel_buffer: &CVPixelBuffer) -> *mut c_void;
        }
        // SAFETY: The retained pixel buffer is a valid CoreVideo object. The
        // Get function returns a non-owning pointer used only for a null check.
        !unsafe { CVPixelBufferGetIOSurface(&self.pixel_buffer) }.is_null()
    }

    /// Map the decoded image for a scoped, read-only CPU access.
    ///
    /// The mapped slices cannot outlive the callback. Native GPU consumers
    /// should use [`Self::pixel_buffer`] directly to avoid this readback.
    pub fn with_mapped_planes<R>(
        &self,
        callback: impl FnOnce(&[MacOsMappedPlane<'_>]) -> R,
    ) -> Result<R, VideoError> {
        // SAFETY: The retained pixel buffer is valid and is unlocked by the
        // guard below before this method returns, including during unwinding.
        let status = unsafe {
            CVPixelBufferLockBaseAddress(&self.pixel_buffer, CVPixelBufferLockFlags::ReadOnly)
        };
        if status != kCVReturnSuccess {
            return Err(VideoError::Platform {
                api: "CVPixelBufferLockBaseAddress",
                status,
            });
        }
        let _lock = PixelBufferLock(&self.pixel_buffer);
        let plane_count = CVPixelBufferGetPlaneCount(&self.pixel_buffer);
        let mut planes = Vec::with_capacity(plane_count.max(1));

        if plane_count == 0 {
            let pointer = CVPixelBufferGetBaseAddress(&self.pixel_buffer).cast::<u8>();
            let stride = CVPixelBufferGetBytesPerRow(&self.pixel_buffer);
            let length = stride.saturating_mul(CVPixelBufferGetHeight(&self.pixel_buffer));
            if pointer.is_null() && length != 0 {
                return Err(VideoError::Backend {
                    backend: "videotoolbox",
                    operation: "map pixel buffer",
                    message: "CoreVideo returned a null base address".to_owned(),
                });
            }
            // SAFETY: CoreVideo keeps the locked base address valid for
            // `stride * height` bytes until `_lock` is dropped.
            let data = unsafe { std::slice::from_raw_parts(pointer, length) };
            planes.push(MacOsMappedPlane { data, stride });
        } else {
            for index in 0..plane_count {
                let pointer =
                    CVPixelBufferGetBaseAddressOfPlane(&self.pixel_buffer, index).cast::<u8>();
                let stride = CVPixelBufferGetBytesPerRowOfPlane(&self.pixel_buffer, index);
                let length =
                    stride.saturating_mul(CVPixelBufferGetHeightOfPlane(&self.pixel_buffer, index));
                if pointer.is_null() && length != 0 {
                    return Err(VideoError::Backend {
                        backend: "videotoolbox",
                        operation: "map pixel-buffer plane",
                        message: format!("CoreVideo returned a null address for plane {index}"),
                    });
                }
                // SAFETY: CoreVideo keeps each locked plane valid for
                // `stride * plane_height` bytes until `_lock` is dropped.
                let data = unsafe { std::slice::from_raw_parts(pointer, length) };
                planes.push(MacOsMappedPlane { data, stride });
            }
        }

        Ok(callback(&planes))
    }
}

impl DecodedSurface for MacOsVideoFrame {
    fn dimensions(&self) -> FrameDimensions {
        FrameDimensions {
            width: u32::try_from(CVPixelBufferGetWidth(&self.pixel_buffer)).unwrap_or(u32::MAX),
            height: u32::try_from(CVPixelBufferGetHeight(&self.pixel_buffer)).unwrap_or(u32::MAX),
        }
    }

    fn pixel_format(&self) -> PixelFormat {
        let value = CVPixelBufferGetPixelFormatType(&self.pixel_buffer);
        if value == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
            PixelFormat::Nv12VideoRange
        } else if value == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
            PixelFormat::Nv12FullRange
        } else if value == kCVPixelFormatType_32BGRA {
            PixelFormat::Bgra8
        } else {
            PixelFormat::Native(value)
        }
    }
}
