use super::{crc8_dvb_s2, TelemetryUpdate, CRSF_ANY_ADDRESS};

const MAX_BUFFER: usize = 4 * 1024;
const MAX_FRAME_SIZE: usize = 64;
const TYPE_GPS: u8 = 0x02;
const TYPE_VARIO: u8 = 0x07;
const TYPE_BATTERY: u8 = 0x08;
const TYPE_LINK_STATISTICS: u8 = 0x14;
const TYPE_ATTITUDE: u8 = 0x1e;
const TYPE_FLIGHT_MODE: u8 = 0x21;

pub(super) struct Parser {
    buffer: Vec<u8>,
    address: u16,
}

impl Parser {
    pub(super) const fn new(address: u16) -> Self {
        Self {
            buffer: Vec::new(),
            address,
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> TelemetryUpdate {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > MAX_BUFFER {
            let excess = self.buffer.len() - MAX_BUFFER;
            self.buffer.drain(..excess);
        }

        let mut combined = TelemetryUpdate::default();
        while self.buffer.len() >= 2 {
            let frame_body_len = usize::from(self.buffer[1]);
            if !(2..=MAX_FRAME_SIZE - 2).contains(&frame_body_len) {
                self.buffer.drain(..1);
                continue;
            }
            let total = frame_body_len + 2;
            if self.buffer.len() < total {
                break;
            }
            let frame = self.buffer[..total].to_vec();
            if crc8_dvb_s2(&frame[2..total - 1]) != frame[total - 1] {
                self.buffer.drain(..1);
                continue;
            }
            self.buffer.drain(..total);
            if self.address != CRSF_ANY_ADDRESS && self.address != u16::from(frame[0]) {
                combined.counters.filtered_frames += 1;
                continue;
            }
            combined.counters.accepted_frames += 1;
            if let Some(update) = decode_frame(&frame) {
                combined.merge(update);
            }
        }
        combined
    }
}

fn decode_frame(frame: &[u8]) -> Option<TelemetryUpdate> {
    let frame_type = *frame.get(2)?;
    let payload = frame.get(3..frame.len().checked_sub(1)?)?;
    let mut update = TelemetryUpdate {
        messages: 1,
        ..TelemetryUpdate::default()
    };
    match frame_type {
        TYPE_GPS if payload.len() >= 15 => {
            update.latitude_deg = coordinate(be_i32(payload, 0)?);
            update.longitude_deg = coordinate(be_i32(payload, 4)?);
            let speed_kph = f32::from(be_u16(payload, 8)?) / 10.0;
            update.ground_speed_mps = Some(speed_kph / 3.6);
            update.heading_deg = Some(f32::from(be_u16(payload, 10)?) / 100.0);
            update.altitude_m = Some(f32::from(be_u16(payload, 12)?) - 1_000.0);
            update.satellites = Some(payload[14]);
            update.gps_fix = Some(if payload[14] > 0 { 3 } else { 0 });
        }
        TYPE_VARIO if payload.len() >= 2 => {
            update.vertical_speed_mps = Some(f32::from(be_i16(payload, 0)?) / 100.0);
        }
        TYPE_BATTERY if payload.len() >= 8 => {
            update.battery_voltage_v = Some(f32::from(be_u16(payload, 0)?) / 10.0);
            update.battery_current_a = Some(f32::from(be_u16(payload, 2)?) / 10.0);
            update.battery_consumed_mah = Some(
                (u32::from(payload[4]) << 16)
                    | (u32::from(payload[5]) << 8)
                    | u32::from(payload[6]),
            );
            update.battery_remaining_pct = Some(payload[7].min(100));
        }
        TYPE_LINK_STATISTICS if payload.len() >= 3 => {
            update.rc_link_quality_pct = Some(payload[2].min(100));
        }
        TYPE_ATTITUDE if payload.len() >= 6 => {
            update.pitch_deg = Some(radians_to_degrees(be_i16(payload, 0)?));
            update.roll_deg = Some(radians_to_degrees(be_i16(payload, 2)?));
            update.yaw_deg = Some(radians_to_degrees(be_i16(payload, 4)?));
        }
        TYPE_FLIGHT_MODE if !payload.is_empty() => {
            let end = payload
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(payload.len());
            let mode = String::from_utf8_lossy(&payload[..end]).trim().to_owned();
            if mode.is_empty() {
                return None;
            }
            update.flight_mode = Some(mode);
        }
        _ => return None,
    }
    Some(update)
}

fn coordinate(value: i32) -> Option<f64> {
    (value != 0).then_some(f64::from(value) / 10_000_000.0)
}

fn radians_to_degrees(value: i16) -> f32 {
    (f32::from(value) / 10_000.0).to_degrees()
}

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn be_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn be_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{crc8_dvb_s2, Parser, CRSF_ANY_ADDRESS, TYPE_BATTERY, TYPE_GPS};

    fn frame(frame_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0xc8, (payload.len() + 2) as u8, frame_type];
        frame.extend_from_slice(payload);
        frame.push(crc8_dvb_s2(&frame[2..]));
        frame
    }

    #[test]
    fn decodes_crsf_battery_and_gps() {
        let battery = [0x00, 0xa8, 0x00, 0xeb, 0x00, 0x04, 0xd2, 81];
        let mut gps = Vec::new();
        gps.extend_from_slice(&410_000_000i32.to_be_bytes());
        gps.extend_from_slice(&(-870_000_000i32).to_be_bytes());
        gps.extend_from_slice(&360u16.to_be_bytes());
        gps.extend_from_slice(&9_000u16.to_be_bytes());
        gps.extend_from_slice(&1_123u16.to_be_bytes());
        gps.push(14);
        let mut bytes = frame(TYPE_BATTERY, &battery);
        bytes.extend_from_slice(&frame(TYPE_GPS, &gps));

        let update = Parser::new(CRSF_ANY_ADDRESS).push(&bytes);
        assert_eq!(update.messages, 2);
        assert_eq!(update.battery_voltage_v, Some(16.8));
        assert_eq!(update.battery_remaining_pct, Some(81));
        assert_eq!(update.satellites, Some(14));
        assert_eq!(update.heading_deg, Some(90.0));
        assert_eq!(update.altitude_m, Some(123.0));
    }

    #[test]
    fn filters_unselected_device_addresses() {
        let frame = frame(TYPE_BATTERY, &[0x00, 0xa8, 0x00, 0xeb, 0, 0, 0, 81]);
        let update = Parser::new(0xce).push(&frame);
        assert_eq!(update.messages, 0);
        assert_eq!(update.counters.filtered_frames, 1);
    }
}
