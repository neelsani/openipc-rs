use std::collections::HashMap;

use bytes::Bytes;
use futures_util::FutureExt;
use wasm_bindgen::JsValue;
use web_codecs::{Dimensions, EncodedFrame, VideoDecoded, VideoDecoderConfig};
use web_time::Instant;

use crate::{
    runtime::{LatestFrameMailbox, StatsHandle},
    CodecCapability, CodecConfig, CodecConfigTracker, CodecStreamInfo, ConfigUpdate, DecodedFrame,
    DecoderCapabilities, DecoderOptions, DecoderStats, EncodedAccessUnit, SubmitOutcome,
    VideoCodec, VideoDecoder, VideoError, VideoTimestamp,
};

use super::WebVideoFrame;

struct PendingFrame {
    timestamp: VideoTimestamp,
    submitted_at: Instant,
}

struct WebCodecsSession {
    decoder: web_codecs::VideoDecoder,
    decoded: VideoDecoded,
}

/// Browser H.264/H.265 decoder backed by WebCodecs.
///
/// This value and its [`WebVideoFrame`] outputs are main-thread/local-executor
/// values because browser `VideoFrame` handles are not Rust `Send` types.
pub struct WebDecoder {
    options: DecoderOptions,
    tracker: CodecConfigTracker,
    session: Option<WebCodecsSession>,
    active_config: Option<CodecConfig>,
    config_prefix: Bytes,
    waiting_for_keyframe: bool,
    frames: LatestFrameMailbox<DecodedFrame<WebVideoFrame>>,
    stats: StatsHandle,
    pending: HashMap<u64, PendingFrame>,
    next_token: u64,
    capabilities: DecoderCapabilities,
    last_error: Option<String>,
}

impl WebDecoder {
    /// Create a WebCodecs decoder facade.
    ///
    /// Codec support is validated when stream configuration is available. The
    /// browser treats hardware acceleration as a preference, not a guarantee.
    pub fn new(options: DecoderOptions) -> Result<Self, VideoError> {
        if options.max_frames_in_flight == 0 {
            return Err(VideoError::InvalidOption(
                "max_frames_in_flight must be greater than zero",
            ));
        }
        if !webcodecs_available() {
            return Err(VideoError::Backend {
                backend: "webcodecs",
                operation: "construct decoder",
                message: "VideoDecoder or EncodedVideoChunk is unavailable in this browser"
                    .to_owned(),
            });
        }
        Ok(Self {
            options,
            tracker: CodecConfigTracker::default(),
            session: None,
            active_config: None,
            config_prefix: Bytes::new(),
            waiting_for_keyframe: true,
            frames: LatestFrameMailbox::default(),
            stats: StatsHandle::default(),
            pending: HashMap::new(),
            next_token: 1,
            capabilities: api_capabilities(),
            last_error: None,
        })
    }

    /// Report whether the WebCodecs API exists in the current browser.
    ///
    /// Use [`Self::is_config_supported`] when an SPS-derived stream
    /// configuration is available and exact codec support is required.
    pub fn probe_capabilities() -> DecoderCapabilities {
        api_capabilities()
    }

    /// Ask the browser whether it supports a concrete SPS-derived stream.
    pub async fn is_config_supported(
        config: &CodecConfig,
        options: DecoderOptions,
    ) -> Result<bool, VideoError> {
        if !webcodecs_available() {
            return Ok(false);
        }
        let stream = config.stream_info()?;
        for candidate in decoder_configs(config.codec(), &stream, options) {
            match candidate.is_supported().await {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(error) => return Err(web_error("query decoder support", error)),
            }
        }
        Ok(false)
    }

    /// Most recent asynchronous decoder error observed while polling output.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Number of encoded chunks currently queued by the browser decoder.
    pub fn decode_queue_size(&self) -> u32 {
        self.session
            .as_ref()
            .map_or(0, |session| session.decoder.queue_size())
    }

    /// Await all chunks accepted by WebCodecs, collect ready output, and reset.
    ///
    /// The synchronous [`VideoDecoder::flush`] implementation intentionally
    /// closes the browser decoder immediately. Use this method when completion
    /// matters, such as before finalizing a recording.
    pub async fn flush_async(&mut self) -> Result<(), VideoError> {
        if let Some(session) = &self.session {
            if let Err(error) = session.decoder.flush().await {
                let error = web_error("flush decoder", error);
                self.remember_poll_error(&error);
                return Err(error);
            }
        }
        if let Err(error) = self.drain_output() {
            self.remember_poll_error(&error);
            return Err(error);
        }
        self.reset();
        Ok(())
    }

    fn replace_session(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        let stream = config.stream_info()?;
        self.session = None;
        self.frames.clear();
        self.pending.clear();
        self.last_error = None;

        let mut errors = Vec::new();
        let mut session = None;
        for candidate in decoder_configs(config.codec(), &stream, self.options) {
            match candidate.build() {
                Ok((decoder, decoded)) => {
                    session = Some(WebCodecsSession { decoder, decoded });
                    break;
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        let Some(session) = session else {
            return Err(VideoError::Backend {
                backend: "webcodecs",
                operation: "configure decoder",
                message: errors.join("; "),
            });
        };
        self.config_prefix = Bytes::from(config.to_annex_b());
        self.session = Some(session);
        self.active_config = Some(config);
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| {
            stats.reconfigurations += 1;
            stats.frames_in_flight = 0;
        });
        Ok(())
    }

    fn drain_output(&mut self) -> Result<(), VideoError> {
        let mut ready = Vec::new();
        if let Some(session) = &mut self.session {
            loop {
                match session.decoded.next().now_or_never() {
                    Some(Ok(Some(frame))) => ready.push(frame),
                    Some(Ok(None)) | None => break,
                    Some(Err(error)) => return Err(web_error("receive decoded frame", error)),
                }
            }
        }
        for frame in ready {
            self.accept_ready_frame(frame);
        }
        Ok(())
    }

    fn accept_ready_frame(&mut self, frame: web_codecs::VideoFrame) {
        let token = u64::try_from(frame.timestamp().as_micros()).unwrap_or(u64::MAX);
        let Some(pending) = self.pending.remove(&token) else {
            return;
        };
        let latency_us =
            u64::try_from(pending.submitted_at.elapsed().as_micros()).unwrap_or(u64::MAX);
        let duration = frame.duration().and_then(duration_timestamp);
        let replaced = self.frames.replace(DecodedFrame {
            surface: WebVideoFrame::new(frame),
            timestamp: pending.timestamp,
            duration,
        });
        self.stats.update(|stats| {
            stats.frames_decoded += 1;
            stats.output_drops += u64::from(replaced);
            stats.frames_in_flight = self.pending.len();
            stats.last_decode_latency_us = latency_us;
            stats.max_decode_latency_us = stats.max_decode_latency_us.max(latency_us);
        });
    }

    fn recover_from_backpressure(&mut self) {
        self.session = None;
        self.pending.clear();
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| {
            stats.backpressure_drops += 1;
            stats.frames_in_flight = 0;
        });
    }

    fn remember_poll_error(&mut self, error: &VideoError) {
        self.last_error = Some(error.to_string());
        self.session = None;
        self.pending.clear();
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| {
            stats.decode_errors += 1;
            stats.frames_in_flight = 0;
        });
    }

    fn reset(&mut self) {
        self.session = None;
        self.frames.clear();
        self.pending.clear();
        self.tracker.reset();
        self.active_config = None;
        self.config_prefix = Bytes::new();
        self.waiting_for_keyframe = true;
        self.last_error = None;
        self.stats.update(|stats| stats.frames_in_flight = 0);
    }
}

impl VideoDecoder for WebDecoder {
    type Surface = WebVideoFrame;

    fn capabilities(&self) -> DecoderCapabilities {
        self.capabilities.clone()
    }

    fn configure(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        self.replace_session(config)
    }

    fn submit(&mut self, mut frame: EncodedAccessUnit) -> Result<SubmitOutcome, VideoError> {
        self.stats.update(|stats| stats.access_units_received += 1);
        if let Err(error) = self.drain_output() {
            self.remember_poll_error(&error);
            return Err(error);
        }
        frame.keyframe |= CodecConfigTracker::is_keyframe(frame.codec, &frame.data)?;
        let update = self.tracker.observe(frame.codec, &frame.data)?;
        let mut reconfigured = false;
        if let ConfigUpdate::Changed(config) = update {
            self.replace_session(config)?;
            reconfigured = true;
        } else if self.session.is_none() {
            let Some(config) = self
                .active_config
                .clone()
                .or_else(|| self.tracker.config(frame.codec).cloned())
            else {
                self.stats.update(|stats| stats.waiting_drops += 1);
                return Ok(SubmitOutcome::WaitingForConfiguration);
            };
            self.replace_session(config)?;
            reconfigured = true;
        }

        let configured = self
            .active_config
            .as_ref()
            .map(CodecConfig::codec)
            .expect("configured WebCodecs session has an active codec");
        if configured != frame.codec {
            return Err(VideoError::CodecMismatch {
                configured,
                received: frame.codec,
            });
        }
        if self.waiting_for_keyframe && !frame.keyframe {
            self.stats.update(|stats| stats.waiting_drops += 1);
            return Ok(SubmitOutcome::WaitingForKeyframe);
        }
        let queue_size = self.decode_queue_size() as usize;
        if self.pending.len() >= self.options.max_frames_in_flight
            || queue_size >= self.options.max_frames_in_flight
        {
            self.recover_from_backpressure();
            return Ok(SubmitOutcome::DroppedForBackpressure);
        }

        let payload = if self.waiting_for_keyframe {
            let mut bytes = Vec::with_capacity(self.config_prefix.len() + frame.data.len());
            bytes.extend_from_slice(&self.config_prefix);
            bytes.extend_from_slice(&frame.data);
            Bytes::from(bytes)
        } else {
            frame.data
        };
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1).max(1);
        self.pending.insert(
            token,
            PendingFrame {
                timestamp: frame.timestamp,
                submitted_at: Instant::now(),
            },
        );
        let result = self
            .session
            .as_ref()
            .expect("configured WebCodecs session exists")
            .decoder
            .decode(EncodedFrame {
                payload,
                timestamp: std::time::Duration::from_micros(token),
                keyframe: frame.keyframe,
            });
        if let Err(error) = result {
            self.pending.remove(&token);
            self.stats.update(|stats| {
                stats.decode_errors += 1;
                stats.frames_in_flight = self.pending.len();
            });
            return Err(web_error("submit encoded frame", error));
        }
        self.stats.update(|stats| {
            stats.access_units_submitted += 1;
            stats.frames_in_flight = self.pending.len();
        });
        if frame.keyframe {
            self.waiting_for_keyframe = false;
        }
        if let Err(error) = self.drain_output() {
            self.remember_poll_error(&error);
            return Err(error);
        }
        Ok(if reconfigured {
            SubmitOutcome::Reconfigured
        } else {
            SubmitOutcome::Submitted
        })
    }

    fn latest_frame(&mut self) -> Option<DecodedFrame<Self::Surface>> {
        if let Err(error) = self.drain_output() {
            self.remember_poll_error(&error);
        }
        self.frames.take()
    }

    fn flush(&mut self) -> Result<(), VideoError> {
        self.reset();
        Ok(())
    }

    fn stats(&self) -> DecoderStats {
        self.stats.snapshot()
    }
}

fn decoder_configs(
    codec: VideoCodec,
    info: &CodecStreamInfo,
    options: DecoderOptions,
) -> Vec<VideoDecoderConfig> {
    codec_strings(codec, &info.codec_string)
        .into_iter()
        .map(|codec| {
            let mut candidate = VideoDecoderConfig::new(codec);
            candidate.resolution = Some(Dimensions::new(
                info.coded_dimensions.width,
                info.coded_dimensions.height,
            ));
            candidate.display = Some(Dimensions::new(
                info.visible_dimensions.width,
                info.visible_dimensions.height,
            ));
            candidate.hardware_acceleration = options.require_hardware.then_some(true);
            candidate.latency_optimized = Some(options.low_latency);
            // No description means Annex-B mode. Parameter sets are prepended
            // in-band to the first keyframe after every configuration.
            candidate.description = None;
            candidate
        })
        .collect()
}

fn codec_strings(codec: VideoCodec, codec_string: &str) -> Vec<String> {
    match codec {
        VideoCodec::H264 => unique_strings([codec_string, "avc1.42E01E"]),
        VideoCodec::H265 => {
            let alternate = codec_string.replacen("hev1.", "hvc1.", 1);
            unique_strings([
                codec_string,
                alternate.as_str(),
                "hev1.1.6.L93.B0",
                "hvc1.1.6.L93.B0",
            ])
        }
    }
}

fn unique_strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    let mut result = Vec::with_capacity(N);
    for value in values {
        if !result.iter().any(|existing| existing == value) {
            result.push(value.to_owned());
        }
    }
    result
}

fn duration_timestamp(duration: std::time::Duration) -> Option<VideoTimestamp> {
    i64::try_from(duration.as_micros())
        .ok()
        .and_then(|value| VideoTimestamp::new(value, 1_000_000))
}

fn webcodecs_available() -> bool {
    let global = js_sys::global();
    ["VideoDecoder", "EncodedVideoChunk"]
        .into_iter()
        .all(|name| js_sys::Reflect::has(&global, &JsValue::from_str(name)).unwrap_or(false))
}

fn api_capabilities() -> DecoderCapabilities {
    let available = webcodecs_available();
    DecoderCapabilities {
        backend: "webcodecs",
        codecs: vec![
            CodecCapability {
                codec: VideoCodec::H264,
                supported: available,
                hardware_accelerated: false,
                hardware_acceleration_known: false,
            },
            CodecCapability {
                codec: VideoCodec::H265,
                supported: available,
                hardware_accelerated: false,
                hardware_acceleration_known: false,
            },
        ],
        native_surfaces: true,
    }
}

fn web_error(operation: &'static str, error: impl std::fmt::Display) -> VideoError {
    VideoError::Backend {
        backend: "webcodecs",
        operation,
        message: error.to_string(),
    }
}
