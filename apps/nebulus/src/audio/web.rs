use openipc_core::rtp::RtpHeader;
use ropus::{Channels, DecodeMode, Decoder};
use wasm_bindgen::JsCast as _;
use web_sys::{AudioContext, AudioNode, GainNode};

use crate::model::AudioStats;

pub(crate) struct AudioPlayer {
    decoder: Decoder,
    sample_rate: u32,
    channels: usize,
    context: AudioContext,
    gain: GainNode,
    next_start: f64,
    decoded: Vec<f32>,
    channel_samples: Vec<f32>,
    stats: AudioStats,
}

impl AudioPlayer {
    pub(crate) fn new(sample_rate: u32, channels: u8, volume: u8) -> Result<Self, String> {
        let channels = channels.clamp(1, 2) as usize;
        let decoder = Decoder::new(
            sample_rate,
            if channels == 2 {
                Channels::Stereo
            } else {
                Channels::Mono
            },
        )
        .map_err(|error| format!("Opus decoder init failed: {error}"))?;
        let context = AudioContext::new().map_err(js_error)?;
        let gain = context.create_gain().map_err(js_error)?;
        gain.gain().set_value(f32::from(volume.min(100)) / 100.0);
        gain.connect_with_audio_node(context.destination().unchecked_ref::<AudioNode>())
            .map_err(js_error)?;
        let _ = context.resume();
        Ok(Self {
            decoder,
            sample_rate,
            channels,
            next_start: context.current_time(),
            context,
            gain,
            decoded: Vec::new(),
            channel_samples: Vec::new(),
            stats: AudioStats {
                enabled: true,
                supported: true,
                decoder_name: "ropus / Web Audio",
                ..AudioStats::default()
            },
        })
    }

    pub(crate) fn push_rtp(&mut self, packet: &[u8]) -> Result<(), String> {
        self.stats.packets = self.stats.packets.saturating_add(1);
        self.stats.bytes = self.stats.bytes.saturating_add(packet.len() as u64);
        let header =
            RtpHeader::parse(packet).map_err(|error| format!("invalid audio RTP: {error:?}"))?;
        let payload = header.payload(packet);
        let max_frames = (self.sample_rate as usize * 120) / 1_000;
        self.decoded.resize(max_frames * self.channels, 0.0);
        let frames = self
            .decoder
            .decode_float(payload, &mut self.decoded, DecodeMode::Normal)
            .map_err(|error| format!("Opus decode failed: {error}"))?;
        let buffer = self
            .context
            .create_buffer(self.channels as u32, frames as u32, self.sample_rate as f32)
            .map_err(js_error)?;
        for channel in 0..self.channels {
            self.channel_samples.clear();
            self.channel_samples.reserve(frames);
            self.channel_samples
                .extend((0..frames).map(|frame| self.decoded[frame * self.channels + channel]));
            buffer
                .copy_to_channel(&self.channel_samples, channel as i32)
                .map_err(js_error)?;
        }
        let source = self.context.create_buffer_source().map_err(js_error)?;
        source.set_buffer(Some(&buffer));
        source
            .connect_with_audio_node(self.gain.unchecked_ref::<AudioNode>())
            .map_err(js_error)?;
        let now = self.context.current_time();
        if self.next_start < now || self.next_start - now > 0.04 {
            self.next_start = now + 0.005;
        }
        source.start_with_when(self.next_start).map_err(js_error)?;
        self.next_start += frames as f64 / self.sample_rate as f64;
        self.stats.decoded_frames = self.stats.decoded_frames.saturating_add(1);
        self.stats.queued_ms = (self.next_start - now).max(0.0) * 1_000.0;
        Ok(())
    }

    pub(crate) fn record_error(&mut self) {
        self.stats.errors = self.stats.errors.saturating_add(1);
    }

    pub(crate) fn set_volume(&mut self, volume: u8) {
        self.gain
            .gain()
            .set_value(f32::from(volume.min(100)) / 100.0);
    }

    pub(crate) fn stats(&self) -> AudioStats {
        let mut stats = self.stats;
        stats.queued_ms = (self.next_start - self.context.current_time()).max(0.0) * 1_000.0;
        stats
    }
}

fn js_error(error: wasm_bindgen::JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}
