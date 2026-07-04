use openipc_core::{channel::DEFAULT_LINK_ID, ChannelId, RadioPort};
use serde::{Deserialize, Serialize};

use crate::{
    presets::{PresetPack, PresetSource, MAX_INSTALLED_PRESETS},
    remote_presets::DEFAULT_REGISTRY_URL,
    telemetry::{TelemetryProtocol, TelemetrySettings},
};

pub(crate) const DEFAULT_KEY_BYTES: &[u8; 64] = &[
    0xbb, 0xb7, 0xed, 0x6e, 0x83, 0xa4, 0x6a, 0x8a, 0x9b, 0x8a, 0x12, 0xa0, 0xf9, 0x8e, 0xce, 0x2b,
    0xdc, 0x97, 0x87, 0x05, 0xb8, 0x20, 0x47, 0x01, 0xb2, 0x08, 0x5f, 0xa2, 0x8c, 0xac, 0x7b, 0x46,
    0x0e, 0x05, 0xc4, 0x8a, 0x61, 0x95, 0xfb, 0x70, 0x92, 0x1c, 0x74, 0x7a, 0x66, 0xe8, 0x3c, 0x02,
    0xe6, 0x40, 0xbd, 0x6b, 0xbe, 0xb5, 0xb2, 0x51, 0x53, 0x7a, 0x98, 0xa2, 0x74, 0x16, 0xa2, 0x63,
];
pub(crate) const DEFAULT_CHANNEL: u8 = 161;
pub(crate) const DEFAULT_CHANNEL_OFFSET: u8 = 0;
pub(crate) const DEFAULT_UDP_RTP_PORT: u16 = 5_600;
pub(crate) const MAX_LINK_ID: u32 = 0x00ff_ffff;

/// Transport that supplies the encoded video payload stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReceiverSource {
    /// Receive encrypted WFB frames through a supported Realtek USB adapter.
    #[default]
    Usb,
    /// Receive already-recovered RTP packets from a native UDP socket.
    UdpRtp,
}

impl ReceiverSource {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Usb => "Realtek USB",
            Self::UdpRtp => "UDP RTP",
        }
    }
}

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

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
    Telemetry,
}

impl RouteAction {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Inspect => "Inspect",
            Self::Log => "Log",
            Self::Udp => "UDP forward",
            Self::Audio => "Audio",
            Self::Telemetry => "Telemetry to OSD",
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
    pub(crate) telemetry_protocol: TelemetryProtocol,
    pub(crate) payload_type: u8,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u8,
    pub(crate) udp_host: String,
    pub(crate) udp_port: u16,
}

/// A value that can be placed on the ground-station video OSD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HudMetric {
    Resolution,
    FrameRate,
    Bitrate,
    Latency,
    Signal,
    PacketLoss,
    LinkScore,
    LinkHealth,
    Armed,
    FlightMode,
    BatteryVoltage,
    BatteryCurrent,
    BatteryRemaining,
    GpsStatus,
    Altitude,
    GroundSpeed,
    VerticalSpeed,
    Heading,
    HomeDistance,
    Throttle,
    Attitude,
    StatusText,
    Coordinates,
    RcLinkQuality,
    // Retained only so layouts written by the first customizable-HUD build can
    // be migrated into presentation options on their corresponding metric.
    #[doc(hidden)]
    SignalBars,
    #[doc(hidden)]
    SignalTrend,
    #[doc(hidden)]
    LossTrend,
    #[doc(hidden)]
    LatencyTrend,
}

impl HudMetric {
    pub(crate) const ALL: [Self; 24] = [
        Self::Resolution,
        Self::FrameRate,
        Self::Bitrate,
        Self::Latency,
        Self::Signal,
        Self::PacketLoss,
        Self::LinkScore,
        Self::LinkHealth,
        Self::Armed,
        Self::FlightMode,
        Self::BatteryVoltage,
        Self::BatteryCurrent,
        Self::BatteryRemaining,
        Self::GpsStatus,
        Self::Altitude,
        Self::GroundSpeed,
        Self::VerticalSpeed,
        Self::Heading,
        Self::HomeDistance,
        Self::Throttle,
        Self::Attitude,
        Self::StatusText,
        Self::Coordinates,
        Self::RcLinkQuality,
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
            Self::LinkHealth => "Link health",
            Self::Armed => "Arm state",
            Self::FlightMode => "Flight mode",
            Self::BatteryVoltage => "Battery voltage",
            Self::BatteryCurrent => "Battery current",
            Self::BatteryRemaining => "Battery remaining",
            Self::GpsStatus => "GPS fix / satellites",
            Self::Altitude => "Altitude",
            Self::GroundSpeed => "Ground speed",
            Self::VerticalSpeed => "Vertical speed",
            Self::Heading => "Heading",
            Self::HomeDistance => "Home distance",
            Self::Throttle => "Throttle",
            Self::Attitude => "Attitude",
            Self::StatusText => "Status message",
            Self::Coordinates => "Coordinates",
            Self::RcLinkQuality => "RC link quality",
            Self::SignalBars => "Legacy signal bars",
            Self::SignalTrend => "RSSI trend",
            Self::LossTrend => "Loss trend",
            Self::LatencyTrend => "Latency trend",
        }
    }

    pub(crate) const fn supports_graph(self) -> bool {
        matches!(
            self,
            Self::FrameRate
                | Self::Bitrate
                | Self::Latency
                | Self::Signal
                | Self::PacketLoss
                | Self::LinkScore
        )
    }

    pub(crate) const fn supports_signal_bars(self) -> bool {
        matches!(self, Self::Signal)
    }

    pub(crate) const fn requires_telemetry(self) -> bool {
        matches!(
            self,
            Self::Armed
                | Self::FlightMode
                | Self::BatteryVoltage
                | Self::BatteryCurrent
                | Self::BatteryRemaining
                | Self::GpsStatus
                | Self::Altitude
                | Self::GroundSpeed
                | Self::VerticalSpeed
                | Self::Heading
                | Self::HomeDistance
                | Self::Throttle
                | Self::Attitude
                | Self::StatusText
                | Self::Coordinates
                | Self::RcLinkQuality
        )
    }
}

/// Position, visibility, and presentation of one OSD value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HudItemSettings {
    pub(crate) metric: HudMetric,
    pub(crate) visible: bool,
    /// Horizontal item center in normalized video coordinates.
    pub(crate) x: f32,
    /// Vertical item center in normalized video coordinates.
    pub(crate) y: f32,
    pub(crate) show_icon: bool,
    pub(crate) show_label: bool,
    pub(crate) show_value: bool,
    pub(crate) show_graph: bool,
    pub(crate) show_signal_bars: bool,
    pub(crate) show_background: bool,
    pub(crate) colorize: bool,
    pub(crate) hide_when_unavailable: bool,
    /// Scale relative to the global HUD scale.
    pub(crate) scale_percent: u16,
    /// Percentage of the global HUD background opacity.
    pub(crate) background_opacity_percent: u8,
    pub(crate) graph_seconds: u16,
    pub(crate) graph_width: u16,
    pub(crate) graph_height: u16,
    pub(crate) graph_fill: bool,
}

impl Default for HudItemSettings {
    fn default() -> Self {
        Self {
            metric: HudMetric::Resolution,
            visible: true,
            x: 0.08,
            y: 0.95,
            show_icon: true,
            show_label: false,
            show_value: true,
            show_graph: false,
            show_signal_bars: false,
            show_background: true,
            colorize: true,
            hide_when_unavailable: false,
            scale_percent: 100,
            background_opacity_percent: 100,
            graph_seconds: 20,
            graph_width: 118,
            graph_height: 42,
            graph_fill: false,
        }
    }
}

/// Persisted layout and appearance of the video OSD.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    pub(crate) fn reset_item(&mut self, metric: HudMetric) {
        let index = HudMetric::ALL
            .iter()
            .position(|candidate| *candidate == metric)
            .unwrap_or_default();
        if let Some(item) = self.items.iter_mut().find(|item| item.metric == metric) {
            *item = default_hud_item(metric, index);
        }
    }

    pub(crate) fn normalize(&mut self) {
        for metric in HudMetric::ALL {
            if !self.items.iter().any(|item| item.metric == metric) {
                let index = HudMetric::ALL
                    .iter()
                    .position(|candidate| *candidate == metric)
                    .unwrap_or_default();
                self.items.push(default_hud_item(metric, index));
            }
        }
        self.migrate_legacy_visual_items();
        let mut seen = Vec::with_capacity(HudMetric::ALL.len());
        self.items.retain(|item| {
            if !HudMetric::ALL.contains(&item.metric) || seen.contains(&item.metric) {
                false
            } else {
                seen.push(item.metric);
                true
            }
        });
        for item in &mut self.items {
            item.x = item.x.clamp(0.03, 0.97);
            item.y = item.y.clamp(0.03, 0.97);
            item.scale_percent = item.scale_percent.clamp(60, 200);
            item.background_opacity_percent = item.background_opacity_percent.min(100);
            item.graph_seconds = item.graph_seconds.clamp(5, 60);
            item.graph_width = item.graph_width.clamp(80, 260);
            item.graph_height = item.graph_height.clamp(32, 120);
            item.show_graph &= item.metric.supports_graph();
            item.show_signal_bars &= item.metric.supports_signal_bars();
            if !item.show_icon
                && !item.show_label
                && !item.show_value
                && !item.show_graph
                && !item.show_signal_bars
            {
                item.show_value = true;
            }
        }
        self.scale_percent = self.scale_percent.clamp(70, 160);
    }

    fn migrate_legacy_visual_items(&mut self) {
        let legacy = self
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item.metric,
                    HudMetric::SignalBars
                        | HudMetric::SignalTrend
                        | HudMetric::LossTrend
                        | HudMetric::LatencyTrend
                )
            })
            .cloned()
            .collect::<Vec<_>>();

        for old in legacy.into_iter().filter(|item| item.visible) {
            let (metric, graph, bars) = match old.metric {
                HudMetric::SignalBars => (HudMetric::Signal, false, true),
                HudMetric::SignalTrend => (HudMetric::Signal, true, false),
                HudMetric::LossTrend => (HudMetric::PacketLoss, true, false),
                HudMetric::LatencyTrend => (HudMetric::Latency, true, false),
                _ => continue,
            };
            let Some(target) = self.items.iter_mut().find(|item| item.metric == metric) else {
                continue;
            };
            target.show_graph |= graph;
            target.show_signal_bars |= bars;
        }
    }
}

impl Default for HudSettings {
    fn default() -> Self {
        Self {
            items: HudMetric::ALL
                .into_iter()
                .enumerate()
                .map(|(index, metric)| default_hud_item(metric, index))
                .collect(),
            scale_percent: 100,
            background_opacity: 205,
        }
    }
}

/// A named video OSD layout that can be reused across receiver profiles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct OsdProfile {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) hud: HudSettings,
    pub(crate) source: Option<PresetSource>,
}

impl OsdProfile {
    pub(crate) fn capture(id: u64, name: String, hud: &HudSettings) -> Self {
        Self {
            id,
            name,
            hud: hud.clone(),
            source: None,
        }
    }
}

impl Default for OsdProfile {
    fn default() -> Self {
        Self {
            id: 1,
            name: "Default OSD".to_owned(),
            hud: HudSettings::default(),
            source: None,
        }
    }
}

fn default_hud_item(metric: HudMetric, index: usize) -> HudItemSettings {
    let (visible, x, y) = match metric {
        HudMetric::LinkHealth => (false, 0.50, 0.10),
        HudMetric::Armed => (true, 0.50, 0.10),
        HudMetric::FlightMode => (true, 0.10, 0.10),
        HudMetric::BatteryVoltage => (true, 0.90, 0.10),
        HudMetric::BatteryRemaining => (true, 0.90, 0.24),
        HudMetric::BatteryCurrent => (true, 0.90, 0.38),
        HudMetric::GpsStatus => (true, 0.10, 0.24),
        HudMetric::Altitude => (true, 0.10, 0.38),
        HudMetric::GroundSpeed => (true, 0.10, 0.52),
        HudMetric::VerticalSpeed => (true, 0.10, 0.66),
        HudMetric::Heading => (true, 0.50, 0.24),
        HudMetric::HomeDistance => (true, 0.50, 0.38),
        HudMetric::Throttle => (true, 0.90, 0.52),
        HudMetric::StatusText => (true, 0.50, 0.52),
        HudMetric::Attitude => (false, 0.50, 0.50),
        HudMetric::Coordinates => (false, 0.50, 0.33),
        HudMetric::RcLinkQuality => (false, 0.90, 0.66),
        _ => {
            let columns = [0.07, 0.21, 0.35, 0.49, 0.63, 0.77, 0.91];
            (true, columns[index.min(columns.len() - 1)], 0.95)
        }
    };
    HudItemSettings {
        metric,
        visible,
        x,
        y,
        hide_when_unavailable: metric.requires_telemetry(),
        ..HudItemSettings::default()
    }
}

/// A named receiver configuration that can be switched before starting RX.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ReceiverProfile {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) receiver_source: ReceiverSource,
    pub(crate) udp_bind_address: String,
    pub(crate) udp_bind_port: u16,
    pub(crate) device_id: Option<String>,
    pub(crate) diversity_device_ids: Vec<String>,
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
    pub(crate) telemetry: TelemetrySettings,
    pub(crate) transfer_size: usize,
    pub(crate) vpn_enabled: bool,
    pub(crate) key_bytes: Vec<u8>,
    pub(crate) osd_profile_id: Option<u64>,
    pub(crate) route_preset_source: Option<PresetSource>,
    pub(crate) telemetry_preset_source: Option<PresetSource>,
    pub(crate) performance_preset_source: Option<PresetSource>,
}

impl ReceiverProfile {
    pub(crate) fn capture(id: u64, name: String, settings: &Settings) -> Self {
        Self {
            id,
            name,
            receiver_source: settings.receiver_source,
            udp_bind_address: settings.udp_bind_address.clone(),
            udp_bind_port: settings.udp_bind_port,
            device_id: settings.device_id.clone(),
            diversity_device_ids: settings.diversity_device_ids.clone(),
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
            telemetry: settings.telemetry.clone(),
            transfer_size: settings.transfer_size,
            vpn_enabled: settings.vpn_enabled,
            key_bytes: settings.key_bytes.clone(),
            osd_profile_id: settings.active_osd_profile_id,
            route_preset_source: settings.route_preset_source.clone(),
            telemetry_preset_source: settings.telemetry_preset_source.clone(),
            performance_preset_source: settings.performance_preset_source.clone(),
        }
    }

    pub(crate) fn apply(&self, settings: &mut Settings) {
        settings.receiver_source = self.receiver_source;
        settings.udp_bind_address.clone_from(&self.udp_bind_address);
        settings.udp_bind_port = self.udp_bind_port;
        settings.device_id.clone_from(&self.device_id);
        settings
            .diversity_device_ids
            .clone_from(&self.diversity_device_ids);
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
        settings.telemetry.clone_from(&self.telemetry);
        settings.transfer_size = self.transfer_size;
        settings.vpn_enabled = self.vpn_enabled;
        settings.key_bytes.clone_from(&self.key_bytes);
        settings.route_preset_source = self.route_preset_source.clone();
        settings.telemetry_preset_source = self.telemetry_preset_source.clone();
        settings.performance_preset_source = self.performance_preset_source.clone();
        settings.active_profile_id = Some(self.id);
        if let Some(id) = self.osd_profile_id {
            settings.apply_osd_profile(id);
        }
    }
}

impl Default for ReceiverProfile {
    fn default() -> Self {
        Self {
            id: 1,
            name: "Default FPV".to_owned(),
            receiver_source: ReceiverSource::Usb,
            udp_bind_address: "0.0.0.0".to_owned(),
            udp_bind_port: DEFAULT_UDP_RTP_PORT,
            device_id: None,
            diversity_device_ids: Vec::new(),
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
            telemetry: TelemetrySettings::default(),
            transfer_size: openipc_core::realtek::DEFAULT_RX_TRANSFER_SIZE,
            vpn_enabled: false,
            key_bytes: DEFAULT_KEY_BYTES.to_vec(),
            osd_profile_id: Some(1),
            route_preset_source: None,
            telemetry_preset_source: None,
            performance_preset_source: None,
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
            telemetry_protocol: TelemetryProtocol::Auto,
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
            action: RouteAction::Telemetry,
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

fn normalize_payload_routes(routes: &mut [PayloadRouteSettings]) {
    for route in routes {
        // Releases before telemetry-backed OSD indicators used this exact
        // built-in route as a byte inspector. Leave custom Inspect routes alone.
        if route.id == 2
            && route.name == "Telemetry"
            && route.radio_port == RadioPort::TelemetryRx.as_u8()
            && route.action == RouteAction::Inspect
        {
            route.action = RouteAction::Telemetry;
            route.telemetry_protocol = TelemetryProtocol::Auto;
        }
    }
}

/// User settings persisted by eframe on desktop and web.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) receiver_source: ReceiverSource,
    pub(crate) udp_bind_address: String,
    pub(crate) udp_bind_port: u16,
    pub(crate) device_id: Option<String>,
    /// Additional receive adapters combined with the primary adapter.
    pub(crate) diversity_device_ids: Vec<String>,
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
    pub(crate) gui_theme_preset_source: Option<PresetSource>,
    pub(crate) interface_scale_percent: u16,
    pub(crate) audio_volume: u8,
    /// Native recording folder. Empty selects the platform default.
    pub(crate) recording_directory: String,
    pub(crate) payload_routes: Vec<PayloadRouteSettings>,
    pub(crate) telemetry: TelemetrySettings,
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
    #[serde(default)]
    pub(crate) osd_profiles: Vec<OsdProfile>,
    #[serde(default)]
    pub(crate) active_osd_profile_id: Option<u64>,
    #[serde(default)]
    pub(crate) installed_presets: Vec<PresetPack>,
    pub(crate) preset_source_url: String,
    pub(crate) route_preset_source: Option<PresetSource>,
    pub(crate) telemetry_preset_source: Option<PresetSource>,
    pub(crate) performance_preset_source: Option<PresetSource>,
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

    pub(crate) fn next_osd_profile_id(&self) -> u64 {
        self.osd_profiles
            .iter()
            .map(|profile| profile.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub(crate) fn install_preset(&mut self, pack: PresetPack) -> Result<(), String> {
        if let Some(existing) = self
            .installed_presets
            .iter()
            .find(|existing| existing.id == pack.id && existing.version == pack.version)
        {
            if existing == &pack {
                return Ok(());
            }
            return Err(format!(
                "preset {} {} is already installed with different contents",
                pack.id, pack.version
            ));
        }
        if self.installed_presets.len() >= MAX_INSTALLED_PRESETS {
            return Err(format!(
                "at most {MAX_INSTALLED_PRESETS} preset versions can be installed"
            ));
        }
        self.installed_presets.push(pack);
        Ok(())
    }

    /// Writes the live OSD editor state back to the selected named layout.
    pub(crate) fn sync_active_osd_profile(&mut self) {
        let Some(id) = self.active_osd_profile_id else {
            return;
        };
        if let Some(profile) = self
            .osd_profiles
            .iter_mut()
            .find(|profile| profile.id == id)
        {
            profile.hud.clone_from(&self.hud);
        }
    }

    /// Selects a named OSD layout after preserving edits to the current one.
    pub(crate) fn apply_osd_profile(&mut self, id: u64) -> bool {
        if self.active_osd_profile_id == Some(id) {
            return self.osd_profiles.iter().any(|profile| profile.id == id);
        }
        self.sync_active_osd_profile();
        let Some(profile) = self
            .osd_profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return false;
        };
        self.hud = profile.hud;
        self.active_osd_profile_id = Some(id);
        true
    }

    pub(crate) fn selected_device_ids(&self) -> Vec<String> {
        let mut selected = Vec::with_capacity(1 + self.diversity_device_ids.len());
        if let Some(primary) = self.device_id.as_ref() {
            selected.push(primary.clone());
        }
        for id in &self.diversity_device_ids {
            if !selected.contains(id) {
                selected.push(id.clone());
            }
        }
        selected
    }

    pub(crate) fn normalize(&mut self) {
        self.hud.normalize();
        for profile in &mut self.osd_profiles {
            profile.hud.normalize();
        }
        normalize_osd_profiles(&mut self.osd_profiles);
        if self.osd_profiles.is_empty() {
            self.osd_profiles
                .push(OsdProfile::capture(1, "Default OSD".to_owned(), &self.hud));
        }
        if self
            .active_osd_profile_id
            .is_none_or(|id| !self.osd_profiles.iter().any(|profile| profile.id == id))
        {
            self.active_osd_profile_id = self.osd_profiles.first().map(|profile| profile.id);
        }
        self.sync_active_osd_profile();
        self.telemetry.normalize();
        normalize_payload_routes(&mut self.payload_routes);
        let mut installed = Vec::with_capacity(self.installed_presets.len());
        for mut pack in std::mem::take(&mut self.installed_presets) {
            let duplicate = installed.iter().any(|existing: &PresetPack| {
                existing.id == pack.id && existing.version == pack.version
            });
            if !duplicate && pack.normalize_and_validate().is_ok() {
                installed.push(pack);
            }
            if installed.len() == MAX_INSTALLED_PRESETS {
                break;
            }
        }
        self.installed_presets = installed;
        if self.preset_source_url.chars().count() > 2_048 {
            self.preset_source_url = self.preset_source_url.chars().take(2_048).collect();
        }
        let fallback_osd = self.active_osd_profile_id;
        for profile in &mut self.profiles {
            profile.telemetry.normalize();
            normalize_payload_routes(&mut profile.payload_routes);
            if profile
                .osd_profile_id
                .is_none_or(|id| !self.osd_profiles.iter().any(|osd| osd.id == id))
            {
                profile.osd_profile_id = fallback_osd;
            }
        }
        if let Some(primary) = self.device_id.as_ref() {
            self.diversity_device_ids.retain(|id| id != primary);
        }
        let mut unique = Vec::with_capacity(self.diversity_device_ids.len());
        self.diversity_device_ids.retain(|id| {
            !id.is_empty() && !unique.contains(id) && {
                unique.push(id.clone());
                true
            }
        });
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
            receiver_source: ReceiverSource::Usb,
            udp_bind_address: "0.0.0.0".to_owned(),
            udp_bind_port: DEFAULT_UDP_RTP_PORT,
            device_id: None,
            diversity_device_ids: Vec::new(),
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
            gui_theme_preset_source: None,
            interface_scale_percent: 100,
            audio_volume: 80,
            recording_directory: String::new(),
            payload_routes: default_routes(),
            telemetry: TelemetrySettings::default(),
            transfer_size: openipc_core::realtek::DEFAULT_RX_TRANSFER_SIZE,
            diagnostic_verbosity: DiagnosticVerbosity::Normal,
            vpn_enabled: false,
            key_bytes: DEFAULT_KEY_BYTES.to_vec(),
            profiles: vec![ReceiverProfile::default()],
            active_profile_id: Some(1),
            auto_recover: true,
            hud: HudSettings::default(),
            osd_profiles: vec![OsdProfile::default()],
            active_osd_profile_id: Some(1),
            installed_presets: Vec::new(),
            preset_source_url: DEFAULT_REGISTRY_URL.to_owned(),
            route_preset_source: None,
            telemetry_preset_source: None,
            performance_preset_source: None,
        }
    }
}

fn normalize_osd_profiles(profiles: &mut [OsdProfile]) {
    let mut reserved_ids = profiles
        .iter()
        .map(|profile| profile.id)
        .filter(|id| *id != 0)
        .collect::<Vec<_>>();
    reserved_ids.sort_unstable();
    reserved_ids.dedup();
    let mut seen_ids = Vec::with_capacity(profiles.len());
    let mut next_id = 1_u64;
    for profile in profiles {
        if profile.id == 0 || seen_ids.contains(&profile.id) {
            while reserved_ids.contains(&next_id) {
                next_id += 1;
            }
            profile.id = next_id;
            reserved_ids.push(next_id);
            next_id += 1;
        }
        seen_ids.push(profile.id);
        if profile.name.trim().is_empty() {
            profile.name = format!("OSD {}", profile.id);
        } else if profile.name.chars().count() > 48 {
            profile.name = profile.name.chars().take(48).collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use openipc_core::channel::RadioPort;

    use crate::telemetry::MavlinkSigningPolicy;

    use super::{
        GuiTheme, HudMetric, OsdProfile, PayloadRouteSettings, ReceiverProfile, ReceiverSource,
        RouteAction, Settings, DEFAULT_UDP_RTP_PORT,
    };

    #[test]
    fn profile_restores_receiver_fields_without_replacing_gui_preferences() {
        let mut settings = Settings {
            receiver_source: ReceiverSource::UdpRtp,
            udp_bind_address: "127.0.0.1".to_owned(),
            udp_bind_port: 5_601,
            channel: 149,
            link_id: 0x12_34_56,
            device_id: Some("0bda:8812@bus/1".to_owned()),
            diversity_device_ids: vec!["0bda:8812@bus/2".to_owned()],
            gui_theme: GuiTheme::Latte,
            recording_directory: "first-recording-folder".to_owned(),
            ..Settings::default()
        };
        settings.telemetry.mavlink_signing = MavlinkSigningPolicy::RequireSigned;
        settings.telemetry.mavlink_signing_key = vec![7; 32];
        settings.hud.scale_percent = 125;
        settings.active_osd_profile_id = Some(1);
        let profile = ReceiverProfile::capture(42, "Race quad".to_owned(), &settings);
        settings.channel = 36;
        settings.link_id = 1;
        settings.receiver_source = ReceiverSource::Usb;
        settings.udp_bind_address = "0.0.0.0".to_owned();
        settings.udp_bind_port = DEFAULT_UDP_RTP_PORT;
        settings.gui_theme = GuiTheme::Mocha;
        settings.recording_directory = "current-recording-folder".to_owned();
        settings.diversity_device_ids.clear();
        settings.telemetry.mavlink_signing = MavlinkSigningPolicy::Disabled;
        settings.telemetry.mavlink_signing_key.clear();

        profile.apply(&mut settings);

        assert_eq!(settings.channel, 149);
        assert_eq!(settings.link_id, 0x12_34_56);
        assert_eq!(settings.receiver_source, ReceiverSource::UdpRtp);
        assert_eq!(settings.udp_bind_address, "127.0.0.1");
        assert_eq!(settings.udp_bind_port, 5_601);
        assert_eq!(settings.gui_theme, GuiTheme::Mocha);
        assert_eq!(settings.recording_directory, "current-recording-folder");
        assert_eq!(settings.hud.scale_percent, 125);
        assert_eq!(settings.active_osd_profile_id, Some(1));
        assert_eq!(
            settings.diversity_device_ids,
            ["0bda:8812@bus/2".to_owned()]
        );
        assert_eq!(settings.active_profile_id, Some(42));
        assert_eq!(
            settings.telemetry.mavlink_signing,
            MavlinkSigningPolicy::RequireSigned
        );
        assert_eq!(settings.telemetry.mavlink_signing_key, vec![7; 32]);
    }

    #[test]
    fn missing_persisted_fields_use_current_defaults() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"channel":149}"#).expect("settings deserialize");
        settings.normalize();
        assert!(settings.auto_recover);
        assert!(!settings.profiles.is_empty());
        assert!(settings.recording_directory.is_empty());
        assert_eq!(settings.receiver_source, ReceiverSource::Usb);
        assert_eq!(settings.udp_bind_address, "0.0.0.0");
        assert_eq!(settings.udp_bind_port, DEFAULT_UDP_RTP_PORT);
        assert_eq!(settings.profiles[0].channel, 149);
        assert_eq!(settings.hud.items.len(), HudMetric::ALL.len());
        assert_eq!(settings.osd_profiles.len(), 1);
        assert_eq!(settings.active_osd_profile_id, Some(1));
        assert_eq!(settings.osd_profiles[0].hud, settings.hud);
        assert_eq!(
            settings.preset_source_url,
            crate::remote_presets::DEFAULT_REGISTRY_URL
        );
    }

    #[test]
    fn legacy_hud_becomes_the_default_osd_profile() {
        let mut settings: Settings = serde_json::from_str(
            r#"{
                "hud": {
                    "scale_percent": 137,
                    "background_opacity": 111,
                    "items": [
                        {"metric":"Resolution","visible":true,"x":0.27,"y":0.81}
                    ]
                }
            }"#,
        )
        .expect("legacy settings deserialize");

        settings.normalize();

        assert_eq!(settings.osd_profiles.len(), 1);
        assert_eq!(settings.osd_profiles[0].name, "Default OSD");
        assert_eq!(settings.osd_profiles[0].hud, settings.hud);
        assert_eq!(settings.osd_profiles[0].hud.scale_percent, 137);
        let resolution = settings.osd_profiles[0]
            .hud
            .items
            .iter()
            .find(|item| item.metric == HudMetric::Resolution)
            .expect("resolution indicator");
        assert_eq!(resolution.x, 0.27);
        assert_eq!(resolution.y, 0.81);
    }

    #[test]
    fn named_osd_profiles_preserve_independent_layouts() {
        let mut settings = Settings::default();
        settings.hud.scale_percent = 115;
        settings.sync_active_osd_profile();
        settings.osd_profiles.push(OsdProfile::capture(
            2,
            "Long range".to_owned(),
            &settings.hud,
        ));

        assert!(settings.apply_osd_profile(2));
        settings.hud.scale_percent = 145;
        settings.hud.background_opacity = 80;
        settings.sync_active_osd_profile();

        assert!(settings.apply_osd_profile(1));
        assert_eq!(settings.hud.scale_percent, 115);
        assert_eq!(settings.hud.background_opacity, 205);
        assert!(settings.apply_osd_profile(2));
        assert_eq!(settings.hud.scale_percent, 145);
        assert_eq!(settings.hud.background_opacity, 80);
    }

    #[test]
    fn receiver_profile_restores_its_referenced_osd() {
        let mut settings = Settings::default();
        settings
            .osd_profiles
            .push(OsdProfile::capture(2, "Race OSD".to_owned(), &settings.hud));
        assert!(settings.apply_osd_profile(2));
        let profile = ReceiverProfile::capture(9, "Race quad".to_owned(), &settings);
        assert_eq!(profile.osd_profile_id, Some(2));

        assert!(settings.apply_osd_profile(1));
        profile.apply(&mut settings);
        assert_eq!(settings.active_osd_profile_id, Some(2));
    }

    #[test]
    fn hud_normalization_restores_missing_items_and_clamps_positions() {
        let mut settings = Settings::default();
        settings.hud.items.truncate(1);
        settings.hud.items[0].x = -4.0;
        settings.hud.items[0].y = 8.0;
        settings.hud.items[0].show_graph = true;
        settings.hud.items[0].show_signal_bars = true;
        settings.hud.items[0].scale_percent = 999;
        settings.hud.items[0].graph_seconds = 1;
        settings.hud.items[0].graph_width = 10;
        settings.hud.items[0].graph_height = 999;
        settings.normalize();

        assert_eq!(settings.hud.items.len(), HudMetric::ALL.len());
        assert_eq!(settings.hud.items[0].x, 0.03);
        assert_eq!(settings.hud.items[0].y, 0.97);
        assert!(!settings.hud.items[0].show_graph);
        assert!(!settings.hud.items[0].show_signal_bars);
        assert_eq!(settings.hud.items[0].scale_percent, 200);
        assert_eq!(settings.hud.items[0].graph_seconds, 5);
        assert_eq!(settings.hud.items[0].graph_width, 80);
        assert_eq!(settings.hud.items[0].graph_height, 120);
        assert!(
            !settings
                .hud
                .items
                .iter()
                .find(|item| item.metric == HudMetric::LinkHealth)
                .expect("link-health HUD item")
                .visible
        );
        assert!(settings
            .hud
            .items
            .iter()
            .filter(|item| item.metric.supports_graph())
            .all(|item| !item.show_graph));
    }

    #[test]
    fn legacy_visual_items_migrate_to_indicator_options() {
        let mut settings: Settings = serde_json::from_str(
            r#"{
                "hud": {
                    "items": [
                        {"metric":"SignalBars","visible":true,"x":0.2,"y":0.2},
                        {"metric":"SignalTrend","visible":true,"x":0.3,"y":0.3},
                        {"metric":"LossTrend","visible":true,"x":0.4,"y":0.4},
                        {"metric":"LatencyTrend","visible":true,"x":0.5,"y":0.5}
                    ]
                }
            }"#,
        )
        .expect("legacy settings deserialize");
        settings.normalize();

        assert_eq!(settings.hud.items.len(), HudMetric::ALL.len());
        let signal = settings
            .hud
            .items
            .iter()
            .find(|item| item.metric == HudMetric::Signal)
            .expect("RSSI indicator");
        assert!(signal.show_signal_bars);
        assert!(signal.show_graph);
        for metric in [HudMetric::PacketLoss, HudMetric::Latency] {
            assert!(
                settings
                    .hud
                    .items
                    .iter()
                    .find(|item| item.metric == metric)
                    .expect("migrated indicator")
                    .show_graph
            );
        }
    }

    #[test]
    fn diversity_selection_is_unique_and_excludes_the_primary() {
        let mut settings = Settings {
            device_id: Some("primary".to_owned()),
            diversity_device_ids: vec![
                "primary".to_owned(),
                "secondary".to_owned(),
                "secondary".to_owned(),
            ],
            ..Settings::default()
        };
        settings.normalize();

        assert_eq!(settings.diversity_device_ids, ["secondary".to_owned()]);
        assert_eq!(
            settings.selected_device_ids(),
            ["primary".to_owned(), "secondary".to_owned()]
        );
    }

    #[test]
    fn old_default_telemetry_inspect_routes_migrate_to_the_osd_decoder() {
        let old_default = PayloadRouteSettings {
            id: 2,
            name: "Telemetry".to_owned(),
            radio_port: RadioPort::TelemetryRx.as_u8(),
            action: RouteAction::Inspect,
            ..PayloadRouteSettings::default()
        };
        let custom_inspector = PayloadRouteSettings {
            id: 22,
            name: "Telemetry".to_owned(),
            radio_port: RadioPort::TelemetryRx.as_u8(),
            action: RouteAction::Inspect,
            ..PayloadRouteSettings::default()
        };
        let mut settings = Settings {
            payload_routes: vec![old_default.clone(), custom_inspector.clone()],
            profiles: vec![ReceiverProfile {
                payload_routes: vec![old_default, custom_inspector],
                ..ReceiverProfile::default()
            }],
            ..Settings::default()
        };

        settings.normalize();

        assert_eq!(settings.payload_routes[0].action, RouteAction::Telemetry);
        assert_eq!(settings.payload_routes[1].action, RouteAction::Inspect);
        assert_eq!(
            settings.profiles[0].payload_routes[0].action,
            RouteAction::Telemetry
        );
        assert_eq!(
            settings.profiles[0].payload_routes[1].action,
            RouteAction::Inspect
        );
    }
}
