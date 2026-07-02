use std::time::Duration;

use ndk::{
    hardware_buffer::HardwareBufferUsage,
    media::{
        image_reader::{AcquireResult, ImageFormat, ImageReader},
        media_codec::{
            DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec,
            MediaCodecDirection,
        },
        media_format::MediaFormat,
    },
};

use crate::{CodecConfig, CodecStreamInfo, VideoCodec, VideoError};

use super::AndroidVideoFrame;

const BUFFER_FLAG_KEY_FRAME: u32 = 1;
const BUFFER_FLAG_CODEC_CONFIG: u32 = 2;
const MAX_DRAINED_OUTPUTS: usize = 64;

pub(crate) enum SessionSubmit {
    Accepted(Option<AndroidVideoFrame>),
    Backpressure(Option<AndroidVideoFrame>),
}

pub(crate) struct MediaCodecSession {
    codec: MediaCodec,
    reader: ImageReader,
}

impl MediaCodecSession {
    pub(crate) fn new(
        config: &CodecConfig,
        stream: &CodecStreamInfo,
        max_frames_in_flight: usize,
        low_latency: bool,
    ) -> Result<Self, VideoError> {
        let width = i32::try_from(stream.visible_dimensions.width)
            .map_err(|_| invalid_dimensions(stream))?;
        let height = i32::try_from(stream.visible_dimensions.height)
            .map_err(|_| invalid_dimensions(stream))?;
        if width <= 0 || height <= 0 {
            return Err(invalid_dimensions(stream));
        }
        let max_images = i32::try_from(max_frames_in_flight.saturating_add(1))
            .map_err(|_| VideoError::InvalidOption("Android image queue is too large"))?;
        let reader = ImageReader::new_with_usage(
            width,
            height,
            ImageFormat::YUV_420_888,
            HardwareBufferUsage::CPU_READ_OFTEN | HardwareBufferUsage::GPU_SAMPLED_IMAGE,
            max_images,
        )
        .or_else(|_| {
            ImageReader::new_with_usage(
                width,
                height,
                ImageFormat::YUV_420_888,
                HardwareBufferUsage::CPU_READ_OFTEN,
                max_images,
            )
        })
        .map_err(|error| android_error("AImageReader_newWithUsage", error))?;
        let window = reader
            .window()
            .map_err(|error| android_error("AImageReader_getWindow", error))?;
        let mime = mime_type(config.codec());
        let codec =
            MediaCodec::from_decoder_type(mime).ok_or(VideoError::HardwareDecoderUnavailable {
                codec: config.codec(),
                backend: "mediacodec",
            })?;
        let format = media_format(config, stream, low_latency);
        codec
            .configure(&format, Some(&window), MediaCodecDirection::Decoder)
            .map_err(|error| android_error("AMediaCodec_configure", error))?;
        codec
            .start()
            .map_err(|error| android_error("AMediaCodec_start", error))?;
        // MediaCodec retained the ANativeWindow during configure.
        drop(window);
        Ok(Self { codec, reader })
    }

    pub(crate) fn submit(
        &self,
        token: u64,
        bitstream: &[u8],
        keyframe: bool,
    ) -> Result<SessionSubmit, VideoError> {
        let before = self.poll()?;
        let input = self
            .codec
            .dequeue_input_buffer(Duration::ZERO)
            .map_err(|error| android_error("AMediaCodec_dequeueInputBuffer", error))?;
        let DequeuedInputBufferResult::Buffer(mut input) = input else {
            return Ok(SessionSubmit::Backpressure(before));
        };
        let destination = input.buffer_mut();
        if bitstream.len() > destination.len() {
            return Err(VideoError::Backend {
                backend: "mediacodec",
                operation: "fill decoder input buffer",
                message: format!(
                    "access unit is {} bytes but MediaCodec provided {} bytes",
                    bitstream.len(),
                    destination.len()
                ),
            });
        }
        for (destination, source) in destination.iter_mut().zip(bitstream) {
            destination.write(*source);
        }
        self.codec
            .queue_input_buffer(
                input,
                0,
                bitstream.len(),
                token,
                if keyframe { BUFFER_FLAG_KEY_FRAME } else { 0 },
            )
            .map_err(|error| android_error("AMediaCodec_queueInputBuffer", error))?;
        let after = self.poll()?;
        Ok(SessionSubmit::Accepted(after.or(before)))
    }

    pub(crate) fn poll(&self) -> Result<Option<AndroidVideoFrame>, VideoError> {
        self.drain_outputs()?;
        match self
            .reader
            .acquire_latest_image()
            .map_err(|error| android_error("AImageReader_acquireLatestImage", error))?
        {
            AcquireResult::Image(image) => AndroidVideoFrame::new(image).map(Some),
            AcquireResult::NoBufferAvailable | AcquireResult::MaxImagesAcquired => Ok(None),
        }
    }

    fn drain_outputs(&self) -> Result<(), VideoError> {
        for _ in 0..MAX_DRAINED_OUTPUTS {
            match self
                .codec
                .dequeue_output_buffer(Duration::ZERO)
                .map_err(|error| android_error("AMediaCodec_dequeueOutputBuffer", error))?
            {
                DequeuedOutputBufferInfoResult::Buffer(output) => {
                    let render = output.info().flags() & BUFFER_FLAG_CODEC_CONFIG == 0;
                    self.codec
                        .release_output_buffer(output, render)
                        .map_err(|error| android_error("AMediaCodec_releaseOutputBuffer", error))?;
                }
                DequeuedOutputBufferInfoResult::TryAgainLater => return Ok(()),
                DequeuedOutputBufferInfoResult::OutputFormatChanged
                | DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
            }
        }
        Err(VideoError::Backend {
            backend: "mediacodec",
            operation: "drain decoder output",
            message: "decoder produced more than 64 outputs in one poll".to_owned(),
        })
    }
}

impl Drop for MediaCodecSession {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}

pub(crate) fn codec_available(codec: VideoCodec) -> bool {
    MediaCodec::from_decoder_type(mime_type(codec)).is_some()
}

fn media_format(config: &CodecConfig, stream: &CodecStreamInfo, low_latency: bool) -> MediaFormat {
    let mut format = MediaFormat::new();
    format.set_str("mime", mime_type(config.codec()));
    format.set_i32("width", stream.visible_dimensions.width as i32);
    format.set_i32("height", stream.visible_dimensions.height as i32);
    format.set_i32("max-input-size", maximum_input_size(stream));
    if low_latency {
        format.set_i32("low-latency", 1);
    }
    match config {
        CodecConfig::H264(config) => {
            format.set_buffer("csd-0", &annex_b_unit(&config.sps));
            format.set_buffer("csd-1", &annex_b_unit(&config.pps));
        }
        CodecConfig::H265(_) => format.set_buffer("csd-0", &config.to_annex_b()),
    }
    format
}

fn maximum_input_size(stream: &CodecStreamInfo) -> i32 {
    let raw_frame_size = u64::from(stream.coded_dimensions.width)
        .saturating_mul(u64::from(stream.coded_dimensions.height))
        .saturating_mul(3)
        / 2;
    raw_frame_size.max(1024 * 1024).min(i32::MAX as u64) as i32
}

fn annex_b_unit(nalu: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4 + nalu.len());
    bytes.extend_from_slice(&[0, 0, 0, 1]);
    bytes.extend_from_slice(nalu);
    bytes
}

const fn mime_type(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::H264 => "video/avc",
        VideoCodec::H265 => "video/hevc",
    }
}

fn invalid_dimensions(stream: &CodecStreamInfo) -> VideoError {
    VideoError::Backend {
        backend: "mediacodec",
        operation: "create AImageReader",
        message: format!(
            "invalid stream dimensions {}x{}",
            stream.visible_dimensions.width, stream.visible_dimensions.height
        ),
    }
}

pub(crate) fn android_error(api: &'static str, error: impl std::fmt::Display) -> VideoError {
    VideoError::Backend {
        backend: "mediacodec",
        operation: api,
        message: error.to_string(),
    }
}
