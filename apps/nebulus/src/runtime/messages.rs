use openipc_core::{FecCounters, ReceiverBatchCounters, RtpDepacketizerStatus, RtpReorderStatus};

use crate::{
    model::LogLevel,
    settings::{CodecPreference, PayloadRouteSettings},
};

#[cfg(target_os = "macos")]
pub(crate) type NativeVideoSurface = openipc_video::MacOsVideoFrame;
#[cfg(target_os = "linux")]
pub(crate) type NativeVideoSurface = openipc_video::LinuxVideoFrame;
#[cfg(target_os = "windows")]
pub(crate) type NativeVideoSurface = openipc_video::WindowsVideoFrame;
#[cfg(target_os = "android")]
pub(crate) type NativeVideoSurface = openipc_video::AndroidVideoFrame;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) type NativeVideoSurface = openipc_video::WebVideoFrame;

/// USB adapter shown in the receiver selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsbDeviceInfo {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
}

/// Configuration sent from the UI to a receive worker.
#[derive(Debug, Clone)]
pub(crate) struct StartRequest {
    pub(crate) device_id: Option<String>,
    pub(crate) channel: u8,
    pub(crate) channel_width_mhz: u16,
    pub(crate) channel_offset: u8,
    pub(crate) channel_id: u32,
    pub(crate) minimum_epoch: u64,
    pub(crate) transfer_size: usize,
    pub(crate) codec_preference: CodecPreference,
    pub(crate) rtp_reorder: bool,
    pub(crate) adaptive_link: bool,
    pub(crate) tx_power: u8,
    pub(crate) key_bytes: Vec<u8>,
    pub(crate) audio_volume: u8,
    pub(crate) vpn_enabled: bool,
    pub(crate) payload_routes: Vec<PayloadRouteSettings>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DecoderEnvironment {
    pub(crate) backend: String,
    pub(crate) h264_supported: bool,
    pub(crate) h265_supported: bool,
    pub(crate) h264_hardware: Option<bool>,
    pub(crate) h265_hardware: Option<bool>,
    pub(crate) native_surfaces: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VpnMetrics {
    pub(crate) active: bool,
    pub(crate) interface_name: String,
    pub(crate) downlink_packets: u64,
    pub(crate) downlink_bytes: u64,
    pub(crate) uplink_packets: u64,
    pub(crate) uplink_bytes: u64,
    pub(crate) errors: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RouteMetricDelta {
    pub(crate) route_id: u64,
    pub(crate) packets: u64,
    pub(crate) bytes: u64,
    pub(crate) last_bytes: usize,
    pub(crate) errors: u64,
}

/// Metrics emitted for one processed USB batch.
#[derive(Debug, Clone, Default)]
pub(crate) struct BatchMetrics {
    pub(crate) transfers: u64,
    pub(crate) transfer_bytes: usize,
    pub(crate) packets: usize,
    pub(crate) rtp_packets: usize,
    pub(crate) video_frames: usize,
    pub(crate) video_bytes: usize,
    pub(crate) usb_latency_ms: f64,
    pub(crate) parse_latency_ms: f64,
    pub(crate) pipeline_latency_ms: f64,
    pub(crate) route_latency_ms: f64,
    pub(crate) decode_submit_latency_ms: f64,
    pub(crate) batch_latency_ms: f64,
    pub(crate) rssi: [i32; 2],
    pub(crate) snr: [i32; 2],
    pub(crate) link_score: [i32; 2],
    pub(crate) decoder_drops: u64,
    pub(crate) decoder_errors: u64,
    pub(crate) fec: FecCounters,
    pub(crate) counters: ReceiverBatchCounters,
    pub(crate) rtp: RtpDepacketizerStatus,
    pub(crate) reorder: RtpReorderStatus,
    pub(crate) vpn: VpnMetrics,
    pub(crate) routes: Vec<RouteMetricDelta>,
    pub(crate) audio: crate::model::AudioStats,
}

impl BatchMetrics {
    pub(crate) fn merge(&mut self, newer: Self) {
        self.transfers = self.transfers.saturating_add(newer.transfers);
        self.transfer_bytes = self.transfer_bytes.saturating_add(newer.transfer_bytes);
        self.packets = self.packets.saturating_add(newer.packets);
        self.rtp_packets = self.rtp_packets.saturating_add(newer.rtp_packets);
        self.video_frames = self.video_frames.saturating_add(newer.video_frames);
        self.video_bytes = self.video_bytes.saturating_add(newer.video_bytes);
        self.usb_latency_ms = newer.usb_latency_ms;
        self.parse_latency_ms = newer.parse_latency_ms;
        self.pipeline_latency_ms = newer.pipeline_latency_ms;
        self.route_latency_ms = newer.route_latency_ms;
        self.decode_submit_latency_ms = newer.decode_submit_latency_ms;
        self.batch_latency_ms = newer.batch_latency_ms;
        self.rssi = newer.rssi;
        self.snr = newer.snr;
        self.link_score = newer.link_score;
        self.decoder_drops = newer.decoder_drops;
        self.decoder_errors = newer.decoder_errors;
        self.fec = newer.fec;
        merge_counters(&mut self.counters, newer.counters);
        self.rtp = newer.rtp;
        self.reorder = newer.reorder;
        self.vpn = newer.vpn;
        for update in newer.routes {
            if let Some(current) = self
                .routes
                .iter_mut()
                .find(|current| current.route_id == update.route_id)
            {
                current.packets = current.packets.saturating_add(update.packets);
                current.bytes = current.bytes.saturating_add(update.bytes);
                current.last_bytes = update.last_bytes;
                current.errors = current.errors.saturating_add(update.errors);
            } else {
                self.routes.push(update);
            }
        }
        self.audio = newer.audio;
    }
}

fn merge_counters(current: &mut ReceiverBatchCounters, newer: ReceiverBatchCounters) {
    current.packets = current.packets.saturating_add(newer.packets);
    current.accepted_packets = current
        .accepted_packets
        .saturating_add(newer.accepted_packets);
    current.dropped_packets = current
        .dropped_packets
        .saturating_add(newer.dropped_packets);
    current.crc_dropped = current.crc_dropped.saturating_add(newer.crc_dropped);
    current.icv_dropped = current.icv_dropped.saturating_add(newer.icv_dropped);
    current.report_dropped = current.report_dropped.saturating_add(newer.report_dropped);
    current.ignored_frames = current.ignored_frames.saturating_add(newer.ignored_frames);
    current.sessions = current.sessions.saturating_add(newer.sessions);
    current.wfb_payloads = current.wfb_payloads.saturating_add(newer.wfb_payloads);
    current.rtp_packets = current.rtp_packets.saturating_add(newer.rtp_packets);
    current.video_frames = current.video_frames.saturating_add(newer.video_frames);
    current.raw_payload_count = current
        .raw_payload_count
        .saturating_add(newer.raw_payload_count);
    current.raw_payload_bytes = current
        .raw_payload_bytes
        .saturating_add(newer.raw_payload_bytes);
    current.route_errors = current.route_errors.saturating_add(newer.route_errors);
}

/// Event sent from a target runtime to the egui application.
#[derive(Debug)]
pub(crate) enum RuntimeEvent {
    Devices(Vec<UsbDeviceInfo>),
    DiscoveryFailed(String),
    Connecting,
    Connected {
        label: String,
        chip: String,
        decoder: DecoderEnvironment,
    },
    Started,
    Batch(Box<BatchMetrics>),
    NativeVideo {
        frame: openipc_video::DecodedFrame<NativeVideoSurface>,
        decode_latency_ms: f64,
    },
    Log {
        level: LogLevel,
        target: &'static str,
        message: String,
    },
    RecordingArmed(String),
    RecordingStarted {
        path: String,
        codec: String,
    },
    RecordingStopped {
        path: String,
        bytes: u64,
    },
    RecordingFailed(String),
    Stopped,
    Failed(String),
}
