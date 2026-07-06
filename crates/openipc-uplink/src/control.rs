use std::fmt;

use crate::{ConfigBundle, SshClient, SshError};

const MAJESTIC_CONFIG: &str = "/etc/majestic.yaml";
const WFB_CONFIG: &str = "/etc/wfb.yaml";
const ADAPTIVE_LINK_CONFIG: &str = "/etc/alink.conf";
const TX_PROFILES_CONFIG: &str = "/etc/txprofiles.conf";

/// WFB settings supported by the current OpenIPC `wifibroadcast cli`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WfbSetting {
    TxPower(u8),
    Channel(u16),
    ChannelWidth(u16),
    McsIndex(u8),
    Stbc(bool),
    Ldpc(bool),
    FecK(u16),
    FecN(u16),
    MultiLink(u16),
}

/// Majestic camera and encoder settings exposed by PixelPilot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraSetting {
    Mirror(bool),
    Flip(bool),
    Contrast(i16),
    Hue(i16),
    Saturation(i16),
    Luminance(i16),
    Resolution(String),
    Fps(u16),
    BitrateKbps(u32),
    Codec(String),
    GopSize(u16),
    RateControl(String),
    RecordingEnabled(bool),
    RecordingSplitSeconds(u32),
    RecordingMaxUsage(u8),
    Exposure(u32),
    AntiFlicker(String),
    SensorConfig(String),
    FpvEnabled(bool),
    NoiseLevel(u8),
}

/// Air telemetry settings stored in `/etc/wfb.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetrySetting {
    Serial(String),
    Router(String),
    OsdFps(u16),
    GroundStationRendering(bool),
}

/// Adaptive-link settings supported by `alink_drone`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdaptiveLinkSetting {
    Enabled(bool),
    Variable { name: String, value: String },
    TxProfiles(Vec<u8>),
}

/// Invalid input or remote-control failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VtxSettingError {
    InvalidValue(&'static str),
    Ssh(SshError),
}

impl fmt::Display for VtxSettingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue(message) => formatter.write_str(message),
            Self::Ssh(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for VtxSettingError {}

impl From<SshError> for VtxSettingError {
    fn from(error: SshError) -> Self {
        Self::Ssh(error)
    }
}

/// Typed control client for current, unmodified OpenIPC firmware.
pub struct VtxController {
    ssh: SshClient,
}

impl VtxController {
    pub fn new(ssh: SshClient) -> Self {
        Self { ssh }
    }

    /// Fetch the files used by the settings UI and diagnostics bundle.
    pub async fn read_config_bundle(&self) -> Result<ConfigBundle, VtxSettingError> {
        let runtime_state = self
            .ssh
            .execute_checked(
                "if [ ! -f /etc/rc.local ]; then echo unknown; elif grep -q '^alink_drone' /etc/rc.local; then echo 1; else echo 0; fi; if [ ! -f /usr/bin/wifibroadcast ]; then echo unknown; elif grep -q '\\-z \"\\$size\"' /usr/bin/wifibroadcast; then echo 0; else echo 1; fi",
            )
            .await?;
        let runtime_state_text = runtime_state.stdout_lossy();
        let mut states = runtime_state_text.lines().map(str::trim);
        Ok(ConfigBundle {
            majestic_yaml: self.ssh.read_file(MAJESTIC_CONFIG).await?,
            wfb_yaml: self.ssh.read_file(WFB_CONFIG).await?,
            adaptive_link: self.ssh.read_optional_file(ADAPTIVE_LINK_CONFIG).await?,
            tx_profiles: self.ssh.read_optional_file(TX_PROFILES_CONFIG).await?,
            adaptive_link_enabled: states.next().and_then(parse_bool_number),
            telemetry_ground_station_rendering: states.next().and_then(parse_bool_number),
        })
    }

    /// Apply a WFB radio or FEC setting using the firmware's own CLI.
    pub async fn set_wfb(&self, setting: WfbSetting) -> Result<(), VtxSettingError> {
        self.set_wfb_batch(&[setting]).await
    }

    /// Apply several WFB values and restart WFB only once.
    pub async fn set_wfb_batch(&self, settings: &[WfbSetting]) -> Result<(), VtxSettingError> {
        let Some(command) = wfb_batch_command(settings)? else {
            return Ok(());
        };
        self.ssh.execute_checked(&command).await?;
        Ok(())
    }

    /// Apply a Majestic setting and reload the encoder process.
    pub async fn set_camera(&self, setting: CameraSetting) -> Result<(), VtxSettingError> {
        self.set_camera_batch(vec![setting]).await
    }

    /// Apply several Majestic values and reload the encoder only once.
    pub async fn set_camera_batch(
        &self,
        settings: Vec<CameraSetting>,
    ) -> Result<(), VtxSettingError> {
        let Some(command) = camera_batch_command(settings)? else {
            return Ok(());
        };
        self.ssh.execute_checked(&command).await?;
        Ok(())
    }

    /// Apply an air telemetry setting through `wifibroadcast cli`.
    pub async fn set_telemetry(&self, setting: TelemetrySetting) -> Result<(), VtxSettingError> {
        self.set_telemetry_batch(vec![setting]).await
    }

    /// Apply several telemetry values and restart WFB only once.
    pub async fn set_telemetry_batch(
        &self,
        settings: Vec<TelemetrySetting>,
    ) -> Result<(), VtxSettingError> {
        let Some(command) = telemetry_batch_command(settings)? else {
            return Ok(());
        };
        self.ssh.execute_checked(&command).await?;
        Ok(())
    }

    /// Enable, disable, or reconfigure the existing `alink_drone` service.
    pub async fn set_adaptive_link(
        &self,
        setting: AdaptiveLinkSetting,
    ) -> Result<(), VtxSettingError> {
        match setting {
            AdaptiveLinkSetting::Enabled(true) => {
                self.ssh
                    .execute_checked(
                        "sed -i '/alink_drone &/d' /etc/rc.local && sed -i -e '$i alink_drone &' /etc/rc.local && cli -s .video0.qpDelta -12 && killall -1 majestic && (nohup alink_drone >/dev/null 2>&1 &)",
                    )
                    .await?;
            }
            AdaptiveLinkSetting::Enabled(false) => {
                self.ssh
                    .execute_checked(
                        "killall -q -9 alink_drone; sed -i '/alink_drone &/d' /etc/rc.local; cli -d .video0.qpDelta && killall -1 majestic",
                    )
                    .await?;
            }
            AdaptiveLinkSetting::Variable { name, value } => {
                validate_variable_name(&name)?;
                validate_adaptive_value(&value)?;
                let assignment = format!("{name}={value}");
                self.ssh
                    .execute_checked(&format!(
                        "sed -i {} /etc/alink.conf; killall -q -9 alink_drone; alink_drone >/dev/null 2>&1 &",
                        shell_value(&format!("s|^{name}=.*|{assignment}|"))
                    ))
                    .await?;
            }
            AdaptiveLinkSetting::TxProfiles(contents) => {
                self.ssh.write_file(TX_PROFILES_CONFIG, &contents).await?;
                self.ssh
                    .execute_checked("killall -q -9 alink_drone; alink_drone >/dev/null 2>&1 &")
                    .await?;
            }
        }
        Ok(())
    }

    /// Restart the VTX.
    pub async fn reboot(&self) -> Result<(), VtxSettingError> {
        // The SSH connection can close before a status arrives, matching the
        // behavior of PixelPilot's `reboot &` action.
        let _ = self.ssh.execute("reboot >/dev/null 2>&1 &").await;
        Ok(())
    }

    /// Access lower-level SSH operations for advanced applications.
    pub fn ssh(&self) -> &SshClient {
        &self.ssh
    }
}

fn wfb_batch_command(settings: &[WfbSetting]) -> Result<Option<String>, VtxSettingError> {
    if settings.is_empty() {
        return Ok(None);
    }
    let mut command = String::new();
    for setting in settings {
        let (path, value) = wfb_value(*setting)?;
        if !command.is_empty() {
            command.push_str(" && ");
        }
        command.push_str(&format!(
            "wifibroadcast cli -s {path} {}",
            shell_value(&value)
        ));
    }
    command.push_str(
        " && sh -c '(wifibroadcast stop; wifibroadcast stop; sleep 1; wifibroadcast start) >/dev/null 2>&1 &'",
    );
    Ok(Some(command))
}

fn wfb_value(setting: WfbSetting) -> Result<(&'static str, String), VtxSettingError> {
    let result = match setting {
        WfbSetting::TxPower(value) if (1..=58).contains(&value) => {
            (".wireless.txpower", value.to_string())
        }
        WfbSetting::Channel(value) if (1..=196).contains(&value) => {
            (".wireless.channel", value.to_string())
        }
        WfbSetting::ChannelWidth(value @ (10 | 20 | 40)) => (".wireless.width", value.to_string()),
        WfbSetting::McsIndex(value) if value <= 10 => (".broadcast.mcs_index", value.to_string()),
        WfbSetting::Stbc(value) => (".broadcast.stbc", bool_number(value)),
        WfbSetting::Ldpc(value) => (".broadcast.ldpc", bool_number(value)),
        WfbSetting::FecK(value) if value <= 15 => (".broadcast.fec_k", value.to_string()),
        WfbSetting::FecN(value) if value <= 15 => (".broadcast.fec_n", value.to_string()),
        WfbSetting::MultiLink(value) if (1_500..=4_000).contains(&value) => {
            (".wireless.mlink", value.to_string())
        }
        _ => {
            return Err(VtxSettingError::InvalidValue(
                "WFB setting is outside its supported range",
            ))
        }
    };
    Ok(result)
}

fn camera_value(setting: CameraSetting) -> Result<(&'static str, String), VtxSettingError> {
    let result = match setting {
        CameraSetting::Mirror(value) => (".image.mirror", bool_word(value)),
        CameraSetting::Flip(value) => (".image.flip", bool_word(value)),
        CameraSetting::Contrast(value) if (0..=100).contains(&value) => {
            (".image.contrast", value.to_string())
        }
        CameraSetting::Hue(value) if (0..=100).contains(&value) => {
            (".image.hue", value.to_string())
        }
        CameraSetting::Saturation(value) if (0..=100).contains(&value) => {
            (".image.saturation", value.to_string())
        }
        CameraSetting::Luminance(value) if (0..=100).contains(&value) => {
            (".image.luminance", value.to_string())
        }
        CameraSetting::Resolution(value) => (".video0.size", validate_token(value)?),
        CameraSetting::Fps(value) if (1..=240).contains(&value) => {
            (".video0.fps", value.to_string())
        }
        CameraSetting::BitrateKbps(value) if (1..=30_720).contains(&value) => {
            (".video0.bitrate", value.to_string())
        }
        CameraSetting::Codec(value) => (".video0.codec", validate_token(value)?),
        CameraSetting::GopSize(value) if value <= 10 => (".video0.gopSize", value.to_string()),
        CameraSetting::RateControl(value) => (".video0.rcMode", validate_token(value)?),
        CameraSetting::RecordingEnabled(value) => (".records.enable", bool_word(value)),
        CameraSetting::RecordingSplitSeconds(value) => (".records.split", value.to_string()),
        CameraSetting::RecordingMaxUsage(value) if value <= 100 => {
            (".records.maxUsage", value.to_string())
        }
        CameraSetting::Exposure(value) if (5..=50).contains(&value) => {
            (".isp.exposure", value.to_string())
        }
        CameraSetting::AntiFlicker(value) if matches!(value.as_str(), "disabled" | "50" | "60") => {
            (".isp.antiFlicker", value)
        }
        CameraSetting::SensorConfig(value) => {
            let sensor = validate_token(value)?;
            (".isp.sensorConfig", format!("/etc/sensors/{sensor}.bin"))
        }
        CameraSetting::FpvEnabled(value) => (".fpv.enabled", bool_word(value)),
        CameraSetting::NoiseLevel(value) if value <= 1 => (".fpv.noiseLevel", value.to_string()),
        _ => {
            return Err(VtxSettingError::InvalidValue(
                "camera setting is outside its supported range",
            ))
        }
    };
    Ok(result)
}

fn camera_batch_command(settings: Vec<CameraSetting>) -> Result<Option<String>, VtxSettingError> {
    if settings.is_empty() {
        return Ok(None);
    }
    let mut commands = Vec::with_capacity(settings.len() + 1);
    for setting in settings {
        let (path, value) = camera_value(setting)?;
        commands.push(format!("cli -s {path} {}", shell_value(&value)));
    }
    commands.push("killall -1 majestic".to_owned());
    Ok(Some(commands.join(" && ")))
}

#[cfg(test)]
fn telemetry_command(setting: TelemetrySetting) -> Result<String, VtxSettingError> {
    telemetry_batch_command(vec![setting])?.ok_or(VtxSettingError::InvalidValue(
        "telemetry batch must contain at least one setting",
    ))
}

fn telemetry_batch_command(
    settings: Vec<TelemetrySetting>,
) -> Result<Option<String>, VtxSettingError> {
    const RESTART: &str =
        "(wifibroadcast stop; wifibroadcast stop; sleep 1; wifibroadcast start) >/dev/null 2>&1 &";
    if settings.is_empty() {
        return Ok(None);
    }
    let mut commands = Vec::with_capacity(settings.len() + 1);
    for setting in settings {
        commands.push(telemetry_operation(setting)?);
    }
    commands.push(RESTART.to_owned());
    Ok(Some(commands.join("; ")))
}

fn telemetry_operation(setting: TelemetrySetting) -> Result<String, VtxSettingError> {
    Ok(match setting {
        TelemetrySetting::Serial(value) => {
            let value = validate_token(value)?;
            let console = if value == "ttyS0" {
                "sed -i 's|^console::respawn:/sbin/getty -L console 0 vt100|#console::respawn:/sbin/getty -L console 0 vt100|' /etc/inittab; kill -HUP 1"
            } else {
                "sed -i 's|^#console::respawn:/sbin/getty -L console 0 vt100|console::respawn:/sbin/getty -L console 0 vt100|' /etc/inittab; kill -HUP 1"
            };
            format!(
                "{console}; wifibroadcast cli -s .telemetry.serial {}",
                shell_value(&value)
            )
        }
        TelemetrySetting::Router(value) => format!(
            "wifibroadcast cli -s .telemetry.router {}",
            shell_value(&validate_token(value)?)
        ),
        TelemetrySetting::OsdFps(value) if value <= 240 => {
            format!("wifibroadcast cli -s .telemetry.osd_fps {value}")
        }
        TelemetrySetting::GroundStationRendering(true) =>
            "sed -i 's|-o 127\\.0\\.0\\.1:\"$port_tx\" -z \"$size\"|-o 10.5.0.1:\"$port_tx\"|' /usr/bin/wifibroadcast".to_owned(),
        TelemetrySetting::GroundStationRendering(false) =>
            "sed -i 's|-o 10\\.5\\.0\\.1:\"$port_tx\"|-o 127.0.0.1:\"$port_tx\" -z \"$size\"|' /usr/bin/wifibroadcast".to_owned(),
        _ => {
            return Err(VtxSettingError::InvalidValue(
                "telemetry setting is outside its supported range",
            ))
        }
    })
}

fn validate_token(value: String) -> Result<String, VtxSettingError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'x'))
    {
        return Err(VtxSettingError::InvalidValue(
            "setting contains unsupported characters",
        ));
    }
    Ok(value)
}

fn validate_variable_name(name: &str) -> Result<(), VtxSettingError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(VtxSettingError::InvalidValue(
            "adaptive-link variable name is invalid",
        ));
    }
    Ok(())
}

fn validate_adaptive_value(value: &str) -> Result<(), VtxSettingError> {
    if value.is_empty()
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'.' | b',' | b':')
        })
    {
        return Err(VtxSettingError::InvalidValue(
            "adaptive-link value contains unsupported characters",
        ));
    }
    Ok(())
}

fn shell_value(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn bool_word(value: bool) -> String {
    if value { "true" } else { "false" }.to_owned()
}

fn bool_number(value: bool) -> String {
    if value { "1" } else { "0" }.to_owned()
}

fn parse_bool_number(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        camera_batch_command, camera_value, telemetry_batch_command, telemetry_command,
        wfb_batch_command, wfb_value, CameraSetting, TelemetrySetting, WfbSetting,
    };

    #[test]
    fn wfb_commands_match_pixelpilot_paths() {
        assert_eq!(
            wfb_value(WfbSetting::TxPower(20)).unwrap(),
            (".wireless.txpower", "20".to_owned())
        );
        assert_eq!(
            wfb_value(WfbSetting::FecK(8)).unwrap(),
            (".broadcast.fec_k", "8".to_owned())
        );
        assert_eq!(
            wfb_value(WfbSetting::MultiLink(2_500)).unwrap(),
            (".wireless.mlink", "2500".to_owned())
        );

        let command = wfb_batch_command(&[
            WfbSetting::Channel(161),
            WfbSetting::ChannelWidth(20),
            WfbSetting::FecK(8),
            WfbSetting::FecN(12),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(command.matches("wifibroadcast start").count(), 1);
        assert!(command.contains(".wireless.channel '161'"));
        assert!(command.contains(".broadcast.fec_n '12'"));
        assert!(wfb_value(WfbSetting::TxPower(59)).is_err());
        assert!(wfb_value(WfbSetting::FecN(16)).is_err());
    }

    #[test]
    fn majestic_commands_match_pixelpilot_paths() {
        assert_eq!(
            camera_value(CameraSetting::Resolution("1920x1080".into())).unwrap(),
            (".video0.size", "1920x1080".to_owned())
        );
        let command = camera_batch_command(vec![
            CameraSetting::Fps(60),
            CameraSetting::BitrateKbps(8_192),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(command.matches("killall -1 majestic").count(), 1);
        assert_eq!(
            camera_value(CameraSetting::SensorConfig("imx415".into())).unwrap(),
            (".isp.sensorConfig", "/etc/sensors/imx415.bin".to_owned())
        );
    }

    #[test]
    fn telemetry_commands_match_pixelpilot_paths() {
        assert_eq!(
            telemetry_command(TelemetrySetting::OsdFps(20)).unwrap(),
            "wifibroadcast cli -s .telemetry.osd_fps 20; (wifibroadcast stop; wifibroadcast stop; sleep 1; wifibroadcast start) >/dev/null 2>&1 &"
        );
        assert!(telemetry_command(TelemetrySetting::Serial("ttyS0".into()))
            .unwrap()
            .contains(".telemetry.serial 'ttyS0'"));
        assert!(
            telemetry_command(TelemetrySetting::GroundStationRendering(true))
                .unwrap()
                .contains("10.5.0.1")
        );
        let command = telemetry_batch_command(vec![
            TelemetrySetting::Router("mavfwd".into()),
            TelemetrySetting::OsdFps(20),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(command.matches("wifibroadcast start").count(), 1);
    }
}
