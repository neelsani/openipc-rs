use std::io::Cursor;

use cros_codecs::codec::{h264, h265};

use crate::{
    codecs::{H264Config, H265Config},
    FrameDimensions, VideoError,
};

/// Encoded video codec accepted by a decoder backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VideoCodec {
    /// H.264 / AVC.
    H264,
    /// H.265 / HEVC.
    H265,
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::H264 => "H.264",
            Self::H265 => "H.265",
        })
    }
}

impl From<openipc_core::Codec> for VideoCodec {
    fn from(value: openipc_core::Codec) -> Self {
        match value {
            openipc_core::Codec::H264 => Self::H264,
            openipc_core::Codec::H265 => Self::H265,
        }
    }
}

/// Complete parameter-set configuration for an encoded stream.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecConfig {
    /// H.264 SPS and PPS configuration.
    H264(H264Config),
    /// H.265 VPS, SPS and PPS configuration.
    H265(H265Config),
}

/// Dimensions, bit depth, and RFC 6381 codec identifier parsed from an SPS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecStreamInfo {
    /// Full coded picture dimensions before conformance cropping.
    pub coded_dimensions: FrameDimensions,
    /// Visible picture dimensions after SPS cropping is applied.
    pub visible_dimensions: FrameDimensions,
    /// Luma bit depth declared by the SPS.
    pub bit_depth: u8,
    /// Browser/media-container codec identifier such as `avc1.64001F`.
    pub codec_string: String,
}

impl CodecConfig {
    /// Codec described by this configuration.
    pub const fn codec(&self) -> VideoCodec {
        match self {
            Self::H264(_) => VideoCodec::H264,
            Self::H265(_) => VideoCodec::H265,
        }
    }

    /// Serialize the parameter sets as a standalone Annex-B byte stream.
    ///
    /// This is useful when a platform decoder or recorder needs in-band
    /// configuration before the first random-access picture.
    pub fn to_annex_b(&self) -> Vec<u8> {
        let units: Vec<&[u8]> = match self {
            Self::H264(config) => vec![&config.sps, &config.pps],
            Self::H265(config) => vec![&config.vps, &config.sps, &config.pps],
        };
        let capacity = units.iter().map(|unit| 4 + unit.len()).sum();
        let mut output = Vec::with_capacity(capacity);
        for unit in units {
            output.extend_from_slice(&[0, 0, 0, 1]);
            output.extend_from_slice(unit);
        }
        output
    }

    /// Parse coded/visible dimensions, bit depth, and the codec identifier.
    pub fn stream_info(&self) -> Result<CodecStreamInfo, VideoError> {
        match self {
            Self::H264(config) => h264_stream_info(config),
            Self::H265(config) => h265_stream_info(config),
        }
    }
}

fn h264_stream_info(config: &H264Config) -> Result<CodecStreamInfo, VideoError> {
    let annex_b = annex_b_unit(&config.sps);
    let mut cursor = Cursor::new(annex_b.as_slice());
    let nalu = h264::parser::Nalu::next(&mut cursor).map_err(codec_metadata_error)?;
    let mut parser = h264::parser::Parser::default();
    let sps = parser.parse_sps(&nalu).map_err(codec_metadata_error)?;
    let visible = sps.visible_rectangle();
    let profile = config.sps.get(1).copied().unwrap_or(0x42);
    let compatibility = config.sps.get(2).copied().unwrap_or(0);
    let level = config.sps.get(3).copied().unwrap_or(0x1e);
    Ok(CodecStreamInfo {
        coded_dimensions: FrameDimensions {
            width: sps.width(),
            height: sps.height(),
        },
        visible_dimensions: FrameDimensions {
            width: visible.max.x.saturating_sub(visible.min.x),
            height: visible.max.y.saturating_sub(visible.min.y),
        },
        bit_depth: 8 + sps.bit_depth_luma_minus8,
        codec_string: format!("avc1.{profile:02X}{compatibility:02X}{level:02X}"),
    })
}

fn h265_stream_info(config: &H265Config) -> Result<CodecStreamInfo, VideoError> {
    let vps = annex_b_unit(&config.vps);
    let sps_bytes = annex_b_unit(&config.sps);
    let mut parser = h265::parser::Parser::default();
    let mut vps_cursor = Cursor::new(vps.as_slice());
    let vps_nalu = h265::parser::Nalu::next(&mut vps_cursor).map_err(codec_metadata_error)?;
    parser.parse_vps(&vps_nalu).map_err(codec_metadata_error)?;
    let mut sps_cursor = Cursor::new(sps_bytes.as_slice());
    let sps_nalu = h265::parser::Nalu::next(&mut sps_cursor).map_err(codec_metadata_error)?;
    let sps = parser.parse_sps(&sps_nalu).map_err(codec_metadata_error)?;
    let visible = sps.visible_rectangle();
    let tier = if sps.profile_tier_level.general_tier_flag {
        'H'
    } else {
        'L'
    };
    let profile_space = match sps.profile_tier_level.general_profile_space {
        0 => String::new(),
        value @ 1..=3 => char::from(b'A' + value - 1).to_string(),
        _ => String::new(),
    };
    let compatibility = sps
        .profile_tier_level
        .general_profile_compatibility_flag
        .iter()
        .enumerate()
        .fold(0u32, |value, (bit, enabled)| {
            value | (u32::from(*enabled) << bit)
        });
    let constraints = h265_constraint_string(&config.sps);
    Ok(CodecStreamInfo {
        coded_dimensions: FrameDimensions {
            width: u32::from(sps.width()),
            height: u32::from(sps.height()),
        },
        visible_dimensions: FrameDimensions {
            width: visible.max.x.saturating_sub(visible.min.x),
            height: visible.max.y.saturating_sub(visible.min.y),
        },
        bit_depth: 8 + sps.bit_depth_luma_minus8,
        codec_string: format!(
            "hev1.{profile_space}{}.{compatibility:X}.{tier}{}{constraints}",
            sps.profile_tier_level.general_profile_idc,
            sps.profile_tier_level.general_level_idc as u8,
        ),
    })
}

fn h265_constraint_string(sps: &[u8]) -> String {
    // The first SPS byte after the two-byte NAL header precedes the fixed-size
    // general profile/tier/level fields. Decode emulation-prevention bytes so
    // the RFC 6381 constraint bytes can be copied exactly from the bitstream.
    let mut rbsp = Vec::with_capacity(sps.len().saturating_sub(2));
    let mut zero_count = 0;
    for &byte in sps.get(2..).unwrap_or_default() {
        if zero_count >= 2 && byte == 3 {
            zero_count = 0;
            continue;
        }
        rbsp.push(byte);
        zero_count = if byte == 0 { zero_count + 1 } else { 0 };
    }
    let Some(bytes) = rbsp.get(6..12) else {
        return String::new();
    };
    let end = bytes
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |index| index + 1);
    bytes[..end]
        .iter()
        .map(|byte| format!(".{byte:02X}"))
        .collect()
}

fn annex_b_unit(nalu: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4 + nalu.len());
    bytes.extend_from_slice(&[0, 0, 0, 1]);
    bytes.extend_from_slice(nalu);
    bytes
}

fn codec_metadata_error(message: String) -> VideoError {
    VideoError::Backend {
        backend: "codec-metadata",
        operation: "parse sequence parameter set",
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::CodecConfig;
    use crate::{
        backends::test_fixtures::{H264_KEYFRAME, H265_KEYFRAME},
        CodecConfigTracker, ConfigUpdate, H264Config, H265Config, VideoCodec,
    };

    #[test]
    fn builds_annex_b_parameter_set_headers() {
        let h264 = CodecConfig::H264(H264Config::new([0x67, 1], [0x68, 2]).unwrap());
        assert_eq!(
            h264.to_annex_b(),
            [0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2]
        );

        let h265 =
            CodecConfig::H265(H265Config::new([32 << 1, 1], [33 << 1, 2], [34 << 1, 3]).unwrap());
        assert_eq!(h265.to_annex_b().len(), 18);
    }

    #[test]
    fn parses_h264_stream_metadata() {
        let mut tracker = CodecConfigTracker::default();
        let ConfigUpdate::Changed(config) = tracker
            .observe(VideoCodec::H264, H264_KEYFRAME)
            .expect("fixture should contain valid Annex-B NAL units")
        else {
            panic!("fixture should contain a complete H.264 configuration");
        };
        let info = config.stream_info().unwrap();
        assert_eq!(info.codec_string, "avc1.64000B");
        assert_eq!(info.visible_dimensions.width, 128);
        assert_eq!(info.visible_dimensions.height, 128);
        assert_eq!(info.bit_depth, 8);
    }

    #[test]
    fn parses_h265_stream_metadata() {
        let mut tracker = CodecConfigTracker::default();
        let ConfigUpdate::Changed(config) = tracker
            .observe(VideoCodec::H265, H265_KEYFRAME)
            .expect("fixture should contain valid Annex-B NAL units")
        else {
            panic!("fixture should contain a complete H.265 configuration");
        };
        let info = config.stream_info().unwrap();
        assert_eq!(info.codec_string, "hev1.1.6.L60.B0");
        assert!(info.visible_dimensions.width > 0);
        assert!(info.visible_dimensions.height > 0);
        assert!(matches!(info.bit_depth, 8 | 10));
    }
}

/// Runtime policy shared by platform decoders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderOptions {
    /// Maximum encoded frames submitted but not yet completed.
    pub max_frames_in_flight: usize,
    /// Prefer decoder latency over frame reordering and throughput.
    pub low_latency: bool,
    /// Require hardware where the platform can enforce that distinction.
    ///
    /// WebCodecs only exposes a hardware preference, and Android API 26 does
    /// not classify the system-selected codec through the NDK API. Those
    /// backends request the hardware path but cannot prove which codec the
    /// operating system selected.
    pub require_hardware: bool,
}

impl Default for DecoderOptions {
    fn default() -> Self {
        Self {
            max_frames_in_flight: 3,
            low_latency: true,
            require_hardware: true,
        }
    }
}
