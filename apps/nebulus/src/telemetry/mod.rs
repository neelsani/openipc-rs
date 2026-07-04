//! App-owned telemetry decoding and protocol-neutral OSD state.

mod crsf;
mod mavlink;
mod msp;

use serde::{Deserialize, Serialize};
use web_time::Instant;

pub(crate) const DEFAULT_STALE_TIMEOUT_MS: u32 = 3_000;
pub(crate) const CRSF_ANY_ADDRESS: u16 = 256;

/// Telemetry protocol selected for a payload route.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TelemetryProtocol {
    #[default]
    Auto,
    Mavlink,
    Msp,
    Crsf,
}

impl TelemetryProtocol {
    pub(crate) const ALL: [Self; 4] = [Self::Auto, Self::Mavlink, Self::Msp, Self::Crsf];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto detect",
            Self::Mavlink => "MAVLink",
            Self::Msp => "MSP",
            Self::Crsf => "CRSF",
        }
    }
}

/// Policy applied to MAVLink 2 packet signatures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum MavlinkSigningPolicy {
    /// Accept signed and unsigned packets without authenticating the signature.
    #[default]
    Disabled,
    /// Authenticate signed packets while continuing to accept unsigned packets.
    VerifySigned,
    /// Accept only correctly signed, non-replayed MAVLink 2 packets.
    RequireSigned,
}

impl MavlinkSigningPolicy {
    pub(crate) const ALL: [Self; 3] = [Self::Disabled, Self::VerifySigned, Self::RequireSigned];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::VerifySigned => "Verify signed",
            Self::RequireSigned => "Require signed",
        }
    }

    pub(crate) const fn requires_key(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

/// MSP frame version accepted by the telemetry decoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum MspVersionFilter {
    #[default]
    Any,
    V1,
    V2,
}

impl MspVersionFilter {
    pub(crate) const ALL: [Self; 3] = [Self::Any, Self::V1, Self::V2];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Any => "Auto (v1 + v2)",
            Self::V1 => "MSP v1 only",
            Self::V2 => "MSP v2 only",
        }
    }
}

/// MSP traffic direction accepted by the telemetry decoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum MspDirectionFilter {
    #[default]
    Any,
    FromFlightController,
    ToFlightController,
}

impl MspDirectionFilter {
    pub(crate) const ALL: [Self; 3] = [
        Self::Any,
        Self::FromFlightController,
        Self::ToFlightController,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Any => "Any direction",
            Self::FromFlightController => "From flight controller",
            Self::ToFlightController => "To flight controller",
        }
    }
}

/// Persisted telemetry decoder policy shared by all Telemetry-to-OSD routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TelemetrySettings {
    pub(crate) stale_timeout_ms: u32,
    pub(crate) mavlink_signing: MavlinkSigningPolicy,
    pub(crate) mavlink_signing_key: Vec<u8>,
    /// Zero accepts every MAVLink system ID.
    pub(crate) mavlink_system_id: u8,
    /// Zero accepts every MAVLink component ID.
    pub(crate) mavlink_component_id: u8,
    pub(crate) msp_version: MspVersionFilter,
    pub(crate) msp_direction: MspDirectionFilter,
    /// `CRSF_ANY_ADDRESS` accepts every valid device address.
    pub(crate) crsf_address: u16,
}

impl TelemetrySettings {
    pub(crate) fn normalize(&mut self) {
        self.stale_timeout_ms = self.stale_timeout_ms.clamp(500, 30_000);
        self.crsf_address = self.crsf_address.min(CRSF_ANY_ADDRESS);
    }
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            stale_timeout_ms: DEFAULT_STALE_TIMEOUT_MS,
            mavlink_signing: MavlinkSigningPolicy::Disabled,
            mavlink_signing_key: Vec::new(),
            mavlink_system_id: 0,
            mavlink_component_id: 0,
            msp_version: MspVersionFilter::Any,
            msp_direction: MspDirectionFilter::Any,
            crsf_address: CRSF_ANY_ADDRESS,
        }
    }
}

/// Counters emitted by telemetry framing and security checks.
#[derive(Debug, Clone, Default)]
pub(crate) struct TelemetryCounters {
    pub(crate) accepted_frames: u64,
    pub(crate) rejected_frames: u64,
    pub(crate) filtered_frames: u64,
    pub(crate) mavlink_v1_frames: u64,
    pub(crate) mavlink_v2_frames: u64,
    pub(crate) mavlink_signed_frames: u64,
    pub(crate) mavlink_unsigned_frames: u64,
    pub(crate) mavlink_verified_frames: u64,
    pub(crate) mavlink_invalid_signatures: u64,
    pub(crate) mavlink_replay_drops: u64,
    pub(crate) mavlink_stale_timestamp_drops: u64,
    pub(crate) mavlink_missing_key_drops: u64,
}

impl TelemetryCounters {
    fn merge(&mut self, newer: Self) {
        self.accepted_frames = self.accepted_frames.saturating_add(newer.accepted_frames);
        self.rejected_frames = self.rejected_frames.saturating_add(newer.rejected_frames);
        self.filtered_frames = self.filtered_frames.saturating_add(newer.filtered_frames);
        self.mavlink_v1_frames = self
            .mavlink_v1_frames
            .saturating_add(newer.mavlink_v1_frames);
        self.mavlink_v2_frames = self
            .mavlink_v2_frames
            .saturating_add(newer.mavlink_v2_frames);
        self.mavlink_signed_frames = self
            .mavlink_signed_frames
            .saturating_add(newer.mavlink_signed_frames);
        self.mavlink_unsigned_frames = self
            .mavlink_unsigned_frames
            .saturating_add(newer.mavlink_unsigned_frames);
        self.mavlink_verified_frames = self
            .mavlink_verified_frames
            .saturating_add(newer.mavlink_verified_frames);
        self.mavlink_invalid_signatures = self
            .mavlink_invalid_signatures
            .saturating_add(newer.mavlink_invalid_signatures);
        self.mavlink_replay_drops = self
            .mavlink_replay_drops
            .saturating_add(newer.mavlink_replay_drops);
        self.mavlink_stale_timestamp_drops = self
            .mavlink_stale_timestamp_drops
            .saturating_add(newer.mavlink_stale_timestamp_drops);
        self.mavlink_missing_key_drops = self
            .mavlink_missing_key_drops
            .saturating_add(newer.mavlink_missing_key_drops);
    }

    pub(crate) const fn observed_frames(&self) -> u64 {
        self.accepted_frames + self.rejected_frames + self.filtered_frames
    }
}

/// Partial telemetry values decoded from one or more valid protocol frames.
#[derive(Debug, Clone, Default)]
pub(crate) struct TelemetryUpdate {
    pub(crate) protocol: Option<TelemetryProtocol>,
    pub(crate) messages: u64,
    pub(crate) counters: TelemetryCounters,
    pub(crate) mavlink_version: Option<u8>,
    pub(crate) mavlink_system_id: Option<u8>,
    pub(crate) mavlink_component_id: Option<u8>,
    pub(crate) mavlink_last_signed: Option<bool>,
    pub(crate) mavlink_signing_link_id: Option<u8>,
    pub(crate) armed: Option<bool>,
    pub(crate) flight_mode: Option<String>,
    pub(crate) status_text: Option<String>,
    pub(crate) battery_voltage_v: Option<f32>,
    pub(crate) battery_current_a: Option<f32>,
    pub(crate) battery_consumed_mah: Option<u32>,
    pub(crate) battery_remaining_pct: Option<u8>,
    pub(crate) latitude_deg: Option<f64>,
    pub(crate) longitude_deg: Option<f64>,
    pub(crate) altitude_m: Option<f32>,
    pub(crate) relative_altitude_m: Option<f32>,
    pub(crate) ground_speed_mps: Option<f32>,
    pub(crate) air_speed_mps: Option<f32>,
    pub(crate) vertical_speed_mps: Option<f32>,
    pub(crate) heading_deg: Option<f32>,
    pub(crate) satellites: Option<u8>,
    pub(crate) gps_fix: Option<u8>,
    pub(crate) throttle_pct: Option<u8>,
    pub(crate) roll_deg: Option<f32>,
    pub(crate) pitch_deg: Option<f32>,
    pub(crate) yaw_deg: Option<f32>,
    pub(crate) home_distance_m: Option<f32>,
    pub(crate) rc_link_quality_pct: Option<u8>,
}

impl TelemetryUpdate {
    pub(crate) fn merge(&mut self, newer: Self) {
        self.messages = self.messages.saturating_add(newer.messages);
        self.counters.merge(newer.counters);
        replace_some(&mut self.protocol, newer.protocol);
        replace_some(&mut self.mavlink_version, newer.mavlink_version);
        replace_some(&mut self.mavlink_system_id, newer.mavlink_system_id);
        replace_some(&mut self.mavlink_component_id, newer.mavlink_component_id);
        replace_some(&mut self.mavlink_last_signed, newer.mavlink_last_signed);
        replace_some(
            &mut self.mavlink_signing_link_id,
            newer.mavlink_signing_link_id,
        );
        replace_some(&mut self.armed, newer.armed);
        replace_some(&mut self.flight_mode, newer.flight_mode);
        replace_some(&mut self.status_text, newer.status_text);
        replace_some(&mut self.battery_voltage_v, newer.battery_voltage_v);
        replace_some(&mut self.battery_current_a, newer.battery_current_a);
        replace_some(&mut self.battery_consumed_mah, newer.battery_consumed_mah);
        replace_some(&mut self.battery_remaining_pct, newer.battery_remaining_pct);
        replace_some(&mut self.latitude_deg, newer.latitude_deg);
        replace_some(&mut self.longitude_deg, newer.longitude_deg);
        replace_some(&mut self.altitude_m, newer.altitude_m);
        replace_some(&mut self.relative_altitude_m, newer.relative_altitude_m);
        replace_some(&mut self.ground_speed_mps, newer.ground_speed_mps);
        replace_some(&mut self.air_speed_mps, newer.air_speed_mps);
        replace_some(&mut self.vertical_speed_mps, newer.vertical_speed_mps);
        replace_some(&mut self.heading_deg, newer.heading_deg);
        replace_some(&mut self.satellites, newer.satellites);
        replace_some(&mut self.gps_fix, newer.gps_fix);
        replace_some(&mut self.throttle_pct, newer.throttle_pct);
        replace_some(&mut self.roll_deg, newer.roll_deg);
        replace_some(&mut self.pitch_deg, newer.pitch_deg);
        replace_some(&mut self.yaw_deg, newer.yaw_deg);
        replace_some(&mut self.home_distance_m, newer.home_distance_m);
        replace_some(&mut self.rc_link_quality_pct, newer.rc_link_quality_pct);
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.messages == 0 && self.counters.observed_frames() == 0
    }

    pub(crate) const fn has_protocol_evidence(&self) -> bool {
        self.counters.observed_frames() > 0
    }
}

fn replace_some<T>(target: &mut Option<T>, newer: Option<T>) {
    if newer.is_some() {
        *target = newer;
    }
}

/// Latest protocol-neutral values consumed by OSD indicators.
#[derive(Debug, Clone, Default)]
pub(crate) struct TelemetryState {
    pub(crate) protocol: Option<TelemetryProtocol>,
    pub(crate) messages: u64,
    pub(crate) counters: TelemetryCounters,
    pub(crate) mavlink_version: Option<u8>,
    pub(crate) mavlink_system_id: Option<u8>,
    pub(crate) mavlink_component_id: Option<u8>,
    pub(crate) mavlink_last_signed: Option<bool>,
    pub(crate) mavlink_signing_link_id: Option<u8>,
    pub(crate) armed: Option<bool>,
    pub(crate) flight_mode: Option<String>,
    pub(crate) status_text: Option<String>,
    pub(crate) battery_voltage_v: Option<f32>,
    pub(crate) battery_current_a: Option<f32>,
    pub(crate) battery_consumed_mah: Option<u32>,
    pub(crate) battery_remaining_pct: Option<u8>,
    pub(crate) latitude_deg: Option<f64>,
    pub(crate) longitude_deg: Option<f64>,
    pub(crate) altitude_m: Option<f32>,
    pub(crate) relative_altitude_m: Option<f32>,
    pub(crate) ground_speed_mps: Option<f32>,
    pub(crate) air_speed_mps: Option<f32>,
    pub(crate) vertical_speed_mps: Option<f32>,
    pub(crate) heading_deg: Option<f32>,
    pub(crate) satellites: Option<u8>,
    pub(crate) gps_fix: Option<u8>,
    pub(crate) throttle_pct: Option<u8>,
    pub(crate) roll_deg: Option<f32>,
    pub(crate) pitch_deg: Option<f32>,
    pub(crate) yaw_deg: Option<f32>,
    pub(crate) home_distance_m: Option<f32>,
    pub(crate) rc_link_quality_pct: Option<u8>,
    pub(crate) last_update: Option<Instant>,
    pub(crate) last_frame: Option<Instant>,
    home_position: Option<(f64, f64)>,
}

impl TelemetryState {
    pub(crate) fn apply(&mut self, update: TelemetryUpdate) {
        if update.is_empty() {
            return;
        }
        self.messages = self.messages.saturating_add(update.messages);
        self.counters.merge(update.counters);
        replace_some(&mut self.protocol, update.protocol);
        replace_some(&mut self.mavlink_version, update.mavlink_version);
        replace_some(&mut self.mavlink_system_id, update.mavlink_system_id);
        replace_some(&mut self.mavlink_component_id, update.mavlink_component_id);
        if update.mavlink_last_signed == Some(false) {
            self.mavlink_signing_link_id = None;
        }
        replace_some(&mut self.mavlink_last_signed, update.mavlink_last_signed);
        replace_some(
            &mut self.mavlink_signing_link_id,
            update.mavlink_signing_link_id,
        );
        replace_some(&mut self.armed, update.armed);
        replace_some(&mut self.flight_mode, update.flight_mode);
        replace_some(&mut self.status_text, update.status_text);
        replace_some(&mut self.battery_voltage_v, update.battery_voltage_v);
        replace_some(&mut self.battery_current_a, update.battery_current_a);
        replace_some(&mut self.battery_consumed_mah, update.battery_consumed_mah);
        replace_some(
            &mut self.battery_remaining_pct,
            update.battery_remaining_pct,
        );
        replace_some(&mut self.latitude_deg, update.latitude_deg);
        replace_some(&mut self.longitude_deg, update.longitude_deg);
        replace_some(&mut self.altitude_m, update.altitude_m);
        replace_some(&mut self.relative_altitude_m, update.relative_altitude_m);
        replace_some(&mut self.ground_speed_mps, update.ground_speed_mps);
        replace_some(&mut self.air_speed_mps, update.air_speed_mps);
        replace_some(&mut self.vertical_speed_mps, update.vertical_speed_mps);
        replace_some(&mut self.heading_deg, update.heading_deg);
        replace_some(&mut self.satellites, update.satellites);
        replace_some(&mut self.gps_fix, update.gps_fix);
        replace_some(&mut self.throttle_pct, update.throttle_pct);
        replace_some(&mut self.roll_deg, update.roll_deg);
        replace_some(&mut self.pitch_deg, update.pitch_deg);
        replace_some(&mut self.yaw_deg, update.yaw_deg);
        replace_some(&mut self.home_distance_m, update.home_distance_m);
        replace_some(&mut self.rc_link_quality_pct, update.rc_link_quality_pct);

        if self.armed == Some(false) {
            self.home_position = None;
            if update.home_distance_m.is_none() {
                self.home_distance_m = None;
            }
        } else if self.armed == Some(true)
            && self.home_position.is_none()
            && self.position_has_fix()
        {
            self.home_position = self.position();
        }
        if update.home_distance_m.is_none() {
            if let (Some(home), Some(position)) = (self.home_position, self.position()) {
                self.home_distance_m = Some(distance_meters(home, position));
            }
        }
        let now = Instant::now();
        self.last_frame = Some(now);
        if update.messages > 0 {
            self.last_update = Some(now);
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn age_seconds(&self) -> Option<f32> {
        self.last_update
            .map(|updated| updated.elapsed().as_secs_f32())
    }

    pub(crate) fn frame_age_seconds(&self) -> Option<f32> {
        self.last_frame
            .map(|updated| updated.elapsed().as_secs_f32())
    }

    pub(crate) fn is_fresh(&self, stale_timeout_ms: u32) -> bool {
        self.age_seconds()
            .is_some_and(|age| age <= stale_timeout_ms as f32 / 1_000.0)
    }

    fn position(&self) -> Option<(f64, f64)> {
        Some((self.latitude_deg?, self.longitude_deg?))
    }

    fn position_has_fix(&self) -> bool {
        self.gps_fix.unwrap_or(0) >= 2 && self.position().is_some()
    }
}

/// Stateful decoder owned by one Telemetry-to-OSD payload route.
pub(crate) struct TelemetryDecoder {
    detected: Option<TelemetryProtocol>,
    mavlink: mavlink::Parser,
    msp: msp::Parser,
    crsf: crsf::Parser,
}

impl TelemetryDecoder {
    pub(crate) fn new(configured: TelemetryProtocol, settings: &TelemetrySettings) -> Self {
        Self {
            detected: (configured != TelemetryProtocol::Auto).then_some(configured),
            mavlink: mavlink::Parser::new(settings),
            msp: msp::Parser::new(settings.msp_version, settings.msp_direction),
            crsf: crsf::Parser::new(settings.crsf_address),
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> TelemetryUpdate {
        if let Some(protocol) = self.detected {
            return self.push_protocol(protocol, bytes);
        }

        for protocol in [
            TelemetryProtocol::Mavlink,
            TelemetryProtocol::Msp,
            TelemetryProtocol::Crsf,
        ] {
            let update = self.push_protocol(protocol, bytes);
            if update.has_protocol_evidence() {
                self.detected = Some(protocol);
                let mut update = update;
                update.protocol = Some(protocol);
                return update;
            }
        }
        TelemetryUpdate::default()
    }

    fn push_protocol(&mut self, protocol: TelemetryProtocol, bytes: &[u8]) -> TelemetryUpdate {
        let mut update = match protocol {
            TelemetryProtocol::Mavlink => self.mavlink.push(bytes),
            TelemetryProtocol::Msp => self.msp.push(bytes),
            TelemetryProtocol::Crsf => self.crsf.push(bytes),
            TelemetryProtocol::Auto => TelemetryUpdate::default(),
        };
        if update.has_protocol_evidence() {
            update.protocol = Some(protocol);
        }
        update
    }
}

fn distance_meters(a: (f64, f64), b: (f64, f64)) -> f32 {
    let latitude_a = a.0.to_radians();
    let latitude_b = b.0.to_radians();
    let latitude_delta = (b.0 - a.0).to_radians();
    let longitude_delta = (b.1 - a.1).to_radians();
    let haversine = (latitude_delta * 0.5).sin().powi(2)
        + latitude_a.cos() * latitude_b.cos() * (longitude_delta * 0.5).sin().powi(2);
    (6_371_000.0 * 2.0 * haversine.sqrt().atan2((1.0 - haversine).sqrt())) as f32
}

pub(super) fn crc8_dvb_s2(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |mut crc, byte| {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0xd5
            } else {
                crc << 1
            };
        }
        crc
    })
}

#[cfg(test)]
mod tests {
    use super::{
        crc8_dvb_s2, distance_meters, TelemetryDecoder, TelemetryProtocol, TelemetrySettings,
        TelemetryState, TelemetryUpdate,
    };

    #[test]
    fn state_sets_home_on_arm_and_tracks_distance() {
        let mut state = TelemetryState::default();
        state.apply(TelemetryUpdate {
            messages: 1,
            armed: Some(true),
            ..TelemetryUpdate::default()
        });
        state.apply(TelemetryUpdate {
            messages: 1,
            gps_fix: Some(3),
            latitude_deg: Some(41.0),
            longitude_deg: Some(-87.0),
            ..TelemetryUpdate::default()
        });
        state.apply(TelemetryUpdate {
            messages: 1,
            latitude_deg: Some(41.0001),
            longitude_deg: Some(-87.0),
            ..TelemetryUpdate::default()
        });
        assert!(state
            .home_distance_m
            .is_some_and(|distance| distance > 10.0));
    }

    #[test]
    fn haversine_distance_is_zero_for_same_point() {
        assert_eq!(distance_meters((41.0, -87.0), (41.0, -87.0)), 0.0);
    }

    #[test]
    fn auto_detection_locks_only_after_a_valid_telemetry_frame() {
        let payload = [0x00, 0x7b, 0x00, 0x2d, 0x00, 0x04, 0xd2, 81];
        let mut frame = vec![0xc8, (payload.len() + 2) as u8, 0x08];
        frame.extend_from_slice(&payload);
        frame.push(crc8_dvb_s2(&frame[2..]));

        let mut decoder =
            TelemetryDecoder::new(TelemetryProtocol::Auto, &TelemetrySettings::default());
        assert!(decoder.push(&[0xc8, 0x0a, 0x08, 0xff]).is_empty());
        assert!(decoder.push(&frame[..4]).is_empty());
        let update = decoder.push(&frame[4..]);

        assert_eq!(update.protocol, Some(TelemetryProtocol::Crsf));
        assert_eq!(update.messages, 1);
        assert_eq!(update.battery_voltage_v, Some(12.3));
        assert_eq!(update.battery_current_a, Some(4.5));
        assert_eq!(update.battery_consumed_mah, Some(1234));
        assert_eq!(update.battery_remaining_pct, Some(81));
    }
}
