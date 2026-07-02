use std::{rc::Rc, sync::Arc};

use cros_codecs::{
    backend::vaapi::decoder::VaapiBackend,
    decoder::{
        stateless::{h264::H264, h265::H265, DecodeError, StatelessDecoder, StatelessVideoDecoder},
        BlockingMode, DecodedHandle, DecoderEvent,
    },
    libva::Display,
    video_frame::gbm_video_frame::GbmDevice,
};

use crate::{FrameDimensions, VideoCodec, VideoError};

use super::frame::{VaFrame, VaFramePool};

type H264Decoder = StatelessDecoder<H264, VaapiBackend<VaFrame>>;
type H265Decoder = StatelessDecoder<H265, VaapiBackend<VaFrame>>;

pub(crate) struct ReadyFrame {
    pub(crate) frame: Arc<VaFrame>,
    pub(crate) token: u64,
    pub(crate) dimensions: FrameDimensions,
}

pub(crate) enum SessionSubmit {
    Accepted(Vec<ReadyFrame>),
    Backpressure(Vec<ReadyFrame>),
}

enum CodecSession {
    H264(Box<H264Decoder>),
    H265(Box<H265Decoder>),
}

pub(crate) struct VaapiSession {
    decoder: CodecSession,
    pool: VaFramePool,
}

impl VaapiSession {
    pub(crate) fn new(
        codec: VideoCodec,
        display: Rc<Display>,
        gbm: Arc<GbmDevice>,
        retained_frame_allowance: usize,
    ) -> Result<Self, VideoError> {
        let decoder = match codec {
            VideoCodec::H264 => CodecSession::H264(Box::new(
                H264Decoder::new_vaapi(display, BlockingMode::NonBlocking)
                    .map_err(|error| backend_error("create H.264 decoder", error))?,
            )),
            VideoCodec::H265 => CodecSession::H265(Box::new(
                H265Decoder::new_vaapi(display, BlockingMode::NonBlocking)
                    .map_err(|error| backend_error("create H.265 decoder", error))?,
            )),
        };
        Ok(Self {
            decoder,
            pool: VaFramePool::new(gbm, retained_frame_allowance),
        })
    }

    pub(crate) fn submit(
        &mut self,
        token: u64,
        bitstream: &[u8],
    ) -> Result<SessionSubmit, VideoError> {
        match &mut self.decoder {
            CodecSession::H264(decoder) => {
                decode_all(decoder.as_mut(), &mut self.pool, token, bitstream)
            }
            CodecSession::H265(decoder) => {
                decode_all(decoder.as_mut(), &mut self.pool, token, bitstream)
            }
        }
    }

    pub(crate) fn flush(&mut self) -> Result<Vec<ReadyFrame>, VideoError> {
        match &mut self.decoder {
            CodecSession::H264(decoder) => flush_decoder(decoder.as_mut(), &mut self.pool),
            CodecSession::H265(decoder) => flush_decoder(decoder.as_mut(), &mut self.pool),
        }
    }
}

fn decode_all<D>(
    decoder: &mut D,
    pool: &mut VaFramePool,
    token: u64,
    bitstream: &[u8],
) -> Result<SessionSubmit, VideoError>
where
    D: StatelessVideoDecoder,
    D::Handle: DecodedHandle<Frame = VaFrame>,
{
    let mut offset = 0;
    let mut frames = Vec::new();
    while offset < bitstream.len() {
        let result = decoder.decode(token, &bitstream[offset..], &mut || pool.allocate());
        match result {
            Ok(0) => {
                return Err(VideoError::Backend {
                    backend: "vaapi",
                    operation: "decode access unit",
                    message: "decoder consumed zero bytes".to_owned(),
                });
            }
            Ok(consumed) => {
                offset = offset.saturating_add(consumed);
                drain_events(decoder, pool, &mut frames)?;
            }
            Err(DecodeError::CheckEvents) => {
                let handled = drain_events(decoder, pool, &mut frames)?;
                if !handled {
                    return Err(VideoError::Backend {
                        backend: "vaapi",
                        operation: "process decoder event",
                        message: "decoder requested events but exposed none".to_owned(),
                    });
                }
            }
            Err(DecodeError::NotEnoughOutputBuffers(_)) => {
                drain_events(decoder, pool, &mut frames)?;
                return Ok(SessionSubmit::Backpressure(frames));
            }
            Err(error) => return Err(backend_error("decode access unit", error)),
        }
    }
    drain_events(decoder, pool, &mut frames)?;
    Ok(SessionSubmit::Accepted(frames))
}

fn flush_decoder<D>(decoder: &mut D, pool: &mut VaFramePool) -> Result<Vec<ReadyFrame>, VideoError>
where
    D: StatelessVideoDecoder,
    D::Handle: DecodedHandle<Frame = VaFrame>,
{
    decoder
        .flush()
        .map_err(|error| backend_error("flush decoder", error))?;
    let mut frames = Vec::new();
    drain_events(decoder, pool, &mut frames)?;
    Ok(frames)
}

fn drain_events<D>(
    decoder: &mut D,
    pool: &mut VaFramePool,
    frames: &mut Vec<ReadyFrame>,
) -> Result<bool, VideoError>
where
    D: StatelessVideoDecoder,
    D::Handle: DecodedHandle<Frame = VaFrame>,
{
    let mut handled = false;
    while let Some(event) = decoder.next_event() {
        handled = true;
        match event {
            DecoderEvent::FormatChanged => {
                let info = decoder.stream_info().ok_or_else(|| VideoError::Backend {
                    backend: "vaapi",
                    operation: "negotiate output format",
                    message: "decoder did not publish stream information".to_owned(),
                })?;
                pool.resize(
                    info.display_resolution,
                    info.coded_resolution,
                    info.min_num_frames,
                )?;
            }
            DecoderEvent::FrameReady(handle) => {
                handle
                    .sync()
                    .map_err(|error| backend_error("synchronize output surface", error))?;
                let display = handle.display_resolution();
                frames.push(ReadyFrame {
                    frame: handle.video_frame(),
                    token: handle.timestamp(),
                    dimensions: FrameDimensions {
                        width: display.width,
                        height: display.height,
                    },
                });
            }
        }
    }
    Ok(handled)
}

fn backend_error(operation: &'static str, error: impl std::fmt::Display) -> VideoError {
    VideoError::Backend {
        backend: "vaapi",
        operation,
        message: error.to_string(),
    }
}
