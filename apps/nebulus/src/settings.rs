use openipc_core::{channel::DEFAULT_LINK_ID, ChannelId, RadioPort};
use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_KEY_BYTES: &[u8; 64] = &[
    0xbb, 0xb7, 0xed, 0x6e, 0x83, 0xa4, 0x6a, 0x8a, 0x9b, 0x8a, 0x12, 0xa0, 0xf9, 0x8e, 0xce, 0x2b,
    0xdc, 0x97, 0x87, 0x05, 0xb8, 0x20, 0x47, 0x01, 0xb2, 0x08, 0x5f, 0xa2, 0x8c, 0xac, 0x7b, 0x46,
    0x0e, 0x05, 0xc4, 0x8a, 0x61, 0x95, 0xfb, 0x70, 0x92, 0x1c, 0x74, 0x7a, 0x66, 0xe8, 0x3c, 0x02,
    0xe6, 0x40, 0xbd, 0x6b, 0xbe, 0xb5, 0xb2, 0x51, 0x53, 0x7a, 0x98, 0xa2, 0x74, 0x16, 0xa2, 0x63,
];
pub(crate) const DEFAULT_CHANNEL: u8 = 161;
pub(crate) const DEFAULT_CHANNEL_OFFSET: u8 = 0;
pub(crate) const MAX_LINK_ID: u32 = 0x00ff_ffff;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CodecPreference {
    #[default]
    Auto,
    H264,
    H265,
}

impl CodecPreference {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::H264 => "H.264",
            Self::H265 => "H.265",
        }
    }

    pub(crate) const fn accepts(self, codec: openipc_core::Codec) -> bool {
        matches!(self, Self::Auto)
            || matches!(
                (self, codec),
                (Self::H264, openipc_core::Codec::H264) | (Self::H265, openipc_core::Codec::H265)
            )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum DiagnosticVerbosity {
    Low,
    #[default]
    Normal,
    High,
    VeryHigh,
}

impl DiagnosticVerbosity {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::VeryHigh => "Very verbose",
        }
    }

    pub(crate) const fn log_level(self) -> log::LevelFilter {
        match self {
            Self::Low => log::LevelFilter::Warn,
            Self::Normal => log::LevelFilter::Info,
            Self::High => log::LevelFilter::Debug,
            Self::VeryHigh => log::LevelFilter::Trace,
        }
    }
}

/// Persisted Catppuccin palette used by the Nebulus interface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum GuiTheme {
    Latte,
    Frappe,
    #[default]
    Macchiato,
    Mocha,
}

impl GuiTheme {
    pub(crate) const ALL: [Self; 4] = [Self::Latte, Self::Frappe, Self::Macchiato, Self::Mocha];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Latte => "Latte",
            Self::Frappe => "Frappé",
            Self::Macchiato => "Macchiato",
            Self::Mocha => "Mocha",
        }
    }

    pub(crate) const fn is_dark(self) -> bool {
        !matches!(self, Self::Latte)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RouteAction {
    Inspect,
    Log,
    Udp,
    Audio,
}

impl RouteAction {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Inspect => "Inspect",
            Self::Log => "Log",
            Self::Udp => "UDP forward",
            Self::Audio => "Audio",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PayloadRouteSettings {
    pub(crate) id: u64,
    pub(crate) enabled: bool,
    pub(crate) name: String,
    pub(crate) radio_port: u8,
    pub(crate) action: RouteAction,
    pub(crate) payload_type: u8,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u8,
    pub(crate) udp_host: String,
    pub(crate) udp_port: u16,
}

/// A value that can be placed on the ground-station video HUD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HudMetric {
    Resolution,
    FrameRate,
    Bitrate,
    Latency,
    Signal,
    PacketLoss,
    LinkScore,
}

impl HudMetric {
    pub(crate) const ALL: [Self; 7] = [
        Self::Resolution,
        Self::FrameRate,
        Self::Bitrate,
        Self::Latency,
        Self::Signal,
        Self::PacketLoss,
        Self::LinkScore,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Resolution => "Resolution",
            Self::FrameRate => "Frame rate",
            Self::Bitrate => "Bitrate",
            Self::Latency => "Local latency",
            Self::Signal => "RSSI",
            Self::PacketLoss => "Packet loss",
            Self::LinkScore => "Link score",
        }
    }
}

/// Position and visibility of one HUD value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HudItemSettings {
    pub(crate) metric: HudMetric,
    pub(crate) visible: bool,
    /// Horizontal item center in normalized video coordinates.
    pub(crate) x: f32,
    /// Vertical item center in normalized video coordinates.
    pub(crate) y: f32,
}

impl Default for HudItemSettings {
    fn default() -> Self {
        Self {
            metric: HudMetric::Resolution,
            visible: true,
            x: 0.08,
            y: 0.95,
        }
    }
}

/// Persisted layout and appearance of the video HUD.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HudSettings {
    pub(crate) items: Vec<HudItemSettings>,
    pub(crate) scale_percent: u16,
    pub(crate) background_opacity: u8,
}

impl HudSettings {
    pub(crate) fn reset_layout(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn normalize(&mut self) {
        for metric in HudMetric::ALL {
            if !self.items.iter().any(|item| item.metric == metric) {
                let fallback = Self::default();
                if let Some(item) = fallback
                    .items
                    .into_iter()
                    .find(|item| item.metric == metric)
                {
                    self.items.push(item);
                }
            }
        }
        let mut seen = Vec::with_capacity(HudMetric::ALL.len());
        self.items.retain(|item| {
            if seen.contains(&item.metric) {
                false
            } else {
                seen.push(item.metric);
                true
            }
        });
        for item in &mut self.items {
            item.x = item.x.clamp(0.03, 0.97);
            item.y = item.y.clamp(0.03, 0.97);
        }
        self.scale_percent = self.scale_percent.clamp(70, 160);
    }
}

impl Default for HudSettings {
    fn default() -> Self {
        Self {
            items: HudMetric::ALL
                .into_iter()
                .enumerate()
                .map(|(index, metric)| HudItemSettings {
                    metric,
                    visible: true,
                    x: 0.075 + index as f32 * 0.14,
                    y: 0.95,
                })
                .collect(),
            scale_percent: 100,
            background_opacity: 205,
        }
    }
}

/// A named receiver configuration that can be switched before starting RX.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ReceiverProfile {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) device_id: Option<String>,
    pub(crate) channel: u8,
    pub(crate) channel_width_mhz: u16,
    pub(crate) channel_offset: u8,
    pub(crate) link_id: u32,
    pub(crate) minimum_epoch: u64,
    pub(crate) codec_preference: CodecPreference,
    pub(crate) rtp_reorder: bool,
    pub(crate) adaptive_link: bool,
    pub(crate) tx_power: u8,
    pub(crate) audio_volume: u8,
    pub(crate) payload_routes: Vec<PayloadRouteSettings>,
    pub(crate) transfer_size: usize,
    pub(crate) vpn_enabled: bool,
    pub(crate) key_bytes: Vec<u8>,
}

impl ReceiverProfile {
    pub(crate) fn capture(id: u64, name: String, settings: &Settings) -> Self {
        Self {
            id,
            name,
            device_id: settings.device_id.clone(),
            channel: settings.channel,
            channel_width_mhz: settings.channel_width_mhz,
            channel_offset: settings.channel_offset,
            link_id: settings.link_id,
            minimum_epoch: settings.minimum_epoch,
            codec_preference: settings.codec_preference,
            rtp_reorder: settings.rtp_reorder,
            adaptive_link: settings.adaptive_link,
            tx_power: settings.tx_power,
            audio_volume: settings.audio_volume,
            payload_routes: settings.payload_routes.clone(),
            transfer_size: settings.transfer_size,
            vpn_enabled: settings.vpn_enabled,
            key_bytes: settings.key_bytes.clone(),
        }
    }

    pub(crate) fn apply(&self, settings: &mut Settings) {
        settings.device_id.clone_from(&self.device_id);
        settings.channel = self.channel;
        settings.channel_width_mhz = self.channel_width_mhz;
        settings.channel_offset = self.channel_offset;
        settings.link_id = self.link_id;
        settings.minimum_epoch = self.minimum_epoch;
        settings.codec_preference = self.codec_preference;
        settings.rtp_reorder = self.rtp_reorder;
        settings.adaptive_link = self.adaptive_link;
        settings.tx_power = self.tx_power;
        settings.audio_volume = self.audio_volume;
        settings.payload_routes.clone_from(&self.payload_routes);
        settings.transfer_size = self.transfer_size;
        settings.vpn_enabled = self.vpn_enabled;
        settings.key_bytes.clone_from(&self.key_bytes);
        settings.active_profile_id = Some(self.id);
    }
}

impl Default for ReceiverProfile {
    fn default() -> Self {
        Self {
            id: 1,
            name: "Default FPV".to_owned(),
            device_id: None,
            channel: DEFAULT_CHANNEL,
            channel_width_mhz: 20,
            channel_offset: DEFAULT_CHANNEL_OFFSET,
            link_id: DEFAULT_LINK_ID,
            minimum_epoch: 0,
            codec_preference: CodecPreference::Auto,
            rtp_reorder: false,
            adaptive_link: false,
            tx_power: 20,
            audio_volume: 80,
            payload_routes: default_routes(),
            transfer_size: openipc_core::realtek::DEFAULT_RX_TRANSFER_SIZE,
            vpn_enabled: false,
            key_bytes: DEFAULT_KEY_BYTES.to_vec(),
        }
    }
}

impl Default for PayloadRouteSettings {
    fn default() -> Self {
        Self {
            id: 4,
            enabled: true,
            name: "Route 4".to_owned(),
            radio_port: RadioPort::TunnelRx.as_u8(),
            action: RouteAction::Inspect,
            payload_type: openipc_core::rtp::RTP_PAYLOAD_TYPE_OPUS,
            sample_rate: 48_000,
            channels: 1,
            udp_host: "127.0.0.1".to_owned(),
            udp_port: 5_600,
        }
    }
}

fn default_routes() -> Vec<PayloadRouteSettings> {
    vec![
        PayloadRouteSettings {
            id: 2,
            name: "Telemetry".to_owned(),
            radio_port: RadioPort::TelemetryRx.as_u8(),
            action: RouteAction::Inspect,
            ..PayloadRouteSettings::default()
        },
        PayloadRouteSettings {
            id: 3,
            name: "Mixed RTP audio".to_owned(),
            radio_port: RadioPort::Video.as_u8(),
            action: RouteAction::Audio,
            ..PayloadRouteSettings::default()
        },
        PayloadRouteSettings {
            id: 4,
            enabled: false,
            name: "Data".to_owned(),
            radio_port: RadioPort::TunnelRx.as_u8(),
            action: RouteAction::Log,
            ..PayloadRouteSettings::default()
        },
    ]
}

/// User settings persisted by eframe on desktop and web.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) device_id: Option<String>,
    pub(crate) channel: u8,
    pub(crate) channel_width_mhz: u16,
    pub(crate) channel_offset: u8,
    pub(crate) link_id: u32,
    pub(crate) minimum_epoch: u64,
    pub(crate) codec_preference: CodecPreference,
    pub(crate) rtp_reorder: bool,
    pub(crate) adaptive_link: bool,
    pub(crate) tx_power: u8,
    pub(crate) show_osd: bool,
    pub(crate) show_sidebar: bool,
    pub(crate) gui_theme: GuiTheme,
    pub(crate) interface_scale_percent: u16,
    pub(crate) audio_volume: u8,
    pub(crate) payload_routes: Vec<PayloadRouteSettings>,
    pub(crate) transfer_size: usize,
    pub(crate) diagnostic_verbosity: DiagnosticVerbosity,
    pub(crate) vpn_enabled: bool,
    pub(crate) key_bytes: Vec<u8>,
    #[serde(default)]
    pub(crate) profiles: Vec<ReceiverProfile>,
    #[serde(default)]
    pub(crate) active_profile_id: Option<u64>,
    pub(crate) auto_recover: bool,
    pub(crate) hud: HudSettings,
}

impl Settings {
    pub(crate) fn video_channel(&self) -> ChannelId {
        ChannelId::from_link_port(self.link_id, RadioPort::Video)
    }

    pub(crate) fn next_profile_id(&self) -> u64 {
        self.profiles
            .iter()
            .map(|profile| profile.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub(crate) fn normalize(&mut self) {
        self.hud.normalize();
        if self.profiles.is_empty() {
            let profile = ReceiverProfile::capture(1, "Default FPV".to_owned(), self);
            self.profiles.push(profile);
        }
        if self
            .active_profile_id
            .is_some_and(|id| !self.profiles.iter().any(|profile| profile.id == id))
        {
            self.active_profile_id = None;
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_id: None,
            channel: DEFAULT_CHANNEL,
            channel_width_mhz: 20,
            channel_offset: DEFAULT_CHANNEL_OFFSET,
            link_id: DEFAULT_LINK_ID,
            minimum_epoch: 0,
            codec_preference: CodecPreference::Auto,
            rtp_reorder: false,
            adaptive_link: false,
            tx_power: 20,
            show_osd: true,
            show_sidebar: true,
            gui_theme: GuiTheme::Macchiato,
            interface_scale_percent: 100,
            audio_volume: 80,
            payload_routes: default_routes(),
            transfer_size: openipc_core::realtek::DEFAULT_RX_TRANSFER_SIZE,
            diagnostic_verbosity: DiagnosticVerbosity::Normal,
            vpn_enabled: false,
            key_bytes: DEFAULT_KEY_BYTES.to_vec(),
            profiles: vec![ReceiverProfile::default()],
            active_profile_id: Some(1),
            auto_recover: true,
            hud: HudSettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GuiTheme, HudMetric, ReceiverProfile, Settings};

    #[test]
    fn profile_restores_receiver_fields_without_replacing_gui_preferences() {
        let mut settings = Settings {
            channel: 149,
            link_id: 0x12_34_56,
            gui_theme: GuiTheme::Latte,
            ..Settings::default()
        };
        let profile = ReceiverProfile::capture(42, "Race quad".to_owned(), &settings);
        settings.channel = 36;
        settings.link_id = 1;
        settings.gui_theme = GuiTheme::Mocha;

        profile.apply(&mut settings);

        assert_eq!(settings.channel, 149);
        assert_eq!(settings.link_id, 0x12_34_56);
        assert_eq!(settings.gui_theme, GuiTheme::Mocha);
        assert_eq!(settings.active_profile_id, Some(42));
    }

    #[test]
    fn missing_persisted_fields_use_current_defaults() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"channel":149}"#).expect("settings deserialize");
        settings.normalize();
        assert!(settings.auto_recover);
        assert!(!settings.profiles.is_empty());
        assert_eq!(settings.profiles[0].channel, 149);
        assert_eq!(settings.hud.items.len(), HudMetric::ALL.len());
    }

    #[test]
    fn hud_normalization_restores_missing_items_and_clamps_positions() {
        let mut settings = Settings::default();
        settings.hud.items.truncate(1);
        settings.hud.items[0].x = -4.0;
        settings.hud.items[0].y = 8.0;
        settings.normalize();

        assert_eq!(settings.hud.items.len(), HudMetric::ALL.len());
        assert_eq!(settings.hud.items[0].x, 0.03);
        assert_eq!(settings.hud.items[0].y, 0.97);
    }
}
