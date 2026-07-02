use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SampleFormat, SizedSample, Stream, StreamConfig,
};
use openipc_core::rtp::RtpHeader;
use ropus::{Channels, DecodeMode, Decoder};

use crate::model::AudioStats;

pub(crate) struct AudioPlayer {
    decoder: Decoder,
    source_rate: u32,
    source_channels: usize,
    output_rate: u32,
    output_channels: usize,
    volume: f32,
    queue: Arc<Mutex<VecDeque<f32>>>,
    stream_errors: Arc<AtomicU64>,
    stats: AudioStats,
    _stream: Stream,
}

impl AudioPlayer {
    pub(crate) fn new(sample_rate: u32, channels: u8, volume: u8) -> Result<Self, String> {
        let channels = channels.clamp(1, 2) as usize;
        let decoder = Decoder::new(sample_rate, opus_channels(channels))
            .map_err(|error| format!("Opus decoder init failed: {error}"))?;
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device".to_owned())?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("audio output config unavailable: {error}"))?;
        let output_rate = supported.sample_rate();
        let output_channels = usize::from(supported.channels());
        let config: StreamConfig = supported.into();
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let stream_errors = Arc::new(AtomicU64::new(0));
        let stream = match supported.sample_format() {
            SampleFormat::I8 => build_stream::<i8>(&device, &config, &queue, &stream_errors),
            SampleFormat::I16 => build_stream::<i16>(&device, &config, &queue, &stream_errors),
            SampleFormat::I24 => {
                build_stream::<cpal::I24>(&device, &config, &queue, &stream_errors)
            }
            SampleFormat::I32 => build_stream::<i32>(&device, &config, &queue, &stream_errors),
            SampleFormat::I64 => build_stream::<i64>(&device, &config, &queue, &stream_errors),
            SampleFormat::U8 => build_stream::<u8>(&device, &config, &queue, &stream_errors),
            SampleFormat::U16 => build_stream::<u16>(&device, &config, &queue, &stream_errors),
            SampleFormat::U32 => build_stream::<u32>(&device, &config, &queue, &stream_errors),
            SampleFormat::U64 => build_stream::<u64>(&device, &config, &queue, &stream_errors),
            SampleFormat::F32 => build_stream::<f32>(&device, &config, &queue, &stream_errors),
            SampleFormat::F64 => build_stream::<f64>(&device, &config, &queue, &stream_errors),
            format => Err(format!("unsupported audio sample format {format}")),
        }?;
        stream
            .play()
            .map_err(|error| format!("audio output start failed: {error}"))?;

        Ok(Self {
            decoder,
            source_rate: sample_rate,
            source_channels: channels,
            output_rate,
            output_channels,
            volume: f32::from(volume.min(100)) / 100.0,
            queue,
            stream_errors,
            stats: AudioStats {
                enabled: true,
                supported: true,
                decoder_name: "ropus / CPAL".to_owned(),
                ..AudioStats::default()
            },
            _stream: stream,
        })
    }

    pub(crate) fn push_rtp(&mut self, packet: &[u8]) -> Result<(), String> {
        self.stats.packets = self.stats.packets.saturating_add(1);
        self.stats.bytes = self.stats.bytes.saturating_add(packet.len() as u64);
        let header =
            RtpHeader::parse(packet).map_err(|error| format!("invalid audio RTP: {error:?}"))?;
        let payload = header.payload(packet);
        let max_frames = (self.source_rate as usize * 120) / 1_000;
        let mut decoded = vec![0.0; max_frames * self.source_channels];
        let frames = self
            .decoder
            .decode_float(payload, &mut decoded, DecodeMode::Normal)
            .map_err(|error| format!("Opus decode failed: {error}"))?;
        decoded.truncate(frames * self.source_channels);
        let output = resample_and_remix(
            &decoded,
            self.source_rate,
            self.source_channels,
            self.output_rate,
            self.output_channels,
            self.volume,
        );
        let mut queue = self.queue.lock().map_err(|_| "audio queue poisoned")?;
        let max_samples = self.output_rate as usize * self.output_channels / 4;
        let overflow = queue
            .len()
            .saturating_add(output.len())
            .saturating_sub(max_samples);
        if overflow > 0 {
            let drop_count = overflow.min(queue.len());
            queue.drain(..drop_count);
        }
        queue.extend(output);
        self.stats.decoded_frames = self.stats.decoded_frames.saturating_add(1);
        self.stats.queued_ms =
            queue.len() as f64 * 1_000.0 / (self.output_rate as f64 * self.output_channels as f64);
        Ok(())
    }

    pub(crate) fn record_error(&mut self) {
        self.stats.errors = self.stats.errors.saturating_add(1);
    }

    pub(crate) fn set_volume(&mut self, volume: u8) {
        self.volume = f32::from(volume.min(100)) / 100.0;
    }

    pub(crate) fn stats(&self) -> AudioStats {
        let mut stats = self.stats.clone();
        stats.errors = stats
            .errors
            .saturating_add(self.stream_errors.load(Ordering::Relaxed));
        if let Ok(queue) = self.queue.lock() {
            stats.queued_ms = queue.len() as f64 * 1_000.0
                / (self.output_rate as f64 * self.output_channels as f64);
        }
        stats
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    queue: &Arc<Mutex<VecDeque<f32>>>,
    errors: &Arc<AtomicU64>,
) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let queue = Arc::clone(queue);
    let errors_for_callback = Arc::clone(errors);
    let errors_for_stream = Arc::clone(errors);
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _| {
                if let Ok(mut queue) = queue.try_lock() {
                    for sample in output {
                        *sample = T::from_sample(queue.pop_front().unwrap_or(0.0));
                    }
                } else {
                    output.fill_with(|| T::from_sample(0.0));
                    errors_for_callback.fetch_add(1, Ordering::Relaxed);
                }
            },
            move |_| {
                errors_for_stream.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|error| format!("audio output creation failed: {error}"))
}

fn opus_channels(channels: usize) -> Channels {
    if channels == 2 {
        Channels::Stereo
    } else {
        Channels::Mono
    }
}

fn resample_and_remix(
    input: &[f32],
    input_rate: u32,
    input_channels: usize,
    output_rate: u32,
    output_channels: usize,
    volume: f32,
) -> Vec<f32> {
    let input_frames = input.len() / input_channels;
    if input_frames == 0 {
        return Vec::new();
    }
    let output_frames =
        (input_frames as u64 * u64::from(output_rate) / u64::from(input_rate)).max(1) as usize;
    let mut output = Vec::with_capacity(output_frames * output_channels);
    for output_frame in 0..output_frames {
        let source = output_frame as f64 * input_rate as f64 / output_rate as f64;
        let left = source.floor() as usize;
        let right = (left + 1).min(input_frames - 1);
        let mix = (source - left as f64) as f32;
        for output_channel in 0..output_channels {
            let input_channel = output_channel.min(input_channels - 1);
            let a = input[left * input_channels + input_channel];
            let b = input[right * input_channels + input_channel];
            output.push((a + (b - a) * mix) * volume);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::resample_and_remix;

    #[test]
    fn mono_is_remixed_to_stereo() {
        let output = resample_and_remix(&[0.5, -0.5], 48_000, 1, 48_000, 2, 1.0);
        assert_eq!(output, [0.5, 0.5, -0.5, -0.5]);
    }
}
