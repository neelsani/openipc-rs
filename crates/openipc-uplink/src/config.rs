use core::fmt;

use serde::Deserialize;

/// Raw configuration files fetched from an existing OpenIPC VTX.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigBundle {
    /// Majestic camera and encoder configuration.
    pub majestic_yaml: Vec<u8>,
    /// WFB radio, FEC, and telemetry configuration.
    pub wfb_yaml: Vec<u8>,
    /// Adaptive-link shell configuration, when installed.
    pub adaptive_link: Vec<u8>,
    /// Adaptive-link profile table, when installed.
    pub tx_profiles: Vec<u8>,
    /// Whether `alink_drone` is configured to start with the VTX.
    pub adaptive_link_enabled: Option<bool>,
    /// Whether WFB forwards telemetry to the ground-station tunnel address.
    pub telemetry_ground_station_rendering: Option<bool>,
}

impl ConfigBundle {
    /// Parse known settings while ignoring keys introduced by newer firmware.
    pub fn parse_settings(&self) -> Result<VtxConfigSnapshot, ConfigParseError> {
        let wfb: WfbDocument = serde_yaml::from_slice(&self.wfb_yaml)
            .map_err(|error| ConfigParseError::Wfb(error.to_string()))?;
        let majestic: MajesticDocument = serde_yaml::from_slice(&self.majestic_yaml)
            .map_err(|error| ConfigParseError::Majestic(error.to_string()))?;
        Ok(VtxConfigSnapshot {
            tx_power: wfb.wireless.txpower,
            channel: wfb.wireless.channel,
            channel_width: wfb.wireless.width,
            multi_link: wfb.wireless.mlink,
            mcs_index: wfb.broadcast.mcs_index,
            fec_k: wfb.broadcast.fec_k,
            fec_n: wfb.broadcast.fec_n,
            stbc: wfb.broadcast.stbc.map(BoolLike::value),
            ldpc: wfb.broadcast.ldpc.map(BoolLike::value),
            telemetry_serial: wfb.telemetry.serial,
            telemetry_router: wfb.telemetry.router,
            telemetry_osd_fps: wfb.telemetry.osd_fps,
            telemetry_ground_station_rendering: self.telemetry_ground_station_rendering,
            mirror: majestic.image.mirror,
            flip: majestic.image.flip,
            contrast: majestic.image.contrast,
            hue: majestic.image.hue,
            saturation: majestic.image.saturation,
            luminance: majestic.image.luminance,
            resolution: majestic.video0.size,
            fps: majestic.video0.fps,
            bitrate_kbps: majestic.video0.bitrate,
            codec: majestic.video0.codec,
            gop_size: majestic.video0.gop_size,
            rate_control: majestic.video0.rate_control,
            recording_enabled: majestic.records.enabled,
            recording_split_seconds: majestic.records.split,
            recording_max_usage: majestic.records.max_usage,
            exposure: majestic.isp.exposure,
            anti_flicker: majestic.isp.anti_flicker,
            sensor_config: majestic
                .isp
                .sensor_config
                .and_then(|path| sensor_name(&path)),
            fpv_enabled: majestic.fpv.enabled,
            noise_level: majestic.fpv.noise_level,
            adaptive_link_enabled: self.adaptive_link_enabled,
        })
    }
}

/// Known values parsed from current OpenIPC WFB and Majestic YAML files.
///
/// Every field is optional so old, custom, and newer firmware files can be
/// loaded without inventing values for keys they do not contain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VtxConfigSnapshot {
    pub tx_power: Option<u8>,
    pub channel: Option<u16>,
    pub channel_width: Option<u16>,
    pub multi_link: Option<u16>,
    pub mcs_index: Option<u8>,
    pub fec_k: Option<u16>,
    pub fec_n: Option<u16>,
    pub stbc: Option<bool>,
    pub ldpc: Option<bool>,
    pub telemetry_serial: Option<String>,
    pub telemetry_router: Option<String>,
    pub telemetry_osd_fps: Option<u16>,
    pub telemetry_ground_station_rendering: Option<bool>,
    pub mirror: Option<bool>,
    pub flip: Option<bool>,
    pub contrast: Option<i16>,
    pub hue: Option<i16>,
    pub saturation: Option<i16>,
    pub luminance: Option<i16>,
    pub resolution: Option<String>,
    pub fps: Option<u16>,
    pub bitrate_kbps: Option<u32>,
    pub codec: Option<String>,
    pub gop_size: Option<u16>,
    pub rate_control: Option<String>,
    pub recording_enabled: Option<bool>,
    pub recording_split_seconds: Option<u32>,
    pub recording_max_usage: Option<u8>,
    pub exposure: Option<u32>,
    pub anti_flicker: Option<String>,
    pub sensor_config: Option<String>,
    pub fpv_enabled: Option<bool>,
    pub noise_level: Option<u8>,
    pub adaptive_link_enabled: Option<bool>,
}

/// A known configuration file could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigParseError {
    Wfb(String),
    Majestic(String),
}

impl fmt::Display for ConfigParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wfb(error) => write!(formatter, "invalid /etc/wfb.yaml: {error}"),
            Self::Majestic(error) => write!(formatter, "invalid /etc/majestic.yaml: {error}"),
        }
    }
}

impl std::error::Error for ConfigParseError {}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WfbDocument {
    wireless: WfbWireless,
    broadcast: WfbBroadcast,
    telemetry: WfbTelemetry,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WfbWireless {
    txpower: Option<u8>,
    channel: Option<u16>,
    width: Option<u16>,
    mlink: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WfbBroadcast {
    mcs_index: Option<u8>,
    fec_k: Option<u16>,
    fec_n: Option<u16>,
    stbc: Option<BoolLike>,
    ldpc: Option<BoolLike>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BoolLike {
    Bool(bool),
    Number(u8),
}

impl BoolLike {
    fn value(self) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::Number(value) => value != 0,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WfbTelemetry {
    serial: Option<String>,
    router: Option<String>,
    osd_fps: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MajesticDocument {
    image: MajesticImage,
    video0: MajesticVideo,
    records: MajesticRecords,
    isp: MajesticIsp,
    fpv: MajesticFpv,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MajesticImage {
    mirror: Option<bool>,
    flip: Option<bool>,
    contrast: Option<i16>,
    hue: Option<i16>,
    saturation: Option<i16>,
    luminance: Option<i16>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MajesticVideo {
    size: Option<String>,
    fps: Option<u16>,
    bitrate: Option<u32>,
    codec: Option<String>,
    gop_size: Option<u16>,
    #[serde(rename = "rcMode")]
    rate_control: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MajesticRecords {
    #[serde(alias = "enable")]
    enabled: Option<bool>,
    split: Option<u32>,
    max_usage: Option<u8>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MajesticIsp {
    exposure: Option<u32>,
    anti_flicker: Option<String>,
    sensor_config: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MajesticFpv {
    enabled: Option<bool>,
    noise_level: Option<u8>,
}

fn sensor_name(path: &str) -> Option<String> {
    let filename = path.rsplit('/').next()?;
    let name = filename.strip_suffix(".bin").unwrap_or(filename);
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::ConfigBundle;

    #[test]
    fn parses_current_openipc_documents_and_ignores_unknown_keys() {
        let bundle = ConfigBundle {
            wfb_yaml: br#"
wireless:
  txpower: 20
  channel: 161
  width: 20
  mlink: 1500
broadcast:
  mcs_index: 2
  fec_k: 8
  fec_n: 12
  stbc: 1
  ldpc: 0
  future_key: true
telemetry:
  router: mavfwd
  serial: ttyS2
  osd_fps: 20
"#
            .to_vec(),
            majestic_yaml: br#"
image:
  mirror: true
video0:
  size: 1920x1080
  fps: 60
  bitrate: 8192
  codec: h265
  gopSize: 1
  rcMode: cbr
records:
  enabled: false
  maxUsage: 80
isp:
  exposure: 5
  antiFlicker: disabled
  sensorConfig: /etc/sensors/imx415_fpv.bin
fpv:
  enabled: true
  noiseLevel: 1
"#
            .to_vec(),
            ..ConfigBundle::default()
        };
        let parsed = bundle.parse_settings().unwrap();
        assert_eq!(parsed.channel, Some(161));
        assert_eq!(parsed.stbc, Some(true));
        assert_eq!(parsed.codec.as_deref(), Some("h265"));
        assert_eq!(parsed.sensor_config.as_deref(), Some("imx415_fpv"));
        assert_eq!(parsed.noise_level, Some(1));
    }
}
