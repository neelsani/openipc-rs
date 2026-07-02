use std::{ffi::c_void, mem::ManuallyDrop};

use windows::{
    core::{Interface, PWSTR},
    Win32::{
        Graphics::Direct3D11::D3D11_BIND_SHADER_RESOURCE,
        Media::MediaFoundation::{
            eAVEncH265VProfile_Main_420_8, IMFActivate, IMFDXGIDeviceManager, IMFMediaType,
            IMFSample, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
            MFMediaType_Video, MFTEnumEx, MFT_FRIENDLY_NAME_Attribute, MFVideoFormat_H264,
            MFVideoFormat_HEVC, MFVideoFormat_NV12, MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_LOCALMFT, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
            MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH,
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
            MFT_MESSAGE_NOTIFY_END_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
            MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER,
            MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
            MFT_REGISTER_TYPE_INFO, MF_E_NOTACCEPTING, MF_E_NO_MORE_TYPES,
            MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY,
            MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_MT_VIDEO_PROFILE,
            MF_SA_D3D11_AWARE, MF_SA_D3D11_BINDFLAGS,
        },
        System::Com::CoTaskMemFree,
    },
};

use crate::{FrameDimensions, VideoCodec, VideoError};

use super::{runtime::platform_error, WindowsVideoFrame};

pub(crate) struct ReadyFrame {
    pub(crate) surface: WindowsVideoFrame,
    pub(crate) token: u64,
}

pub(crate) enum SessionSubmit {
    Accepted(Vec<ReadyFrame>),
    Backpressure(Vec<ReadyFrame>),
}

pub(crate) struct MediaFoundationSession {
    transform: IMFTransform,
    decoder_name: Option<String>,
    dimensions: Option<FrameDimensions>,
    streaming: bool,
}

impl MediaFoundationSession {
    pub(crate) fn new(
        codec: VideoCodec,
        manager: &IMFDXGIDeviceManager,
        low_latency: bool,
    ) -> Result<Self, VideoError> {
        let (transform, decoder_name) = activate_decoder(codec, manager)?;
        if low_latency {
            // SAFETY: The transform's attribute store is live. Some third-party
            // decoders omit this optional attribute, so failure is non-fatal.
            if let Ok(attributes) = unsafe { transform.GetAttributes() } {
                let _ = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) };
            }
        }
        // Request textures that can be sampled directly by a renderer. The MFT
        // may add its own decoder bind flag or ignore this optional hint.
        if let Ok(attributes) = unsafe { transform.GetOutputStreamAttributes(0) } {
            let _ = unsafe {
                attributes.SetUINT32(&MF_SA_D3D11_BINDFLAGS, D3D11_BIND_SHADER_RESOURCE.0 as u32)
            };
        }

        let input_type = create_input_type(codec)?;
        // SAFETY: Stream zero is the single compressed input exposed by video
        // decoder MFTs and `input_type` remains live for the call.
        unsafe { transform.SetInputType(0, &input_type, 0) }
            .map_err(|error| platform_error("IMFTransform::SetInputType", error))?;

        let mut session = Self {
            transform,
            decoder_name,
            dimensions: None,
            streaming: false,
        };
        session.negotiate_output()?;
        // SAFETY: Media types and the D3D manager are configured before these
        // standard synchronous-MFT lifecycle messages are sent.
        unsafe {
            session
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
        }
        .map_err(|error| platform_error("IMFTransform::ProcessMessage(BEGIN_STREAMING)", error))?;
        unsafe {
            session
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
        }
        .map_err(|error| platform_error("IMFTransform::ProcessMessage(START_OF_STREAM)", error))?;
        session.streaming = true;
        Ok(session)
    }

    pub(crate) fn decoder_name(&self) -> Option<&str> {
        self.decoder_name.as_deref()
    }

    pub(crate) fn submit(
        &mut self,
        token: u64,
        bitstream: &[u8],
    ) -> Result<SessionSubmit, VideoError> {
        let sample = create_input_sample(token, bitstream)?;
        let mut frames = Vec::new();
        // SAFETY: The sample owns a complete Annex-B access unit and stream
        // zero is configured for the matching compressed codec.
        match unsafe { self.transform.ProcessInput(0, &sample, 0) } {
            Ok(()) => {}
            Err(error) if error.code() == MF_E_NOTACCEPTING => {
                frames.extend(self.drain_outputs()?);
                // SAFETY: Draining output is the prescribed response to
                // MF_E_NOTACCEPTING before retrying the same input sample.
                if let Err(error) = unsafe { self.transform.ProcessInput(0, &sample, 0) } {
                    if error.code() == MF_E_NOTACCEPTING {
                        return Ok(SessionSubmit::Backpressure(frames));
                    }
                    return Err(platform_error("IMFTransform::ProcessInput", error));
                }
            }
            Err(error) => return Err(platform_error("IMFTransform::ProcessInput", error)),
        }
        frames.extend(self.drain_outputs()?);
        Ok(SessionSubmit::Accepted(frames))
    }

    pub(crate) fn flush(&mut self) -> Result<Vec<ReadyFrame>, VideoError> {
        // SAFETY: The transform is configured and streaming. Drain asks it to
        // release every complete pending frame before the state reset.
        unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0) }
            .map_err(|error| platform_error("IMFTransform::ProcessMessage(DRAIN)", error))?;
        let frames = self.drain_outputs()?;
        // SAFETY: Flush is valid after draining and clears partial input.
        unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) }
            .map_err(|error| platform_error("IMFTransform::ProcessMessage(FLUSH)", error))?;
        Ok(frames)
    }

    fn drain_outputs(&mut self) -> Result<Vec<ReadyFrame>, VideoError> {
        let mut frames = Vec::new();
        for _ in 0..64 {
            match self.process_output()? {
                OutputResult::Frame(sample) => {
                    // SAFETY: Input samples are assigned non-negative token
                    // timestamps and synchronous decoder MFTs preserve them.
                    let timestamp = unsafe { sample.GetSampleTime() }
                        .map_err(|error| platform_error("IMFSample::GetSampleTime", error))?;
                    let token = u64::try_from(timestamp).map_err(|_| VideoError::Backend {
                        backend: "media-foundation",
                        operation: "read output timestamp",
                        message: "decoder returned a negative sample timestamp".to_owned(),
                    })?;
                    let surface = WindowsVideoFrame::from_sample(sample, self.dimensions)?;
                    frames.push(ReadyFrame { surface, token });
                }
                OutputResult::StreamChanged => self.negotiate_output()?,
                OutputResult::NoSample => continue,
                OutputResult::NeedMoreInput => return Ok(frames),
            }
        }
        Err(VideoError::Backend {
            backend: "media-foundation",
            operation: "drain decoder output",
            message: "decoder did not reach NEED_MORE_INPUT after 64 outputs".to_owned(),
        })
    }

    fn process_output(&self) -> Result<OutputResult, VideoError> {
        let mut buffer = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(None),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        // SAFETY: `buffer` and `status` are initialized writable storage. The
        // configured D3D-aware decoder owns allocation of output samples.
        let result = unsafe {
            self.transform
                .ProcessOutput(0, std::slice::from_mut(&mut buffer), &mut status)
        };
        // SAFETY: ProcessOutput initialized these COM option slots. Taking them
        // transfers any returned references into normal Rust-owned Options.
        let sample = unsafe { ManuallyDrop::take(&mut buffer.pSample) };
        // SAFETY: Same ownership transfer as `pSample`; dropping releases any
        // in-band event collection supplied by the transform.
        let events = unsafe { ManuallyDrop::take(&mut buffer.pEvents) };
        drop(events);

        match result {
            Ok(()) => Ok(sample.map_or(OutputResult::NoSample, OutputResult::Frame)),
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                Ok(OutputResult::NeedMoreInput)
            }
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                Ok(OutputResult::StreamChanged)
            }
            Err(error) => Err(platform_error("IMFTransform::ProcessOutput", error)),
        }
    }

    fn negotiate_output(&mut self) -> Result<(), VideoError> {
        let mut selected = None;
        for index in 0..64 {
            // SAFETY: Stream zero is the transform's sole output and `index`
            // is bounded; MF_E_NO_MORE_TYPES terminates enumeration.
            match unsafe { self.transform.GetOutputAvailableType(0, index) } {
                Ok(media_type) => {
                    // SAFETY: `media_type` is a live IMFAttributes-derived object.
                    let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }
                        .map_err(|error| platform_error("IMFMediaType::GetGUID", error))?;
                    if subtype == MFVideoFormat_NV12 {
                        selected = Some(media_type);
                        break;
                    }
                }
                Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
                Err(error) => {
                    return Err(platform_error(
                        "IMFTransform::GetOutputAvailableType",
                        error,
                    ));
                }
            }
        }
        let selected = selected.ok_or(VideoError::Backend {
            backend: "media-foundation",
            operation: "negotiate output format",
            message: "decoder exposes no NV12 output type".to_owned(),
        })?;
        // SAFETY: The selected type was advertised by this output stream.
        unsafe { self.transform.SetOutputType(0, &selected, 0) }
            .map_err(|error| platform_error("IMFTransform::SetOutputType", error))?;
        // SAFETY: Frame size is an optional UINT64 IMF media-type attribute.
        self.dimensions = unsafe { selected.GetUINT64(&MF_MT_FRAME_SIZE) }
            .ok()
            .map(|packed| FrameDimensions {
                width: (packed >> 32) as u32,
                height: packed as u32,
            })
            .filter(|size| size.width > 0 && size.height > 0);

        // SAFETY: Output stream info is valid after setting the media type.
        let stream_info = unsafe { self.transform.GetOutputStreamInfo(0) }
            .map_err(|error| platform_error("IMFTransform::GetOutputStreamInfo", error))?;
        let allocator_flags =
            (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0) as u32;
        if stream_info.dwFlags & allocator_flags == 0 {
            return Err(VideoError::Backend {
                backend: "media-foundation",
                operation: "negotiate D3D11 output",
                message: "decoder requires caller-owned output buffers".to_owned(),
            });
        }
        Ok(())
    }
}

impl Drop for MediaFoundationSession {
    fn drop(&mut self) {
        if !self.streaming {
            return;
        }
        // SAFETY: These best-effort lifecycle messages release resources held
        // by a configured synchronous transform during teardown.
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }
}

enum OutputResult {
    Frame(IMFSample),
    StreamChanged,
    NoSample,
    NeedMoreInput,
}

pub(crate) fn probe_codec(codec: VideoCodec, manager: &IMFDXGIDeviceManager) -> bool {
    activate_decoder(codec, manager).is_ok()
}

fn activate_decoder(
    codec: VideoCodec,
    manager: &IMFDXGIDeviceManager,
) -> Result<(IMFTransform, Option<String>), VideoError> {
    let activations = enumerate_decoders(codec)?;
    for activation in activations {
        let decoder_name = friendly_name(&activation);
        // SAFETY: The activation object was returned by MFTEnumEx for a video decoder.
        let Ok(transform) = (unsafe { activation.ActivateObject::<IMFTransform>() }) else {
            continue;
        };
        // SAFETY: The attribute is read-only and queried on a live transform.
        let d3d11_aware = unsafe {
            transform
                .GetAttributes()
                .and_then(|attributes| attributes.GetUINT32(&MF_SA_D3D11_AWARE))
        }
        .unwrap_or(0)
            != 0;
        if !d3d11_aware {
            continue;
        }
        // SAFETY: The COM pointer remains owned by `manager` while the
        // transform uses it; ProcessMessage AddRefs it when retained.
        if unsafe {
            transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
        }
        .is_err()
        {
            continue;
        }
        return Ok((transform, decoder_name));
    }
    Err(VideoError::HardwareDecoderUnavailable {
        codec,
        backend: "media-foundation",
    })
}

fn enumerate_decoders(codec: VideoCodec) -> Result<Vec<IMFActivate>, VideoError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: compressed_subtype(codec),
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let mut raw: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0;
    // SAFETY: Both output parameters are writable. MFTEnumEx returns a
    // CoTaskMem-allocated array whose entries each own one COM reference.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_LOCALMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut raw,
            &mut count,
        )
    }
    .map_err(|error| platform_error("MFTEnumEx", error))?;
    if raw.is_null() {
        return Ok(Vec::new());
    }
    if count == 0 {
        // MFTEnumEx normally returns a null pointer for an empty result, but
        // release any defensive non-null allocation before returning.
        unsafe { CoTaskMemFree(Some(raw.cast::<c_void>())) };
        return Ok(Vec::new());
    }
    // SAFETY: The array contains exactly `count` initialized Option slots.
    let slice = unsafe { std::slice::from_raw_parts_mut(raw, count as usize) };
    let activations = slice.iter_mut().filter_map(Option::take).collect();
    // SAFETY: The entries have been moved out; only the task-allocated array
    // storage remains to be released.
    unsafe { CoTaskMemFree(Some(raw.cast::<c_void>())) };
    Ok(activations)
}

fn friendly_name(activation: &IMFActivate) -> Option<String> {
    let mut value = PWSTR::null();
    let mut len = 0;
    // SAFETY: Both out parameters are writable and activation derives from IMFAttributes.
    unsafe { activation.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut value, &mut len) }
        .ok()?;
    if value.is_null() {
        return None;
    }
    // SAFETY: Media Foundation returned a UTF-16 allocation of exactly `len`
    // code units, excluding its trailing NUL.
    let name =
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value.0, len as usize) });
    // SAFETY: GetAllocatedString uses CoTaskMemAlloc for this buffer.
    unsafe { CoTaskMemFree(Some(value.0.cast::<c_void>())) };
    Some(name)
}

fn create_input_type(codec: VideoCodec) -> Result<IMFMediaType, VideoError> {
    // SAFETY: Media Foundation is initialized by the owning decoder runtime.
    let media_type = unsafe { MFCreateMediaType() }
        .map_err(|error| platform_error("MFCreateMediaType", error))?;
    // SAFETY: Both attribute values are stable Media Foundation GUIDs.
    unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }
        .map_err(|error| platform_error("IMFMediaType::SetGUID(MAJOR_TYPE)", error))?;
    unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &compressed_subtype(codec)) }
        .map_err(|error| platform_error("IMFMediaType::SetGUID(SUBTYPE)", error))?;
    if codec == VideoCodec::H265 {
        // The Windows backend negotiates NV12 output and therefore supports
        // 8-bit HEVC Main. Main10 requires a P010 output path.
        unsafe {
            media_type.SetUINT32(&MF_MT_VIDEO_PROFILE, eAVEncH265VProfile_Main_420_8.0 as u32)
        }
        .map_err(|error| platform_error("IMFMediaType::SetUINT32(VIDEO_PROFILE)", error))?;
    }
    Ok(media_type)
}

fn create_input_sample(token: u64, data: &[u8]) -> Result<IMFSample, VideoError> {
    let length = u32::try_from(data.len()).map_err(|_| VideoError::Backend {
        backend: "media-foundation",
        operation: "allocate input sample",
        message: "encoded access unit exceeds 4 GiB".to_owned(),
    })?;
    // SAFETY: Media Foundation is initialized and `length` is the exact input size.
    let buffer = unsafe { MFCreateMemoryBuffer(length) }
        .map_err(|error| platform_error("MFCreateMemoryBuffer", error))?;
    let mut destination = std::ptr::null_mut();
    // SAFETY: `destination` is writable pointer storage. The returned mapping
    // stays valid until the matching Unlock below.
    unsafe { buffer.Lock(&mut destination, None, None) }
        .map_err(|error| platform_error("IMFMediaBuffer::Lock", error))?;
    if destination.is_null() {
        // SAFETY: Balances the successful Lock even on this malformed result.
        let _ = unsafe { buffer.Unlock() };
        return Err(VideoError::Backend {
            backend: "media-foundation",
            operation: "map input sample",
            message: "Media Foundation returned a null buffer mapping".to_owned(),
        });
    }
    // SAFETY: MFCreateMemoryBuffer allocated at least `data.len()` writable
    // bytes and the source slice is valid and non-overlapping.
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), destination, data.len()) };
    // SAFETY: Balances the successful Lock above.
    unsafe { buffer.Unlock() }.map_err(|error| platform_error("IMFMediaBuffer::Unlock", error))?;
    // SAFETY: `length` does not exceed the allocation.
    unsafe { buffer.SetCurrentLength(length) }
        .map_err(|error| platform_error("IMFMediaBuffer::SetCurrentLength", error))?;

    // SAFETY: Media Foundation is initialized.
    let sample =
        unsafe { MFCreateSample() }.map_err(|error| platform_error("MFCreateSample", error))?;
    let timestamp = i64::try_from(token).map_err(|_| VideoError::Backend {
        backend: "media-foundation",
        operation: "timestamp input sample",
        message: "decoder token exceeds the signed Media Foundation timestamp range".to_owned(),
    })?;
    // SAFETY: The sample and buffer are live, and the token is a non-negative
    // monotonic correlation timestamp rather than a presentation clock.
    unsafe { sample.AddBuffer(&buffer) }
        .map_err(|error| platform_error("IMFSample::AddBuffer", error))?;
    unsafe { sample.SetSampleTime(timestamp) }
        .map_err(|error| platform_error("IMFSample::SetSampleTime", error))?;
    unsafe { sample.SetSampleDuration(1) }
        .map_err(|error| platform_error("IMFSample::SetSampleDuration", error))?;
    Ok(sample)
}

const fn compressed_subtype(codec: VideoCodec) -> windows::core::GUID {
    match codec {
        VideoCodec::H264 => MFVideoFormat_H264,
        VideoCodec::H265 => MFVideoFormat_HEVC,
    }
}
