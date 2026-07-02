use objc2_core_foundation::CFRetained;
use objc2_core_media::CMFormatDescription;

use crate::{codecs::H264Config, VideoError};

pub(crate) fn format_description(
    config: &H264Config,
) -> Result<CFRetained<CMFormatDescription>, VideoError> {
    super::super::ffi::h264_format_description([config.sps.as_ref(), config.pps.as_ref()])
}

#[cfg(test)]
mod tests {
    use crate::codecs::H264Config;

    #[test]
    fn creates_core_media_format_description() {
        let config = H264Config::new(
            vec![0x67, 0x42, 0x00, 0x1f, 0x95, 0xa8, 0x14, 0x01, 0x6e, 0x40],
            vec![0x68, 0xce, 0x06, 0xe2],
        )
        .unwrap();
        assert!(super::format_description(&config).is_ok());
    }
}
