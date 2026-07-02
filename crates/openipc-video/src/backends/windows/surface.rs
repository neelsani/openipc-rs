use windows::{
    core::Interface,
    Win32::{
        Graphics::{
            Direct3D11::{ID3D11Texture2D, D3D11_TEXTURE2D_DESC},
            Dxgi::Common::DXGI_FORMAT_NV12,
        },
        Media::MediaFoundation::{IMFDXGIBuffer, IMFSample},
    },
};

use crate::{DecodedSurface, FrameDimensions, PixelFormat, VideoError};

use super::{d3d::D3dDevice, runtime::platform_error};

/// Tightly packed NV12 copy of a D3D11 decoder surface.
#[derive(Debug, Clone)]
pub struct WindowsNv12Frame {
    pub(crate) dimensions: FrameDimensions,
    pub(crate) y: Vec<u8>,
    pub(crate) uv: Vec<u8>,
    pub(crate) stride: usize,
}

impl WindowsNv12Frame {
    /// Visible image dimensions.
    pub const fn dimensions(&self) -> FrameDimensions {
        self.dimensions
    }

    /// Tightly packed luma plane.
    pub fn y_plane(&self) -> &[u8] {
        &self.y
    }

    /// Tightly packed interleaved U/V plane.
    pub fn uv_plane(&self) -> &[u8] {
        &self.uv
    }

    /// Bytes per row in both planes.
    pub const fn stride(&self) -> usize {
        self.stride
    }
}

/// Retained Media Foundation output backed by a D3D11 texture.
pub struct WindowsVideoFrame {
    texture: ID3D11Texture2D,
    sample: IMFSample,
    subresource_index: u32,
    dimensions: FrameDimensions,
    texture_desc: D3D11_TEXTURE2D_DESC,
    readback: Option<D3dDevice>,
}

// Media Foundation samples are free-threaded pipeline objects. Retaining the
// sample keeps its decoder-owned texture subresource leased while another
// thread imports or renders the D3D11 texture.
unsafe impl Send for WindowsVideoFrame {}
unsafe impl Sync for WindowsVideoFrame {}

impl std::fmt::Debug for WindowsVideoFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsVideoFrame")
            .field("texture", &self.texture.as_raw())
            .field("subresource_index", &self.subresource_index)
            .field("dimensions", &self.dimensions)
            .field("format", &self.texture_desc.Format)
            .finish()
    }
}

impl WindowsVideoFrame {
    pub(crate) fn from_sample(
        sample: IMFSample,
        visible_dimensions: Option<FrameDimensions>,
    ) -> Result<Self, VideoError> {
        // SAFETY: The sample owns its media buffers for the duration of this call.
        let buffer = unsafe { sample.GetBufferByIndex(0) }
            .map_err(|error| platform_error("IMFSample::GetBufferByIndex", error))?;
        let dxgi: IMFDXGIBuffer = buffer
            .cast()
            .map_err(|error| platform_error("IMFDXGIBuffer::QueryInterface", error))?;
        let mut resource = std::ptr::null_mut();
        // SAFETY: `resource` is writable pointer storage and the requested IID
        // exactly matches the interface constructed below.
        unsafe { dxgi.GetResource(&ID3D11Texture2D::IID, &mut resource) }
            .map_err(|error| platform_error("IMFDXGIBuffer::GetResource", error))?;
        if resource.is_null() {
            return Err(VideoError::Backend {
                backend: "media-foundation",
                operation: "get D3D11 output texture",
                message: "DXGI buffer returned a null texture".to_owned(),
            });
        }
        // SAFETY: GetResource returned an owned reference for the requested IID.
        let texture = unsafe { ID3D11Texture2D::from_raw(resource) };
        // SAFETY: The texture is live and `texture_desc` is writable storage.
        let texture_desc = unsafe {
            let mut description = std::mem::zeroed();
            texture.GetDesc(&mut description);
            description
        };
        // SAFETY: The DXGI media buffer remains live through `sample`.
        let subresource_index = unsafe { dxgi.GetSubresourceIndex() }
            .map_err(|error| platform_error("IMFDXGIBuffer::GetSubresourceIndex", error))?;
        let dimensions = visible_dimensions.unwrap_or(FrameDimensions {
            width: texture_desc.Width,
            height: texture_desc.Height,
        });
        Ok(Self {
            texture,
            sample,
            subresource_index,
            dimensions,
            texture_desc,
            readback: None,
        })
    }

    pub(crate) fn attach_readback(&mut self, readback: D3dDevice) {
        self.readback = Some(readback);
    }

    /// Copy this decoder texture into tightly packed NV12 planes.
    ///
    /// Applications should keep the native texture for as long as possible
    /// and call this only for renderers that cannot import D3D11 textures.
    pub fn copy_nv12(&self) -> Result<WindowsNv12Frame, VideoError> {
        self.readback
            .as_ref()
            .ok_or(VideoError::Backend {
                backend: "media-foundation",
                operation: "read D3D11 frame",
                message: "decoder readback device is unavailable".to_owned(),
            })?
            .copy_nv12(self.texture(), self.subresource_index(), self.dimensions())
    }

    /// Borrow the retained D3D11 texture for renderer interop.
    pub fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }

    /// Texture-array subresource containing this decoded frame.
    pub const fn subresource_index(&self) -> u32 {
        self.subresource_index
    }

    /// D3D11 texture description reported by the decoder.
    pub const fn texture_desc(&self) -> D3D11_TEXTURE2D_DESC {
        self.texture_desc
    }

    /// Borrow the retained sample that leases the decoder surface.
    pub fn sample(&self) -> &IMFSample {
        &self.sample
    }
}

impl DecodedSurface for WindowsVideoFrame {
    fn dimensions(&self) -> FrameDimensions {
        self.dimensions
    }

    fn pixel_format(&self) -> PixelFormat {
        if self.texture_desc.Format == DXGI_FORMAT_NV12 {
            PixelFormat::Nv12VideoRange
        } else {
            PixelFormat::Native(self.texture_desc.Format.0 as u32)
        }
    }
}
