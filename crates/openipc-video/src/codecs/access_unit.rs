use bytes::Bytes;

use crate::{CodecConfig, VideoCodec, VideoError};

use super::{annex_b, h264, h265, H264Config, H265Config};

/// Result of inspecting an access unit for decoder parameter sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUpdate {
    /// Required parameter sets have not all been observed.
    Incomplete,
    /// Complete configuration is unchanged.
    Unchanged,
    /// A complete new configuration was observed.
    Changed(CodecConfig),
}

/// Tracks H.264 and H.265 parameter sets across access units.
#[derive(Debug, Clone, Default)]
pub struct CodecConfigTracker {
    h264_sps: Option<Bytes>,
    h264_pps: Option<Bytes>,
    h265_vps: Option<Bytes>,
    h265_sps: Option<Bytes>,
    h265_pps: Option<Bytes>,
    current: Option<CodecConfig>,
}

impl CodecConfigTracker {
    /// Inspect one access unit for both parameter-set changes and keyframes.
    ///
    /// Decoder backends should prefer this method over calling [`Self::observe`]
    /// and [`Self::is_keyframe`] separately because it scans Annex-B only once.
    pub fn inspect(
        &mut self,
        codec: VideoCodec,
        data: &[u8],
    ) -> Result<(ConfigUpdate, bool), VideoError> {
        let mut parameter_sets_changed = false;
        let mut keyframe = false;
        for unit in annex_b::nal_units_iter(data)? {
            match codec {
                VideoCodec::H264 => {
                    keyframe |= h264::is_keyframe(unit.data);
                    match h264::nal_type(unit.data) {
                        Some(7) if self.h264_sps.as_deref() != Some(unit.data) => {
                            self.h264_sps = Some(Bytes::copy_from_slice(unit.data));
                            parameter_sets_changed = true;
                        }
                        Some(8) if self.h264_pps.as_deref() != Some(unit.data) => {
                            self.h264_pps = Some(Bytes::copy_from_slice(unit.data));
                            parameter_sets_changed = true;
                        }
                        _ => {}
                    }
                }
                VideoCodec::H265 => {
                    keyframe |= h265::is_keyframe(unit.data);
                    match h265::nal_type(unit.data) {
                        Some(32) if self.h265_vps.as_deref() != Some(unit.data) => {
                            self.h265_vps = Some(Bytes::copy_from_slice(unit.data));
                            parameter_sets_changed = true;
                        }
                        Some(33) if self.h265_sps.as_deref() != Some(unit.data) => {
                            self.h265_sps = Some(Bytes::copy_from_slice(unit.data));
                            parameter_sets_changed = true;
                        }
                        Some(34) if self.h265_pps.as_deref() != Some(unit.data) => {
                            self.h265_pps = Some(Bytes::copy_from_slice(unit.data));
                            parameter_sets_changed = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        if !parameter_sets_changed
            && self
                .current
                .as_ref()
                .is_some_and(|config| config.codec() == codec)
        {
            return Ok((ConfigUpdate::Unchanged, keyframe));
        }

        let Some(config) = self.complete_config(codec)? else {
            return Ok((ConfigUpdate::Incomplete, keyframe));
        };
        let update = if self.current.as_ref() == Some(&config) {
            ConfigUpdate::Unchanged
        } else {
            log::debug!(
                target: "openipc_video::codec",
                "complete codec configuration observed codec={codec}"
            );
            self.current = Some(config.clone());
            ConfigUpdate::Changed(config)
        };
        Ok((update, keyframe))
    }

    /// Inspect an Annex-B access unit and update cached parameter sets.
    pub fn observe(&mut self, codec: VideoCodec, data: &[u8]) -> Result<ConfigUpdate, VideoError> {
        self.inspect(codec, data).map(|(update, _)| update)
    }

    /// Return the latest complete configuration for `codec`.
    pub fn config(&self, codec: VideoCodec) -> Option<&CodecConfig> {
        self.current
            .as_ref()
            .filter(|config| config.codec() == codec)
    }

    /// Clear all cached parameter sets and active configuration.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Return true when the access unit contains a random-access NAL unit.
    pub fn is_keyframe(codec: VideoCodec, data: &[u8]) -> Result<bool, VideoError> {
        Ok(annex_b::nal_units_iter(data)?.any(|unit| match codec {
            VideoCodec::H264 => h264::is_keyframe(unit.data),
            VideoCodec::H265 => h265::is_keyframe(unit.data),
        }))
    }

    fn complete_config(&self, codec: VideoCodec) -> Result<Option<CodecConfig>, VideoError> {
        match codec {
            VideoCodec::H264 => match (&self.h264_sps, &self.h264_pps) {
                (Some(sps), Some(pps)) => Ok(Some(CodecConfig::H264(H264Config::new(
                    sps.clone(),
                    pps.clone(),
                )?))),
                _ => Ok(None),
            },
            VideoCodec::H265 => {
                match (&self.h265_vps, &self.h265_sps, &self.h265_pps) {
                    (Some(vps), Some(sps), Some(pps)) => Ok(Some(CodecConfig::H265(
                        H265Config::new(vps.clone(), sps.clone(), pps.clone())?,
                    ))),
                    _ => Ok(None),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CodecConfigTracker, ConfigUpdate};
    use crate::{CodecConfig, VideoCodec};

    #[test]
    fn tracks_h264_parameter_sets_across_access_units() {
        let mut tracker = CodecConfigTracker::default();
        assert!(matches!(
            tracker.observe(VideoCodec::H264, &[0, 0, 1, 0x67, 1]),
            Ok(ConfigUpdate::Incomplete)
        ));
        let update = tracker
            .observe(VideoCodec::H264, &[0, 0, 1, 0x68, 2])
            .unwrap();
        assert!(matches!(
            update,
            ConfigUpdate::Changed(CodecConfig::H264(_))
        ));
    }

    #[test]
    fn tracks_h265_parameter_sets() {
        let data = [
            0,
            0,
            1,
            32 << 1,
            1,
            0,
            0,
            1,
            33 << 1,
            2,
            0,
            0,
            1,
            34 << 1,
            3,
        ];
        let mut tracker = CodecConfigTracker::default();
        let update = tracker.observe(VideoCodec::H265, &data).unwrap();
        assert!(matches!(
            update,
            ConfigUpdate::Changed(CodecConfig::H265(_))
        ));
    }

    #[test]
    fn detects_h264_and_h265_keyframes() {
        assert!(CodecConfigTracker::is_keyframe(VideoCodec::H264, &[0, 0, 1, 0x65, 1]).unwrap());
        assert!(CodecConfigTracker::is_keyframe(VideoCodec::H265, &[0, 0, 1, 19 << 1, 1]).unwrap());
    }

    #[test]
    fn inspect_reports_configuration_and_keyframe_in_one_pass() {
        let mut tracker = CodecConfigTracker::default();
        let data = [0, 0, 1, 0x67, 1, 0, 0, 1, 0x68, 2, 0, 0, 1, 0x65, 3];
        let (update, keyframe) = tracker.inspect(VideoCodec::H264, &data).unwrap();
        assert!(matches!(
            update,
            ConfigUpdate::Changed(CodecConfig::H264(_))
        ));
        assert!(keyframe);
    }
}
