use bytes::Bytes;

use super::{DecodedSurface, VideoCodec};

/// Rational media timestamp.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct VideoTimestamp {
    /// Timestamp numerator.
    pub value: i64,
    /// Ticks per second.
    pub timescale: i32,
}

impl VideoTimestamp {
    /// RTP video clock frequency used by OpenIPC streams.
    pub const RTP_VIDEO_TIMESCALE: i32 = 90_000;

    /// Construct a timestamp, rejecting a non-positive timescale.
    pub const fn new(value: i64, timescale: i32) -> Option<Self> {
        if timescale > 0 {
            Some(Self { value, timescale })
        } else {
            None
        }
    }

    /// Construct a timestamp from a 90 kHz RTP video timestamp.
    pub const fn from_rtp(value: u32) -> Self {
        Self {
            value: value as i64,
            timescale: Self::RTP_VIDEO_TIMESCALE,
        }
    }
}

/// Complete encoded Annex-B access unit submitted to a decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAccessUnit {
    /// Encoded codec family.
    pub codec: VideoCodec,
    /// Annex-B NAL units including start codes.
    pub data: Bytes,
    /// Presentation timestamp.
    pub timestamp: VideoTimestamp,
    /// Whether this access unit provides a random-access entry point.
    pub keyframe: bool,
    /// Source packet sequence number when available.
    pub sequence_number: Option<u16>,
}

impl EncodedAccessUnit {
    /// Create an encoded access unit.
    pub fn new(
        codec: VideoCodec,
        data: impl Into<Bytes>,
        timestamp: VideoTimestamp,
        keyframe: bool,
    ) -> Self {
        Self {
            codec,
            data: data.into(),
            timestamp,
            keyframe,
            sequence_number: None,
        }
    }
}

impl From<openipc_core::DepacketizedFrame> for EncodedAccessUnit {
    fn from(value: openipc_core::DepacketizedFrame) -> Self {
        Self {
            codec: value.codec.into(),
            data: Bytes::from(value.data),
            timestamp: VideoTimestamp::from_rtp(value.timestamp),
            keyframe: value.is_keyframe,
            sequence_number: Some(value.sequence_number),
        }
    }
}

/// Decoded frame and its presentation metadata.
#[derive(Debug)]
pub struct DecodedFrame<S> {
    /// Native decoded surface.
    pub surface: S,
    /// Presentation timestamp carried by the source access unit.
    pub timestamp: VideoTimestamp,
    /// Optional frame duration.
    pub duration: Option<VideoTimestamp>,
}

impl<S: DecodedSurface> DecodedFrame<S> {
    /// Visible dimensions reported by the decoded surface.
    pub fn dimensions(&self) -> super::FrameDimensions {
        self.surface.dimensions()
    }
}
