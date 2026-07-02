use std::sync::{Arc, Mutex};

use windows::{
    core::Interface,
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D10::ID3D10Multithread,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
                ID3D11Texture2D, ID3D11VideoDevice, D3D11_CPU_ACCESS_READ,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                D3D11_DECODER_PROFILE_H264_VLD_FGT, D3D11_DECODER_PROFILE_H264_VLD_NOFGT,
                D3D11_DECODER_PROFILE_HEVC_VLD_MAIN, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
                D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
            },
            Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC},
        },
        Media::MediaFoundation::{IMFDXGIDeviceManager, MFCreateDXGIDeviceManager},
    },
};

use crate::{VideoCodec, VideoError};

use super::runtime::platform_error;

#[derive(Clone)]
pub(crate) struct D3dDevice {
    pub(crate) device: ID3D11Device,
    pub(crate) manager: IMFDXGIDeviceManager,
    context: ID3D11DeviceContext,
    staging: Arc<Mutex<Option<StagingTexture>>>,
}

struct StagingTexture {
    resource: ID3D11Resource,
    width: u32,
    height: u32,
}

impl D3dDevice {
    pub(crate) fn new() -> Result<Self, VideoError> {
        let mut device = None;
        let mut context = None;
        // SAFETY: All output pointers refer to initialized `Option` storage.
        // The hardware driver type requires a null adapter and software module.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(|error| platform_error("D3D11CreateDevice", error))?;
        let device = device.ok_or(VideoError::Backend {
            backend: "media-foundation",
            operation: "create D3D11 device",
            message: "D3D11 returned no device".to_owned(),
        })?;
        let context = context.ok_or(VideoError::Backend {
            backend: "media-foundation",
            operation: "create D3D11 device",
            message: "D3D11 returned no immediate context".to_owned(),
        })?;

        let multithread: ID3D10Multithread = device
            .cast()
            .map_err(|error| platform_error("ID3D10Multithread::QueryInterface", error))?;
        // SAFETY: The interface was queried from the live D3D11 device.
        let _ = unsafe { multithread.SetMultithreadProtected(true) };

        let mut reset_token = 0;
        let mut manager = None;
        // SAFETY: Both out parameters are valid writable storage.
        unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut manager) }
            .map_err(|error| platform_error("MFCreateDXGIDeviceManager", error))?;
        let manager = manager.ok_or(VideoError::Backend {
            backend: "media-foundation",
            operation: "create DXGI device manager",
            message: "Media Foundation returned no device manager".to_owned(),
        })?;
        // SAFETY: The manager and D3D11 device are live COM interfaces and the
        // reset token belongs to this manager instance.
        unsafe { manager.ResetDevice(&device, reset_token) }
            .map_err(|error| platform_error("IMFDXGIDeviceManager::ResetDevice", error))?;

        Ok(Self {
            device,
            manager,
            context,
            staging: Arc::new(Mutex::new(None)),
        })
    }

    pub(crate) fn supports_hardware_decode(&self, codec: VideoCodec) -> bool {
        let Ok(video_device) = self.device.cast::<ID3D11VideoDevice>() else {
            return false;
        };
        let desired_profiles = match codec {
            VideoCodec::H264 => &[
                D3D11_DECODER_PROFILE_H264_VLD_NOFGT,
                D3D11_DECODER_PROFILE_H264_VLD_FGT,
            ][..],
            // The backend exposes NV12, so Main10/P010 is deliberately not
            // included until the public surface contract supports it.
            VideoCodec::H265 => &[D3D11_DECODER_PROFILE_HEVC_VLD_MAIN][..],
        };
        // SAFETY: `video_device` is a live interface queried from `self.device`.
        let profile_count = unsafe { video_device.GetVideoDecoderProfileCount() };
        (0..profile_count).any(|index| {
            // SAFETY: `index` is bounded by GetVideoDecoderProfileCount.
            let Ok(profile) = (unsafe { video_device.GetVideoDecoderProfile(index) }) else {
                return false;
            };
            desired_profiles.contains(&profile)
                // SAFETY: `profile` came from this device and remains live for
                // the duration of the format query.
                && unsafe {
                    video_device
                        .CheckVideoDecoderFormat(&profile, DXGI_FORMAT_NV12)
                        .is_ok_and(|supported| supported.as_bool())
                }
        })
    }

    pub(crate) fn copy_nv12(
        &self,
        source: &ID3D11Texture2D,
        source_subresource: u32,
        dimensions: crate::FrameDimensions,
    ) -> Result<super::WindowsNv12Frame, VideoError> {
        let mut source_desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `source` is a retained decoder texture and the output
        // descriptor is valid writable storage.
        unsafe { source.GetDesc(&mut source_desc) };
        if source_desc.Format != DXGI_FORMAT_NV12 {
            return Err(VideoError::Backend {
                backend: "media-foundation",
                operation: "read D3D11 frame",
                message: format!("expected NV12 texture, got {:?}", source_desc.Format),
            });
        }
        let source_resource: ID3D11Resource = source
            .cast()
            .map_err(|error| platform_error("ID3D11Texture2D::QueryInterface", error))?;
        let mut staging = self.staging.lock().map_err(|_| VideoError::Backend {
            backend: "media-foundation",
            operation: "lock D3D11 staging texture",
            message: "staging texture lock was poisoned".to_owned(),
        })?;
        if staging.as_ref().is_none_or(|texture| {
            texture.width != source_desc.Width || texture.height != source_desc.Height
        }) {
            *staging = Some(self.create_staging_texture(&source_desc)?);
        }
        let staging = staging
            .as_ref()
            .expect("staging texture was initialized above");
        // SAFETY: Both resources belong to this D3D11 device. The destination
        // staging texture has the same dimensions and format as the source.
        unsafe {
            self.context.CopySubresourceRegion(
                &staging.resource,
                0,
                0,
                0,
                0,
                &source_resource,
                source_subresource,
                None,
            )
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: `mapped` is writable storage and the staging resource was
        // created with CPU read access.
        unsafe {
            self.context
                .Map(&staging.resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|error| platform_error("ID3D11DeviceContext::Map", error))?;
        let _unmap = UnmapGuard {
            context: &self.context,
            resource: &staging.resource,
        };
        if mapped.pData.is_null() {
            return Err(VideoError::Backend {
                backend: "media-foundation",
                operation: "map D3D11 staging texture",
                message: "D3D11 returned a null mapped pointer".to_owned(),
            });
        }
        let row_pitch = mapped.RowPitch as usize;
        let coded_height = source_desc.Height as usize;
        let total_rows = coded_height.saturating_add(coded_height.div_ceil(2));
        // SAFETY: A mapped NV12 staging texture exposes `RowPitch` bytes for
        // each luma row followed by half-height interleaved chroma rows. The
        // map remains live until `_unmap` is dropped below.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                mapped.pData.cast::<u8>(),
                row_pitch.saturating_mul(total_rows),
            )
        };
        let width = dimensions.width.min(source_desc.Width) as usize;
        let height = dimensions.height.min(source_desc.Height) as usize;
        let mut y = vec![0; width.saturating_mul(height)];
        let chroma_rows = height.div_ceil(2);
        let mut uv = vec![0; width.saturating_mul(chroma_rows)];
        for row in 0..height {
            let source_offset = row * row_pitch;
            y[row * width..(row + 1) * width]
                .copy_from_slice(&bytes[source_offset..source_offset + width]);
        }
        let chroma_offset = coded_height * row_pitch;
        for row in 0..chroma_rows {
            let source_offset = chroma_offset + row * row_pitch;
            uv[row * width..(row + 1) * width]
                .copy_from_slice(&bytes[source_offset..source_offset + width]);
        }
        Ok(super::WindowsNv12Frame {
            dimensions: crate::FrameDimensions {
                width: width as u32,
                height: height as u32,
            },
            y,
            uv,
            stride: width,
        })
    }

    fn create_staging_texture(
        &self,
        source: &D3D11_TEXTURE2D_DESC,
    ) -> Result<StagingTexture, VideoError> {
        let description = D3D11_TEXTURE2D_DESC {
            Width: source.Width,
            Height: source.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: source.Format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut texture = None;
        // SAFETY: The descriptor is initialized and `texture` is writable COM
        // output storage. No initial subresource data is supplied.
        unsafe {
            self.device
                .CreateTexture2D(&description, None, Some(&mut texture))
        }
        .map_err(|error| platform_error("ID3D11Device::CreateTexture2D", error))?;
        let texture = texture.ok_or_else(|| VideoError::Backend {
            backend: "media-foundation",
            operation: "create D3D11 staging texture",
            message: "D3D11 returned no staging texture".to_owned(),
        })?;
        let resource = texture
            .cast()
            .map_err(|error| platform_error("ID3D11Texture2D::QueryInterface", error))?;
        Ok(StagingTexture {
            resource,
            width: source.Width,
            height: source.Height,
        })
    }
}

struct UnmapGuard<'a> {
    context: &'a ID3D11DeviceContext,
    resource: &'a ID3D11Resource,
}

impl Drop for UnmapGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: This guard is created only after a successful Map call for
        // subresource zero and is dropped exactly once.
        unsafe { self.context.Unmap(self.resource, 0) };
    }
}
