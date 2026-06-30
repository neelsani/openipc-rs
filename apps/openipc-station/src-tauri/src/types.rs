use super::*;

pub(crate) const RX_BATCH_EVENT: &str = "openipc://rx-batch";
pub(crate) const LOG_EVENT: &str = "openipc://log";
pub(crate) const STOPPED_EVENT: &str = "openipc://stopped";
pub(crate) const VPN_STATUS_EVENT: &str = "openipc://vpn-status";
pub(crate) const RX_TRANSFERS_IN_FLIGHT: usize = 4;

#[derive(Default)]
pub(crate) struct DesktopState {
    pub(crate) device: Mutex<Option<Arc<RealtekDevice>>>,
    pub(crate) chip_family: Mutex<Option<ChipFamily>>,
    pub(crate) worker: Mutex<Option<RxWorker>>,
}

pub(crate) struct RxWorker {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) join: Option<JoinHandle<()>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StationUsbDevice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) product: Option<String>,
    pub(crate) manufacturer: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectRequest {
    pub(crate) channel: u8,
    pub(crate) channel_width_mhz: u16,
    pub(crate) channel_offset: u8,
    pub(crate) skip_reset: Option<bool>,
    pub(crate) device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct ConnectFromFdRequest {
    #[serde(flatten)]
    pub(crate) connect: ConnectRequest,
    pub(crate) fd: i32,
    pub(crate) android_device_id: Option<String>,
    pub(crate) vendor_id: Option<u16>,
    pub(crate) product_id: Option<u16>,
    pub(crate) product: Option<String>,
    pub(crate) manufacturer: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectReport {
    pub(crate) device_id: String,
    pub(crate) usb_info: UsbInfoPayload,
    pub(crate) init_report: InitReportPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsbInfoPayload {
    pub(crate) label: String,
    pub(crate) bulk_in: u8,
    pub(crate) bulk_out: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitReportPayload {
    pub(crate) chip: String,
    pub(crate) rf_paths: usize,
    pub(crate) cut_version: u8,
    pub(crate) status: String,
    pub(crate) firmware_downloaded: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartRxRequest {
    pub(crate) keypair_base64: String,
    pub(crate) channel_id: u32,
    pub(crate) minimum_epoch: String,
    pub(crate) transfer_size: usize,
    pub(crate) adaptive_enabled: bool,
    pub(crate) vpn_enabled: bool,
    pub(crate) vpn_tun_fd: Option<i32>,
    pub(crate) rf_channel: u8,
    pub(crate) alink_tx_power: u8,
    #[serde(default)]
    pub(crate) payload_routes: Vec<PayloadRouteRequest>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PayloadRouteRequest {
    pub(crate) route_id: u64,
    pub(crate) enabled: bool,
    pub(crate) name: String,
    pub(crate) channel_id: u32,
    pub(crate) action: PayloadRouteAction,
    pub(crate) payload_type: Option<u8>,
    pub(crate) udp_host: Option<String>,
    pub(crate) udp_port: Option<u16>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PayloadRouteAction {
    Inspect,
    Log,
    Udp,
    Audio,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoFramePayload {
    pub(crate) data_base64: String,
    pub(crate) codec: &'static str,
    pub(crate) codec_string: String,
    pub(crate) is_key_frame: bool,
    pub(crate) timestamp: u32,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawPayloadPayload {
    pub(crate) data_base64: String,
    pub(crate) packet_seq: String,
    pub(crate) route_id: u64,
    pub(crate) channel_id: u32,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FecCountersPayload {
    pub(crate) total_packets: u64,
    pub(crate) recovered_packets: u64,
    pub(crate) lost_packets: u64,
    pub(crate) bad_packets: u64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LinkQualityPayload {
    pub(crate) lost_last_second: u32,
    pub(crate) recovered_last_second: u32,
    pub(crate) total_last_second: u32,
    pub(crate) rssi: [i32; 2],
    pub(crate) snr: [i32; 2],
    pub(crate) link_score: [i32; 2],
    pub(crate) idr_code: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RxBatchPayload {
    pub(crate) frames: Vec<VideoFramePayload>,
    pub(crate) raw_payloads: Vec<RawPayloadPayload>,
    pub(crate) mavlink_payloads: Vec<RawPayloadPayload>,
    pub(crate) transfer_bytes: usize,
    pub(crate) packets: usize,
    pub(crate) accepted_packets: usize,
    pub(crate) dropped_packets: usize,
    pub(crate) crc_dropped: usize,
    pub(crate) icv_dropped: usize,
    pub(crate) report_dropped: usize,
    pub(crate) ignored_frames: usize,
    pub(crate) sessions: usize,
    pub(crate) wfb_payloads: usize,
    pub(crate) rtp_packets: usize,
    pub(crate) video_frames: usize,
    pub(crate) raw_payload_count: usize,
    pub(crate) raw_payload_bytes: usize,
    pub(crate) mavlink_payload_count: usize,
    pub(crate) mavlink_bytes: usize,
    pub(crate) parse_ms: f64,
    pub(crate) pipeline_ms: f64,
    pub(crate) total_ms: f64,
    pub(crate) fec_counters: FecCountersPayload,
    pub(crate) link_quality: Option<LinkQualityPayload>,
    pub(crate) adaptive_tx_frames: usize,
    pub(crate) adaptive_tx_errors: usize,
    pub(crate) usb_read_ms: f64,
    pub(crate) adaptive_rx_ms: f64,
    pub(crate) adaptive_quality_ms: f64,
    pub(crate) tx_power_ms: f64,
    pub(crate) adaptive_tx_ms: f64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogPayload {
    pub(crate) level: &'static str,
    pub(crate) message: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoppedPayload {
    pub(crate) reason: &'static str,
    pub(crate) message: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VpnStatusPayload {
    pub(crate) interface_name: String,
    pub(crate) local_ip: &'static str,
    pub(crate) prefix_length: u8,
    pub(crate) rx_port: u8,
    pub(crate) tx_port: u8,
}

pub(crate) struct AdaptiveRuntime {
    pub(crate) sender: AdaptiveLinkSender,
    pub(crate) last_counters: FecCounters,
    pub(crate) tx_options: RealtekTxOptions,
}

pub(crate) struct RxBatchContext<'a> {
    pub(crate) receiver: &'a mut ReceiverRuntime,
    pub(crate) adaptive: Option<&'a mut AdaptiveRuntime>,
    pub(crate) ep_out: Option<&'a mut nusb::Endpoint<Bulk, Out>>,
    pub(crate) now_ms: u64,
    pub(crate) rx_descriptor_kind: openipc_core::realtek::RxDescriptorKind,
    pub(crate) usb_read_ms: f64,
    pub(crate) loop_start: Instant,
    pub(crate) raw_payload_routes: &'a [PayloadRouteId],
    pub(crate) rtp_payload_taps: &'a [RtpPayloadTap],
    pub(crate) udp_sinks: &'a [crate::worker::UdpRouteSink],
    pub(crate) tun: Option<&'a mut crate::worker::TunRuntime>,
}
