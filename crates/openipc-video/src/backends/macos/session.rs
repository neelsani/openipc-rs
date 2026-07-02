use std::{ffi::c_void, ptr, ptr::NonNull};

use objc2_core_foundation::{CFBoolean, CFRetained};
use objc2_core_media::CMFormatDescription;
use objc2_video_toolbox::{
    kVTDecompressionPropertyKey_RealTime, VTDecodeFrameFlags, VTDecompressionOutputCallbackRecord,
    VTDecompressionSession, VTSessionSetProperty,
};

use crate::{codecs::annex_b, CodecConfig, DecoderOptions, EncodedAccessUnit, VideoError};

use super::{
    callback::{decompression_output_callback, CallbackState},
    codecs, ffi,
};

pub(crate) struct VideoToolboxSession {
    decoder: CFRetained<VTDecompressionSession>,
    format: CFRetained<CMFormatDescription>,
    callback: Box<CallbackState>,
    drained: bool,
}

// VideoToolbox decompression sessions accept submissions from arbitrary
// threads and dispatch completion callbacks on framework-owned queues. The
// callback state is synchronized, and teardown drains callbacks first.
unsafe impl Send for VideoToolboxSession {}
unsafe impl Sync for VideoToolboxSession {}

impl VideoToolboxSession {
    pub(crate) fn new(
        config: &CodecConfig,
        options: DecoderOptions,
        callback: CallbackState,
    ) -> Result<Self, VideoError> {
        let format = codecs::format_description(config)?;
        let image_attributes = ffi::decoder_image_attributes();
        let decoder_specification = ffi::decoder_specification(options.require_hardware);
        let callback = Box::new(callback);
        let callback_record = VTDecompressionOutputCallbackRecord {
            decompressionOutputCallback: Some(decompression_output_callback),
            decompressionOutputRefCon: ptr::from_ref(callback.as_ref()).cast_mut().cast::<c_void>(),
        };
        let mut decoder_ptr = ptr::null_mut();
        // SAFETY: All CoreFoundation objects are live for this call, the
        // callback context remains stable in its Box for the session lifetime,
        // and `decoder_ptr` is a valid out pointer.
        let status = unsafe {
            VTDecompressionSession::create(
                None,
                &format,
                Some(decoder_specification.as_opaque()),
                Some(image_attributes.as_opaque()),
                ptr::from_ref(&callback_record),
                NonNull::from(&mut decoder_ptr),
            )
        };
        if status != 0 {
            return Err(VideoError::Platform {
                api: "VTDecompressionSessionCreate",
                status,
            });
        }
        let decoder_ptr = NonNull::new(decoder_ptr).ok_or(VideoError::Platform {
            api: "VTDecompressionSessionCreate",
            status,
        })?;
        // SAFETY: VTDecompressionSessionCreate follows the Create rule.
        let decoder = unsafe { CFRetained::from_raw(decoder_ptr) };
        if options.low_latency {
            // SAFETY: A VTDecompressionSession is a VTSession, and both the
            // property key and CFBoolean are valid process-lifetime objects.
            let status = unsafe {
                VTSessionSetProperty(
                    (*decoder).as_ref(),
                    kVTDecompressionPropertyKey_RealTime,
                    Some(CFBoolean::new(true).as_ref()),
                )
            };
            if status != 0 {
                // SAFETY: The just-created session is valid and has no work.
                unsafe { decoder.invalidate() };
                return Err(VideoError::Platform {
                    api: "VTSessionSetProperty(RealTime)",
                    status,
                });
            }
        }
        Ok(Self {
            decoder,
            format,
            callback,
            drained: false,
        })
    }

    pub(crate) fn submit(&self, frame: &EncodedAccessUnit) -> Result<(), VideoError> {
        let data = annex_b::to_length_prefixed(&frame.data)?;
        let sample = ffi::sample_buffer(&data, &self.format, frame.timestamp)?;
        self.callback.submitted(frame.timestamp);
        // Asynchronous decode without temporal processing minimizes latency.
        // The 1x-real-time flag is intentionally omitted because it permits a
        // lower-power decoder that cannot run faster than real time.
        let flags = VTDecodeFrameFlags::Frame_EnableAsynchronousDecompression;
        // SAFETY: `sample` is a valid compressed-video sample. No source
        // refcon or synchronous info-flags output is requested.
        let status = unsafe {
            self.decoder
                .decode_frame(&sample, flags, ptr::null_mut(), ptr::null_mut())
        };
        if status != 0 {
            self.callback.rejected(frame.timestamp, status);
            return Err(VideoError::Platform {
                api: "VTDecompressionSessionDecodeFrame",
                status,
            });
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<(), VideoError> {
        // SAFETY: The session is valid. This waits until no callback can still
        // access the boxed callback context.
        let status = unsafe { self.decoder.wait_for_asynchronous_frames() };
        self.drained = status == 0;
        if status == 0 {
            Ok(())
        } else {
            Err(VideoError::Platform {
                api: "VTDecompressionSessionWaitForAsynchronousFrames",
                status,
            })
        }
    }
}

impl Drop for VideoToolboxSession {
    fn drop(&mut self) {
        if !self.drained {
            // SAFETY: Waiting before releasing the callback context prevents
            // use-after-free even when a caller drops without flushing.
            let _ = unsafe { self.decoder.wait_for_asynchronous_frames() };
        }
        // SAFETY: Invalidating is the documented deterministic teardown for a
        // live VideoToolbox decompression session.
        unsafe { self.decoder.invalidate() };
    }
}
