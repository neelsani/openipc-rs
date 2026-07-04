use std::{collections::BTreeMap, io::Cursor};

use mavlink::{
    dialects::common::{self, MavMessage},
    peek_reader::PeekReader,
    read_v1_raw_message, read_v2_raw_message, MAVLinkV2MessageRaw, MavlinkVersion, Message,
};
use web_time::SystemTime;

use super::{MavlinkSigningPolicy, TelemetrySettings, TelemetryUpdate};

const MAX_BUFFER: usize = 8 * 1024;
const MAX_SIGNING_STREAMS: usize = 64;
const MAVLINK_IFLAG_SIGNED: u8 = 0x01;
const MAVLINK_EPOCH_OFFSET_MICROS: u128 = 1_420_070_400 * 1_000_000;
const SIGNING_TIMESTAMP_TOLERANCE: u64 = 60 * 1000 * 100;

pub(super) struct Parser {
    buffer: Vec<u8>,
    signing: MavlinkSigningPolicy,
    signing_key: Option<[u8; 32]>,
    system_id: u8,
    component_id: u8,
    stream_timestamps: BTreeMap<(u8, u8, u8), u64>,
}

impl Parser {
    pub(super) fn new(settings: &TelemetrySettings) -> Self {
        Self {
            buffer: Vec::new(),
            signing: settings.mavlink_signing,
            signing_key: settings.mavlink_signing_key.as_slice().try_into().ok(),
            system_id: settings.mavlink_system_id,
            component_id: settings.mavlink_component_id,
            stream_timestamps: BTreeMap::new(),
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> TelemetryUpdate {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > MAX_BUFFER {
            let excess = self.buffer.len() - MAX_BUFFER;
            self.buffer.drain(..excess);
        }

        let mut combined = TelemetryUpdate::default();
        loop {
            let Some(start) = self
                .buffer
                .iter()
                .position(|byte| matches!(*byte, mavlink::MAV_STX | mavlink::MAV_STX_V2))
            else {
                self.buffer.clear();
                break;
            };
            if start > 0 {
                self.buffer.drain(..start);
            }

            let version_two = self.buffer[0] == mavlink::MAV_STX_V2;
            let header_len = if version_two { 10 } else { 6 };
            if self.buffer.len() < header_len {
                break;
            }
            let payload_len = usize::from(self.buffer[1]);
            let signed = version_two && self.buffer[2] & 0x01 != 0;
            let frame_len = header_len + payload_len + 2 + usize::from(signed) * 13;
            if self.buffer.len() < frame_len {
                break;
            }

            if let Some(frame) = parse_frame(&self.buffer[..frame_len]) {
                self.buffer.drain(..frame_len);
                combined.merge(self.process_frame(frame));
            } else {
                // A false magic byte or damaged frame must not hide a valid frame behind it.
                self.buffer.drain(..1);
            }
        }
        combined
    }

    fn process_frame(&mut self, frame: ParsedFrame) -> TelemetryUpdate {
        let signed = frame
            .raw_v2
            .as_ref()
            .is_some_and(|raw| raw.incompatibility_flags() & MAVLINK_IFLAG_SIGNED != 0);
        let mut update = TelemetryUpdate {
            mavlink_version: Some(frame.version),
            mavlink_system_id: Some(frame.system_id),
            mavlink_component_id: Some(frame.component_id),
            mavlink_last_signed: Some(signed),
            mavlink_signing_link_id: frame
                .raw_v2
                .as_ref()
                .filter(|raw| raw.incompatibility_flags() & MAVLINK_IFLAG_SIGNED != 0)
                .map(MAVLinkV2MessageRaw::signature_link_id),
            ..TelemetryUpdate::default()
        };
        if frame.version == 1 {
            update.counters.mavlink_v1_frames = 1;
        } else {
            update.counters.mavlink_v2_frames = 1;
        }

        if signed {
            update.counters.mavlink_signed_frames = 1;
        } else {
            update.counters.mavlink_unsigned_frames = 1;
        }

        if (self.system_id != 0 && self.system_id != frame.system_id)
            || (self.component_id != 0 && self.component_id != frame.component_id)
        {
            update.counters.filtered_frames = 1;
            return update;
        }

        match self.security_decision(&frame, signed) {
            SecurityDecision::Accept { verified } => {
                update.counters.accepted_frames = 1;
                update.counters.mavlink_verified_frames = u64::from(verified);
                if let Some(decoded) = decode_message(frame.message) {
                    update.merge(decoded);
                }
            }
            SecurityDecision::Reject(reason) => {
                update.counters.rejected_frames = 1;
                match reason {
                    SecurityRejection::MissingKey => update.counters.mavlink_missing_key_drops = 1,
                    SecurityRejection::InvalidSignature => {
                        update.counters.mavlink_invalid_signatures = 1
                    }
                    SecurityRejection::Replay => update.counters.mavlink_replay_drops = 1,
                    SecurityRejection::StaleTimestamp => {
                        update.counters.mavlink_stale_timestamp_drops = 1
                    }
                    SecurityRejection::Unsigned => {}
                }
            }
        }
        update
    }

    fn security_decision(&mut self, frame: &ParsedFrame, signed: bool) -> SecurityDecision {
        if self.signing == MavlinkSigningPolicy::Disabled {
            return SecurityDecision::Accept { verified: false };
        }
        if !signed {
            return if self.signing == MavlinkSigningPolicy::VerifySigned {
                SecurityDecision::Accept { verified: false }
            } else {
                SecurityDecision::Reject(SecurityRejection::Unsigned)
            };
        }
        let Some(key) = self.signing_key.as_ref() else {
            return SecurityDecision::Reject(SecurityRejection::MissingKey);
        };
        let Some(raw) = frame.raw_v2.as_ref() else {
            return SecurityDecision::Reject(SecurityRejection::InvalidSignature);
        };

        let mut expected = [0u8; 6];
        raw.calculate_signature(key, &mut expected);
        if expected.as_slice() != raw.signature_value() {
            return SecurityDecision::Reject(SecurityRejection::InvalidSignature);
        }

        let timestamp = raw.signature_timestamp();
        let stream = (raw.signature_link_id(), frame.system_id, frame.component_id);
        if self
            .stream_timestamps
            .get(&stream)
            .is_some_and(|previous| timestamp <= *previous)
        {
            return SecurityDecision::Reject(SecurityRejection::Replay);
        }
        if !self.stream_timestamps.contains_key(&stream)
            && timestamp.saturating_add(SIGNING_TIMESTAMP_TOLERANCE) < current_mavlink_timestamp()
        {
            return SecurityDecision::Reject(SecurityRejection::StaleTimestamp);
        }
        if self.stream_timestamps.len() >= MAX_SIGNING_STREAMS
            && !self.stream_timestamps.contains_key(&stream)
        {
            self.stream_timestamps.pop_first();
        }
        self.stream_timestamps.insert(stream, timestamp);
        SecurityDecision::Accept { verified: true }
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new(&TelemetrySettings::default())
    }
}

struct ParsedFrame {
    message: MavMessage,
    version: u8,
    system_id: u8,
    component_id: u8,
    raw_v2: Option<MAVLinkV2MessageRaw>,
}

enum SecurityDecision {
    Accept { verified: bool },
    Reject(SecurityRejection),
}

enum SecurityRejection {
    MissingKey,
    InvalidSignature,
    Replay,
    StaleTimestamp,
    Unsigned,
}

fn parse_frame(frame: &[u8]) -> Option<ParsedFrame> {
    if frame.first().copied()? == mavlink::MAV_STX_V2 {
        let mut reader = PeekReader::new(Cursor::new(frame));
        let raw = read_v2_raw_message::<MavMessage, _>(&mut reader).ok()?;
        let message =
            MavMessage::parse(MavlinkVersion::V2, raw.message_id(), raw.payload()).ok()?;
        return Some(ParsedFrame {
            message,
            version: 2,
            system_id: raw.system_id(),
            component_id: raw.component_id(),
            raw_v2: Some(raw),
        });
    }
    if frame.first().copied()? != mavlink::MAV_STX {
        return None;
    }
    let mut reader = PeekReader::new(Cursor::new(frame));
    let raw = read_v1_raw_message::<MavMessage, _>(&mut reader).ok()?;
    Some(ParsedFrame {
        message: MavMessage::parse(
            MavlinkVersion::V1,
            u32::from(raw.message_id()),
            raw.payload(),
        )
        .ok()?,
        version: 1,
        system_id: raw.system_id(),
        component_id: raw.component_id(),
        raw_v2: None,
    })
}

fn current_mavlink_timestamp() -> u64 {
    let micros = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or(0);
    (micros.saturating_sub(MAVLINK_EPOCH_OFFSET_MICROS) / 10) as u64
}

fn decode_message(message: MavMessage) -> Option<TelemetryUpdate> {
    let mut update = TelemetryUpdate {
        messages: 1,
        ..TelemetryUpdate::default()
    };
    match message {
        MavMessage::HEARTBEAT(data) => {
            update.armed = Some(
                data.base_mode
                    .contains(common::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED),
            );
            update.flight_mode = Some(flight_mode(data.autopilot, data.mavtype, data.custom_mode));
        }
        MavMessage::SYS_STATUS(data) => {
            if !matches!(data.voltage_battery, 0 | u16::MAX) {
                update.battery_voltage_v = Some(f32::from(data.voltage_battery) / 1_000.0);
            }
            if data.current_battery >= 0 {
                update.battery_current_a = Some(f32::from(data.current_battery) / 100.0);
            }
            if data.battery_remaining >= 0 {
                update.battery_remaining_pct = Some(data.battery_remaining as u8);
            }
        }
        MavMessage::GPS_RAW_INT(data) => {
            update.latitude_deg = coordinate(data.lat);
            update.longitude_deg = coordinate(data.lon);
            update.altitude_m = Some(data.alt as f32 / 1_000.0);
            if data.vel != u16::MAX {
                update.ground_speed_mps = Some(f32::from(data.vel) / 100.0);
            }
            if data.cog != u16::MAX {
                update.heading_deg = Some(f32::from(data.cog) / 100.0);
            }
            update.gps_fix = Some(data.fix_type as u8);
            if data.satellites_visible != u8::MAX {
                update.satellites = Some(data.satellites_visible);
            }
        }
        MavMessage::ATTITUDE(data) => {
            update.roll_deg = Some(data.roll.to_degrees());
            update.pitch_deg = Some(data.pitch.to_degrees());
            update.yaw_deg = Some(data.yaw.to_degrees());
        }
        MavMessage::GLOBAL_POSITION_INT(data) => {
            update.latitude_deg = coordinate(data.lat);
            update.longitude_deg = coordinate(data.lon);
            update.altitude_m = Some(data.alt as f32 / 1_000.0);
            update.relative_altitude_m = Some(data.relative_alt as f32 / 1_000.0);
            let north = f32::from(data.vx) / 100.0;
            let east = f32::from(data.vy) / 100.0;
            update.ground_speed_mps = Some(north.hypot(east));
            update.vertical_speed_mps = Some(-f32::from(data.vz) / 100.0);
            if data.hdg != u16::MAX {
                update.heading_deg = Some(f32::from(data.hdg) / 100.0);
            }
        }
        MavMessage::RC_CHANNELS_RAW(data) => {
            update.rc_link_quality_pct = mavlink_rssi(data.rssi);
        }
        MavMessage::RC_CHANNELS(data) => {
            update.rc_link_quality_pct = mavlink_rssi(data.rssi);
        }
        MavMessage::VFR_HUD(data) => {
            update.air_speed_mps = Some(data.airspeed);
            update.ground_speed_mps = Some(data.groundspeed);
            update.altitude_m = Some(data.alt);
            update.vertical_speed_mps = Some(data.climb);
            update.heading_deg = Some(f32::from(data.heading).rem_euclid(360.0));
            update.throttle_pct = Some(data.throttle.min(100) as u8);
        }
        MavMessage::BATTERY_STATUS(data) => {
            if data.current_consumed >= 0 {
                update.battery_consumed_mah = Some(data.current_consumed as u32);
            }
            let voltage = data
                .voltages
                .into_iter()
                .take_while(|cell| !matches!(*cell, 0 | u16::MAX))
                .fold(0u32, |total, cell| total.saturating_add(u32::from(cell)));
            if voltage > 0 {
                update.battery_voltage_v = Some(voltage as f32 / 1_000.0);
            }
            if data.current_battery >= 0 {
                update.battery_current_a = Some(f32::from(data.current_battery) / 100.0);
            }
            if data.battery_remaining >= 0 {
                update.battery_remaining_pct = Some(data.battery_remaining as u8);
            }
        }
        MavMessage::STATUSTEXT(data) => {
            let text = data.text.to_str().ok()?.trim().to_owned();
            if text.is_empty() {
                return None;
            }
            update.status_text = Some(text);
        }
        _ => return None,
    }
    Some(update)
}

fn coordinate(value: i32) -> Option<f64> {
    (value != 0).then_some(f64::from(value) / 10_000_000.0)
}

fn mavlink_rssi(value: u8) -> Option<u8> {
    (value != u8::MAX).then_some(((u16::from(value) * 100) / 255) as u8)
}

fn flight_mode(
    autopilot: common::MavAutopilot,
    vehicle_type: common::MavType,
    custom_mode: u32,
) -> String {
    if autopilot == common::MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA {
        let mode = if vehicle_type == common::MavType::MAV_TYPE_FIXED_WING {
            match custom_mode {
                0 => "MANUAL",
                1 => "CIRCLE",
                2 => "STABILIZE",
                5 => "FBWA",
                6 => "FBWB",
                10 => "AUTO",
                11 => "RTL",
                12 => "LOITER",
                13 => "TAKEOFF",
                15 => "GUIDED",
                17 => "QSTABILIZE",
                18 => "QHOVER",
                19 => "QLOITER",
                20 => "QLAND",
                21 => "QRTL",
                _ => return format!("MODE {custom_mode}"),
            }
        } else {
            match custom_mode {
                0 => "STABILIZE",
                1 => "ACRO",
                2 => "ALT HOLD",
                3 => "AUTO",
                4 => "GUIDED",
                5 => "LOITER",
                6 => "RTL",
                9 => "LAND",
                13 => "SPORT",
                16 => "POSHOLD",
                17 => "BRAKE",
                21 => "SMART RTL",
                24 => "FOLLOW",
                _ => return format!("MODE {custom_mode}"),
            }
        };
        return mode.to_owned();
    }
    if autopilot == common::MavAutopilot::MAV_AUTOPILOT_PX4 {
        let main = (custom_mode >> 16) as u8;
        let sub = (custom_mode >> 24) as u8;
        return match (main, sub) {
            (1, _) => "MANUAL".to_owned(),
            (2, _) => "ALTCTL".to_owned(),
            (3, _) => "POSCTL".to_owned(),
            (4, 4) => "AUTO RTL".to_owned(),
            (4, 3) => "AUTO LOITER".to_owned(),
            (4, 2) => "AUTO MISSION".to_owned(),
            (4, 1) => "AUTO READY".to_owned(),
            (4, 5) => "AUTO LAND".to_owned(),
            (4, 6) => "AUTO TAKEOFF".to_owned(),
            (5, _) => "ACRO".to_owned(),
            (6, _) => "OFFBOARD".to_owned(),
            (7, _) => "STABILIZED".to_owned(),
            _ => format!("MODE {custom_mode}"),
        };
    }
    format!("MODE {custom_mode}")
}

#[cfg(test)]
mod tests {
    use mavlink::{
        dialects::common::{
            self, MavMessage, BATTERY_STATUS_DATA, HEARTBEAT_DATA, SYS_STATUS_DATA, VFR_HUD_DATA,
        },
        MavHeader, SigningConfig, SigningData,
    };

    use super::{MavlinkSigningPolicy, Parser, TelemetrySettings};

    const HEADER: MavHeader = MavHeader {
        system_id: 1,
        component_id: 1,
        sequence: 7,
    };

    fn v1_frame(message: MavMessage) -> Vec<u8> {
        let mut frame = Vec::new();
        mavlink::write_v1_msg(&mut frame, HEADER, &message).expect("serialize MAVLink 1 frame");
        frame
    }

    fn v2_frame(message: MavMessage) -> Vec<u8> {
        let mut frame = Vec::new();
        mavlink::write_v2_msg(&mut frame, HEADER, &message).expect("serialize MAVLink 2 frame");
        frame
    }

    fn signed_v2_frame(key: [u8; 32], message: MavMessage) -> Vec<u8> {
        let signing = SigningData::from_config(SigningConfig::new(key, 7, true, false));
        let mut frame = Vec::new();
        mavlink::write_v2_msg_signed(&mut frame, HEADER, &message, Some(&signing))
            .expect("serialize signed MAVLink 2 frame");
        frame
    }

    fn heartbeat() -> MavMessage {
        MavMessage::HEARTBEAT(HEARTBEAT_DATA {
            custom_mode: 6,
            mavtype: common::MavType::MAV_TYPE_QUADROTOR,
            autopilot: common::MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
            base_mode: common::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED,
            ..HEARTBEAT_DATA::default()
        })
    }

    #[test]
    fn decodes_split_heartbeat_and_rejects_bad_crc() {
        let frame = v1_frame(heartbeat());
        let mut parser = Parser::default();
        assert!(parser.push(&frame[..4]).is_empty());
        let update = parser.push(&frame[4..]);
        assert_eq!(update.armed, Some(true));
        assert_eq!(update.flight_mode.as_deref(), Some("RTL"));

        let mut corrupt = frame.clone();
        *corrupt.last_mut().unwrap() ^= 0xff;
        assert!(Parser::default().push(&corrupt).is_empty());

        corrupt.extend_from_slice(&frame);
        let recovered = Parser::default().push(&corrupt);
        assert_eq!(recovered.armed, Some(true));
        assert_eq!(recovered.flight_mode.as_deref(), Some("RTL"));
    }

    #[test]
    fn decodes_system_battery_units() {
        let update = Parser::default().push(&v1_frame(MavMessage::SYS_STATUS(SYS_STATUS_DATA {
            voltage_battery: 16_800,
            current_battery: 2_350,
            battery_remaining: 72,
            ..SYS_STATUS_DATA::default()
        })));
        assert_eq!(update.battery_voltage_v, Some(16.8));
        assert_eq!(update.battery_current_a, Some(23.5));
        assert_eq!(update.battery_remaining_pct, Some(72));
    }

    #[test]
    fn decodes_vfr_hud_from_generated_definition() {
        let update = Parser::default().push(&v1_frame(MavMessage::VFR_HUD(VFR_HUD_DATA {
            airspeed: 22.5,
            groundspeed: 18.25,
            alt: 123.75,
            climb: -1.5,
            heading: 275,
            throttle: 63,
        })));
        assert_eq!(update.air_speed_mps, Some(22.5));
        assert_eq!(update.ground_speed_mps, Some(18.25));
        assert_eq!(update.altitude_m, Some(123.75));
        assert_eq!(update.vertical_speed_mps, Some(-1.5));
        assert_eq!(update.heading_deg, Some(275.0));
        assert_eq!(update.throttle_pct, Some(63));
    }

    #[test]
    fn generated_decoder_pads_truncated_mavlink_two_payloads() {
        let frame = v2_frame(MavMessage::VFR_HUD(VFR_HUD_DATA {
            airspeed: 22.5,
            groundspeed: 18.25,
            alt: 123.75,
            ..VFR_HUD_DATA::default()
        }));
        assert!(usize::from(frame[1]) < VFR_HUD_DATA::ENCODED_LEN);

        let update = Parser::default().push(&frame);
        assert_eq!(update.altitude_m, Some(123.75));
        assert_eq!(update.vertical_speed_mps, Some(0.0));
        assert_eq!(update.heading_deg, Some(0.0));
        assert_eq!(update.throttle_pct, Some(0));
    }

    #[test]
    fn decodes_battery_status_and_status_text() {
        let mut voltages = [u16::MAX; 10];
        voltages[0] = 16_800;
        let mut frames = v2_frame(MavMessage::BATTERY_STATUS(BATTERY_STATUS_DATA {
            current_consumed: 1_234,
            voltages,
            current_battery: 2_350,
            battery_remaining: 72,
            ..BATTERY_STATUS_DATA::default()
        }));
        frames.extend_from_slice(&v2_frame(MavMessage::STATUSTEXT(common::STATUSTEXT_DATA {
            text: "GPS home acquired".into(),
            ..common::STATUSTEXT_DATA::default()
        })));

        let update = Parser::default().push(&frames);
        assert_eq!(update.battery_consumed_mah, Some(1_234));
        assert_eq!(update.battery_voltage_v, Some(16.8));
        assert_eq!(update.status_text.as_deref(), Some("GPS home acquired"));
        assert_eq!(update.messages, 2);
    }

    #[test]
    fn verifies_signed_frames_and_rejects_replays() {
        let key = [0x5a; 32];
        let settings = TelemetrySettings {
            mavlink_signing: MavlinkSigningPolicy::RequireSigned,
            mavlink_signing_key: key.to_vec(),
            ..TelemetrySettings::default()
        };
        let mut parser = Parser::new(&settings);
        let frame = signed_v2_frame(key, heartbeat());

        let accepted = parser.push(&frame);
        assert_eq!(accepted.messages, 1);
        assert_eq!(accepted.counters.mavlink_signed_frames, 1);
        assert_eq!(accepted.counters.mavlink_verified_frames, 1);
        assert_eq!(accepted.counters.accepted_frames, 1);

        let replay = parser.push(&frame);
        assert_eq!(replay.messages, 0);
        assert_eq!(replay.counters.rejected_frames, 1);
        assert_eq!(replay.counters.mavlink_replay_drops, 1);
    }

    #[test]
    fn signing_policy_rejects_invalid_and_unsigned_frames() {
        let key = [0x5a; 32];
        let settings = TelemetrySettings {
            mavlink_signing: MavlinkSigningPolicy::RequireSigned,
            mavlink_signing_key: key.to_vec(),
            ..TelemetrySettings::default()
        };
        let mut damaged = signed_v2_frame(key, heartbeat());
        *damaged.last_mut().unwrap() ^= 0x01;
        let invalid = Parser::new(&settings).push(&damaged);
        assert_eq!(invalid.messages, 0);
        assert_eq!(invalid.counters.mavlink_invalid_signatures, 1);

        let unsigned = Parser::new(&settings).push(&v1_frame(heartbeat()));
        assert_eq!(unsigned.messages, 0);
        assert_eq!(unsigned.counters.rejected_frames, 1);
        assert_eq!(unsigned.counters.mavlink_unsigned_frames, 1);
    }

    #[test]
    fn verify_signed_policy_accepts_unsigned_and_filters_sources() {
        let settings = TelemetrySettings {
            mavlink_signing: MavlinkSigningPolicy::VerifySigned,
            mavlink_signing_key: vec![0x5a; 32],
            mavlink_system_id: 2,
            ..TelemetrySettings::default()
        };
        let filtered = Parser::new(&settings).push(&v1_frame(heartbeat()));
        assert_eq!(filtered.messages, 0);
        assert_eq!(filtered.counters.filtered_frames, 1);

        let settings = TelemetrySettings {
            mavlink_signing: MavlinkSigningPolicy::VerifySigned,
            mavlink_signing_key: vec![0x5a; 32],
            ..TelemetrySettings::default()
        };
        let accepted = Parser::new(&settings).push(&v1_frame(heartbeat()));
        assert_eq!(accepted.messages, 1);
        assert_eq!(accepted.counters.mavlink_unsigned_frames, 1);
        assert_eq!(accepted.counters.mavlink_verified_frames, 0);
    }
}
