//! Portable, data-only Nebulus preset packs.

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{
    settings::{
        CodecPreference, GuiTheme, HudItemSettings, HudMetric, HudSettings, OsdProfile,
        PayloadRouteSettings, RouteAction, Settings,
    },
    telemetry::{
        MavlinkSigningPolicy, MspDirectionFilter, MspVersionFilter, TelemetryProtocol,
        TelemetrySettings, CRSF_ANY_ADDRESS,
    },
};

pub(crate) const SCHEMA_VERSION: u32 = 1;
pub(crate) const SCHEMA_URL: &str = "https://raw.githubusercontent.com/neelsani/openipc-rs/master/apps/nebulus/presets/schema-v1.json";
pub(crate) const MAX_PRESET_BYTES: usize = 512 * 1024;
pub(crate) const MAX_INSTALLED_PRESETS: usize = 64;
const MAX_ROUTES: usize = 32;

/// Immutable identity of one installed community preset version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PresetSource {
    pub(crate) id: String,
    pub(crate) version: String,
}

/// A portable preset pack. It deliberately has no fields for secrets, devices,
/// local paths, Link IDs, radio channels, or concrete network destinations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PresetPack {
    // Keep the normal serialized name RON-safe because installed packs live
    // inside eframe's persisted settings. Public JSON uses `$schema` through
    // `to_pretty_json`, while imports accept either spelling.
    #[serde(alias = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub(crate) json_schema: Option<String>,
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) name: String,
    pub(crate) author: String,
    pub(crate) license: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) minimum_nebulus_version: Option<String>,
    #[serde(default)]
    pub(crate) components: PresetComponents,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PresetComponents {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) osd: Option<OsdPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) theme: Option<PresetTheme>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) routes: Vec<PresetRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) telemetry: Option<TelemetryPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) performance: Option<PerformancePreset>,
}

impl PresetComponents {
    pub(crate) fn is_empty(&self) -> bool {
        self.osd.is_none()
            && self.theme.is_none()
            && self.routes.is_empty()
            && self.telemetry.is_none()
            && self.performance.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OsdPreset {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) hud: PresetHudSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PresetHudSettings {
    pub(crate) items: Vec<PresetHudItem>,
    pub(crate) scale_percent: u16,
    pub(crate) background_opacity: u8,
}

impl PresetHudSettings {
    fn capture(hud: &HudSettings) -> Self {
        Self {
            items: hud.items.iter().map(PresetHudItem::capture).collect(),
            scale_percent: hud.scale_percent,
            background_opacity: hud.background_opacity,
        }
    }

    pub(crate) fn materialize(&self) -> HudSettings {
        let mut hud = HudSettings {
            items: self.items.iter().map(PresetHudItem::materialize).collect(),
            scale_percent: self.scale_percent,
            background_opacity: self.background_opacity,
        };
        hud.normalize();
        hud
    }
}

impl Default for PresetHudSettings {
    fn default() -> Self {
        Self::capture(&HudSettings::default())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PresetHudItem {
    pub(crate) metric: PresetHudMetric,
    pub(crate) visible: bool,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) show_icon: bool,
    pub(crate) show_label: bool,
    pub(crate) show_value: bool,
    pub(crate) show_graph: bool,
    pub(crate) show_signal_bars: bool,
    pub(crate) show_background: bool,
    pub(crate) colorize: bool,
    pub(crate) hide_when_unavailable: bool,
    pub(crate) scale_percent: u16,
    pub(crate) background_opacity_percent: u8,
    pub(crate) graph_seconds: u16,
    pub(crate) graph_width: u16,
    pub(crate) graph_height: u16,
    pub(crate) graph_fill: bool,
}

impl PresetHudItem {
    fn capture(item: &HudItemSettings) -> Self {
        Self {
            metric: item.metric.into(),
            visible: item.visible,
            x: item.x,
            y: item.y,
            show_icon: item.show_icon,
            show_label: item.show_label,
            show_value: item.show_value,
            show_graph: item.show_graph,
            show_signal_bars: item.show_signal_bars,
            show_background: item.show_background,
            colorize: item.colorize,
            hide_when_unavailable: item.hide_when_unavailable,
            scale_percent: item.scale_percent,
            background_opacity_percent: item.background_opacity_percent,
            graph_seconds: item.graph_seconds,
            graph_width: item.graph_width,
            graph_height: item.graph_height,
            graph_fill: item.graph_fill,
        }
    }

    fn materialize(&self) -> HudItemSettings {
        HudItemSettings {
            metric: self.metric.into(),
            visible: self.visible,
            x: self.x,
            y: self.y,
            show_icon: self.show_icon,
            show_label: self.show_label,
            show_value: self.show_value,
            show_graph: self.show_graph,
            show_signal_bars: self.show_signal_bars,
            show_background: self.show_background,
            colorize: self.colorize,
            hide_when_unavailable: self.hide_when_unavailable,
            scale_percent: self.scale_percent,
            background_opacity_percent: self.background_opacity_percent,
            graph_seconds: self.graph_seconds,
            graph_width: self.graph_width,
            graph_height: self.graph_height,
            graph_fill: self.graph_fill,
        }
    }
}

impl Default for PresetHudItem {
    fn default() -> Self {
        Self::capture(&HudItemSettings::default())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PresetHudMetric {
    #[default]
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
}

impl From<HudMetric> for PresetHudMetric {
    fn from(value: HudMetric) -> Self {
        match value {
            HudMetric::Resolution => Self::Resolution,
            HudMetric::FrameRate => Self::FrameRate,
            HudMetric::Bitrate => Self::Bitrate,
            HudMetric::Latency => Self::Latency,
            HudMetric::Signal => Self::Signal,
            HudMetric::PacketLoss => Self::PacketLoss,
            HudMetric::LinkScore => Self::LinkScore,
            HudMetric::LinkHealth => Self::LinkHealth,
            HudMetric::Armed => Self::Armed,
            HudMetric::FlightMode => Self::FlightMode,
            HudMetric::BatteryVoltage => Self::BatteryVoltage,
            HudMetric::BatteryCurrent => Self::BatteryCurrent,
            HudMetric::BatteryRemaining => Self::BatteryRemaining,
            HudMetric::GpsStatus => Self::GpsStatus,
            HudMetric::Altitude => Self::Altitude,
            HudMetric::GroundSpeed => Self::GroundSpeed,
            HudMetric::VerticalSpeed => Self::VerticalSpeed,
            HudMetric::Heading => Self::Heading,
            HudMetric::HomeDistance => Self::HomeDistance,
            HudMetric::Throttle => Self::Throttle,
            HudMetric::Attitude => Self::Attitude,
            HudMetric::StatusText => Self::StatusText,
            HudMetric::Coordinates => Self::Coordinates,
            HudMetric::RcLinkQuality => Self::RcLinkQuality,
            HudMetric::SignalBars => Self::Signal,
            HudMetric::SignalTrend => Self::Signal,
            HudMetric::LossTrend => Self::PacketLoss,
            HudMetric::LatencyTrend => Self::Latency,
        }
    }
}

impl From<PresetHudMetric> for HudMetric {
    fn from(value: PresetHudMetric) -> Self {
        match value {
            PresetHudMetric::Resolution => Self::Resolution,
            PresetHudMetric::FrameRate => Self::FrameRate,
            PresetHudMetric::Bitrate => Self::Bitrate,
            PresetHudMetric::Latency => Self::Latency,
            PresetHudMetric::Signal => Self::Signal,
            PresetHudMetric::PacketLoss => Self::PacketLoss,
            PresetHudMetric::LinkScore => Self::LinkScore,
            PresetHudMetric::LinkHealth => Self::LinkHealth,
            PresetHudMetric::Armed => Self::Armed,
            PresetHudMetric::FlightMode => Self::FlightMode,
            PresetHudMetric::BatteryVoltage => Self::BatteryVoltage,
            PresetHudMetric::BatteryCurrent => Self::BatteryCurrent,
            PresetHudMetric::BatteryRemaining => Self::BatteryRemaining,
            PresetHudMetric::GpsStatus => Self::GpsStatus,
            PresetHudMetric::Altitude => Self::Altitude,
            PresetHudMetric::GroundSpeed => Self::GroundSpeed,
            PresetHudMetric::VerticalSpeed => Self::VerticalSpeed,
            PresetHudMetric::Heading => Self::Heading,
            PresetHudMetric::HomeDistance => Self::HomeDistance,
            PresetHudMetric::Throttle => Self::Throttle,
            PresetHudMetric::Attitude => Self::Attitude,
            PresetHudMetric::StatusText => Self::StatusText,
            PresetHudMetric::Coordinates => Self::Coordinates,
            PresetHudMetric::RcLinkQuality => Self::RcLinkQuality,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PresetTheme {
    Latte,
    Frappe,
    Macchiato,
    Mocha,
}

impl From<GuiTheme> for PresetTheme {
    fn from(value: GuiTheme) -> Self {
        match value {
            GuiTheme::Latte => Self::Latte,
            GuiTheme::Frappe => Self::Frappe,
            GuiTheme::Macchiato => Self::Macchiato,
            GuiTheme::Mocha => Self::Mocha,
        }
    }
}

impl From<PresetTheme> for GuiTheme {
    fn from(value: PresetTheme) -> Self {
        match value {
            PresetTheme::Latte => Self::Latte,
            PresetTheme::Frappe => Self::Frappe,
            PresetTheme::Macchiato => Self::Macchiato,
            PresetTheme::Mocha => Self::Mocha,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PresetRouteAction {
    Inspect,
    Log,
    Udp,
    Audio,
    Telemetry,
}

impl From<RouteAction> for PresetRouteAction {
    fn from(value: RouteAction) -> Self {
        match value {
            RouteAction::Inspect => Self::Inspect,
            RouteAction::Log => Self::Log,
            RouteAction::Udp => Self::Udp,
            RouteAction::Audio => Self::Audio,
            RouteAction::Telemetry => Self::Telemetry,
        }
    }
}

impl From<PresetRouteAction> for RouteAction {
    fn from(value: PresetRouteAction) -> Self {
        match value {
            PresetRouteAction::Inspect => Self::Inspect,
            PresetRouteAction::Log => Self::Log,
            PresetRouteAction::Udp => Self::Udp,
            PresetRouteAction::Audio => Self::Audio,
            PresetRouteAction::Telemetry => Self::Telemetry,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PresetTelemetryProtocol {
    #[default]
    Auto,
    Mavlink,
    Msp,
    Crsf,
}

impl From<TelemetryProtocol> for PresetTelemetryProtocol {
    fn from(value: TelemetryProtocol) -> Self {
        match value {
            TelemetryProtocol::Auto => Self::Auto,
            TelemetryProtocol::Mavlink => Self::Mavlink,
            TelemetryProtocol::Msp => Self::Msp,
            TelemetryProtocol::Crsf => Self::Crsf,
        }
    }
}

impl From<PresetTelemetryProtocol> for TelemetryProtocol {
    fn from(value: PresetTelemetryProtocol) -> Self {
        match value {
            PresetTelemetryProtocol::Auto => Self::Auto,
            PresetTelemetryProtocol::Mavlink => Self::Mavlink,
            PresetTelemetryProtocol::Msp => Self::Msp,
            PresetTelemetryProtocol::Crsf => Self::Crsf,
        }
    }
}

/// Shareable route template. IDs and UDP destinations are assigned locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PresetRoute {
    pub(crate) name: String,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    pub(crate) radio_port: u8,
    pub(crate) action: PresetRouteAction,
    #[serde(default)]
    pub(crate) telemetry_protocol: PresetTelemetryProtocol,
    #[serde(default = "default_payload_type")]
    pub(crate) payload_type: u8,
    #[serde(default = "default_sample_rate")]
    pub(crate) sample_rate: u32,
    #[serde(default = "default_channels")]
    pub(crate) channels: u8,
}

impl PresetRoute {
    fn capture(route: &PayloadRouteSettings) -> Self {
        Self {
            name: route.name.clone(),
            enabled: route.enabled,
            radio_port: route.radio_port,
            action: route.action.into(),
            telemetry_protocol: route.telemetry_protocol.into(),
            payload_type: route.payload_type,
            sample_rate: route.sample_rate,
            channels: route.channels,
        }
    }

    pub(crate) fn materialize(&self, id: u64) -> PayloadRouteSettings {
        let action: RouteAction = self.action.into();
        PayloadRouteSettings {
            id,
            // Public packs cannot carry a destination. Keep UDP templates
            // disabled until the user explicitly configures one locally.
            enabled: self.enabled && action != RouteAction::Udp,
            name: self.name.clone(),
            radio_port: self.radio_port,
            action,
            telemetry_protocol: self.telemetry_protocol.into(),
            payload_type: self.payload_type,
            sample_rate: self.sample_rate,
            channels: self.channels,
            udp_host: "127.0.0.1".to_owned(),
            udp_port: 5_600,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PresetMavlinkSigningPolicy {
    Disabled,
    VerifySigned,
    RequireSigned,
}

impl From<MavlinkSigningPolicy> for PresetMavlinkSigningPolicy {
    fn from(value: MavlinkSigningPolicy) -> Self {
        match value {
            MavlinkSigningPolicy::Disabled => Self::Disabled,
            MavlinkSigningPolicy::VerifySigned => Self::VerifySigned,
            MavlinkSigningPolicy::RequireSigned => Self::RequireSigned,
        }
    }
}

impl From<PresetMavlinkSigningPolicy> for MavlinkSigningPolicy {
    fn from(value: PresetMavlinkSigningPolicy) -> Self {
        match value {
            PresetMavlinkSigningPolicy::Disabled => Self::Disabled,
            PresetMavlinkSigningPolicy::VerifySigned => Self::VerifySigned,
            PresetMavlinkSigningPolicy::RequireSigned => Self::RequireSigned,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PresetMspVersion {
    Any,
    V1,
    V2,
}

impl From<MspVersionFilter> for PresetMspVersion {
    fn from(value: MspVersionFilter) -> Self {
        match value {
            MspVersionFilter::Any => Self::Any,
            MspVersionFilter::V1 => Self::V1,
            MspVersionFilter::V2 => Self::V2,
        }
    }
}

impl From<PresetMspVersion> for MspVersionFilter {
    fn from(value: PresetMspVersion) -> Self {
        match value {
            PresetMspVersion::Any => Self::Any,
            PresetMspVersion::V1 => Self::V1,
            PresetMspVersion::V2 => Self::V2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PresetMspDirection {
    Any,
    FromFlightController,
    ToFlightController,
}

impl From<MspDirectionFilter> for PresetMspDirection {
    fn from(value: MspDirectionFilter) -> Self {
        match value {
            MspDirectionFilter::Any => Self::Any,
            MspDirectionFilter::FromFlightController => Self::FromFlightController,
            MspDirectionFilter::ToFlightController => Self::ToFlightController,
        }
    }
}

impl From<PresetMspDirection> for MspDirectionFilter {
    fn from(value: PresetMspDirection) -> Self {
        match value {
            PresetMspDirection::Any => Self::Any,
            PresetMspDirection::FromFlightController => Self::FromFlightController,
            PresetMspDirection::ToFlightController => Self::ToFlightController,
        }
    }
}

/// Telemetry policy without the MAVLink signing key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TelemetryPreset {
    pub(crate) stale_timeout_ms: u32,
    pub(crate) mavlink_signing: PresetMavlinkSigningPolicy,
    pub(crate) mavlink_system_id: u8,
    pub(crate) mavlink_component_id: u8,
    pub(crate) msp_version: PresetMspVersion,
    pub(crate) msp_direction: PresetMspDirection,
    pub(crate) crsf_address: u16,
}

impl TelemetryPreset {
    fn capture(settings: &TelemetrySettings) -> Self {
        Self {
            stale_timeout_ms: settings.stale_timeout_ms,
            mavlink_signing: settings.mavlink_signing.into(),
            mavlink_system_id: settings.mavlink_system_id,
            mavlink_component_id: settings.mavlink_component_id,
            msp_version: settings.msp_version.into(),
            msp_direction: settings.msp_direction.into(),
            crsf_address: settings.crsf_address,
        }
    }

    pub(crate) fn apply(&self, settings: &mut TelemetrySettings) {
        let signing_key = std::mem::take(&mut settings.mavlink_signing_key);
        *settings = TelemetrySettings {
            stale_timeout_ms: self.stale_timeout_ms,
            mavlink_signing: self.mavlink_signing.into(),
            mavlink_signing_key: signing_key,
            mavlink_system_id: self.mavlink_system_id,
            mavlink_component_id: self.mavlink_component_id,
            msp_version: self.msp_version.into(),
            msp_direction: self.msp_direction.into(),
            crsf_address: self.crsf_address,
        };
        settings.normalize();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PresetCodecPreference {
    Auto,
    H264,
    H265,
}

impl From<CodecPreference> for PresetCodecPreference {
    fn from(value: CodecPreference) -> Self {
        match value {
            CodecPreference::Auto => Self::Auto,
            CodecPreference::H264 => Self::H264,
            CodecPreference::H265 => Self::H265,
        }
    }
}

impl From<PresetCodecPreference> for CodecPreference {
    fn from(value: PresetCodecPreference) -> Self {
        match value {
            PresetCodecPreference::Auto => Self::Auto,
            PresetCodecPreference::H264 => Self::H264,
            PresetCodecPreference::H265 => Self::H265,
        }
    }
}

/// Portable latency policy. Hardware-specific transfer sizes and TX power are
/// intentionally excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PerformancePreset {
    pub(crate) codec_preference: PresetCodecPreference,
    pub(crate) rtp_reorder: bool,
}

impl PerformancePreset {
    fn capture(settings: &Settings) -> Self {
        Self {
            codec_preference: settings.codec_preference.into(),
            rtp_reorder: settings.rtp_reorder,
        }
    }

    pub(crate) fn apply(self, settings: &mut Settings) {
        settings.codec_preference = self.codec_preference.into();
        settings.rtp_reorder = self.rtp_reorder;
    }
}

/// Component choices shown before applying an installed pack.
#[derive(Debug, Clone)]
pub(crate) struct PresetInstallDraft {
    pub(crate) pack: PresetPack,
    pub(crate) install_osd: bool,
    pub(crate) install_theme: bool,
    pub(crate) install_routes: bool,
    pub(crate) install_telemetry: bool,
    pub(crate) install_performance: bool,
    pub(crate) warnings: Vec<String>,
}

impl PresetInstallDraft {
    pub(crate) fn new(pack: PresetPack) -> Self {
        let mut warnings = Vec::new();
        if pack
            .components
            .routes
            .iter()
            .any(|route| route.action == PresetRouteAction::Udp)
        {
            warnings.push(
                "UDP templates install disabled and require a local destination before use"
                    .to_owned(),
            );
        }
        if pack.components.telemetry.as_ref().is_some_and(|telemetry| {
            telemetry.mavlink_signing != PresetMavlinkSigningPolicy::Disabled
        }) {
            warnings.push(
                "Signed MAVLink policy uses your existing local signing key; preset packs never include that key"
                    .to_owned(),
            );
        }
        Self {
            install_osd: pack.components.osd.is_some(),
            install_theme: pack.components.theme.is_some(),
            install_routes: !pack.components.routes.is_empty(),
            install_telemetry: pack.components.telemetry.is_some(),
            install_performance: pack.components.performance.is_some(),
            pack,
            warnings,
        }
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.install_osd
            || self.install_theme
            || self.install_routes
            || self.install_telemetry
            || self.install_performance
    }

    pub(crate) fn apply_to(&self, settings: &mut Settings) -> Result<PresetApplyResult, String> {
        if !self.has_selection() {
            return Err("Select at least one preset component".to_owned());
        }
        settings.install_preset(self.pack.clone())?;
        let source = self.pack.source();
        let mut result = PresetApplyResult::default();

        if self.install_osd {
            if let Some(component) = self.pack.components.osd.as_ref() {
                let existing = settings
                    .osd_profiles
                    .iter()
                    .position(|profile| profile.source.as_ref() == Some(&source));
                let id = if let Some(index) = existing {
                    let profile = &mut settings.osd_profiles[index];
                    profile.name.clone_from(&component.name);
                    profile.hud = component.hud.materialize();
                    profile.id
                } else {
                    let id = settings.next_osd_profile_id();
                    settings.osd_profiles.push(OsdProfile {
                        id,
                        name: component.name.clone(),
                        hud: component.hud.materialize(),
                        source: Some(source.clone()),
                    });
                    id
                };
                settings.apply_osd_profile(id);
                result.osd_changed = true;
            }
        }
        if self.install_theme {
            if let Some(theme) = self.pack.components.theme {
                settings.gui_theme = theme.into();
                settings.gui_theme_preset_source = Some(source.clone());
                result.theme_changed = true;
            }
        }
        if self.install_routes {
            settings.payload_routes = self
                .pack
                .components
                .routes
                .iter()
                .enumerate()
                // Route 1 belongs to the built-in video pipeline.
                .map(|(index, route)| route.materialize(index as u64 + 2))
                .collect();
            settings.route_preset_source = Some(source.clone());
        }
        if self.install_telemetry {
            if let Some(telemetry) = self.pack.components.telemetry.as_ref() {
                telemetry.apply(&mut settings.telemetry);
                settings.telemetry_preset_source = Some(source.clone());
            }
        }
        if self.install_performance {
            if let Some(performance) = self.pack.components.performance {
                performance.apply(settings);
                settings.performance_preset_source = Some(source);
            }
        }

        sync_active_profile(settings, self);
        settings.normalize();
        Ok(result)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PresetApplyResult {
    pub(crate) osd_changed: bool,
    pub(crate) theme_changed: bool,
}

fn sync_active_profile(settings: &mut Settings, draft: &PresetInstallDraft) {
    let Some(id) = settings.active_profile_id else {
        return;
    };
    let Some(profile) = settings
        .profiles
        .iter_mut()
        .find(|profile| profile.id == id)
    else {
        return;
    };
    if draft.install_osd {
        profile.osd_profile_id = settings.active_osd_profile_id;
    }
    if draft.install_routes {
        profile.payload_routes.clone_from(&settings.payload_routes);
        profile.route_preset_source = settings.route_preset_source.clone();
    }
    if draft.install_telemetry {
        profile.telemetry.clone_from(&settings.telemetry);
        profile.telemetry_preset_source = settings.telemetry_preset_source.clone();
    }
    if draft.install_performance {
        profile.codec_preference = settings.codec_preference;
        profile.rtp_reorder = settings.rtp_reorder;
        profile.performance_preset_source = settings.performance_preset_source.clone();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PresetExportDraft {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) name: String,
    pub(crate) author: String,
    pub(crate) license: String,
    pub(crate) description: String,
    pub(crate) include_osd: bool,
    pub(crate) include_theme: bool,
    pub(crate) include_routes: bool,
    pub(crate) include_telemetry: bool,
    pub(crate) include_performance: bool,
}

impl PresetExportDraft {
    pub(crate) fn from_settings(settings: &Settings) -> Self {
        let profile_name = settings
            .active_profile_id
            .and_then(|id| settings.profiles.iter().find(|profile| profile.id == id))
            .map_or("Nebulus preset", |profile| profile.name.as_str());
        Self {
            id: format!("community.{}", slug(profile_name)),
            version: "1.0.0".to_owned(),
            name: profile_name.to_owned(),
            author: String::new(),
            license: "MIT".to_owned(),
            description: String::new(),
            include_osd: true,
            include_theme: false,
            include_routes: true,
            include_telemetry: true,
            include_performance: true,
        }
    }

    pub(crate) fn build(&self, settings: &Settings) -> Result<PresetPack, String> {
        let osd_name = settings
            .active_osd_profile_id
            .and_then(|id| {
                settings
                    .osd_profiles
                    .iter()
                    .find(|profile| profile.id == id)
            })
            .map_or("OSD", |profile| profile.name.as_str());
        let mut pack = PresetPack {
            json_schema: Some(SCHEMA_URL.to_owned()),
            schema_version: SCHEMA_VERSION,
            id: self.id.clone(),
            version: self.version.clone(),
            name: self.name.clone(),
            author: self.author.clone(),
            license: self.license.clone(),
            description: self.description.clone(),
            minimum_nebulus_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            components: PresetComponents {
                osd: self.include_osd.then(|| OsdPreset {
                    name: osd_name.to_owned(),
                    hud: PresetHudSettings::capture(&settings.hud),
                }),
                theme: self.include_theme.then(|| settings.gui_theme.into()),
                routes: if self.include_routes {
                    settings
                        .payload_routes
                        .iter()
                        .map(PresetRoute::capture)
                        .collect()
                } else {
                    Vec::new()
                },
                telemetry: self
                    .include_telemetry
                    .then(|| TelemetryPreset::capture(&settings.telemetry)),
                performance: self
                    .include_performance
                    .then(|| PerformancePreset::capture(settings)),
            },
        };
        pack.normalize_and_validate()?;
        Ok(pack)
    }
}

impl PresetPack {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_PRESET_BYTES {
            return Err(format!(
                "preset is {} bytes; maximum is {MAX_PRESET_BYTES}",
                bytes.len()
            ));
        }
        let mut pack: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid preset JSON: {error}"))?;
        pack.normalize_and_validate()?;
        Ok(pack)
    }

    pub(crate) fn to_pretty_json(&self) -> Result<Vec<u8>, String> {
        let mut value =
            serde_json::to_value(self).map_err(|error| format!("serialize preset: {error}"))?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| "serialize preset: expected a JSON object".to_owned())?;
        if let Some(schema) = object.remove("jsonSchema") {
            object.insert("$schema".to_owned(), schema);
        }
        serde_json::to_vec_pretty(&value).map_err(|error| format!("serialize preset: {error}"))
    }

    pub(crate) fn source(&self) -> PresetSource {
        PresetSource {
            id: self.id.clone(),
            version: self.version.clone(),
        }
    }

    pub(crate) fn filename(&self) -> String {
        format!("{}-{}.nebulus-preset.json", slug(&self.id), self.version)
    }

    pub(crate) fn normalize_and_validate(&mut self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported preset schema {}; this build supports {SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if let Some(schema) = self.json_schema.as_deref() {
            validate_text("$schema", schema, 256, false)?;
        }
        validate_id(&self.id)?;
        Version::parse(&self.version)
            .map_err(|error| format!("invalid preset version: {error}"))?;
        validate_text("name", &self.name, 96, false)?;
        validate_text("author", &self.author, 96, false)?;
        validate_text("license", &self.license, 64, false)?;
        validate_text("description", &self.description, 1_024, true)?;
        if let Some(minimum) = self.minimum_nebulus_version.as_deref() {
            let minimum = Version::parse(minimum)
                .map_err(|error| format!("invalid minimumNebulusVersion: {error}"))?;
            let current = Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("CARGO_PKG_VERSION is valid semver");
            if minimum > current {
                return Err(format!(
                    "preset requires Nebulus {minimum} or newer; this build is {current}"
                ));
            }
        }
        if self.components.is_empty() {
            return Err("preset contains no components".to_owned());
        }
        if let Some(osd) = self.components.osd.as_mut() {
            validate_text("OSD name", &osd.name, 96, false)?;
            validate_hud(&osd.hud)?;
            osd.hud = PresetHudSettings::capture(&osd.hud.materialize());
        }
        if self.components.routes.len() > MAX_ROUTES {
            return Err(format!("preset contains more than {MAX_ROUTES} routes"));
        }
        for route in &mut self.components.routes {
            validate_text("route name", &route.name, 64, false)?;
            if route.payload_type > 127 {
                return Err(format!(
                    "route '{}' payloadType must be between 0 and 127",
                    route.name
                ));
            }
            if !(8_000..=192_000).contains(&route.sample_rate) {
                return Err(format!(
                    "route '{}' sampleRate must be between 8000 and 192000",
                    route.name
                ));
            }
            if !(1..=8).contains(&route.channels) {
                return Err(format!(
                    "route '{}' channels must be between 1 and 8",
                    route.name
                ));
            }
        }
        if let Some(telemetry) = self.components.telemetry.as_ref() {
            if !(500..=30_000).contains(&telemetry.stale_timeout_ms) {
                return Err("telemetry staleTimeoutMs must be between 500 and 30000".to_owned());
            }
            if telemetry.crsf_address > CRSF_ANY_ADDRESS {
                return Err(format!(
                    "telemetry crsfAddress must be between 0 and {CRSF_ANY_ADDRESS}"
                ));
            }
        }
        Ok(())
    }
}

fn validate_hud(hud: &PresetHudSettings) -> Result<(), String> {
    if hud.items.len() > HudMetric::ALL.len() {
        return Err(format!(
            "OSD contains more than {} indicators",
            HudMetric::ALL.len()
        ));
    }
    if !(70..=160).contains(&hud.scale_percent) {
        return Err("OSD scalePercent must be between 70 and 160".to_owned());
    }
    for item in &hud.items {
        let metric = item.metric;
        if !(0.03..=0.97).contains(&item.x) || !(0.03..=0.97).contains(&item.y) {
            return Err(format!(
                "OSD indicator {metric:?} coordinates must be between 0.03 and 0.97"
            ));
        }
        if !(60..=200).contains(&item.scale_percent) {
            return Err(format!(
                "OSD indicator {metric:?} scalePercent must be between 60 and 200"
            ));
        }
        if item.background_opacity_percent > 100 {
            return Err(format!(
                "OSD indicator {metric:?} backgroundOpacityPercent must be between 0 and 100"
            ));
        }
        if !(5..=60).contains(&item.graph_seconds) {
            return Err(format!(
                "OSD indicator {metric:?} graphSeconds must be between 5 and 60"
            ));
        }
        if !(80..=260).contains(&item.graph_width) {
            return Err(format!(
                "OSD indicator {metric:?} graphWidth must be between 80 and 260"
            ));
        }
        if !(32..=120).contains(&item.graph_height) {
            return Err(format!(
                "OSD indicator {metric:?} graphHeight must be between 32 and 120"
            ));
        }
    }
    Ok(())
}

pub(crate) fn is_preset_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".nebulus-preset.json") || lower.ends_with(".nebulus-preset")
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub(crate) fn save(pack: &PresetPack) -> Result<String, String> {
    let filename = pack.filename();
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export Nebulus preset pack")
        .set_file_name(&filename)
        .add_filter("Nebulus preset", &["json", "nebulus-preset"])
        .save_file()
    else {
        return Ok("Preset export cancelled".to_owned());
    };
    std::fs::write(&path, pack.to_pretty_json()?)
        .map_err(|error| format!("write {} failed: {error}", path.display()))?;
    Ok(format!("Preset exported to {}", path.display()))
}

#[cfg(target_os = "android")]
pub(crate) fn save(pack: &PresetPack) -> Result<String, String> {
    crate::android::save_document(
        &pack.filename(),
        "application/json",
        &pack.to_pretty_json()?,
    )?;
    Ok("Android document picker opened for the preset pack".to_owned())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn save(pack: &PresetPack) -> Result<String, String> {
    use wasm_bindgen::{closure::Closure, JsCast as _};

    let bytes = pack.to_pretty_json()?;
    let parts = js_sys::Array::new();
    let bytes = js_sys::Uint8Array::from(bytes.as_slice());
    parts.push(&bytes.buffer());
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/json");
    let blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &options)
        .map_err(js_error)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_error)?;
    let window = web_sys::window().ok_or_else(|| "browser window is unavailable".to_owned())?;
    let document = window
        .document()
        .ok_or_else(|| "browser document is unavailable".to_owned())?;
    let body = document
        .body()
        .ok_or_else(|| "browser document body is unavailable".to_owned())?;
    let anchor = document
        .create_element("a")
        .map_err(js_error)?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "could not create preset download link".to_owned())?;
    anchor.set_href(&url);
    anchor.set_download(&pack.filename());
    body.append_child(&anchor).map_err(js_error)?;
    anchor.click();

    let revoke_url = url.clone();
    let cleanup = Closure::once(move || {
        let _ = web_sys::Url::revoke_object_url(&revoke_url);
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(cleanup.as_ref().unchecked_ref(), 0)
        .map_err(js_error)?;
    cleanup.forget();
    Ok(format!("Preset download started: {}", pack.filename()))
}

#[cfg(target_arch = "wasm32")]
fn js_error(error: wasm_bindgen::JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| format!("browser preset error: {error:?}"))
}

pub(crate) fn validate_id(id: &str) -> Result<(), String> {
    let mut segments = id.split('.');
    let namespace = segments.next().unwrap_or_default();
    let name = segments.collect::<Vec<_>>().join(".");
    if id.len() > 96
        || namespace.is_empty()
        || name.is_empty()
        || !namespace
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err("preset id must be a namespaced value such as author.preset-name".to_owned());
    }
    if !id.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
    }) {
        return Err(
            "preset id may contain only lowercase ASCII letters, numbers, '.', '-' and '_'"
                .to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn validate_text(
    field: &str,
    value: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<(), String> {
    if !allow_empty && value.trim().is_empty() {
        return Err(format!("preset {field} is required"));
    }
    if value.chars().count() > max_chars {
        return Err(format!(
            "preset {field} is longer than {max_chars} characters"
        ));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(format!("preset {field} contains control characters"));
    }
    Ok(())
}

fn slug(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            output.push(character);
            separator = false;
        } else if !separator && !output.is_empty() {
            output.push('-');
            separator = true;
        }
    }
    while output.ends_with('-') {
        output.pop();
    }
    if output.is_empty() {
        "preset".to_owned()
    } else {
        output
    }
}

const fn default_true() -> bool {
    true
}

const fn default_payload_type() -> u8 {
    openipc_core::rtp::RTP_PAYLOAD_TYPE_OPUS
}

const fn default_sample_rate() -> u32 {
    48_000
}

const fn default_channels() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryStorage {
        value: Option<String>,
    }

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, _key: &str) -> Option<String> {
            self.value.clone()
        }

        fn set_string(&mut self, _key: &str, value: String) {
            self.value = Some(value);
        }

        fn remove_string(&mut self, _key: &str) {
            self.value = None;
        }

        fn flush(&mut self) {}
    }

    fn draft() -> PresetExportDraft {
        PresetExportDraft {
            id: "neels.test-pack".to_owned(),
            version: "1.0.0".to_owned(),
            name: "Test pack".to_owned(),
            author: "Test author".to_owned(),
            license: "MIT".to_owned(),
            description: "Portable test".to_owned(),
            include_osd: true,
            include_theme: true,
            include_routes: true,
            include_telemetry: true,
            include_performance: true,
        }
    }

    #[test]
    fn exported_pack_never_contains_private_configuration() {
        let mut settings = Settings {
            device_id: Some("usb-secret-location".to_owned()),
            key_bytes: vec![0x5a; 64],
            recording_directory: "/private/recordings".to_owned(),
            ..Settings::default()
        };
        settings.telemetry.mavlink_signing_key = vec![0xa5; 32];
        settings.payload_routes[0].udp_host = "192.0.2.44".to_owned();
        let json =
            String::from_utf8(draft().build(&settings).unwrap().to_pretty_json().unwrap()).unwrap();

        assert!(!json.contains("usb-secret-location"));
        assert!(!json.contains("192.0.2.44"));
        assert!(!json.contains("/private/recordings"));
        assert!(!json.contains(&"5a".repeat(64)));
        assert!(!json.contains(&"a5".repeat(32)));
    }

    #[test]
    fn public_json_uses_schema_keyword_but_eframe_persistence_is_ron_safe() {
        let pack = draft().build(&Settings::default()).unwrap();
        let json = String::from_utf8(pack.to_pretty_json().unwrap()).unwrap();
        assert!(json.contains("\"$schema\""));
        assert!(!json.contains("\"jsonSchema\""));

        let settings = Settings {
            installed_presets: vec![pack],
            ..Settings::default()
        };
        let mut storage = MemoryStorage::default();
        eframe::set_value(&mut storage, eframe::APP_KEY, &settings);

        let encoded = storage.value.as_deref().expect("RON was stored");
        assert!(encoded.contains("jsonSchema"));
        assert!(!encoded.contains("$schema"));
        let restored: Settings = eframe::get_value(&storage, eframe::APP_KEY).unwrap();
        assert_eq!(restored.installed_presets, settings.installed_presets);
    }

    #[test]
    fn telemetry_application_preserves_local_signing_key() {
        let settings = Settings::default();
        let preset = TelemetryPreset::capture(&settings.telemetry);
        let mut local = TelemetrySettings {
            mavlink_signing_key: vec![7; 32],
            ..TelemetrySettings::default()
        };
        preset.apply(&mut local);
        assert_eq!(local.mavlink_signing_key, vec![7; 32]);
    }

    #[test]
    fn udp_templates_are_installed_disabled_without_a_public_destination() {
        let route = PresetRoute {
            name: "Forward video".to_owned(),
            enabled: true,
            radio_port: 0,
            action: PresetRouteAction::Udp,
            telemetry_protocol: PresetTelemetryProtocol::Auto,
            payload_type: 96,
            sample_rate: 90_000,
            channels: 1,
        }
        .materialize(9);
        assert!(!route.enabled);
        assert_eq!(route.udp_host, "127.0.0.1");
        assert_eq!(route.id, 9);
    }

    #[test]
    fn parse_rejects_future_or_unnamespaced_packs() {
        let mut pack = draft().build(&Settings::default()).unwrap();
        pack.schema_version = SCHEMA_VERSION + 1;
        assert!(PresetPack::parse(&pack.to_pretty_json().unwrap()).is_err());

        pack.schema_version = SCHEMA_VERSION;
        pack.id = "unnamespaced".to_owned();
        assert!(PresetPack::parse(&pack.to_pretty_json().unwrap()).is_err());

        pack.id = ".missing-namespace".to_owned();
        assert!(PresetPack::parse(&pack.to_pretty_json().unwrap()).is_err());

        pack.id = "missing-name.".to_owned();
        assert!(PresetPack::parse(&pack.to_pretty_json().unwrap()).is_err());
    }

    #[test]
    fn parser_rejects_values_outside_the_public_schema() {
        let mut pack = draft().build(&Settings::default()).unwrap();
        pack.components.osd.as_mut().unwrap().hud.items[0].x = 2.0;
        assert!(PresetPack::parse(&pack.to_pretty_json().unwrap()).is_err());

        let mut pack = draft().build(&Settings::default()).unwrap();
        pack.components.routes[0].sample_rate = 1;
        assert!(PresetPack::parse(&pack.to_pretty_json().unwrap()).is_err());

        let mut pack = draft().build(&Settings::default()).unwrap();
        pack.components.telemetry.as_mut().unwrap().stale_timeout_ms = 1;
        assert!(PresetPack::parse(&pack.to_pretty_json().unwrap()).is_err());
    }

    #[test]
    fn bundled_example_matches_the_public_parser() {
        let pack = PresetPack::parse(include_bytes!(
            "../presets/openipc-standard.nebulus-preset.json"
        ))
        .unwrap();
        assert_eq!(pack.id, "openipc.standard-fpv");
        assert_eq!(pack.components.routes.len(), 2);
        assert!(pack.components.osd.is_some());
    }

    #[test]
    fn applying_every_component_preserves_local_only_state() {
        let mut settings = Settings {
            device_id: Some("usb-device-7".to_owned()),
            channel: 149,
            link_id: 0x123456,
            key_bytes: vec![0x42; 64],
            recording_directory: "/local/video".to_owned(),
            ..Settings::default()
        };
        settings.telemetry.mavlink_signing_key = vec![0x24; 32];
        let pack = draft().build(&Settings::default()).unwrap();
        let result = PresetInstallDraft::new(pack)
            .apply_to(&mut settings)
            .unwrap();

        assert!(result.osd_changed);
        assert!(result.theme_changed);
        assert_eq!(settings.device_id.as_deref(), Some("usb-device-7"));
        assert_eq!(settings.channel, 149);
        assert_eq!(settings.link_id, 0x123456);
        assert_eq!(settings.key_bytes, vec![0x42; 64]);
        assert_eq!(settings.telemetry.mavlink_signing_key, vec![0x24; 32]);
        assert_eq!(settings.recording_directory, "/local/video");
        assert_eq!(settings.installed_presets.len(), 1);
        assert!(settings.payload_routes.iter().all(|route| route.id >= 2));
        assert_eq!(
            settings.profiles[0].osd_profile_id,
            settings.active_osd_profile_id
        );
    }
}
