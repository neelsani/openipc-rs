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

#[derive(Debug, Default)]
pub(super) struct SurfacePoll {
    pub(super) latest_token: Option<u64>,
    pub(super) rendered_outputs: usize,
}

impl SurfacePoll {
    fn append(&mut self, newer: Self) {
        self.rendered_outputs = self.rendered_outputs.saturating_add(newer.rendered_outputs);
        if newer.latest_token.is_some() {
            self.latest_token = newer.latest_token;
        }
    }
}

pub(super) enum SurfaceSessionSubmit {
    Accepted(SurfacePoll),
    Backpressure(SurfacePoll),
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
        let mut completed = self.poll()?;
        // A very short wait avoids declaring overload during the normal handoff
        // between MediaCodec input buffers. It is bounded well below one video
        // frame and only blocks when every codec input slot is busy.
        let input_wait = Duration::from_millis(2);
        let input = self
            .codec
            .dequeue_input_buffer(input_wait)
            .map_err(|error| android_error("AMediaCodec_dequeueInputBuffer", error))?;
        let DequeuedInputBufferResult::Buffer(mut input) = input else {
            return Ok(SurfaceSessionSubmit::Backpressure(completed));
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
        completed.append(self.poll()?);
        Ok(SurfaceSessionSubmit::Accepted(completed))
    }

    /// Release all ready output buffers to the SurfaceTexture producer queue.
    ///
    /// SurfaceTexture's consumer latches the newest image during the egui paint
    /// callback, so draining here avoids stalling MediaCodec behind old frames.
    pub(super) fn poll(&self) -> Result<SurfacePoll, VideoError> {
        let mut completed = SurfacePoll::default();
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
                        completed.rendered_outputs = completed.rendered_outputs.saturating_add(1);
                        completed.latest_token = u64::try_from(info.presentation_time_us())
                            .ok()
                            .or(completed.latest_token);
                    }
                }
                DequeuedOutputBufferInfoResult::TryAgainLater => break,
                DequeuedOutputBufferInfoResult::OutputFormatChanged
                | DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
            }
        }
        Ok(completed)
    }
}

impl Drop for SurfaceMediaCodecSession {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}
