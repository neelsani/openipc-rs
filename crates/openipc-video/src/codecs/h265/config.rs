use bytes::Bytes;

use crate::VideoError;

use super::nal_type;

/// H.265 parameter sets required to configure a decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H265Config {
    /// Video parameter set without an Annex-B start code.
    pub vps: Bytes,
    /// Sequence parameter set without an Annex-B start code.
    pub sps: Bytes,
    /// Picture parameter set without an Annex-B start code.
    pub pps: Bytes,
}

impl H265Config {
    /// Validate and construct an H.265 decoder configuration.
    pub fn new(
        vps: impl AsRef<[u8]>,
        sps: impl AsRef<[u8]>,
        pps: impl AsRef<[u8]>,
    ) -> Result<Self, VideoError> {
        let vps = Bytes::copy_from_slice(vps.as_ref());
        let sps = Bytes::copy_from_slice(sps.as_ref());
        let pps = Bytes::copy_from_slice(pps.as_ref());
        if nal_type(&vps) != Some(32) {
            return Err(VideoError::InvalidAnnexB(
                "H.265 VPS has the wrong NAL type",
            ));
        }
        if nal_type(&sps) != Some(33) {
            return Err(VideoError::InvalidAnnexB(
                "H.265 SPS has the wrong NAL type",
            ));
        }
        if nal_type(&pps) != Some(34) {
            return Err(VideoError::InvalidAnnexB(
                "H.265 PPS has the wrong NAL type",
            ));
        }
        Ok(Self { vps, sps, pps })
    }
}
