use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};

use crate::{
    runtime::{LatestFrameMailbox, StatsHandle},
    CodecConfig, CodecConfigTracker, ConfigUpdate, DecodedFrame, DecoderCapabilities,
    DecoderOptions, DecoderStats, EncodedAccessUnit, SubmitOutcome, VideoCodec, VideoDecoder,
    VideoError, VideoTimestamp,
};

use super::{
    device::{probe_capabilities, VaDevice},
    session::{ReadyFrame, SessionSubmit, VaapiSession},
    LinuxVideoFrame,
};

struct PendingFrame {
    timestamp: VideoTimestamp,
    submitted_at: Instant,
}

/// Low-latency H.264/H.265 decoder backed by Linux VA-API.
///
/// Construct and drive this value on the same worker thread. The underlying
/// libva display is thread-affine, while produced [`LinuxVideoFrame`] values
/// are safe to hand to a renderer on another thread.
pub struct LinuxDecoder {
    options: DecoderOptions,
    device: VaDevice,
    tracker: CodecConfigTracker,
    session: Option<VaapiSession>,
    active_config: Option<CodecConfig>,
    waiting_for_keyframe: bool,
    frames: LatestFrameMailbox<DecodedFrame<LinuxVideoFrame>>,
    stats: StatsHandle,
    pending: HashMap<u64, PendingFrame>,
    next_token: u64,
}

impl LinuxDecoder {
    /// Open the first available VA-API DRM render node.
    ///
    /// Set `OPENIPC_VAAPI_DEVICE` to choose a node such as
    /// `/dev/dri/renderD129`.
    pub fn new(options: DecoderOptions) -> Result<Self, VideoError> {
        Self::with_optional_device(options, None)
    }

    /// Open a specific VA-API DRM render node.
    pub fn with_device(
        options: DecoderOptions,
        device_path: impl AsRef<Path>,
    ) -> Result<Self, VideoError> {
        Self::with_optional_device(options, Some(device_path.as_ref().to_owned()))
    }

    fn with_optional_device(
        options: DecoderOptions,
        device_path: Option<PathBuf>,
    ) -> Result<Self, VideoError> {
        if options.max_frames_in_flight == 0 {
            return Err(VideoError::InvalidOption(
                "max_frames_in_flight must be greater than zero",
            ));
        }
        let device = VaDevice::open(device_path)?;
        Ok(Self {
            options,
            device,
            tracker: CodecConfigTracker::default(),
            session: None,
            active_config: None,
            waiting_for_keyframe: true,
            frames: LatestFrameMailbox::default(),
            stats: StatsHandle::default(),
            pending: HashMap::new(),
            next_token: 1,
        })
    }

    /// Probe the first available VA-API device without constructing a decoder.
    pub fn probe_capabilities() -> DecoderCapabilities {
        probe_capabilities()
    }

    /// DRM render node used by this decoder.
    pub fn device_path(&self) -> &Path {
        &self.device.path
    }

    /// Vendor string reported by the active VA-API implementation.
    pub fn vendor(&self) -> Option<&str> {
        self.device.vendor.as_deref()
    }

    fn ensure_supported(&self, codec: VideoCodec) -> Result<(), VideoError> {
        let capability = self.device.capabilities.codec(codec);
        let Some(capability) = capability.filter(|entry| entry.supported) else {
            return Err(VideoError::UnsupportedCodec {
                codec,
                backend: "vaapi",
            });
        };
        if self.options.require_hardware && !capability.hardware_accelerated {
            return Err(VideoError::HardwareDecoderUnavailable {
                codec,
                backend: "vaapi",
            });
        }
        Ok(())
    }

    fn replace_session(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        log::info!(target: "openipc_video::vaapi", "configuring decoder codec={}", config.codec());
        self.ensure_supported(config.codec())?;
        if let Some(mut session) = self.session.take() {
            let _ = session.flush();
        }
        self.frames.clear();
        self.pending.clear();
        self.stats.update(|stats| stats.frames_in_flight = 0);

        let mut session = VaapiSession::new(
            config.codec(),
            self.device.display.clone(),
            self.device.gbm.clone(),
            self.options.max_frames_in_flight.saturating_add(1),
        )?;
        match session.submit(0, &config.to_annex_b())? {
            SessionSubmit::Accepted(_) => {}
            SessionSubmit::Backpressure(_) => {
                return Err(VideoError::Backend {
                    backend: "vaapi",
                    operation: "configure decoder",
                    message: "output pool was exhausted while parsing parameter sets".to_owned(),
                });
            }
        }
        self.session = Some(session);
        self.active_config = Some(config);
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| stats.reconfigurations += 1);
        Ok(())
    }

    fn accept_ready_frames(&mut self, frames: Vec<ReadyFrame>) {
        for ready in frames {
            let Some(pending) = self.pending.remove(&ready.token) else {
                continue;
            };
            let elapsed = pending.submitted_at.elapsed().as_micros();
            let latency_us = u64::try_from(elapsed).unwrap_or(u64::MAX);
            let replaced = self.frames.replace(DecodedFrame {
                surface: LinuxVideoFrame::new(ready.frame, ready.dimensions),
                timestamp: pending.timestamp,
                duration: None,
            });
            self.stats.update(|stats| {
                stats.frames_decoded += 1;
                stats.output_drops += u64::from(replaced);
                stats.frames_in_flight = self.pending.len();
                stats.last_decode_latency_us = latency_us;
                stats.max_decode_latency_us = stats.max_decode_latency_us.max(latency_us);
            });
        }
    }

    fn recover_from_backpressure(&mut self) {
        if let Some(session) = &mut self.session {
            let _ = session.flush();
        }
        self.pending.clear();
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| {
            stats.backpressure_drops += 1;
            stats.frames_in_flight = 0;
        });
    }
}

impl VideoDecoder for LinuxDecoder {
    type Surface = LinuxVideoFrame;

    fn capabilities(&self) -> DecoderCapabilities {
        self.device.capabilities.clone()
    }

    fn configure(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        self.replace_session(config)
    }

    fn submit(&mut self, mut frame: EncodedAccessUnit) -> Result<SubmitOutcome, VideoError> {
        self.stats.update(|stats| stats.access_units_received += 1);
        frame.keyframe |= CodecConfigTracker::is_keyframe(frame.codec, &frame.data)?;

        let update = self.tracker.observe(frame.codec, &frame.data)?;
        let mut reconfigured = false;
        if let ConfigUpdate::Changed(config) = update {
            self.replace_session(config)?;
            reconfigured = true;
        } else if self.session.is_none() {
            let Some(config) = self.tracker.config(frame.codec).cloned() else {
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
            .expect("configured VA-API session has an active codec");
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
            log::warn!(target: "openipc_video::vaapi", "decoder backpressure; dropping access unit");
            self.stats.update(|stats| stats.backpressure_drops += 1);
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
            .as_mut()
            .expect("configured VA-API session exists")
            .submit(token, &frame.data);
        match result {
            Ok(SessionSubmit::Accepted(frames)) => {
                self.stats.update(|stats| {
                    stats.access_units_submitted += 1;
                    stats.frames_in_flight = self.pending.len();
                });
                self.accept_ready_frames(frames);
            }
            Ok(SessionSubmit::Backpressure(frames)) => {
                self.accept_ready_frames(frames);
                self.recover_from_backpressure();
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
        self.frames.take()
    }

    fn flush(&mut self) -> Result<(), VideoError> {
        if let Some(mut session) = self.session.take() {
            let _ = session.flush()?;
        }
        self.frames.clear();
        self.pending.clear();
        self.tracker.reset();
        self.active_config = None;
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| stats.frames_in_flight = 0);
        Ok(())
    }

    fn stats(&self) -> DecoderStats {
        self.stats.snapshot()
    }
}
