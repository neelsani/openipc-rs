//! Encoded audio/video recording helpers shared by native and browser builds.

use std::io::Write;

use muxide::api::{AudioCodec, Muxer, MuxerBuilder, VideoCodec};
use openipc_core::{Codec, DepacketizedFrame};
use openipc_video::{CodecConfig, CodecConfigTracker, VideoCodec as DecoderCodec};

const RTP_VIDEO_CLOCK_HZ: u32 = 90_000;
#[cfg(any(target_arch = "wasm32", test))]
const DEFAULT_FRAME_DURATION_TICKS: u32 = RTP_VIDEO_CLOCK_HZ / 30;
const MAX_REASONABLE_FRAME_GAP_TICKS: u32 = RTP_VIDEO_CLOCK_HZ * 2;

/// MP4 video-track metadata recovered from in-band codec parameter sets.
#[derive(Debug, Clone)]
pub(crate) struct Mp4TrackConfig {
    pub(crate) codec: Codec,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Mp4TrackConfig {
    /// Build MP4 track metadata from a keyframe carrying SPS/PPS or VPS/SPS/PPS.
    pub(crate) fn from_keyframe(frame: &DepacketizedFrame) -> Result<Self, String> {
        let codec = DecoderCodec::from(frame.codec);
        let mut tracker = CodecConfigTracker::default();
        tracker
            .observe(codec, &frame.data)
            .map_err(|error| format!("inspect codec configuration failed: {error}"))?;
        let config = tracker.config(codec).ok_or_else(|| {
            "keyframe did not contain the codec parameter sets required for MP4".to_owned()
        })?;
        let stream = config
            .stream_info()
            .map_err(|error| format!("read video dimensions failed: {error}"))?;
        match config {
            CodecConfig::H264(_) | CodecConfig::H265(_) => {}
            _ => return Err("unsupported codec configuration for MP4".to_owned()),
        }
        Ok(Self {
            codec: frame.codec,
            width: stream.visible_dimensions.width,
            height: stream.visible_dimensions.height,
        })
    }

    pub(crate) fn muxer<W: Write>(
        &self,
        writer: W,
        audio: Option<AudioTrackConfig>,
    ) -> Result<Muxer<W>, String> {
        let codec = match self.codec {
            Codec::H264 => VideoCodec::H264,
            Codec::H265 => VideoCodec::H265,
        };
        let mut builder = MuxerBuilder::new(writer).video(codec, self.width, self.height, 30.0);
        if let Some(audio) = audio {
            builder = builder.audio(
                AudioCodec::Opus,
                audio.sample_rate,
                u16::from(audio.channels),
            );
        }
        builder
            .build()
            .map_err(|error| format!("create MP4 muxer failed: {error}"))
    }
}

/// Opus track configuration selected from the first enabled audio route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AudioTrackConfig {
    pub(crate) sample_rate: u32,
    pub(crate) channels: u8,
}

/// An encoded video access unit retained until it is muxed.
#[derive(Debug)]
pub(crate) struct RecordedAccessUnit {
    pub(crate) timestamp: u32,
    pub(crate) is_keyframe: bool,
    pub(crate) data: Vec<u8>,
}

impl From<&DepacketizedFrame> for RecordedAccessUnit {
    fn from(frame: &DepacketizedFrame) -> Self {
        Self {
            timestamp: frame.timestamp,
            is_keyframe: frame.is_keyframe,
            data: frame.data.clone(),
        }
    }
}

/// One raw Opus RTP payload retained for the MP4 audio track.
#[derive(Debug)]
pub(crate) struct RecordedAudioPacket {
    pub(crate) timestamp: u32,
    pub(crate) data: Vec<u8>,
}

/// Return a sane delta between two RTP timestamps.
pub(crate) fn timestamp_delta(current: u32, next: u32, fallback: u32, max_gap: u32) -> u32 {
    let ticks = next.wrapping_sub(current);
    if ticks == 0 || ticks > max_gap {
        fallback.max(1)
    } else {
        ticks
    }
}

/// Return a sane delta between two 90 kHz video RTP timestamps.
pub(crate) fn frame_delta_ticks(current: u32, next: u32, fallback: u32) -> u32 {
    timestamp_delta(current, next, fallback, MAX_REASONABLE_FRAME_GAP_TICKS)
}

/// Mux encoded video access units and optional raw Opus packets into MP4.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn mux_mp4<W: Write>(
    writer: W,
    config: &Mp4TrackConfig,
    frames: &[RecordedAccessUnit],
    audio_config: Option<AudioTrackConfig>,
    audio_packets: &[RecordedAudioPacket],
) -> Result<(), String> {
    if frames.is_empty() {
        return Err("recording contains no video frames".to_owned());
    }
    let mut muxer = config.muxer(writer, audio_config)?;
    let mut video_pts = 0u64;
    let mut video_delta = DEFAULT_FRAME_DURATION_TICKS;
    for (index, frame) in frames.iter().enumerate() {
        muxer
            .write_video(
                video_pts as f64 / RTP_VIDEO_CLOCK_HZ as f64,
                &frame.data,
                frame.is_keyframe,
            )
            .map_err(|error| format!("mux video frame failed: {error}"))?;
        if let Some(next) = frames.get(index + 1) {
            video_delta = frame_delta_ticks(frame.timestamp, next.timestamp, video_delta);
            video_pts = video_pts.saturating_add(u64::from(video_delta));
        }
    }

    if let Some(audio) = audio_config {
        let fallback = (audio.sample_rate / 50).max(1);
        let max_gap = audio.sample_rate.saturating_mul(2);
        let mut audio_pts = 0u64;
        let mut audio_delta = fallback;
        for (index, packet) in audio_packets.iter().enumerate() {
            muxer
                .write_audio(audio_pts as f64 / audio.sample_rate as f64, &packet.data)
                .map_err(|error| format!("mux Opus packet failed: {error}"))?;
            if let Some(next) = audio_packets.get(index + 1) {
                audio_delta =
                    timestamp_delta(packet.timestamp, next.timestamp, audio_delta, max_gap);
                audio_pts = audio_pts.saturating_add(u64::from(audio_delta));
            }
        }
    }
    muxer
        .finish()
        .map_err(|error| format!("finalize MP4 failed: {error}"))
}

#[cfg(test)]
mod tests {
    use openipc_core::{RtpDepacketizer, RtpHeader};

    use super::{
        frame_delta_ticks, mux_mp4, AudioTrackConfig, Mp4TrackConfig, RecordedAccessUnit,
        RecordedAudioPacket,
    };

    #[test]
    fn preserves_video_clock_ticks_and_wraparound() {
        assert_eq!(frame_delta_ticks(1_000, 4_000, 2_970), 3_000);
        assert_eq!(frame_delta_ticks(u32::MAX - 1_499, 1_500, 2_970), 3_000);
    }

    #[test]
    fn rejects_duplicate_and_implausible_timestamp_gaps() {
        assert_eq!(frame_delta_ticks(10, 10, 1_530), 1_530);
        assert_eq!(frame_delta_ticks(10, 200_000, 1_530), 1_530);
    }

    #[test]
    fn muxes_h264_and_opus_tracks() {
        let mut source = crate::runtime::codec_mock::MockAvStream::new().unwrap();
        let mut depacketizer = RtpDepacketizer::new();
        let mut frames = Vec::new();
        let mut audio = Vec::new();
        while frames.len() < 4 || audio.len() < 4 {
            for packet in source.next_event().packets {
                let header = RtpHeader::parse(&packet).unwrap();
                if header.payload_type == openipc_core::rtp::RTP_PAYLOAD_TYPE_OPUS {
                    audio.push(RecordedAudioPacket {
                        timestamp: header.timestamp,
                        data: header.payload(&packet).to_vec(),
                    });
                } else if let Some(frame) = depacketizer.push(&packet).unwrap() {
                    frames.push(frame);
                }
            }
        }
        assert!(frames[0].is_keyframe);
        let config = Mp4TrackConfig::from_keyframe(&frames[0]).unwrap();
        let recorded: Vec<_> = frames.iter().map(RecordedAccessUnit::from).collect();
        let mut output = Vec::new();
        mux_mp4(
            &mut output,
            &config,
            &recorded,
            Some(AudioTrackConfig {
                sample_rate: 48_000,
                channels: 1,
            }),
            &audio,
        )
        .unwrap();

        assert_eq!(&output[4..8], b"ftyp");
        assert!(output.windows(4).any(|bytes| bytes == b"avc1"));
        assert!(output.windows(4).any(|bytes| bytes == b"Opus"));
        assert!(output.windows(4).any(|bytes| bytes == b"dOps"));
        assert!(output.windows(4).any(|bytes| bytes == b"mdat"));
    }
}
