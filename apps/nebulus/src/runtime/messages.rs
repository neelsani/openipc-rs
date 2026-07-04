use openipc_core::{
    DiversityStats, FecCounters, ReceiverBatchCounters, RtpDepacketizerStatus, RtpReorderStatus,
};
use std::time::Duration;
use web_time::Instant;

use crate::{
    model::LogLevel,
    settings::{CodecPreference, PayloadRouteSettings, ReceiverSource},
    telemetry::{TelemetrySettings, TelemetryUpdate},
};

/// Physical or synthetic transport behind a connected receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverTransport {
    Usb,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    UdpRtp,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Synthetic,
}

/// Hardware and initialization details captured when a receiver connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceiverInfo {
    pub(crate) transport: ReceiverTransport,
    pub(crate) id: String,
    pub(crate) source_id: u16,
    pub(crate) label: String,
    pub(crate) vendor_id: Option<u16>,
    pub(crate) product_id: Option<u16>,
    pub(crate) chip: String,
    pub(crate) rf_paths: String,
    pub(crate) cut_version: Option<u8>,
    pub(crate) usb_speed: String,
    pub(crate) bulk_in_endpoint: Option<u8>,
    pub(crate) bulk_out_endpoint: Option<u8>,
    pub(crate) initialization: String,
    pub(crate) firmware_downloaded: Option<bool>,
}

impl ReceiverInfo {
    pub(crate) fn initialized(
        id: String,
        source_id: u16,
        label: String,
        device: &openipc_rtl88xx::RealtekDevice,
        report: &openipc_rtl88xx::InitReport,
    ) -> Self {
        use openipc_rtl88xx::{InitStatus, RfType};

        let rf_paths = match report.chip.rf_type {
            RfType::OneTOneR => "1T1R",
            RfType::TwoTTwoR => "2T2R",
            RfType::FourTFourR => "4T4R",
        };
        let usb_speed = match device.device_speed() {
            Some(nusb::Speed::Low) => "Low speed (1.5 Mbps)",
            Some(nusb::Speed::Full) => "Full speed (12 Mbps)",
            Some(nusb::Speed::High) => "High speed (480 Mbps)",
            Some(nusb::Speed::Super) => "SuperSpeed (5 Gbps)",
            Some(nusb::Speed::SuperPlus) => "SuperSpeed+ (10 Gbps)",
            Some(_) | None => "Not reported",
        };
        Self {
            transport: ReceiverTransport::Usb,
            id,
            source_id,
            label,
            vendor_id: Some(device.vendor_id()),
            product_id: Some(device.product_id()),
            chip: report.chip.family.name().to_owned(),
            rf_paths: rf_paths.to_owned(),
            cut_version: Some(report.chip.cut_version),
            usb_speed: usb_speed.to_owned(),
            bulk_in_endpoint: Some(device.bulk_in_endpoint_address()),
            bulk_out_endpoint: Some(device.bulk_out_endpoint_address()),
            initialization: match report.status {
                InitStatus::AlreadyRunning => "Already initialized",
                InitStatus::Initialized => "Cold initialization completed",
            }
            .to_owned(),
            firmware_downloaded: Some(report.firmware_downloaded),
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn udp_rtp(local_address: String) -> Self {
        Self {
            transport: ReceiverTransport::UdpRtp,
            id: local_address.clone(),
            source_id: 0,
            label: format!("UDP RTP {local_address}"),
            vendor_id: None,
            product_id: None,
            chip: "Direct RTP".to_owned(),
            rf_paths: "Not applicable".to_owned(),
            cut_version: None,
            usb_speed: "UDP socket".to_owned(),
            bulk_in_endpoint: None,
            bulk_out_endpoint: None,
            initialization: "Listening for RTP datagrams".to_owned(),
            firmware_downloaded: None,
        }
    }

    #[cfg(debug_assertions)]
    pub(crate) fn codec_mock() -> Self {
        Self {
            transport: ReceiverTransport::Synthetic,
            id: "codec-mock".to_owned(),
            source_id: 0,
            label: "Pre-recorded 1080p H.264 + Opus".to_owned(),
            vendor_id: None,
            product_id: None,
            chip: "Synthetic A/V RTP".to_owned(),
            rf_paths: "Synthetic".to_owned(),
            cut_version: None,
            usb_speed: "No USB device".to_owned(),
            bulk_in_endpoint: None,
            bulk_out_endpoint: None,
            initialization: "Development codec mock".to_owned(),
            firmware_downloaded: None,
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) type NativeVideoSurface = openipc_video::MacOsVideoFrame;
#[cfg(target_os = "linux")]
pub(crate) type NativeVideoSurface = openipc_video::LinuxVideoFrame;
#[cfg(target_os = "windows")]
pub(crate) type NativeVideoSurface = openipc_video::WindowsVideoFrame;
#[cfg(target_os = "android")]
pub(crate) type NativeVideoSurface = openipc_video::AndroidPresentedFrame;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) type NativeVideoSurface = openipc_video::WebVideoFrame;

/// USB adapter shown in the receiver selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsbDeviceInfo {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) location: String,
}

/// Configuration sent from the UI to a receive worker.
#[derive(Debug, Clone)]
pub(crate) struct StartRequest {
    #[cfg(target_os = "android")]
    pub(crate) video_output: Option<ndk::native_window::NativeWindow>,
    pub(crate) receiver_source: ReceiverSource,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) udp_bind_address: String,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) udp_bind_port: u16,
    pub(crate) primary_device_id: Option<String>,
    pub(crate) device_ids: Vec<String>,
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
    pub(crate) telemetry: TelemetrySettings,
}

/// Configuration for an idle radio channel survey.
#[derive(Debug, Clone)]
pub(crate) struct ScanRequest {
    pub(crate) device_id: Option<String>,
    pub(crate) channels: Vec<u8>,
    pub(crate) channel_width_mhz: u16,
    pub(crate) channel_offset: u8,
    pub(crate) transfer_size: usize,
    pub(crate) dwell: Duration,
}

/// RF activity observed while dwelling on one channel.
#[derive(Debug, Clone, Default)]
pub(crate) struct ChannelScanResult {
    pub(crate) channel: u8,
    pub(crate) packets: u64,
    pub(crate) bytes: u64,
    pub(crate) wfb_frames: u64,
    pub(crate) average_rssi_dbm: [i32; 2],
    pub(crate) strongest_rssi_dbm: [i32; 2],
    pub(crate) dwell_ms: u64,
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

/// Latest health and contribution snapshot for one receive adapter.
#[derive(Debug, Clone, Default)]
pub(crate) struct AdapterRuntimeMetrics {
    pub(crate) source_id: u16,
    pub(crate) device_id: String,
    pub(crate) label: String,
    pub(crate) online: bool,
    pub(crate) transfers: u64,
    pub(crate) transfer_bytes: u64,
    pub(crate) usb_errors: u64,
    pub(crate) queue_drops: u64,
    pub(crate) rssi: [i32; 4],
    pub(crate) snr: [i32; 4],
    pub(crate) accepted: u64,
    pub(crate) duplicates: u64,
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
    pub(crate) video_submit_path_ms: f64,
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
    pub(crate) telemetry: Option<TelemetryUpdate>,
    pub(crate) audio: crate::model::AudioStats,
    pub(crate) diversity: DiversityStats,
    pub(crate) adapters: Vec<AdapterRuntimeMetrics>,
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
        self.video_submit_path_ms = newer.video_submit_path_ms;
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
        if let Some(update) = newer.telemetry {
            if let Some(current) = self.telemetry.as_mut() {
                current.merge(update);
            } else {
                self.telemetry = Some(update);
            }
        }
        self.audio = newer.audio;
        self.diversity = newer.diversity;
        self.adapters = newer.adapters;
    }
}

/// Coalesces high-rate receiver statistics into a UI-friendly cadence.
///
/// Video presentation is never throttled. Only diagnostics use this path, so
/// USB completion rates cannot force an egui redraw for every transfer.
pub(crate) struct MetricsThrottle {
    pending: Option<BatchMetrics>,
    last_emit: Instant,
    interval: Duration,
}

impl MetricsThrottle {
    pub(crate) fn new() -> Self {
        Self {
            pending: None,
            last_emit: Instant::now(),
            interval: Duration::from_millis(50),
        }
    }

    pub(crate) fn push(&mut self, metrics: BatchMetrics) -> Option<BatchMetrics> {
        if let Some(pending) = self.pending.as_mut() {
            pending.merge(metrics);
        } else {
            self.pending = Some(metrics);
        }
        (self.last_emit.elapsed() >= self.interval)
            .then(|| self.take())
            .flatten()
    }

    pub(crate) fn flush(&mut self) -> Option<BatchMetrics> {
        self.take()
    }

    fn take(&mut self) -> Option<BatchMetrics> {
        let pending = self.pending.take();
        if pending.is_some() {
            self.last_emit = Instant::now();
        }
        pending
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

#[cfg(test)]
mod metrics_throttle_tests {
    use std::time::Duration;

    use super::{BatchMetrics, MetricsThrottle};
    use crate::telemetry::{TelemetryProtocol, TelemetryUpdate};

    #[test]
    fn flush_preserves_all_coalesced_counts() {
        let mut throttle = MetricsThrottle::new();
        throttle.interval = Duration::from_secs(60);
        assert!(throttle
            .push(BatchMetrics {
                transfers: 1,
                packets: 2,
                ..BatchMetrics::default()
            })
            .is_none());
        assert!(throttle
            .push(BatchMetrics {
                transfers: 3,
                packets: 5,
                ..BatchMetrics::default()
            })
            .is_none());

        let merged = throttle.flush().expect("pending metrics");
        assert_eq!(merged.transfers, 4);
        assert_eq!(merged.packets, 7);
        assert!(throttle.flush().is_none());
    }

    #[test]
    fn coalescing_keeps_partial_telemetry_updates() {
        let mut metrics = BatchMetrics {
            telemetry: Some(TelemetryUpdate {
                protocol: Some(TelemetryProtocol::Mavlink),
                messages: 1,
                armed: Some(true),
                ..TelemetryUpdate::default()
            }),
            ..BatchMetrics::default()
        };
        metrics.merge(BatchMetrics {
            telemetry: Some(TelemetryUpdate {
                protocol: Some(TelemetryProtocol::Mavlink),
                messages: 2,
                battery_voltage_v: Some(16.8),
                ..TelemetryUpdate::default()
            }),
            ..BatchMetrics::default()
        });

        let telemetry = metrics.telemetry.expect("coalesced telemetry");
        assert_eq!(telemetry.messages, 3);
        assert_eq!(telemetry.armed, Some(true));
        assert_eq!(telemetry.battery_voltage_v, Some(16.8));
    }
}

/// Event sent from a target runtime to the egui application.
#[derive(Debug)]
pub(crate) enum RuntimeEvent {
    Devices(Vec<UsbDeviceInfo>),
    DiscoveryFailed(String),
    Connecting,
    Connected {
        receivers: Vec<ReceiverInfo>,
        decoder: DecoderEnvironment,
    },
    Started,
    ScanStarted {
        total: usize,
    },
    ScanProgress {
        index: usize,
        total: usize,
        result: ChannelScanResult,
    },
    ScanCompleted,
    ScanFailed(String),
    Batch(Box<BatchMetrics>),
    DiversityUpdate {
        stats: DiversityStats,
        adapters: Vec<AdapterRuntimeMetrics>,
    },
    NativeVideo {
        frame: openipc_video::DecodedFrame<NativeVideoSurface>,
        decode_latency_ms: f64,
        ready_at: Instant,
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
