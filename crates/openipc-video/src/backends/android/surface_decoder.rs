use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use ndk::native_window::NativeWindow;

use crate::{
    runtime::{LatestFrameMailbox, StatsHandle},
    CodecCapability, CodecConfig, CodecConfigTracker, ConfigUpdate, DecodedFrame, DecodedSurface,
    DecoderCapabilities, DecoderOptions, DecoderStats, EncodedAccessUnit, FrameDimensions,
    PixelFormat, SubmitOutcome, VideoCodec, VideoDecoder, VideoError, VideoTimestamp,
};

use super::{
    session::codec_available,
    surface_session::{SurfaceMediaCodecSession, SurfaceSessionSubmit},
};

const STALL_RECOVERY_AFTER: Duration = Duration::from_millis(500);

struct PendingFrame {
    timestamp: VideoTimestamp,
    submitted_at: Instant,
}

/// Notification that MediaCodec released a decoded frame to its output surface.
#[derive(Debug, Clone, Copy)]
pub struct AndroidPresentedFrame {
    dimensions: FrameDimensions,
}

impl DecodedSurface for AndroidPresentedFrame {
    fn dimensions(&self) -> FrameDimensions {
        self.dimensions
    }

    fn pixel_format(&self) -> PixelFormat {
        PixelFormat::Native(0)
    }
}

/// Android H.264/H.265 decoder that renders directly to an `ANativeWindow`.
///
/// Unlike [`super::AndroidDecoder`], this decoder does not expose image planes.
/// It is intended for the lowest-latency display path where the application owns
/// a SurfaceTexture or another GPU-presentable Android surface.
pub struct AndroidSurfaceDecoder {
    options: DecoderOptions,
    output_window: NativeWindow,
    tracker: CodecConfigTracker,
    session: Option<SurfaceMediaCodecSession>,
    active_config: Option<CodecConfig>,
    active_dimensions: Option<FrameDimensions>,
    waiting_for_keyframe: bool,
    frames: LatestFrameMailbox<DecodedFrame<AndroidPresentedFrame>>,
    stats: StatsHandle,
    pending: HashMap<u64, PendingFrame>,
    next_token: u64,
    backpressure_warning_emitted: bool,
    capabilities: DecoderCapabilities,
}

impl AndroidSurfaceDecoder {
    /// Create a decoder whose output is rendered into `output_window`.
    pub fn new(options: DecoderOptions, output_window: NativeWindow) -> Result<Self, VideoError> {
        if options.max_frames_in_flight == 0 {
            return Err(VideoError::InvalidOption(
                "max_frames_in_flight must be greater than zero",
            ));
        }
        Ok(Self {
            options,
            output_window,
            tracker: CodecConfigTracker::default(),
            session: None,
            active_config: None,
            active_dimensions: None,
            waiting_for_keyframe: true,
            frames: LatestFrameMailbox::default(),
            stats: StatsHandle::default(),
            pending: HashMap::new(),
            next_token: 1,
            backpressure_warning_emitted: false,
            capabilities: Self::probe_capabilities(),
        })
    }

    /// Probe AVC and HEVC support exposed by Android MediaCodec.
    pub fn probe_capabilities() -> DecoderCapabilities {
        let h264 = codec_available(VideoCodec::H264);
        let h265 = codec_available(VideoCodec::H265);
        DecoderCapabilities {
            backend: "mediacodec-surface",
            codecs: vec![
                CodecCapability {
                    codec: VideoCodec::H264,
                    supported: h264,
                    hardware_accelerated: false,
                    hardware_acceleration_known: false,
                },
                CodecCapability {
                    codec: VideoCodec::H265,
                    supported: h265,
                    hardware_accelerated: false,
                    hardware_acceleration_known: false,
                },
            ],
            native_surfaces: true,
        }
    }

    fn ensure_supported(&self, codec: VideoCodec) -> Result<(), VideoError> {
        if self
            .capabilities
            .codec(codec)
            .is_some_and(|capability| capability.supported)
        {
            Ok(())
        } else {
            Err(VideoError::UnsupportedCodec {
                codec,
                backend: "mediacodec-surface",
            })
        }
    }

    fn replace_session(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        self.ensure_supported(config.codec())?;
        let stream = config.stream_info()?;
        log::info!(
            target: "openipc_video::mediacodec_surface",
            "configuring direct-surface decoder codec={} dimensions={}x{}",
            config.codec(),
            stream.visible_dimensions.width,
            stream.visible_dimensions.height
        );
        self.session = None;
        self.frames.clear();
        self.pending.clear();
        let session = SurfaceMediaCodecSession::new(
            &config,
            &stream,
            self.options.low_latency,
            self.output_window.clone(),
        )?;
        self.session = Some(session);
        self.active_dimensions = Some(stream.visible_dimensions);
        self.active_config = Some(config);
        self.waiting_for_keyframe = true;
        self.backpressure_warning_emitted = false;
        self.stats.update(|stats| {
            stats.reconfigurations += 1;
            stats.frames_in_flight = 0;
        });
        Ok(())
    }

    fn accept_token(&mut self, token: u64) {
        let Some(pending) = self.pending.remove(&token) else {
            return;
        };
        let superseded = self
            .pending
            .keys()
            .copied()
            .filter(|pending_token| *pending_token < token)
            .collect::<Vec<_>>();
        for token in &superseded {
            self.pending.remove(token);
        }
        let Some(dimensions) = self.active_dimensions else {
            return;
        };
        let latency_us =
            u64::try_from(pending.submitted_at.elapsed().as_micros()).unwrap_or(u64::MAX);
        let replaced = self.frames.replace(DecodedFrame {
            surface: AndroidPresentedFrame { dimensions },
            timestamp: pending.timestamp,
            duration: None,
        });
        self.stats.update(|stats| {
            stats.frames_decoded += 1;
            stats.output_drops += u64::from(replaced) + superseded.len() as u64;
            stats.frames_in_flight = self.pending.len();
            stats.last_decode_latency_us = latency_us;
            stats.max_decode_latency_us = stats.max_decode_latency_us.max(latency_us);
        });
    }

    fn poll_output(&mut self) -> Result<(), VideoError> {
        if let Some(token) = self
            .session
            .as_ref()
            .map(SurfaceMediaCodecSession::poll)
            .transpose()?
            .flatten()
        {
            self.accept_token(token);
        }
        Ok(())
    }

    fn record_backpressure_drop(&mut self) {
        if !self.backpressure_warning_emitted {
            log::warn!(
                target: "openipc_video::mediacodec_surface",
                "decoder backpressure; dropping dependent access units until the next keyframe"
            );
            self.backpressure_warning_emitted = true;
        }
        self.waiting_for_keyframe = true;
        let frames_in_flight = self.pending.len();
        self.stats.update(|stats| {
            stats.backpressure_drops += 1;
            stats.frames_in_flight = frames_in_flight;
        });
    }

    fn recover_stalled_session(&mut self) {
        let stalled = self
            .pending
            .values()
            .map(|pending| pending.submitted_at)
            .min()
            .is_some_and(|oldest| oldest.elapsed() >= STALL_RECOVERY_AFTER);
        if stalled {
            log::warn!(
                target: "openipc_video::mediacodec_surface",
                "MediaCodec surface output stalled for 500 ms; resetting decoder session"
            );
            self.session = None;
            self.frames.clear();
            self.pending.clear();
            self.waiting_for_keyframe = true;
            self.backpressure_warning_emitted = false;
            self.stats.update(|stats| {
                stats.decode_errors += 1;
                stats.frames_in_flight = 0;
            });
        }
    }
}

impl VideoDecoder for AndroidSurfaceDecoder {
    type Surface = AndroidPresentedFrame;

    fn capabilities(&self) -> DecoderCapabilities {
        self.capabilities.clone()
    }

    fn configure(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        self.replace_session(config)
    }

    fn submit(&mut self, mut frame: EncodedAccessUnit) -> Result<SubmitOutcome, VideoError> {
        self.stats.update(|stats| stats.access_units_received += 1);
        self.poll_output()?;
        self.recover_stalled_session();
        let (update, observed_keyframe) = self.tracker.inspect(frame.codec, &frame.data)?;
        frame.keyframe |= observed_keyframe;
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
            .expect("configured MediaCodec surface session has an active codec");
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
        if self.pending.len() >= self.options.max_frames_in_flight {
            self.record_backpressure_drop();
            return Ok(SubmitOutcome::DroppedForBackpressure);
        }
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
            .expect("configured MediaCodec surface session exists")
            .submit(token, &frame.data, frame.keyframe);
        match result {
            Ok(SurfaceSessionSubmit::Accepted(ready)) => {
                self.stats.update(|stats| {
                    stats.access_units_submitted += 1;
                    stats.frames_in_flight = self.pending.len();
                });
                if let Some(token) = ready {
                    self.accept_token(token);
                }
            }
            Ok(SurfaceSessionSubmit::Backpressure(ready)) => {
                self.pending.remove(&token);
                if let Some(token) = ready {
                    self.accept_token(token);
                }
                self.record_backpressure_drop();
                return Ok(SubmitOutcome::DroppedForBackpressure);
            }
            Err(error) => {
                self.pending.remove(&token);
                self.stats.update(|stats| {
                    stats.decode_errors += 1;
                    stats.frames_in_flight = self.pending.len();
                });
                return Err(error);
            }
        }
        if frame.keyframe {
            self.waiting_for_keyframe = false;
        }
        Ok(if reconfigured {
            SubmitOutcome::Reconfigured
        } else {
            SubmitOutcome::Submitted
        })
    }

    fn latest_frame(&mut self) -> Option<DecodedFrame<Self::Surface>> {
        if self.poll_output().is_err() {
            self.stats.update(|stats| stats.decode_errors += 1);
        }
        self.frames.take()
    }

    fn flush(&mut self) -> Result<(), VideoError> {
        self.session = None;
        self.frames.clear();
        self.pending.clear();
        self.tracker.reset();
        self.active_config = None;
        self.active_dimensions = None;
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| stats.frames_in_flight = 0);
        Ok(())
    }

    fn stats(&self) -> DecoderStats {
        self.stats.snapshot()
    }
}
