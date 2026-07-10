use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{
    runtime::{LatestFrameMailbox, StatsHandle},
    CodecCapability, CodecConfig, CodecConfigTracker, ConfigUpdate, DecodedFrame,
    DecoderCapabilities, DecoderOptions, DecoderStats, EncodedAccessUnit, SubmitOutcome,
    VideoCodec, VideoDecoder, VideoError, VideoTimestamp,
};

use super::{
    session::{codec_available, MediaCodecSession, SessionSubmit},
    AndroidVideoFrame,
};

const STALL_RECOVERY_AFTER: Duration = Duration::from_millis(500);
const BACKPRESSURE_RECOVERY_WINDOW: Duration = Duration::from_millis(100);
const BACKPRESSURE_RECOVERY_DROPS: u8 = 3;

struct PendingFrame {
    timestamp: VideoTimestamp,
    submitted_at: Instant,
}

/// Android H.264/H.265 decoder backed by NDK MediaCodec and AImageReader.
///
/// Construct and drive the decoder on one worker thread. Output images remain
/// leased until their [`AndroidVideoFrame`] is dropped.
pub struct AndroidDecoder {
    options: DecoderOptions,
    tracker: CodecConfigTracker,
    session: Option<MediaCodecSession>,
    active_config: Option<CodecConfig>,
    waiting_for_keyframe: bool,
    frames: LatestFrameMailbox<DecodedFrame<AndroidVideoFrame>>,
    stats: StatsHandle,
    pending: HashMap<u64, PendingFrame>,
    next_token: u64,
    backpressure_warning_emitted: bool,
    last_backpressure_at: Option<Instant>,
    backpressure_drops_in_window: u8,
    capabilities: DecoderCapabilities,
}

impl AndroidDecoder {
    /// Create a decoder that produces GPU-importable `AHardwareBuffer` frames.
    pub fn new(options: DecoderOptions) -> Result<Self, VideoError> {
        if options.max_frames_in_flight == 0 {
            return Err(VideoError::InvalidOption(
                "max_frames_in_flight must be greater than zero",
            ));
        }
        Ok(Self {
            options,
            tracker: CodecConfigTracker::default(),
            session: None,
            active_config: None,
            waiting_for_keyframe: true,
            frames: LatestFrameMailbox::default(),
            stats: StatsHandle::default(),
            pending: HashMap::new(),
            next_token: 1,
            backpressure_warning_emitted: false,
            last_backpressure_at: None,
            backpressure_drops_in_window: 0,
            capabilities: Self::probe_capabilities(),
        })
    }

    /// Probe whether Android can construct its preferred AVC and HEVC decoders.
    pub fn probe_capabilities() -> DecoderCapabilities {
        let h264 = codec_available(VideoCodec::H264);
        let h265 = codec_available(VideoCodec::H265);
        DecoderCapabilities {
            backend: "mediacodec",
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
        let Some(capability) = self
            .capabilities
            .codec(codec)
            .filter(|capability| capability.supported)
        else {
            return Err(VideoError::UnsupportedCodec {
                codec,
                backend: "mediacodec",
            });
        };
        if self.options.require_hardware
            && capability.hardware_acceleration_known
            && !capability.hardware_accelerated
        {
            return Err(VideoError::HardwareDecoderUnavailable {
                codec,
                backend: "mediacodec",
            });
        }
        Ok(())
    }

    fn replace_session(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        log::info!(target: "openipc_video::mediacodec", "configuring decoder codec={}", config.codec());
        self.ensure_supported(config.codec())?;
        let stream = config.stream_info()?;
        self.session = None;
        self.frames.clear();
        self.pending.clear();
        let session = MediaCodecSession::new(
            &config,
            &stream,
            self.options.max_frames_in_flight,
            self.options.low_latency,
        )?;
        self.session = Some(session);
        self.active_config = Some(config);
        self.waiting_for_keyframe = true;
        self.backpressure_warning_emitted = false;
        self.last_backpressure_at = None;
        self.backpressure_drops_in_window = 0;
        self.stats.update(|stats| {
            stats.reconfigurations += 1;
            stats.frames_in_flight = 0;
        });
        Ok(())
    }

    fn accept_ready_frame(&mut self, frame: AndroidVideoFrame, rendered_outputs: usize) {
        let token = u64::try_from(frame.timestamp_ns())
            .ok()
            .map(|timestamp| timestamp / 1_000)
            .filter(|token| self.pending.contains_key(token))
            .or_else(|| self.pending.keys().copied().max());
        let Some(token) = token else {
            return;
        };
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
        let latency_us =
            u64::try_from(pending.submitted_at.elapsed().as_micros()).unwrap_or(u64::MAX);
        let replaced = self.frames.replace(DecodedFrame {
            surface: frame,
            timestamp: pending.timestamp,
            duration: None,
        });
        let rendered_outputs = rendered_outputs.max(1) as u64;
        self.stats.update(|stats| {
            stats.frames_decoded = stats.frames_decoded.saturating_add(rendered_outputs);
            stats.output_drops = stats
                .output_drops
                .saturating_add(u64::from(replaced) + rendered_outputs.saturating_sub(1));
            stats.frames_in_flight = self.pending.len();
            stats.last_decode_latency_us = latency_us;
            stats.max_decode_latency_us = stats.max_decode_latency_us.max(latency_us);
        });
    }

    fn poll_output(&mut self) -> Result<(), VideoError> {
        let completed = self
            .session
            .as_ref()
            .map(MediaCodecSession::poll)
            .transpose()?
            .unwrap_or_default();
        if let Some(frame) = completed.latest {
            self.accept_ready_frame(frame, completed.rendered_outputs);
        }
        Ok(())
    }

    fn record_backpressure_drop(&mut self) {
        let now = Instant::now();
        if self
            .last_backpressure_at
            .is_none_or(|last| now.duration_since(last) > BACKPRESSURE_RECOVERY_WINDOW)
        {
            self.backpressure_drops_in_window = 0;
        }
        self.last_backpressure_at = Some(now);
        self.backpressure_drops_in_window = self.backpressure_drops_in_window.saturating_add(1);
        if !self.backpressure_warning_emitted {
            log::warn!(
                target: "openipc_video::mediacodec",
                "decoder backpressure; dropping the newest access unit to preserve latency"
            );
            self.backpressure_warning_emitted = true;
        }
        if self.backpressure_drops_in_window >= BACKPRESSURE_RECOVERY_DROPS {
            log::warn!(
                target: "openipc_video::mediacodec",
                "sustained decoder backpressure; suppressing dependent access units until the next keyframe"
            );
            self.waiting_for_keyframe = true;
            self.last_backpressure_at = None;
            self.backpressure_drops_in_window = 0;
        }
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
        if !stalled {
            return;
        }
        log::warn!(
            target: "openipc_video::mediacodec",
            "MediaCodec made no output progress for 500 ms; resetting decoder session"
        );
        self.session = None;
        self.frames.clear();
        self.pending.clear();
        self.waiting_for_keyframe = true;
        self.backpressure_warning_emitted = false;
        self.last_backpressure_at = None;
        self.backpressure_drops_in_window = 0;
        self.stats.update(|stats| {
            stats.decode_errors += 1;
            stats.frames_in_flight = 0;
        });
    }
}

impl VideoDecoder for AndroidDecoder {
    type Surface = AndroidVideoFrame;

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
            .expect("configured MediaCodec session has an active codec");
        if configured != frame.codec {
            return Err(VideoError::CodecMismatch {
                configured,
                received: frame.codec,
            });
        }
        if self.waiting_for_keyframe && !frame.can_resynchronize() {
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
            .expect("configured MediaCodec session exists")
            .submit(token, &frame.data, frame.keyframe);
        match result {
            Ok(SessionSubmit::Accepted(completed)) => {
                self.stats.update(|stats| {
                    stats.access_units_submitted += 1;
                    stats.frames_in_flight = self.pending.len();
                });
                if let Some(ready) = completed.latest {
                    self.accept_ready_frame(ready, completed.rendered_outputs);
                }
            }
            Ok(SessionSubmit::Backpressure(completed)) => {
                // This access unit was not accepted by MediaCodec. Remove its
                // token before matching any output that became ready while the
                // input queue was checked.
                self.pending.remove(&token);
                if let Some(ready) = completed.latest {
                    self.accept_ready_frame(ready, completed.rendered_outputs);
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
            self.last_backpressure_at = None;
            self.backpressure_drops_in_window = 0;
            self.backpressure_warning_emitted = false;
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
        self.waiting_for_keyframe = true;
        self.last_backpressure_at = None;
        self.backpressure_drops_in_window = 0;
        self.backpressure_warning_emitted = false;
        self.stats.update(|stats| stats.frames_in_flight = 0);
        Ok(())
    }

    fn stats(&self) -> DecoderStats {
        self.stats.snapshot()
    }
}
