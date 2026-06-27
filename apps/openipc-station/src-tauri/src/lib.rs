use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use nusb::transfer::{Bulk, Out};
use openipc_core::realtek::{parse_rx_aggregate, RxPacketAttrib, RxPacketType};
use openipc_core::realtek_tx::RealtekTxOptions;
use openipc_core::rtp::{Codec, DepacketizedFrame};
use openipc_core::{
    AdaptiveLinkSender, ChannelId, FecCounters, FrameLayout, PipelineEvent, ReceiverPipeline,
    WfbKeypair, WfbTxKeypair,
};
use openipc_rtl88xx::{
    list_supported_devices, ChannelWidth, ChipFamily, DriverOptions, InitReport, InitStatus,
    RadioConfig, RealtekDevice,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

const RX_BATCH_EVENT: &str = "openipc://rx-batch";
const LOG_EVENT: &str = "openipc://log";
const STOPPED_EVENT: &str = "openipc://stopped";

#[derive(Default)]
struct DesktopState {
    device: Mutex<Option<Arc<RealtekDevice>>>,
    chip_family: Mutex<Option<ChipFamily>>,
    worker: Mutex<Option<RxWorker>>,
}

struct RxWorker {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopUsbDevice {
    vendor_id: u16,
    product_id: u16,
    product: Option<String>,
    manufacturer: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectRequest {
    channel: u8,
    channel_width_mhz: u16,
    channel_offset: u8,
    skip_reset: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectReport {
    device_id: String,
    usb_info: UsbInfoPayload,
    init_report: InitReportPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsbInfoPayload {
    label: String,
    bulk_in: u8,
    bulk_out: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitReportPayload {
    chip: String,
    rf_paths: usize,
    cut_version: u8,
    status: String,
    firmware_downloaded: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StartRxRequest {
    keypair_base64: String,
    channel_id: u32,
    minimum_epoch: String,
    transfer_size: usize,
    adaptive_enabled: bool,
    rf_channel: u8,
    alink_tx_power: u8,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct VideoFramePayload {
    data_base64: String,
    codec: &'static str,
    codec_string: String,
    is_key_frame: bool,
    timestamp: u32,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FecCountersPayload {
    total_packets: u64,
    recovered_packets: u64,
    lost_packets: u64,
    bad_packets: u64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LinkQualityPayload {
    lost_last_second: u32,
    recovered_last_second: u32,
    total_last_second: u32,
    rssi: [i32; 2],
    snr: [i32; 2],
    link_score: [i32; 2],
    idr_code: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RxBatchPayload {
    frames: Vec<VideoFramePayload>,
    transfer_bytes: usize,
    packets: usize,
    accepted_packets: usize,
    dropped_packets: usize,
    crc_dropped: usize,
    icv_dropped: usize,
    report_dropped: usize,
    ignored_frames: usize,
    sessions: usize,
    wfb_payloads: usize,
    rtp_packets: usize,
    video_frames: usize,
    parse_ms: f64,
    pipeline_ms: f64,
    total_ms: f64,
    fec_counters: FecCountersPayload,
    link_quality: Option<LinkQualityPayload>,
    adaptive_tx_frames: usize,
    adaptive_tx_errors: usize,
    usb_read_ms: f64,
    adaptive_rx_ms: f64,
    adaptive_quality_ms: f64,
    tx_power_ms: f64,
    adaptive_tx_ms: f64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LogPayload {
    level: &'static str,
    message: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StoppedPayload {
    reason: &'static str,
    message: String,
}

struct AdaptiveRuntime {
    sender: AdaptiveLinkSender,
    last_counters: FecCounters,
    tx_options: RealtekTxOptions,
}

impl AdaptiveRuntime {
    fn record_rx(&mut self, now_ms: u64, attrib: &RxPacketAttrib) {
        self.sender.record_rx_paths(now_ms, attrib.rssi, attrib.snr);
    }

    fn record_pipeline(&mut self, now_ms: u64, counters: FecCounters) {
        let total = counters
            .total_packets
            .saturating_sub(self.last_counters.total_packets);
        let recovered = counters
            .recovered_packets
            .saturating_sub(self.last_counters.recovered_packets);
        let lost = counters
            .lost_packets
            .saturating_sub(self.last_counters.lost_packets);
        self.last_counters = counters;
        self.sender.record_fec(
            now_ms,
            total.min(u32::MAX as u64) as u32,
            recovered.min(u32::MAX as u64) as u32,
            lost.min(u32::MAX as u64) as u32,
        );
    }

    fn quality(&mut self, now_ms: u64) -> LinkQualityPayload {
        let quality = self.sender.link_mut().quality(now_ms);
        LinkQualityPayload {
            lost_last_second: quality.lost_last_second,
            recovered_last_second: quality.recovered_last_second,
            total_last_second: quality.total_last_second,
            rssi: quality.rssi,
            snr: quality.snr,
            link_score: quality.link_score,
            idr_code: quality.idr_code,
        }
    }

    fn tick(
        &mut self,
        now_ms: u64,
        ep_out: &mut nusb::Endpoint<Bulk, Out>,
    ) -> Result<usize, String> {
        let frames = self.sender.tick(now_ms).map_err(|err| err.to_string())?;
        let count = frames.len();
        for frame in frames {
            RealtekDevice::send_packet_on(ep_out, &frame, self.tx_options)
                .map_err(|err| err.to_string())?;
        }
        Ok(count)
    }
}

#[tauri::command]
fn openipc_list_devices() -> Result<Vec<DesktopUsbDevice>, String> {
    Ok(list_supported_devices()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|device| DesktopUsbDevice {
            vendor_id: device.vendor_id,
            product_id: device.product_id,
            product: device.product,
            manufacturer: device.manufacturer,
        })
        .collect())
}

#[tauri::command]
fn openipc_connect(
    request: ConnectRequest,
    state: State<'_, DesktopState>,
) -> Result<ConnectReport, String> {
    if state
        .worker
        .lock()
        .map_err(|_| "worker lock poisoned")?
        .is_some()
    {
        return Err("receiver is already running".to_owned());
    }

    let summary = list_supported_devices()
        .map_err(|err| err.to_string())?
        .into_iter()
        .next();
    let device = RealtekDevice::open_first(DriverOptions {
        skip_reset: request.skip_reset.unwrap_or(false),
        initialize_hardware: true,
    })
    .map_err(|err| err.to_string())?;
    let report = device
        .initialize_monitor(radio_config(
            request.channel,
            request.channel_width_mhz,
            request.channel_offset,
        )?)
        .map_err(|err| err.to_string())?;
    let device_id = summary
        .as_ref()
        .map(|device| usb_id(device.vendor_id, device.product_id))
        .unwrap_or_else(|| report.chip.family.name().to_owned());
    let label = summary
        .as_ref()
        .map(|device| {
            device_label(
                device.manufacturer.as_deref(),
                device.product.as_deref(),
                &device_id,
            )
        })
        .unwrap_or_else(|| device_id.clone());

    let usb_info = UsbInfoPayload {
        label,
        bulk_in: device.bulk_in_ep,
        bulk_out: device.bulk_out_ep,
    };
    let chip_family = report.chip.family;
    let init_report = init_report_payload(report);

    *state.device.lock().map_err(|_| "device lock poisoned")? = Some(Arc::new(device));
    *state.chip_family.lock().map_err(|_| "chip lock poisoned")? = Some(chip_family);

    Ok(ConnectReport {
        device_id,
        usb_info,
        init_report,
    })
}

#[tauri::command]
fn openipc_start_rx(
    app: AppHandle,
    request: StartRxRequest,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    let mut worker = state.worker.lock().map_err(|_| "worker lock poisoned")?;
    if worker.is_some() {
        return Err("receiver is already running".to_owned());
    }
    let device = state
        .device
        .lock()
        .map_err(|_| "device lock poisoned")?
        .clone()
        .ok_or_else(|| "connect to a Realtek adapter before starting RX".to_owned())?;
    let chip_family = state
        .chip_family
        .lock()
        .map_err(|_| "chip lock poisoned")?
        .ok_or_else(|| "chip family is unknown; reconnect the adapter".to_owned())?;

    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let handle = thread::spawn(move || {
        if let Err(err) = run_rx_worker(app.clone(), device, chip_family, request, worker_stop) {
            emit_stopped(&app, "error", err);
        }
    });
    *worker = Some(RxWorker {
        stop,
        join: Some(handle),
    });
    Ok(())
}

#[tauri::command]
fn openipc_stop_rx(state: State<'_, DesktopState>) -> Result<(), String> {
    let worker = state
        .worker
        .lock()
        .map_err(|_| "worker lock poisoned")?
        .take();
    if let Some(mut worker) = worker {
        worker.stop.store(true, Ordering::Relaxed);
        if let Some(join) = worker.join.take() {
            let _ = join.join();
        }
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            openipc_list_devices,
            openipc_connect,
            openipc_start_rx,
            openipc_stop_rx,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenIPC Station desktop app");
}

fn run_rx_worker(
    app: AppHandle,
    device: Arc<RealtekDevice>,
    chip_family: ChipFamily,
    request: StartRxRequest,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let keypair_bytes = BASE64
        .decode(request.keypair_base64.as_bytes())
        .map_err(|err| format!("invalid keypair base64: {err}"))?;
    let minimum_epoch = request
        .minimum_epoch
        .parse::<u64>()
        .map_err(|err| format!("invalid minimum epoch: {err}"))?;
    let keypair = WfbKeypair::from_bytes(&keypair_bytes).map_err(|err| err.to_string())?;
    let mut pipeline = ReceiverPipeline::with_keypair(
        ChannelId::new(request.channel_id),
        FrameLayout::WithFcs,
        keypair,
        minimum_epoch,
    )
    .map_err(|err| err.to_string())?;
    let mut ep_in = device.bulk_in_endpoint().map_err(|err| err.to_string())?;
    let mut ep_out = if request.adaptive_enabled {
        Some(device.bulk_out_endpoint().map_err(|err| err.to_string())?)
    } else {
        None
    };
    let mut adaptive = if request.adaptive_enabled {
        let tx_power_start = Instant::now();
        device
            .set_tx_power_override(request.rf_channel, request.alink_tx_power)
            .map_err(|err| err.to_string())?;
        emit_log(
            &app,
            "info",
            format!(
                "Adaptive uplink TX power set to {} ({:.1} ms)",
                request.alink_tx_power,
                elapsed_ms(tx_power_start)
            ),
        );
        let tx_keypair = WfbTxKeypair::from_bytes(&keypair_bytes).map_err(|err| err.to_string())?;
        let link_id = request.channel_id >> 8;
        Some(AdaptiveRuntime {
            sender: AdaptiveLinkSender::new(link_id, tx_keypair, 0, 1, 5)
                .map_err(|err| err.to_string())?,
            last_counters: pipeline.fec_counters(),
            tx_options: RealtekTxOptions {
                current_channel: request.rf_channel,
                is_8814a: chip_family == ChipFamily::Rtl8814,
            },
        })
    } else {
        None
    };

    emit_log(&app, "info", "Native RX loop started".to_owned());

    while !stop.load(Ordering::Relaxed) {
        let loop_start = Instant::now();
        let read_start = Instant::now();
        let buffer = ep_in.allocate(request.transfer_size);
        let completion = ep_in.transfer_blocking(buffer, Duration::from_millis(1000));
        let usb_read_ms = elapsed_ms(read_start);
        if let Err(err) = completion.status {
            emit_log(&app, "warn", format!("bulk IN transfer failed: {err}"));
            continue;
        }
        let bytes = &completion.buffer[..completion.actual_len];
        let now_ms = unix_time_ms();
        match build_rx_batch(
            bytes,
            &mut pipeline,
            adaptive.as_mut(),
            ep_out.as_mut(),
            now_ms,
            usb_read_ms,
            loop_start,
        ) {
            Ok(batch) => {
                let _ = app.emit(RX_BATCH_EVENT, batch);
            }
            Err(err) => {
                emit_log(&app, "error", err);
            }
        }
    }

    emit_stopped(&app, "stopped", "Native RX loop stopped".to_owned());
    Ok(())
}

fn build_rx_batch(
    bytes: &[u8],
    pipeline: &mut ReceiverPipeline,
    mut adaptive: Option<&mut AdaptiveRuntime>,
    ep_out: Option<&mut nusb::Endpoint<Bulk, Out>>,
    now_ms: u64,
    usb_read_ms: f64,
    loop_start: Instant,
) -> Result<RxBatchPayload, String> {
    let parse_start = Instant::now();
    let packets =
        parse_rx_aggregate(bytes).map_err(|err| format!("RX aggregate parse failed: {err}"))?;
    let parse_ms = elapsed_ms(parse_start);

    let mut frames = Vec::new();
    let mut accepted_packets = 0usize;
    let mut crc_dropped = 0usize;
    let mut icv_dropped = 0usize;
    let mut report_dropped = 0usize;
    let mut ignored_frames = 0usize;
    let mut sessions = 0usize;
    let mut wfb_payloads = 0usize;
    let mut rtp_packets = 0usize;
    let mut video_frames = 0usize;
    let mut adaptive_rx_ms = 0.0;

    let pipeline_start = Instant::now();
    let packet_count = packets.len();
    for packet in packets {
        if packet.attrib.crc_err {
            crc_dropped += 1;
            continue;
        }
        if packet.attrib.icv_err {
            icv_dropped += 1;
            continue;
        }
        if packet.attrib.pkt_rpt_type != RxPacketType::NormalRx {
            report_dropped += 1;
            continue;
        }
        accepted_packets += 1;

        if let Some(runtime) = adaptive.as_deref_mut() {
            let adaptive_rx_start = Instant::now();
            if pipeline.accepts_80211_frame(packet.data) {
                runtime.record_rx(now_ms, &packet.attrib);
            }
            adaptive_rx_ms += elapsed_ms(adaptive_rx_start);
        }

        let events = pipeline
            .push_80211_frame(packet.data)
            .map_err(|err| format!("OpenIPC frame rejected: {err}"))?;
        for event in events {
            match event {
                PipelineEvent::IgnoredFrame => ignored_frames += 1,
                PipelineEvent::SessionEstablished { .. } => sessions += 1,
                PipelineEvent::WfbPayload { .. } => wfb_payloads += 1,
                PipelineEvent::RtpPacket { .. } => rtp_packets += 1,
                PipelineEvent::VideoFrame(frame) => {
                    video_frames += 1;
                    frames.push(video_frame_payload(frame));
                }
            }
        }
    }
    let pipeline_ms = elapsed_ms(pipeline_start);
    let counters = pipeline.fec_counters();

    let mut link_quality = None;
    let mut adaptive_quality_ms = 0.0;
    let mut adaptive_tx_ms = 0.0;
    let mut adaptive_tx_frames = 0usize;
    let mut adaptive_tx_errors = 0usize;
    if let Some(runtime) = adaptive {
        let quality_start = Instant::now();
        runtime.record_pipeline(now_ms, counters);
        link_quality = Some(runtime.quality(now_ms));
        adaptive_quality_ms = elapsed_ms(quality_start);

        if let Some(ep_out) = ep_out {
            let tx_start = Instant::now();
            match runtime.tick(now_ms, ep_out) {
                Ok(count) => adaptive_tx_frames = count,
                Err(_) => adaptive_tx_errors = 1,
            }
            adaptive_tx_ms = elapsed_ms(tx_start);
        }
    }

    Ok(RxBatchPayload {
        frames,
        transfer_bytes: bytes.len(),
        packets: packet_count,
        accepted_packets,
        dropped_packets: crc_dropped + icv_dropped + report_dropped,
        crc_dropped,
        icv_dropped,
        report_dropped,
        ignored_frames,
        sessions,
        wfb_payloads,
        rtp_packets,
        video_frames,
        parse_ms,
        pipeline_ms,
        total_ms: elapsed_ms(loop_start),
        fec_counters: fec_counters_payload(counters),
        link_quality,
        adaptive_tx_frames,
        adaptive_tx_errors,
        usb_read_ms,
        adaptive_rx_ms,
        adaptive_quality_ms,
        tx_power_ms: 0.0,
        adaptive_tx_ms,
    })
}

fn emit_log(app: &AppHandle, level: &'static str, message: String) {
    let _ = app.emit(LOG_EVENT, LogPayload { level, message });
}

fn emit_stopped(app: &AppHandle, reason: &'static str, message: String) {
    let _ = app.emit(
        STOPPED_EVENT,
        StoppedPayload {
            reason,
            message: message.clone(),
        },
    );
    let level = if reason == "error" { "error" } else { "info" };
    emit_log(app, level, message);
}

fn radio_config(
    channel: u8,
    channel_width_mhz: u16,
    channel_offset: u8,
) -> Result<RadioConfig, String> {
    let channel_width = match channel_width_mhz {
        20 => ChannelWidth::Mhz20,
        40 => ChannelWidth::Mhz40,
        80 => ChannelWidth::Mhz80,
        _ => return Err(format!("unsupported channel width {channel_width_mhz}")),
    };
    Ok(RadioConfig {
        channel,
        channel_offset,
        channel_width,
    })
}

fn init_report_payload(report: InitReport) -> InitReportPayload {
    let status = match report.status {
        InitStatus::AlreadyRunning => "already_running",
        InitStatus::Initialized => "initialized",
    };
    InitReportPayload {
        chip: report.chip.family.name().to_owned(),
        rf_paths: report.chip.total_rf_paths(),
        cut_version: report.chip.cut_version,
        status: status.to_owned(),
        firmware_downloaded: report.firmware_downloaded,
    }
}

fn fec_counters_payload(counters: FecCounters) -> FecCountersPayload {
    FecCountersPayload {
        total_packets: counters.total_packets,
        recovered_packets: counters.recovered_packets,
        lost_packets: counters.lost_packets,
        bad_packets: counters.bad_packets,
    }
}

fn video_frame_payload(frame: DepacketizedFrame) -> VideoFramePayload {
    let codec_string = codec_string(&frame);
    VideoFramePayload {
        data_base64: BASE64.encode(&frame.data),
        codec: codec_name(frame.codec),
        codec_string,
        is_key_frame: frame.is_keyframe,
        timestamp: frame.timestamp,
    }
}

fn codec_name(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "h264",
        Codec::H265 => "h265",
    }
}

fn codec_string(frame: &DepacketizedFrame) -> String {
    match frame.codec {
        Codec::H264 => h264_codec_string(&frame.data).unwrap_or_else(|| "avc1.42E01E".to_owned()),
        Codec::H265 => "hev1.1.6.L93.B0".to_owned(),
    }
}

fn h264_codec_string(frame: &[u8]) -> Option<String> {
    for unit in annex_b_units(frame) {
        let nalu = &frame[unit.start..unit.end];
        if nalu.len() >= 4 && nalu[0] & 0x1f == 7 {
            return Some(format!(
                "avc1.{}{}{}",
                hex_byte(nalu[1]),
                hex_byte(nalu[2]),
                hex_byte(nalu[3])
            ));
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct AnnexBUnit {
    start: usize,
    end: usize,
}

fn annex_b_units(frame: &[u8]) -> Vec<AnnexBUnit> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 < frame.len() {
        let len = start_code_len(frame, index);
        if len > 0 {
            starts.push(index);
            index += len;
        } else {
            index += 1;
        }
    }
    let mut units = Vec::new();
    for (position, start_code) in starts.iter().enumerate() {
        let start = start_code + start_code_len(frame, *start_code);
        let end = starts.get(position + 1).copied().unwrap_or(frame.len());
        if start < end {
            units.push(AnnexBUnit { start, end });
        }
    }
    units
}

fn start_code_len(frame: &[u8], index: usize) -> usize {
    if index + 4 <= frame.len() && frame[index..index + 4] == [0, 0, 0, 1] {
        4
    } else if index + 3 <= frame.len() && frame[index..index + 3] == [0, 0, 1] {
        3
    } else {
        0
    }
}

fn hex_byte(byte: u8) -> String {
    format!("{byte:02X}")
}

fn usb_id(vendor_id: u16, product_id: u16) -> String {
    format!("{vendor_id:04x}:{product_id:04x}")
}

fn device_label(manufacturer: Option<&str>, product: Option<&str>, device_id: &str) -> String {
    let name = [manufacturer, product]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        device_id.to_owned()
    } else {
        format!("{name} ({device_id})")
    }
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
