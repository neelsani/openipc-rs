use std::{collections::HashMap, time::Instant};

use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use crate::{
    runtime::{LatestFrameMailbox, StatsHandle},
    CodecCapability, CodecConfig, CodecConfigTracker, ConfigUpdate, DecodedFrame,
    DecoderCapabilities, DecoderOptions, DecoderStats, EncodedAccessUnit, SubmitOutcome,
    VideoCodec, VideoDecoder, VideoError, VideoTimestamp,
};

use super::{
    d3d::D3dDevice,
    mft::{probe_codec, MediaFoundationSession, ReadyFrame, SessionSubmit},
    runtime::MediaFoundationRuntime,
    WindowsVideoFrame,
};

struct PendingFrame {
    timestamp: VideoTimestamp,
    submitted_at: Instant,
}

/// Low-latency H.264/H.265 decoder backed by Media Foundation and D3D11.
///
/// Construct and drive this decoder on one worker thread. Produced
/// [`WindowsVideoFrame`] values retain their decoder samples and may be handed
/// to a D3D11/wgpu renderer on another thread.
pub struct WindowsDecoder {
    options: DecoderOptions,
    tracker: CodecConfigTracker,
    session: Option<MediaFoundationSession>,
    active_config: Option<CodecConfig>,
    config_prefix: Vec<u8>,
    waiting_for_keyframe: bool,
    frames: LatestFrameMailbox<DecodedFrame<WindowsVideoFrame>>,
    stats: StatsHandle,
    pending: HashMap<u64, PendingFrame>,
    next_token: u64,
    capabilities: DecoderCapabilities,
    d3d: D3dDevice,
    // Media Foundation must remain started until sessions, retained samples,
    // and the D3D device have all been dropped. Fields drop in declaration order.
    _runtime: MediaFoundationRuntime,
}

impl WindowsDecoder {
    /// Create a hardware decoder using the default D3D11 adapter.
    pub fn new(options: DecoderOptions) -> Result<Self, VideoError> {
        if options.max_frames_in_flight == 0 {
            return Err(VideoError::InvalidOption(
                "max_frames_in_flight must be greater than zero",
            ));
        }
        let runtime = MediaFoundationRuntime::new()?;
        let d3d = D3dDevice::new()?;
        let capabilities = capabilities_for_device(&d3d);
        Ok(Self {
            options,
            tracker: CodecConfigTracker::default(),
            session: None,
            active_config: None,
            config_prefix: Vec::new(),
            waiting_for_keyframe: true,
            frames: LatestFrameMailbox::default(),
            stats: StatsHandle::default(),
            pending: HashMap::new(),
            next_token: 1,
            capabilities,
            d3d,
            _runtime: runtime,
        })
    }

    /// Probe D3D11-aware Media Foundation decoder availability.
    pub fn probe_capabilities() -> DecoderCapabilities {
        MediaFoundationRuntime::new()
            .and_then(|_runtime| D3dDevice::new().map(|d3d| capabilities_for_device(&d3d)))
            .unwrap_or_else(|_| unsupported_capabilities())
    }

    /// D3D11 device shared with the decoder's DXGI device manager.
    pub fn d3d_device(&self) -> &ID3D11Device {
        &self.d3d.device
    }

    /// Friendly name reported by the active Media Foundation decoder.
    pub fn decoder_name(&self) -> Option<&str> {
        self.session
            .as_ref()
            .and_then(MediaFoundationSession::decoder_name)
    }

    /// Copy a decoder-owned NV12 texture into tightly packed CPU planes.
    ///
    /// Renderers with D3D11/wgpu interop should import the texture directly;
    /// this helper exists for portable CPU presentation surfaces such as egui.
    pub fn copy_nv12(
        &self,
        frame: &WindowsVideoFrame,
    ) -> Result<super::WindowsNv12Frame, VideoError> {
        frame.copy_nv12()
    }

    fn ensure_supported(&self, codec: VideoCodec) -> Result<(), VideoError> {
        let capability = self.capabilities.codec(codec);
        let Some(capability) = capability.filter(|entry| entry.supported) else {
            return Err(VideoError::UnsupportedCodec {
                codec,
                backend: "media-foundation",
            });
        };
        if self.options.require_hardware && !capability.hardware_accelerated {
            return Err(VideoError::HardwareDecoderUnavailable {
                codec,
                backend: "media-foundation",
            });
        }
        Ok(())
    }

    fn replace_session(&mut self, config: CodecConfig) -> Result<(), VideoError> {
        self.ensure_supported(config.codec())?;
        if let Some(mut session) = self.session.take() {
            let _ = session.flush();
        }
        self.frames.clear();
        self.pending.clear();
        self.stats.update(|stats| stats.frames_in_flight = 0);

        let session = MediaFoundationSession::new(
            config.codec(),
            &self.d3d.manager,
            self.options.low_latency,
        )?;
        self.config_prefix = config.to_annex_b();
        self.session = Some(session);
        self.active_config = Some(config);
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| stats.reconfigurations += 1);
        Ok(())
    }

    fn accept_ready_frames(&mut self, frames: Vec<ReadyFrame>) {
        for mut ready in frames {
            let Some(pending) = self.pending.remove(&ready.token) else {
                continue;
            };
            let elapsed = pending.submitted_at.elapsed().as_micros();
            let latency_us = u64::try_from(elapsed).unwrap_or(u64::MAX);
            ready.surface.attach_readback(self.d3d.clone());
            let replaced = self.frames.replace(DecodedFrame {
                surface: ready.surface,
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

impl VideoDecoder for WindowsDecoder {
    type Surface = WindowsVideoFrame;

    fn capabilities(&self) -> DecoderCapabilities {
        self.capabilities.clone()
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
            .expect("configured Media Foundation session has an active codec");
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
            self.stats.update(|stats| stats.backpressure_drops += 1);
            return Ok(SubmitOutcome::DroppedForBackpressure);
        }

        let mut prefixed = Vec::new();
        let bitstream = if self.waiting_for_keyframe {
            prefixed.reserve(self.config_prefix.len() + frame.data.len());
            prefixed.extend_from_slice(&self.config_prefix);
            prefixed.extend_from_slice(&frame.data);
            prefixed.as_slice()
        } else {
            &frame.data
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
            .as_mut()
            .expect("configured Media Foundation session exists")
            .submit(token, bitstream);
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
                    if let VideoError::Platform { status, .. } = &error {
                        stats.last_platform_status = Some(*status);
                    }
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
        self.config_prefix.clear();
        self.waiting_for_keyframe = true;
        self.stats.update(|stats| stats.frames_in_flight = 0);
        Ok(())
    }

    fn stats(&self) -> DecoderStats {
        self.stats.snapshot()
    }
}

fn capabilities_for_device(d3d: &D3dDevice) -> DecoderCapabilities {
    let h264 = d3d.supports_hardware_decode(VideoCodec::H264)
        && probe_codec(VideoCodec::H264, &d3d.manager);
    let h265 = d3d.supports_hardware_decode(VideoCodec::H265)
        && probe_codec(VideoCodec::H265, &d3d.manager);
    DecoderCapabilities {
        backend: "media-foundation",
        codecs: vec![
            CodecCapability {
                codec: VideoCodec::H264,
                supported: h264,
                hardware_accelerated: h264,
                hardware_acceleration_known: true,
            },
            CodecCapability {
                codec: VideoCodec::H265,
                supported: h265,
                hardware_accelerated: h265,
                hardware_acceleration_known: true,
            },
        ],
        native_surfaces: true,
    }
}

fn unsupported_capabilities() -> DecoderCapabilities {
    DecoderCapabilities {
        backend: "media-foundation",
        codecs: vec![
            CodecCapability {
                codec: VideoCodec::H264,
                supported: false,
                hardware_accelerated: false,
                hardware_acceleration_known: true,
            },
            CodecCapability {
                codec: VideoCodec::H265,
                supported: false,
                hardware_accelerated: false,
                hardware_acceleration_known: true,
            },
        ],
        native_surfaces: true,
    }
}
