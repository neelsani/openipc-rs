use objc2_core_foundation::CFRetained;
use objc2_core_media::CMFormatDescription;

use crate::{CodecConfig, VideoError};

mod h264;
mod h265;

pub(crate) fn format_description(
    config: &CodecConfig,
) -> Result<CFRetained<CMFormatDescription>, VideoError> {
    match config {
        CodecConfig::H264(config) => h264::format_description(config),
        CodecConfig::H265(config) => h265::format_description(config),
    }
}
