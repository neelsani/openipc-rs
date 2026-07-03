use std::time::Duration;

use ndk::{
    media::media_codec::{
        DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection,
    },
    native_window::NativeWindow,
};

use crate::{CodecConfig, CodecStreamInfo, VideoError};

use super::session::{android_error, media_format, mime_type};

const BUFFER_FLAG_KEY_FRAME: u32 = 1;
const BUFFER_FLAG_CODEC_CONFIG: u32 = 2;
const MAX_DRAINED_OUTPUTS: usize = 64;

pub(super) enum SurfaceSessionSubmit {
    Accepted(Option<u64>),
    Backpressure(Option<u64>),
}

/// MediaCodec session that renders directly into an application-owned surface.
pub(super) struct SurfaceMediaCodecSession {
    codec: MediaCodec,
    // MediaCodec retains the window internally, but keeping our own reference
    // makes that ownership explicit across decoder reconfiguration.
    _window: NativeWindow,
}

impl SurfaceMediaCodecSession {
    pub(super) fn new(
        config: &CodecConfig,
        stream: &CodecStreamInfo,
        low_latency: bool,
        window: NativeWindow,
    ) -> Result<Self, VideoError> {
        if stream.visible_dimensions.width == 0 || stream.visible_dimensions.height == 0 {
            return Err(VideoError::Backend {
                backend: "mediacodec-surface",
                operation: "configure output surface",
                message: "stream dimensions must be non-zero".to_owned(),
            });
        }
        let codec = MediaCodec::from_decoder_type(mime_type(config.codec())).ok_or(
            VideoError::HardwareDecoderUnavailable {
                codec: config.codec(),
                backend: "mediacodec-surface",
            },
        )?;
        let format = media_format(config, stream, low_latency);
        codec
            .configure(&format, Some(&window), MediaCodecDirection::Decoder)
            .map_err(|error| android_error("AMediaCodec_configure(surface)", error))?;
        codec
            .start()
            .map_err(|error| android_error("AMediaCodec_start(surface)", error))?;
        Ok(Self {
            codec,
            _window: window,
        })
    }

    pub(super) fn submit(
        &self,
        token: u64,
        bitstream: &[u8],
        keyframe: bool,
    ) -> Result<SurfaceSessionSubmit, VideoError> {
        let before = self.poll()?;
        // A very short wait avoids declaring overload during the normal handoff
        // between MediaCodec input buffers. It is bounded well below one video
        // frame and only blocks when every codec input slot is busy.
        let input_wait = Duration::from_millis(2);
        let input = self
            .codec
            .dequeue_input_buffer(input_wait)
            .map_err(|error| android_error("AMediaCodec_dequeueInputBuffer", error))?;
        let DequeuedInputBufferResult::Buffer(mut input) = input else {
            return Ok(SurfaceSessionSubmit::Backpressure(before));
        };
        let destination = input.buffer_mut();
        if bitstream.len() > destination.len() {
            return Err(VideoError::Backend {
                backend: "mediacodec-surface",
                operation: "fill decoder input buffer",
                message: format!(
                    "access unit is {} bytes but MediaCodec provided {} bytes",
                    bitstream.len(),
                    destination.len()
                ),
            });
        }
        destination[..bitstream.len()].write_copy_of_slice(bitstream);
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
        Ok(SurfaceSessionSubmit::Accepted(after.or(before)))
    }

    /// Release all ready output buffers to the SurfaceTexture producer queue.
    ///
    /// SurfaceTexture's consumer latches the newest image during the egui paint
    /// callback, so draining here avoids stalling MediaCodec behind old frames.
    pub(super) fn poll(&self) -> Result<Option<u64>, VideoError> {
        let mut latest_token = None;
        for _ in 0..MAX_DRAINED_OUTPUTS {
            match self
                .codec
                .dequeue_output_buffer(Duration::ZERO)
                .map_err(|error| android_error("AMediaCodec_dequeueOutputBuffer", error))?
            {
                DequeuedOutputBufferInfoResult::Buffer(output) => {
                    let info = *output.info();
                    let render = info.flags() & BUFFER_FLAG_CODEC_CONFIG == 0;
                    self.codec
                        .release_output_buffer(output, render)
                        .map_err(|error| android_error("AMediaCodec_releaseOutputBuffer", error))?;
                    if render {
                        latest_token = u64::try_from(info.presentation_time_us())
                            .ok()
                            .or(latest_token);
                    }
                }
                DequeuedOutputBufferInfoResult::TryAgainLater => break,
                DequeuedOutputBufferInfoResult::OutputFormatChanged
                | DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
            }
        }
        Ok(latest_token)
    }
}

impl Drop for SurfaceMediaCodecSession {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}
