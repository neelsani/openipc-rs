use rand_core::{OsRng, RngCore};

use crate::channel::{ChannelId, RadioPort};
use crate::radiotap::TxRadioParams;
use crate::wfb::WfbError;
use crate::wfb_tx::{WfbTransmitter, WfbTxKeypair};

const WINDOW_MS: u64 = 1_000;
const DEFAULT_FEEDBACK_INTERVAL_MS: u64 = 100;
const DEFAULT_SESSION_INTERVAL_MS: u64 = 1_000;
const DEFAULT_IDR_REQUEST_MESSAGES: u32 = 20;
const DEFAULT_VIDEO_START_IDLE_MS: u64 = 1_000;

#[derive(Debug, Clone, PartialEq)]
pub struct LinkQuality {
    pub lost_last_second: u32,
    pub recovered_last_second: u32,
    pub total_last_second: u32,
    pub rssi: [i32; 2],
    pub snr: [i32; 2],
    pub link_score: [i32; 2],
    pub idr_code: String,
}

#[derive(Debug, Clone)]
struct SignalEntry {
    at_ms: u64,
    ant1: i32,
    ant2: i32,
}

#[derive(Debug, Clone)]
struct FecEntry {
    at_ms: u64,
    total: u32,
    recovered: u32,
    lost: u32,
}

#[derive(Debug, Clone)]
struct FecController {
    enabled: bool,
    value: i32,
    last_change_ms: u64,
}

impl FecController {
    const fn new() -> Self {
        Self {
            enabled: true,
            value: 0,
            last_change_ms: 0,
        }
    }

    fn value(&mut self, now_ms: u64) -> i32 {
        if !self.enabled {
            return 0;
        }
        self.decay(now_ms);
        self.value
    }

    fn bump(&mut self, now_ms: u64, new_value: i32) {
        if new_value > self.value {
            self.value = new_value;
            self.last_change_ms = now_ms;
        }
    }

    fn decay(&mut self, now_ms: u64) {
        if self.value == 0 {
            return;
        }
        let elapsed = now_ms.saturating_sub(self.last_change_ms);
        if elapsed < WINDOW_MS {
            return;
        }
        let ticks = (elapsed / WINDOW_MS) as i32;
        self.value = (self.value - ticks).max(0);
        self.last_change_ms = self.last_change_ms.saturating_add(ticks as u64 * WINDOW_MS);
    }
}

#[derive(Debug, Clone)]
pub struct AdaptiveLink {
    rssi: Vec<SignalEntry>,
    snr: Vec<SignalEntry>,
    fec: Vec<FecEntry>,
    fec_controller: FecController,
    idr_code: Option<String>,
    idr_remaining_messages: u32,
    idr_max_messages: u32,
    last_video_activity_ms: Option<u64>,
    video_start_idle_ms: u64,
    ip_packet_id: u16,
}

impl AdaptiveLink {
    pub fn new() -> Self {
        Self {
            rssi: Vec::new(),
            snr: Vec::new(),
            fec: Vec::new(),
            fec_controller: FecController::new(),
            idr_code: None,
            idr_remaining_messages: 0,
            idr_max_messages: DEFAULT_IDR_REQUEST_MESSAGES,
            last_video_activity_ms: None,
            video_start_idle_ms: DEFAULT_VIDEO_START_IDLE_MS,
            ip_packet_id: 0,
        }
    }

    pub fn record_rx_paths(&mut self, now_ms: u64, rssi: [u8; 4], snr: [i8; 4]) {
        self.record_rx(now_ms, rssi[0], rssi[1], snr[0], snr[1]);
    }

    pub fn record_rx(&mut self, now_ms: u64, rssi0: u8, rssi1: u8, snr0: i8, snr1: i8) {
        self.rssi.push(SignalEntry {
            at_ms: now_ms,
            ant1: rssi0 as i32,
            ant2: rssi1 as i32,
        });
        self.snr.push(SignalEntry {
            at_ms: now_ms,
            ant1: snr0 as i32,
            ant2: snr1 as i32,
        });
        self.cleanup(now_ms);
    }

    pub fn record_fec(&mut self, now_ms: u64, total: u32, recovered: u32, lost: u32) {
        if total == 0 && recovered == 0 && lost == 0 {
            return;
        }
        let video_started = self.video_started_after_idle(now_ms);
        self.last_video_activity_ms = Some(now_ms);
        if video_started || lost > 0 {
            self.request_keyframe();
        }
        self.fec.push(FecEntry {
            at_ms: now_ms,
            total,
            recovered,
            lost,
        });
        self.cleanup(now_ms);
    }

    pub fn request_keyframe(&mut self) {
        if self.idr_max_messages == 0 {
            self.idr_code = None;
            self.idr_remaining_messages = 0;
            return;
        }
        self.idr_code = Some(random_idr_code());
        self.idr_remaining_messages = self.idr_max_messages;
    }

    pub fn set_keyframe_request_messages(&mut self, messages: u32) {
        self.idr_max_messages = messages;
        if self.idr_remaining_messages > messages {
            self.idr_remaining_messages = messages;
        }
        if messages == 0 {
            self.idr_code = None;
            self.idr_remaining_messages = 0;
        }
    }

    pub fn set_video_start_idle_ms(&mut self, idle_ms: u64) {
        self.video_start_idle_ms = idle_ms;
    }

    pub fn quality(&mut self, now_ms: u64) -> LinkQuality {
        self.cleanup(now_ms);
        let (avg_rssi0, avg_rssi1) = avg_signal(&self.rssi);
        let (avg_snr0, avg_snr1) = avg_signal(&self.snr);
        let (total, recovered, lost) = self.fec.iter().fold((0u32, 0u32, 0u32), |acc, entry| {
            (
                acc.0.saturating_add(entry.total),
                acc.1.saturating_add(entry.recovered),
                acc.2.saturating_add(entry.lost),
            )
        });

        let rssi = [avg_rssi0.round() as i32, avg_rssi1.round() as i32];
        let snr = [avg_snr0.round() as i32, avg_snr1.round() as i32];
        let link_score = [
            link_score(avg_rssi0, avg_snr0),
            link_score(avg_rssi1, avg_snr1),
        ];

        LinkQuality {
            lost_last_second: lost,
            recovered_last_second: recovered,
            total_last_second: total,
            rssi,
            snr,
            link_score,
            idr_code: self
                .idr_code
                .clone()
                .filter(|_| self.idr_remaining_messages > 0)
                .unwrap_or_default(),
        }
    }

    pub fn feedback_udp_payload(&mut self, now_ms: u64) -> Vec<u8> {
        let quality = self.quality(now_ms);
        if quality.lost_last_second > 2 || quality.recovered_last_second > 30 {
            self.fec_controller.bump(now_ms, 5);
        } else if quality.recovered_last_second > 24 {
            self.fec_controller.bump(now_ms, 3);
        } else if quality.recovered_last_second > 22 {
            self.fec_controller.bump(now_ms, 2);
        } else if quality.recovered_last_second > 18 {
            self.fec_controller.bump(now_ms, 1);
        } else if quality.recovered_last_second < 18 {
            self.fec_controller.bump(now_ms, 0);
        }

        let fec_change = self.fec_controller.value(now_ms);
        let best_link_score = quality.link_score[0].max(quality.link_score[1]);
        let best_rssi = quality.rssi[0].max(quality.rssi[1]);
        let best_snr = quality.snr[0].max(quality.snr[1]);
        let mut message = format!(
            "{}:{}:{}:{}:{}:{}:{:.6}:0:-1:{}",
            now_ms / 1000,
            best_link_score,
            best_link_score,
            quality.recovered_last_second,
            quality.lost_last_second,
            best_rssi,
            best_snr as f64,
            fec_change
        );
        let idr_code = (self.idr_remaining_messages > 0)
            .then(|| self.idr_code.clone())
            .flatten();
        if let Some(idr_code) = idr_code {
            message.push(':');
            message.push_str(&idr_code);
            self.idr_remaining_messages = self.idr_remaining_messages.saturating_sub(1);
            if self.idr_remaining_messages == 0 {
                self.idr_code = None;
            }
        }
        message.push('\n');
        let mut udp_payload = Vec::with_capacity(4 + message.len());
        udp_payload.extend_from_slice(&(message.len() as u32).to_be_bytes());
        udp_payload.extend_from_slice(message.as_bytes());
        udp_payload
    }

    pub fn feedback_ip_packet(&mut self, now_ms: u64) -> Vec<u8> {
        let packet =
            wrap_udp_ipv4_payload_with_id(&self.feedback_udp_payload(now_ms), self.ip_packet_id);
        self.ip_packet_id = self.ip_packet_id.wrapping_add(1);
        packet
    }

    fn cleanup(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(WINDOW_MS);
        self.rssi.retain(|entry| entry.at_ms >= cutoff);
        self.snr.retain(|entry| entry.at_ms >= cutoff);
        self.fec.retain(|entry| entry.at_ms >= cutoff);
    }

    fn video_started_after_idle(&self, now_ms: u64) -> bool {
        self.last_video_activity_ms
            .map(|last| now_ms.saturating_sub(last) >= self.video_start_idle_ms)
            .unwrap_or(true)
    }
}

impl Default for AdaptiveLink {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct AdaptiveLinkSender {
    link: AdaptiveLink,
    tx: WfbTransmitter,
    tx_params: TxRadioParams,
    feedback_interval_ms: u64,
    session_interval_ms: u64,
    last_feedback_ms: Option<u64>,
    last_session_ms: Option<u64>,
}

impl AdaptiveLinkSender {
    pub fn new(
        link_id: u32,
        keypair: WfbTxKeypair,
        epoch: u64,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<Self, WfbError> {
        let channel_id = ChannelId::from_link_port(link_id, RadioPort::MavlinkTx);
        Ok(Self {
            link: AdaptiveLink::new(),
            tx: WfbTransmitter::new(channel_id, keypair, epoch, fec_k, fec_n)?,
            tx_params: TxRadioParams::openipc_uplink_default(),
            feedback_interval_ms: DEFAULT_FEEDBACK_INTERVAL_MS,
            session_interval_ms: DEFAULT_SESSION_INTERVAL_MS,
            last_feedback_ms: None,
            last_session_ms: None,
        })
    }

    pub fn link(&self) -> &AdaptiveLink {
        &self.link
    }

    pub fn link_mut(&mut self) -> &mut AdaptiveLink {
        &mut self.link
    }

    pub fn set_tx_params(&mut self, params: TxRadioParams) {
        self.tx_params = params;
    }

    pub fn record_rx_paths(&mut self, now_ms: u64, rssi: [u8; 4], snr: [i8; 4]) {
        self.link.record_rx_paths(now_ms, rssi, snr);
    }

    pub fn record_fec(&mut self, now_ms: u64, total: u32, recovered: u32, lost: u32) {
        self.link.record_fec(now_ms, total, recovered, lost);
    }

    pub fn tick(&mut self, now_ms: u64) -> Result<Vec<Vec<u8>>, WfbError> {
        let mut out = Vec::new();
        let send_session = self
            .last_session_ms
            .map(|last| now_ms.saturating_sub(last) >= self.session_interval_ms)
            .unwrap_or(true);
        if send_session {
            out.push(self.tx.session_radio_packet(self.tx_params));
            self.last_session_ms = Some(now_ms);
        }

        let send_feedback = self
            .last_feedback_ms
            .map(|last| now_ms.saturating_sub(last) >= self.feedback_interval_ms)
            .unwrap_or(true);
        if send_feedback {
            let payload = self.link.feedback_ip_packet(now_ms);
            out.extend(
                self.tx
                    .radio_packets_for_payload(&payload, self.tx_params)?,
            );
            self.last_feedback_ms = Some(now_ms);
        }
        Ok(out)
    }
}

pub fn wrap_udp_ipv4_payload(udp_payload: &[u8]) -> Vec<u8> {
    wrap_udp_ipv4_payload_with_id(udp_payload, 0)
}

pub fn wrap_udp_ipv4_payload_with_id(udp_payload: &[u8], packet_id: u16) -> Vec<u8> {
    let udp_len = 8 + udp_payload.len();
    let ip_len = 20 + udp_len;
    let mut out = Vec::with_capacity(2 + ip_len);
    out.extend_from_slice(&(ip_len as u16).to_be_bytes());
    out.push(0x45);
    out.push(0x00);
    out.extend_from_slice(&(ip_len as u16).to_be_bytes());
    out.extend_from_slice(&packet_id.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.push(64);
    out.push(17);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&[10, 5, 0, 1]);
    out.extend_from_slice(&[10, 5, 0, 10]);

    let checksum = ipv4_checksum(&out[2..22]);
    out[12] = (checksum >> 8) as u8;
    out[13] = checksum as u8;

    out.extend_from_slice(&54321u16.to_be_bytes());
    out.extend_from_slice(&9999u16.to_be_bytes());
    out.extend_from_slice(&(udp_len as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(udp_payload);
    out
}

fn avg_signal(entries: &[SignalEntry]) -> (f64, f64) {
    if entries.is_empty() {
        return (0.0, 0.0);
    }
    let (sum0, sum1) = entries.iter().fold((0i64, 0i64), |acc, entry| {
        (acc.0 + entry.ant1 as i64, acc.1 + entry.ant2 as i64)
    });
    let count = entries.len() as f64;
    (sum0 as f64 / count, sum1 as f64 / count)
}

fn link_score(rssi: f64, snr: f64) -> i32 {
    (0.5 * map_range(rssi, 50.0, 110.0, 1000.0, 2000.0)
        + 0.5 * map_range(snr, 20.0, 50.0, 1000.0, 2000.0))
    .round() as i32
}

fn map_range(input: f64, input_min: f64, input_max: f64, output_min: f64, output_max: f64) -> f64 {
    let clamped = input.clamp(input_min, input_max);
    output_min + (clamped - input_min) * (output_max - output_min) / (input_max - input_min)
}

fn ipv4_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in header.chunks_exact(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn random_idr_code() -> String {
    let mut bytes = [0u8; 4];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|byte| (b'a' + (byte % 26)) as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_link_quality_and_feedback_payload() {
        let mut link = AdaptiveLink::new();
        link.set_keyframe_request_messages(0);
        link.record_rx(1_000, 80, 70, 35, 25);
        link.record_fec(1_000, 10, 2, 0);
        let quality = link.quality(1_050);
        assert_eq!(quality.rssi, [80, 70]);
        assert_eq!(quality.snr, [35, 25]);
        assert_eq!(quality.recovered_last_second, 2);

        let payload = link.feedback_udp_payload(1_050);
        let len = u32::from_be_bytes(payload[0..4].try_into().unwrap()) as usize;
        assert_eq!(len, payload.len() - 4);
        let text = std::str::from_utf8(&payload[4..]).unwrap();
        assert!(text.contains(":2:0:"));
        assert_eq!(text.trim_end().split(':').count(), 10);
        assert_eq!(quality.idr_code, "");
    }

    #[test]
    fn keyframe_request_code_is_sent_only_for_active_window() {
        let mut link = AdaptiveLink::new();

        let no_request = link.feedback_udp_payload(1_000);
        let text = std::str::from_utf8(&no_request[4..]).unwrap();
        assert_eq!(text.trim_end().split(':').count(), 10);

        link.record_fec(1_100, 10, 0, 1);
        let first_request = link.feedback_udp_payload(1_100);
        let text = std::str::from_utf8(&first_request[4..]).unwrap();
        let fields: Vec<_> = text.trim_end().split(':').collect();
        assert_eq!(fields.len(), 11);
        assert_eq!(fields[10].len(), 4);

        for i in 1..DEFAULT_IDR_REQUEST_MESSAGES {
            let request = link.feedback_udp_payload(1_100 + i as u64);
            let text = std::str::from_utf8(&request[4..]).unwrap();
            assert_eq!(text.trim_end().split(':').count(), 11);
        }

        let expired = link.feedback_udp_payload(2_000);
        let text = std::str::from_utf8(&expired[4..]).unwrap();
        assert_eq!(text.trim_end().split(':').count(), 10);
    }

    #[test]
    fn keyframe_request_can_be_disabled() {
        let mut link = AdaptiveLink::new();
        link.set_keyframe_request_messages(0);
        link.record_fec(1_000, 1, 0, 1);
        assert_eq!(link.quality(1_000).idr_code, "");

        let payload = link.feedback_udp_payload(1_000);
        let text = std::str::from_utf8(&payload[4..]).unwrap();
        assert_eq!(text.trim_end().split(':').count(), 10);
    }

    #[test]
    fn first_video_after_idle_requests_keyframe() {
        let mut link = AdaptiveLink::new();
        link.set_keyframe_request_messages(1);

        link.record_fec(1_000, 10, 0, 0);
        let payload = link.feedback_udp_payload(1_000);
        let text = std::str::from_utf8(&payload[4..]).unwrap();
        assert_eq!(text.trim_end().split(':').count(), 11);

        let expired = link.feedback_udp_payload(1_001);
        let text = std::str::from_utf8(&expired[4..]).unwrap();
        assert_eq!(text.trim_end().split(':').count(), 10);

        link.record_fec(1_500, 10, 0, 0);
        let continuous = link.feedback_udp_payload(1_500);
        let text = std::str::from_utf8(&continuous[4..]).unwrap();
        assert_eq!(text.trim_end().split(':').count(), 10);

        link.record_fec(2_500, 10, 0, 0);
        let restarted = link.feedback_udp_payload(2_500);
        let text = std::str::from_utf8(&restarted[4..]).unwrap();
        assert_eq!(text.trim_end().split(':').count(), 11);
    }

    #[test]
    fn loss_renews_keyframe_request_during_active_video() {
        let mut link = AdaptiveLink::new();
        link.set_keyframe_request_messages(1);

        link.record_fec(1_000, 10, 0, 0);
        let _ = link.feedback_udp_payload(1_000);
        link.record_fec(1_100, 10, 0, 1);
        let payload = link.feedback_udp_payload(1_100);
        let text = std::str::from_utf8(&payload[4..]).unwrap();
        assert_eq!(text.trim_end().split(':').count(), 11);
    }

    #[test]
    fn wraps_udp_payload_in_length_prefixed_ipv4_packet() {
        let packet = wrap_udp_ipv4_payload(b"abc");
        let ip_len = u16::from_be_bytes([packet[0], packet[1]]) as usize;
        assert_eq!(ip_len, packet.len() - 2);
        assert_eq!(&packet[2..4], &[0x45, 0x00]);
        assert_eq!(&packet[14..18], &[10, 5, 0, 1]);
        assert_eq!(&packet[18..22], &[10, 5, 0, 10]);
        assert_eq!(u16::from_be_bytes([packet[22], packet[23]]), 54321);
        assert_eq!(u16::from_be_bytes([packet[24], packet[25]]), 9999);
    }

    #[test]
    fn feedback_ip_packet_increments_ipv4_id() {
        let mut link = AdaptiveLink::new();
        let first = link.feedback_ip_packet(1_000);
        let second = link.feedback_ip_packet(1_100);
        assert_eq!(u16::from_be_bytes([first[6], first[7]]), 0);
        assert_eq!(u16::from_be_bytes([second[6], second[7]]), 1);
    }
}
