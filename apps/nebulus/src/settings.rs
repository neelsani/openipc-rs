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
}

impl DiagnosticVerbosity {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
        }
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
    pub(crate) audio_volume: u8,
    pub(crate) payload_routes: Vec<PayloadRouteSettings>,
    pub(crate) transfer_size: usize,
    pub(crate) diagnostic_verbosity: DiagnosticVerbosity,
    pub(crate) vpn_enabled: bool,
    pub(crate) key_bytes: Vec<u8>,
}

impl Settings {
    pub(crate) fn video_channel(&self) -> ChannelId {
        ChannelId::from_link_port(self.link_id, RadioPort::Video)
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
            audio_volume: 80,
            payload_routes: default_routes(),
            transfer_size: openipc_core::realtek::DEFAULT_RX_TRANSFER_SIZE,
            diagnostic_verbosity: DiagnosticVerbosity::Normal,
            vpn_enabled: false,
            key_bytes: DEFAULT_KEY_BYTES.to_vec(),
        }
    }
}
