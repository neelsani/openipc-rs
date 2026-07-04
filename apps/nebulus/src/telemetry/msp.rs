use super::{crc8_dvb_s2, MspDirectionFilter, MspVersionFilter, TelemetryUpdate};

const MAX_BUFFER: usize = 8 * 1024;
const MSP_RAW_GPS: u16 = 106;
const MSP_COMP_GPS: u16 = 107;
const MSP_ATTITUDE: u16 = 108;
const MSP_ALTITUDE: u16 = 109;
const MSP_ANALOG: u16 = 110;
const MSP_BATTERY_STATE: u16 = 130;

pub(super) struct Parser {
    buffer: Vec<u8>,
    version: MspVersionFilter,
    direction: MspDirectionFilter,
}

impl Parser {
    pub(super) const fn new(version: MspVersionFilter, direction: MspDirectionFilter) -> Self {
        Self {
            buffer: Vec::new(),
            version,
            direction,
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
            let Some(start) = self.buffer.iter().position(|byte| *byte == b'$') else {
                self.buffer.clear();
                break;
            };
            if start > 0 {
                self.buffer.drain(..start);
            }
            if self.buffer.len() < 3 {
                break;
            }
            let parsed = match self.buffer[1] {
                b'M' => self.parse_v1(),
                b'X' => self.parse_v2(),
                _ => {
                    self.buffer.drain(..1);
                    continue;
                }
            };
            match parsed {
                FrameResult::NeedMore => break,
                FrameResult::Invalid(consumed) => {
                    self.buffer.drain(..consumed.min(self.buffer.len()));
                }
                FrameResult::Valid {
                    consumed,
                    version,
                    direction,
                    command,
                    payload,
                } => {
                    self.buffer.drain(..consumed);
                    if !self.accepts(version, direction) {
                        combined.counters.filtered_frames += 1;
                        continue;
                    }
                    combined.counters.accepted_frames += 1;
                    if let Some(update) = decode_message(command, &payload) {
                        combined.merge(update);
                    }
                }
            }
        }
        combined
    }

    fn accepts(&self, version: MspVersionFilter, direction: u8) -> bool {
        let version_matches =
            matches!(self.version, MspVersionFilter::Any) || self.version == version;
        let direction_matches = match self.direction {
            MspDirectionFilter::Any => true,
            MspDirectionFilter::FromFlightController => matches!(direction, b'>' | b'!'),
            MspDirectionFilter::ToFlightController => direction == b'<',
        };
        version_matches && direction_matches
    }

    fn parse_v1(&self) -> FrameResult {
        if self.buffer.len() < 6 {
            return FrameResult::NeedMore;
        }
        if !matches!(self.buffer[2], b'>' | b'<' | b'!') {
            return FrameResult::Invalid(1);
        }
        let payload_len = usize::from(self.buffer[3]);
        let total = 6 + payload_len;
        if self.buffer.len() < total {
            return FrameResult::NeedMore;
        }
        let checksum = self.buffer[3..5 + payload_len]
            .iter()
            .fold(0, |checksum, byte| checksum ^ byte);
        if checksum != self.buffer[5 + payload_len] {
            return FrameResult::Invalid(total);
        }
        FrameResult::Valid {
            consumed: total,
            version: MspVersionFilter::V1,
            direction: self.buffer[2],
            command: u16::from(self.buffer[4]),
            payload: self.buffer[5..5 + payload_len].to_vec(),
        }
    }

    fn parse_v2(&self) -> FrameResult {
        if self.buffer.len() < 9 {
            return FrameResult::NeedMore;
        }
        if !matches!(self.buffer[2], b'>' | b'<' | b'!') {
            return FrameResult::Invalid(1);
        }
        let payload_len = usize::from(u16::from_le_bytes([self.buffer[6], self.buffer[7]]));
        let total = 9 + payload_len;
        if self.buffer.len() < total {
            return FrameResult::NeedMore;
        }
        if crc8_dvb_s2(&self.buffer[3..8 + payload_len]) != self.buffer[8 + payload_len] {
            return FrameResult::Invalid(total);
        }
        FrameResult::Valid {
            consumed: total,
            version: MspVersionFilter::V2,
            direction: self.buffer[2],
            command: u16::from_le_bytes([self.buffer[4], self.buffer[5]]),
            payload: self.buffer[8..8 + payload_len].to_vec(),
        }
    }
}

enum FrameResult {
    NeedMore,
    Invalid(usize),
    Valid {
        consumed: usize,
        version: MspVersionFilter,
        direction: u8,
        command: u16,
        payload: Vec<u8>,
    },
}

fn decode_message(command: u16, payload: &[u8]) -> Option<TelemetryUpdate> {
    let mut update = TelemetryUpdate {
        messages: 1,
        ..TelemetryUpdate::default()
    };
    match command {
        MSP_RAW_GPS if payload.len() >= 16 => {
            update.gps_fix = Some(payload[0]);
            update.satellites = Some(payload[1]);
            update.latitude_deg = coordinate(le_i32(payload, 2)?);
            update.longitude_deg = coordinate(le_i32(payload, 6)?);
            update.altitude_m = Some(f32::from(le_u16(payload, 10)?));
            update.ground_speed_mps = Some(f32::from(le_u16(payload, 12)?) / 100.0);
            update.heading_deg = Some(f32::from(le_u16(payload, 14)?) / 10.0);
        }
        MSP_COMP_GPS if payload.len() >= 4 => {
            update.home_distance_m = Some(f32::from(le_u16(payload, 0)?));
        }
        MSP_ATTITUDE if payload.len() >= 6 => {
            update.roll_deg = Some(f32::from(le_i16(payload, 0)?) / 10.0);
            update.pitch_deg = Some(f32::from(le_i16(payload, 2)?) / 10.0);
            update.yaw_deg = Some(f32::from(le_i16(payload, 4)?));
        }
        MSP_ALTITUDE if payload.len() >= 6 => {
            update.relative_altitude_m = Some(le_i32(payload, 0)? as f32 / 100.0);
            update.vertical_speed_mps = Some(f32::from(le_i16(payload, 4)?) / 100.0);
        }
        MSP_ANALOG if payload.len() >= 7 => {
            update.battery_voltage_v = Some(f32::from(payload[0]) / 10.0);
            update.battery_consumed_mah = Some(u32::from(le_u16(payload, 1)?));
            update.rc_link_quality_pct =
                Some(((u32::from(le_u16(payload, 3)?) * 100) / 1_023).min(100) as u8);
            update.battery_current_a = Some(f32::from(le_i16(payload, 5)?) / 100.0);
            if payload.len() >= 9 {
                let precise_voltage = le_u16(payload, 7)?;
                if precise_voltage > 0 {
                    update.battery_voltage_v = Some(f32::from(precise_voltage) / 100.0);
                }
            }
        }
        MSP_BATTERY_STATE if payload.len() >= 8 => {
            update.battery_voltage_v = Some(f32::from(payload[3]) / 10.0);
            update.battery_consumed_mah = Some(u32::from(le_u16(payload, 4)?));
            update.battery_current_a = Some(f32::from(le_u16(payload, 6)?) / 100.0);
            if payload.len() >= 11 {
                let precise_voltage = le_u16(payload, 9)?;
                if precise_voltage > 0 {
                    update.battery_voltage_v = Some(f32::from(precise_voltage) / 100.0);
                }
            }
        }
        _ => return None,
    }
    Some(update)
}

fn coordinate(value: i32) -> Option<f64> {
    (value != 0).then_some(f64::from(value) / 10_000_000.0)
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn le_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn le_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{MspDirectionFilter, MspVersionFilter, Parser, MSP_ALTITUDE, MSP_ANALOG};

    fn parser() -> Parser {
        Parser::new(MspVersionFilter::Any, MspDirectionFilter::Any)
    }

    fn v1_frame(command: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![b'$', b'M', b'>', payload.len() as u8, command];
        frame.extend_from_slice(payload);
        let checksum = frame[3..].iter().fold(0, |checksum, byte| checksum ^ byte);
        frame.push(checksum);
        frame
    }

    #[test]
    fn decodes_msp_analog_and_altitude() {
        let analog = [168, 0x7b, 0x00, 0xff, 0x03, 0x2e, 0x09];
        let mut altitude = Vec::new();
        altitude.extend_from_slice(&12_345i32.to_le_bytes());
        altitude.extend_from_slice(&(-125i16).to_le_bytes());
        let mut bytes = v1_frame(MSP_ANALOG as u8, &analog);
        bytes.extend_from_slice(&v1_frame(MSP_ALTITUDE as u8, &altitude));

        let update = parser().push(&bytes);
        assert_eq!(update.messages, 2);
        assert_eq!(update.battery_voltage_v, Some(16.8));
        assert_eq!(update.battery_current_a, Some(23.5));
        assert_eq!(update.relative_altitude_m, Some(123.45));
        assert_eq!(update.vertical_speed_mps, Some(-1.25));
    }

    #[test]
    fn filters_version_and_direction_after_checksum_validation() {
        let frame = v1_frame(MSP_ANALOG as u8, &[168, 0, 0, 0, 0, 0, 0]);
        let mut parser = Parser::new(
            MspVersionFilter::V2,
            MspDirectionFilter::FromFlightController,
        );
        let update = parser.push(&frame);
        assert_eq!(update.messages, 0);
        assert_eq!(update.counters.filtered_frames, 1);
    }
}
