use crate::{
    runtime::{LatestFrameMailbox, StatsHandle},
    CodecCapability, CodecConfig, CodecConfigTracker, ConfigUpdate, DecodedFrame,
    DecoderCapabilities, DecoderOptions, DecoderStats, EncodedAccessUnit, SubmitOutcome,
    VideoCodec, VideoDecoder, VideoError,
};

use super::{callback::CallbackState, session::VideoToolboxSession, MacOsVideoFrame};

/// Low-latency H.264/H.265 decoder backed by macOS VideoToolbox.
pub struct MacOsDecoder {
    options: DecoderOptions,
    tracker: CodecConfigTracker,
    session: Option<VideoToolboxSession>,
    active_config: Option<CodecConfig>,
    waiting_for_keyframe: bool,
    frames: LatestFrameMailbox<DecodedFrame<MacOsVideoFrame>>,
    stats: StatsHandle,
    callback: CallbackState,
}

impl MacOsDecoder {
    /// Create a VideoToolbox decoder using the supplied latency policy.
    pub fn new(options: DecoderOptions) -> Result<Self, VideoError> {
        if options.max_frames_in_flight == 0 {
            return Err(VideoError::InvalidOption(
                "max_frames_in_flight must be greater than zero",
            ));
        }
        let frames = LatestFrameMailbox::default();
        let stats = StatsHandle::default();
        let callback = CallbackState::new(frames.clone(), stats.clone());
        Ok(Self {
            options,
            tracker: CodecConfigTracker::default(),
            session: None,
            active_config: None,
            waiting_for_keyframe: true,
            frames,
            stats,
            callback,
        })
    }

    /// Query VideoToolbox H.264 and H.265 hardware support.
    pub fn probe_capabilities() -> DecoderCapabilities {
        use objc2_core_media::{kCMVideoCodecType_H264, kCMVideoCodecType_HEVC};
        use objc2_video_toolbox::VTIsHardwareDecodeSupported;

        // SAFETY: These codec constants are valid inputs and the function is
        // available on every macOS version supported by this crate.
        let h264 = unsafe { VTIsHardwareDecodeSupported(kCMVideoCodecType_H264) };
        // SAFETY: See the H.264 call above.
        let h265 = unsafe { VTIsHardwareDecodeSupported(kCMVideoCodecType_HEVC) };
        DecoderCapabilities {
            backend: "videotoolbox",
            codecs: vec![
                CodecCapability {
                    codec: VideoCodec::H264,
                    supported: true,
                    hardware_accelerated: h264,
                    hardware_acceleration_known: true,
                },
                CodecCapability {
                    codec: VideoCodec::H265,
                    supported: true,
                    hardware_accelerated: h265,
                    hardware_acceleration_known: true,
                },
            ],
            native_surfaces: true,
        }
    }

    fn supports_requested_codec(&self, codec: VideoCodec) -> Result<(), VideoError> {
        let capability = Self::probe_capabilities().codec(codec);
        let Some(capability) = capability.filter(|entry| entry.supported) else {
            return Err(VideoError::UnsupportedCodec {
                codec,
                backend: "videotoolbox",
            });
        };
        if self.options.require_hardware && !capability.hardware_accelerated {
            return Err(VideoError::HardwareDecoderUnavailable {
                codec,
                backend: "videotoolbox",
            });
        }
        Ok(())
    }

    fn replace_session(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        log::info!(target: "openipc_video::videotoolbox", "configuring decoder codec={}", config.codec());
        self.supports_requested_codec(config.codec())?;
        if let Some(session) = self.session.take() {
            session.finish()?;
        }
        self.frames.clear();
        self.session = Some(VideoToolboxSession::new(
            &config,
            self.options,
            self.callback.clone(),
        )?);
        self.active_config = Some(config);
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| stats.reconfigurations += 1);
        Ok(())
    }
}

impl VideoDecoder for MacOsDecoder {
    type Surface = MacOsVideoFrame;

    fn capabilities(&self) -> DecoderCapabilities {
        Self::probe_capabilities()
    }

    fn configure(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        self.replace_session(config)
    }

    fn submit(&mut self, mut frame: EncodedAccessUnit) -> Result<SubmitOutcome, VideoError> {
        self.stats.update(|stats| stats.access_units_received += 1);
        let observed_keyframe = CodecConfigTracker::is_keyframe(frame.codec, &frame.data)?;
        frame.keyframe |= observed_keyframe;

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
            .expect("configured VideoToolbox session must have an active codec");
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
        if self.stats.snapshot().frames_in_flight >= self.options.max_frames_in_flight {
            log::warn!(target: "openipc_video::videotoolbox", "decoder backpressure; dropping access unit");
            self.stats.update(|stats| stats.backpressure_drops += 1);
            return Ok(SubmitOutcome::DroppedForBackpressure);
        }

        self.session
            .as_ref()
            .expect("configured VideoToolbox session must exist")
            .submit(&frame)?;
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
        if let Some(session) = self.session.take() {
            session.finish()?;
        }
        self.frames.clear();
        self.tracker.reset();
        self.active_config = None;
        self.waiting_for_keyframe = true;
        Ok(())
    }

    fn stats(&self) -> DecoderStats {
        self.stats.snapshot()
    }
}

impl Drop for MacOsDecoder {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            let _ = session.finish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MacOsDecoder;

    #[test]
    fn decoder_can_move_to_a_worker_thread() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MacOsDecoder>();
    }
}
