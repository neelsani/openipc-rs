use bytes::Bytes;

use crate::VideoError;

use super::nal_type;

/// H.264 parameter sets required to configure a decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H264Config {
    /// Sequence parameter set without an Annex-B start code.
    pub sps: Bytes,
    /// Picture parameter set without an Annex-B start code.
    pub pps: Bytes,
}

impl H264Config {
    /// Validate and construct an H.264 decoder configuration.
    pub fn new(sps: impl AsRef<[u8]>, pps: impl AsRef<[u8]>) -> Result<Self, VideoError> {
        let sps = Bytes::copy_from_slice(sps.as_ref());
        let pps = Bytes::copy_from_slice(pps.as_ref());
        if nal_type(&sps) != Some(7) {
            return Err(VideoError::InvalidAnnexB(
                "H.264 SPS has the wrong NAL type",
            ));
        }
        if nal_type(&pps) != Some(8) {
            return Err(VideoError::InvalidAnnexB(
                "H.264 PPS has the wrong NAL type",
            ));
        }
        Ok(Self { sps, pps })
    }
}
