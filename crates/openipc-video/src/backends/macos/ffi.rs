//! CoreFoundation and CoreMedia construction boundary.

use std::{ffi::c_void, ptr, ptr::NonNull};

use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFType};
use objc2_core_media::{
    kCMBlockBufferAssureMemoryNowFlag, CMBlockBuffer, CMFormatDescription, CMSampleBuffer,
    CMSampleTimingInfo, CMTime, CMTimeFlags,
};
use objc2_core_video::{
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_video_toolbox::{
    kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder,
    kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder,
};

use crate::{VideoError, VideoTimestamp};

pub(crate) fn decoder_image_attributes() -> CFRetained<CFDictionary<CFType, CFType>> {
    let empty_surface_properties = CFDictionary::<CFType, CFType>::empty();
    let pixel_format = CFNumber::new_i32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32);
    // SAFETY: CoreVideo exports these keys as process-lifetime CFString constants.
    let metal_key = unsafe { kCVPixelBufferMetalCompatibilityKey };
    // SAFETY: See the Metal key above.
    let io_surface_key = unsafe { kCVPixelBufferIOSurfacePropertiesKey };
    // SAFETY: See the Metal key above.
    let pixel_format_key = unsafe { kCVPixelBufferPixelFormatTypeKey };
    CFDictionary::from_slices(
        &[
            metal_key.as_ref(),
            io_surface_key.as_ref(),
            pixel_format_key.as_ref(),
        ],
        &[
            CFBoolean::new(true).as_ref(),
            (*empty_surface_properties).as_ref(),
            pixel_format.as_ref(),
        ],
    )
}

pub(crate) fn decoder_specification(
    require_hardware: bool,
) -> CFRetained<CFDictionary<CFType, CFType>> {
    // Prefer hardware in both modes. Requiring it makes session creation fail
    // instead of silently falling back to software.
    let key = if require_hardware {
        // SAFETY: VideoToolbox exports this process-lifetime CFString constant.
        unsafe { kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder }
    } else {
        // SAFETY: See the required-hardware key above.
        unsafe { kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder }
    };
    CFDictionary::from_slices(&[key.as_ref()], &[CFBoolean::new(true).as_ref()])
}

pub(crate) fn h264_format_description<const N: usize>(
    parameter_sets: [&[u8]; N],
) -> Result<CFRetained<CMFormatDescription>, VideoError> {
    let mut pointers = parameter_sets.map(|parameter_set| {
        NonNull::new(parameter_set.as_ptr().cast_mut())
            .expect("slice pointers are non-null, including empty slices")
    });
    let sizes = parameter_sets.map(<[u8]>::len);
    let mut output: *const CMFormatDescription = ptr::null();
    // SAFETY: Every pointer and size originates from a live borrowed slice,
    // `output` is writable, and CoreMedia copies the parameter-set data.
    let status = unsafe {
        objc2_core_media::CMVideoFormatDescriptionCreateFromH264ParameterSets(
            None,
            N,
            NonNull::new(pointers.as_mut_ptr()).expect("arrays have a non-null base pointer"),
            NonNull::new(sizes.as_ptr().cast_mut()).expect("arrays have a non-null base pointer"),
            4,
            NonNull::from(&mut output),
        )
    };
    format_description_result(
        "CMVideoFormatDescriptionCreateFromH264ParameterSets",
        status,
        output,
    )
}

pub(crate) fn h265_format_description<const N: usize>(
    parameter_sets: [&[u8]; N],
) -> Result<CFRetained<CMFormatDescription>, VideoError> {
    let mut pointers = parameter_sets.map(|parameter_set| {
        NonNull::new(parameter_set.as_ptr().cast_mut())
            .expect("slice pointers are non-null, including empty slices")
    });
    let sizes = parameter_sets.map(<[u8]>::len);
    let mut output: *const CMFormatDescription = ptr::null();
    // SAFETY: Every pointer and size originates from a live borrowed slice,
    // `output` is writable, and CoreMedia copies the parameter-set data.
    let status = unsafe {
        objc2_core_media::CMVideoFormatDescriptionCreateFromHEVCParameterSets(
            None,
            N,
            NonNull::new(pointers.as_mut_ptr()).expect("arrays have a non-null base pointer"),
            NonNull::new(sizes.as_ptr().cast_mut()).expect("arrays have a non-null base pointer"),
            4,
            None,
            NonNull::from(&mut output),
        )
    };
    format_description_result(
        "CMVideoFormatDescriptionCreateFromHEVCParameterSets",
        status,
        output,
    )
}

pub(crate) fn sample_buffer(
    data: &[u8],
    format: &CMFormatDescription,
    timestamp: VideoTimestamp,
) -> Result<CFRetained<CMSampleBuffer>, VideoError> {
    let mut block_ptr = ptr::null_mut();
    // SAFETY: CoreMedia allocates and owns the memory block. All lengths are
    // derived from `data`, and `block_ptr` is a valid out pointer.
    let status = unsafe {
        CMBlockBuffer::create_with_memory_block(
            None,
            ptr::null_mut(),
            data.len(),
            None,
            ptr::null(),
            0,
            data.len(),
            kCMBlockBufferAssureMemoryNowFlag,
            NonNull::from(&mut block_ptr),
        )
    };
    let block_ptr = platform_pointer("CMBlockBufferCreateWithMemoryBlock", status, block_ptr)?;
    // SAFETY: `create_with_memory_block` follows the CoreFoundation Create rule.
    let block = unsafe { CFRetained::from_raw(block_ptr) };
    let source = NonNull::new(data.as_ptr().cast_mut().cast::<c_void>()).ok_or(
        VideoError::InvalidAnnexB("access unit contains no NAL data"),
    )?;
    // SAFETY: `source` addresses `data.len()` initialized bytes and the block
    // was allocated with exactly that writable capacity.
    let status = unsafe { CMBlockBuffer::replace_data_bytes(source, &block, 0, data.len()) };
    platform_status("CMBlockBufferReplaceDataBytes", status)?;

    let timing = CMSampleTimingInfo {
        duration: invalid_time(),
        presentationTimeStamp: valid_time(timestamp),
        decodeTimeStamp: invalid_time(),
    };
    let sample_size = data.len();
    let mut output = ptr::null_mut();
    // SAFETY: `block` and `format` are valid retained CoreMedia objects,
    // timing and size point to one initialized entry, and `output` is writable.
    // CMSampleBuffer retains its block buffer and format description.
    let status = unsafe {
        CMSampleBuffer::create(
            None,
            Some(&block),
            true,
            None,
            ptr::null_mut(),
            Some(format),
            1,
            1,
            &timing,
            1,
            &sample_size,
            NonNull::from(&mut output),
        )
    };
    let output = platform_pointer("CMSampleBufferCreate", status, output)?;
    // SAFETY: `CMSampleBuffer::create` follows the CoreFoundation Create rule.
    Ok(unsafe { CFRetained::from_raw(output) })
}

fn format_description_result(
    api: &'static str,
    status: i32,
    output: *const CMFormatDescription,
) -> Result<CFRetained<CMFormatDescription>, VideoError> {
    let output = platform_pointer(api, status, output.cast_mut())?;
    // SAFETY: Both format-description functions follow the Create rule.
    Ok(unsafe { CFRetained::from_raw(output) })
}

fn platform_status(api: &'static str, status: i32) -> Result<(), VideoError> {
    if status == 0 {
        Ok(())
    } else {
        Err(VideoError::Platform { api, status })
    }
}

fn platform_pointer<T>(
    api: &'static str,
    status: i32,
    output: *mut T,
) -> Result<NonNull<T>, VideoError> {
    platform_status(api, status)?;
    NonNull::new(output).ok_or(VideoError::Platform { api, status })
}

fn valid_time(timestamp: VideoTimestamp) -> CMTime {
    CMTime {
        value: timestamp.value,
        timescale: timestamp.timescale,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    }
}

fn invalid_time() -> CMTime {
    CMTime {
        value: 0,
        timescale: 0,
        flags: CMTimeFlags::empty(),
        epoch: 0,
    }
}
