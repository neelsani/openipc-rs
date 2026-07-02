use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use super::{RuntimeEvent, StartRequest, UsbDeviceInfo};

type EventQueue = Arc<Mutex<VecDeque<RuntimeEvent>>>;

#[derive(Default)]
struct RecordingControl {
    start: Option<PathBuf>,
    stop: bool,
}

/// Native worker owner used by the egui event loop.
pub(crate) struct Runtime {
    events: EventQueue,
    stop: Arc<AtomicBool>,
    audio_volume: Arc<AtomicU8>,
    recording: Arc<Mutex<RecordingControl>>,
    worker: Option<JoinHandle<()>>,
    context: eframe::egui::Context,
}

impl Runtime {
    pub(crate) fn new(context: eframe::egui::Context) -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::new())),
            stop: Arc::new(AtomicBool::new(false)),
            audio_volume: Arc::new(AtomicU8::new(100)),
            recording: Arc::new(Mutex::new(RecordingControl::default())),
            worker: None,
            context,
        }
    }

    pub(crate) fn refresh_devices(&self) {
        let events = Arc::clone(&self.events);
        let context = self.context.clone();
        thread::spawn(move || {
            let event = match discover_devices() {
                Ok(devices) => {
                    RuntimeEvent::Devices(devices.into_iter().map(device_info).collect())
                }
                Err(error) => {
                    RuntimeEvent::DiscoveryFailed(format!("USB discovery failed: {error}"))
                }
            };
            queue(&events, event);
            context.request_repaint();
        });
    }

    pub(crate) fn start(&mut self, request: StartRequest, context: eframe::egui::Context) {
        self.stop();
        *self.recording.lock().expect("recording control poisoned") = RecordingControl::default();
        self.stop = Arc::new(AtomicBool::new(false));
        self.audio_volume
            .store(request.audio_volume.min(100), Ordering::Relaxed);
        let stop = Arc::clone(&self.stop);
        let audio_volume = Arc::clone(&self.audio_volume);
        let events = Arc::clone(&self.events);
        let recording = Arc::clone(&self.recording);
        self.worker = Some(thread::spawn(move || {
            queue(&events, RuntimeEvent::Connecting);
            context.request_repaint();
            if let Err(error) = super::native::worker::run(
                request,
                &stop,
                &audio_volume,
                &recording,
                &events,
                &context,
            ) {
                queue(&events, RuntimeEvent::Failed(error));
                context.request_repaint();
            }
        }));
    }

    #[cfg(debug_assertions)]
    pub(crate) fn start_codec_mock(
        &mut self,
        request: StartRequest,
        context: eframe::egui::Context,
    ) {
        self.stop();
        *self.recording.lock().expect("recording control poisoned") = RecordingControl::default();
        self.stop = Arc::new(AtomicBool::new(false));
        self.audio_volume
            .store(request.audio_volume.min(100), Ordering::Relaxed);
        let stop = Arc::clone(&self.stop);
        let audio_volume = Arc::clone(&self.audio_volume);
        let events = Arc::clone(&self.events);
        let recording = Arc::clone(&self.recording);
        self.worker = Some(thread::spawn(move || {
            queue(&events, RuntimeEvent::Connecting);
            context.request_repaint();
            if let Err(error) = super::native::worker::run_codec_mock(
                request,
                &stop,
                &audio_volume,
                &recording,
                &events,
                &context,
            ) {
                queue(&events, RuntimeEvent::Failed(error));
                context.request_repaint();
            }
        }));
    }

    pub(crate) fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        *self.recording.lock().expect("recording control poisoned") = RecordingControl::default();
    }

    pub(crate) fn set_audio_volume(&self, volume: u8) {
        self.audio_volume.store(volume.min(100), Ordering::Relaxed);
    }

    pub(crate) fn start_recording(&self, path: PathBuf) {
        let display = path.display().to_string();
        let mut control = self.recording.lock().expect("recording control poisoned");
        control.start = Some(path);
        control.stop = false;
        drop(control);
        queue(&self.events, RuntimeEvent::RecordingArmed(display));
        self.context.request_repaint();
    }

    pub(crate) fn stop_recording(&self) {
        self.recording
            .lock()
            .expect("recording control poisoned")
            .stop = true;
    }

    pub(crate) fn drain(&self) -> impl Iterator<Item = RuntimeEvent> {
        self.events
            .lock()
            .expect("Nebulus event queue poisoned")
            .drain(..)
            .collect::<Vec<_>>()
            .into_iter()
    }
}

fn queue(events: &EventQueue, event: RuntimeEvent) {
    super::queue_event(
        &mut events.lock().expect("Nebulus event queue poisoned"),
        event,
    );
}

#[cfg(not(target_os = "android"))]
fn discover_devices() -> Result<Vec<openipc_rtl88xx::UsbDeviceSummary>, String> {
    openipc_rtl88xx::list_supported_devices().map_err(|error| error.to_string())
}

#[cfg(not(target_os = "android"))]
fn device_info(device: openipc_rtl88xx::UsbDeviceSummary) -> UsbDeviceInfo {
    UsbDeviceInfo {
        id: format!("{:04x}:{:04x}", device.vendor_id, device.product_id),
        label: device
            .product
            .or(device.manufacturer)
            .unwrap_or_else(|| format!("{:04x}:{:04x}", device.vendor_id, device.product_id)),
        vendor_id: device.vendor_id,
        product_id: device.product_id,
    }
}

#[cfg(target_os = "android")]
fn discover_devices() -> Result<Vec<UsbDeviceInfo>, String> {
    crate::android::list_devices()
}

#[cfg(target_os = "android")]
fn device_info(device: UsbDeviceInfo) -> UsbDeviceInfo {
    device
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
    }
}

mod worker {
    use std::{
        fs::File,
        io::{BufWriter, Write as _},
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicU8, Ordering},
            Arc, Mutex,
        },
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use nusb::transfer::TransferError;
    use nusb::MaybeFuture;
    use openipc_core::{
        realtek::{parse_rx_aggregate_with_kind, RxPacketType},
        AdaptiveLink, AdaptiveLinkSender, ChannelId, FecCounters, FrameLayout, PayloadRouteId,
        RadioPort, ReceiverRuntime, TxRadioParams, WfbKeypair, WfbTransmitter, WfbTxKeypair,
    };
    use openipc_rtl88xx::{
        ChannelWidth, ChipFamily, DriverOptions, Jaguar3PowerTrackingState, MonitorOptions,
        RadioConfig, RealtekDevice, RealtekTxDescriptor, RealtekTxOptions,
    };
    use openipc_video::{DecoderOptions, PlatformDecoder, VideoDecoder};

    use crate::{
        model::LogLevel,
        runtime::{
            route_runtime::{configure_receiver, RouteProcessor, VPN_ROUTE_ID},
            BatchMetrics, RuntimeEvent, StartRequest, VpnMetrics,
        },
    };

    struct EncodedRecorder {
        path: PathBuf,
        writer: BufWriter<File>,
        codec: openipc_core::Codec,
        bytes: u64,
    }

    impl EncodedRecorder {
        fn start(path: PathBuf, frame: &openipc_core::DepacketizedFrame) -> Result<Self, String> {
            let file = File::create(&path)
                .map_err(|error| format!("create recording {} failed: {error}", path.display()))?;
            let mut recorder = Self {
                path,
                writer: BufWriter::with_capacity(256 * 1024, file),
                codec: frame.codec,
                bytes: 0,
            };
            recorder.write(frame)?;
            Ok(recorder)
        }

        fn write(&mut self, frame: &openipc_core::DepacketizedFrame) -> Result<(), String> {
            if frame.codec != self.codec {
                return Ok(());
            }
            self.writer
                .write_all(&frame.data)
                .map_err(|error| format!("write recording failed: {error}"))?;
            self.bytes = self.bytes.saturating_add(frame.data.len() as u64);
            Ok(())
        }

        fn finish(mut self) -> Result<(String, u64), String> {
            self.writer
                .flush()
                .map_err(|error| format!("flush recording failed: {error}"))?;
            Ok((self.path.display().to_string(), self.bytes))
        }
    }

    const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);
    const RX_TRANSFERS_IN_FLIGHT: usize = 4;

    struct LinkRuntime {
        quality: AdaptiveLink,
        sender: Option<AdaptiveLinkSender>,
        last_fec: FecCounters,
        tx_options: RealtekTxOptions,
    }

    struct TunRuntime {
        bridge: crate::tun_bridge::TunBridge,
        transmitter: WfbTransmitter,
        tx_options: RealtekTxOptions,
        tx_params: TxRadioParams,
        last_session_ms: Option<u64>,
        metrics: VpnMetrics,
    }

    impl TunRuntime {
        fn new(request: &StartRequest, chip: ChipFamily) -> Result<Self, String> {
            let bridge = crate::tun_bridge::TunBridge::open_default()?;
            let interface_name = bridge.name().to_owned();
            let keypair = WfbTxKeypair::from_bytes(&request.key_bytes)
                .map_err(|error| format!("VPN transmit key is invalid: {error}"))?;
            let transmitter = WfbTransmitter::new(
                ChannelId::from_link_port(request.channel_id >> 8, RadioPort::TunnelTx),
                keypair,
                0,
                1,
                5,
            )
            .map_err(|error| error.to_string())?;
            Ok(Self {
                bridge,
                transmitter,
                tx_options: RealtekTxOptions {
                    current_channel: request.channel,
                    descriptor: RealtekTxDescriptor::for_chip_family(chip),
                    ..RealtekTxOptions::default()
                },
                tx_params: TxRadioParams::openipc_uplink_default(),
                last_session_ms: None,
                metrics: VpnMetrics {
                    active: true,
                    interface_name,
                    ..VpnMetrics::default()
                },
            })
        }

        fn write_downlink(&mut self, payload: &[u8]) {
            match self.bridge.write_downlink(payload) {
                Ok(0) => {}
                Ok(bytes) => {
                    self.metrics.downlink_packets += 1;
                    self.metrics.downlink_bytes += bytes as u64;
                }
                Err(_) => self.metrics.errors += 1,
            }
        }

        fn tick(
            &mut self,
            now: u64,
            endpoint: &mut nusb::Endpoint<nusb::transfer::Bulk, nusb::transfer::Out>,
        ) {
            let session_due = self
                .last_session_ms
                .is_none_or(|last| now.saturating_sub(last) >= 1_000);
            if session_due {
                let frame = self.transmitter.session_radio_packet(self.tx_params);
                if RealtekDevice::send_packet_on(endpoint, &frame, self.tx_options).is_ok() {
                    self.last_session_ms = Some(now);
                } else {
                    self.metrics.errors += 1;
                }
            }
            for _ in 0..32 {
                let payload = match self.bridge.read_uplink() {
                    Ok(Some(payload)) => payload,
                    Ok(None) => break,
                    Err(_) => {
                        self.metrics.errors += 1;
                        break;
                    }
                };
                let payload_bytes = payload.len() as u64;
                match self
                    .transmitter
                    .radio_packets_for_payload(&payload, self.tx_params)
                {
                    Ok(frames) => {
                        let mut sent = true;
                        for frame in frames {
                            if RealtekDevice::send_packet_on(endpoint, &frame, self.tx_options)
                                .is_err()
                            {
                                sent = false;
                                self.metrics.errors += 1;
                                break;
                            }
                        }
                        if sent {
                            self.metrics.uplink_packets += 1;
                            self.metrics.uplink_bytes += payload_bytes;
                        }
                    }
                    Err(_) => self.metrics.errors += 1,
                }
            }
        }
    }

    pub(super) fn run(
        request: StartRequest,
        stop: &AtomicBool,
        audio_volume: &AtomicU8,
        recording_control: &Mutex<super::RecordingControl>,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) -> Result<(), String> {
        let mut driver = DriverOptions::default();
        if let Some(device_id) = &request.device_id {
            if let Some((vendor, product)) = parse_device_id(device_id) {
                driver.target_vendor_id = Some(vendor);
                driver.target_product_id = Some(product);
            }
        }
        #[cfg(not(target_os = "android"))]
        let (device, device_label) = (
            Arc::new(RealtekDevice::open_first(driver).map_err(|error| error.to_string())?),
            request
                .device_id
                .clone()
                .unwrap_or_else(|| "Realtek USB adapter".to_owned()),
        );
        #[cfg(target_os = "android")]
        let (device, _android_connection, device_label) = {
            driver.skip_reset = true;
            let opened = crate::android::open_device(request.device_id.as_deref())?;
            let label = opened.info.label.clone();
            let device = RealtekDevice::from_nusb_device(opened.device, driver)
                .map_err(|error| error.to_string())?;
            (Arc::new(device), opened.connection, label)
        };
        let radio = RadioConfig {
            channel: request.channel,
            channel_width: channel_width(request.channel_width_mhz)?,
            channel_offset: request.channel_offset,
        };
        let report = device
            .initialize_monitor_with_options(radio, MonitorOptions::from_env())
            .map_err(|error| error.to_string())?;
        let chip = report.chip.family;
        let mut decoder = PlatformDecoder::new(DecoderOptions::default())
            .map_err(|error| format!("video decoder unavailable: {error}"))?;
        let decoder_environment = decoder_environment(decoder.capabilities());
        send(
            events,
            context,
            RuntimeEvent::Connected {
                label: device_label,
                chip: chip.name().to_owned(),
                decoder: decoder_environment,
            },
        );

        let keypair =
            WfbKeypair::from_bytes(&request.key_bytes).map_err(|error| error.to_string())?;
        let mut receiver = ReceiverRuntime::with_keyed_video_route(
            FrameLayout::WithFcs,
            VIDEO_ROUTE,
            ChannelId::new(request.channel_id),
            0,
            keypair,
            request.minimum_epoch,
        )
        .map_err(|error| error.to_string())?;
        receiver.set_rtp_reorder_enabled(request.rtp_reorder);
        let options = configure_receiver(&mut receiver, &request)?;
        let mut route_processor = RouteProcessor::new(&request)?;
        for entry in route_processor.take_startup_logs() {
            log(
                events,
                context,
                if entry.warning {
                    LogLevel::Warn
                } else {
                    LogLevel::Info
                },
                "route",
                entry.message,
            );
        }
        let mut link = build_link(&request, chip, receiver.video_fec_counters(), &device)?;
        let mut tun = request
            .vpn_enabled
            .then(|| TunRuntime::new(&request, chip))
            .transpose()?;
        if let Some(tun) = tun.as_ref() {
            log(
                events,
                context,
                LogLevel::Info,
                "vpn",
                format!(
                    "VPN active on {} at {}/{}",
                    tun.bridge.name(),
                    crate::tun_bridge::ADDRESS,
                    crate::tun_bridge::PREFIX_LENGTH
                ),
            );
        }
        let mut ep_in = device
            .bulk_in_endpoint()
            .map_err(|error| error.to_string())?;
        let mut ep_out = (link.sender.is_some() || tun.is_some())
            .then(|| {
                device
                    .bulk_out_endpoint()
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        while ep_in.pending() < RX_TRANSFERS_IN_FLIGHT {
            ep_in.submit(ep_in.allocate(request.transfer_size));
        }
        send(events, context, RuntimeEvent::Started);
        log(events, context, LogLevel::Info, "rx", "Receiver started");

        let descriptor = device.rx_descriptor_kind();
        let mut last_coex_ms = 0;
        let mut power_tracking = Jaguar3PowerTrackingState::default();
        let mut last_decode_errors = 0;
        let mut recorder: Option<EncodedRecorder> = None;
        let mut armed_path: Option<PathBuf> = None;
        while !stop.load(Ordering::Relaxed) {
            let usb_start = Instant::now();
            let Some(completion) = ep_in.wait_next_complete(Duration::from_millis(250)) else {
                update_recording(
                    &[],
                    recording_control,
                    &mut armed_path,
                    &mut recorder,
                    events,
                    context,
                );
                tick_maintenance(&device, chip, &mut last_coex_ms, &mut power_tracking);
                tick_adaptive(&mut link, ep_out.as_mut(), now_ms(), events, context);
                if let (Some(tun), Some(endpoint)) = (tun.as_mut(), ep_out.as_mut()) {
                    tun.tick(now_ms(), endpoint);
                }
                continue;
            };
            let usb_latency_ms = usb_start.elapsed().as_secs_f64() * 1000.0;
            let actual_len = completion.actual_len;
            if let Err(error) = completion.status {
                if error == TransferError::Stall {
                    let _ = ep_in.clear_halt().wait();
                }
                log(
                    events,
                    context,
                    LogLevel::Warn,
                    "usb",
                    format!("bulk IN failed: {error}"),
                );
                ep_in.submit(completion.buffer);
                continue;
            }
            let bytes = &completion.buffer[..actual_len];
            let batch_start = Instant::now();
            let parse_start = Instant::now();
            let packets = match parse_rx_aggregate_with_kind(bytes, descriptor) {
                Ok(packets) => packets,
                Err(error) => {
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "usb",
                        format!("RX aggregate rejected: {error}"),
                    );
                    ep_in.submit(completion.buffer);
                    continue;
                }
            };
            let parse_latency_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            let now = now_ms();
            for packet in &packets {
                if !packet.attrib.crc_err
                    && !packet.attrib.icv_err
                    && packet.attrib.pkt_rpt_type == RxPacketType::NormalRx
                    && receiver.accepts_video_frame(packet.data)
                {
                    link.record_rx(now, packet.attrib.rssi, packet.attrib.snr);
                }
            }
            let pipeline_start = Instant::now();
            let batch = receiver.push_rx_packets(packets, &options);
            let pipeline_latency_ms = pipeline_start.elapsed().as_secs_f64() * 1000.0;
            if let Some(tun) = tun.as_mut() {
                for payload in &batch.raw_payloads {
                    if payload.route_id == VPN_ROUTE_ID {
                        tun.write_downlink(&payload.data);
                    }
                }
            }
            update_recording(
                &batch.frames,
                recording_control,
                &mut armed_path,
                &mut recorder,
                events,
                context,
            );
            route_processor.set_audio_volume(audio_volume.load(Ordering::Relaxed));
            let route_start = Instant::now();
            let (route_updates, route_logs) = route_processor.process(&batch.raw_payloads);
            let route_latency_ms = route_start.elapsed().as_secs_f64() * 1000.0;
            for entry in route_logs {
                log(
                    events,
                    context,
                    if entry.warning {
                        LogLevel::Warn
                    } else {
                        LogLevel::Info
                    },
                    "route",
                    entry.message,
                );
            }
            link.record_fec(now, batch.fec_counters);
            let quality = link.quality.quality(now);
            let decode_submit_start = Instant::now();
            for frame in batch
                .frames
                .iter()
                .filter(|frame| request.codec_preference.accepts(frame.codec))
                .cloned()
            {
                if let Err(error) = decoder.submit(frame.into()) {
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "decoder",
                        format!("decode submit failed: {error}"),
                    );
                }
            }
            let decode_submit_latency_ms = decode_submit_start.elapsed().as_secs_f64() * 1000.0;
            if let Some(decoded) = decoder.latest_frame() {
                send_decoded(events, context, decoded, &decoder);
            }
            let stats = decoder.stats();
            send(
                events,
                context,
                RuntimeEvent::Batch(Box::new(BatchMetrics {
                    transfers: 1,
                    transfer_bytes: actual_len,
                    packets: batch.counters.packets,
                    rtp_packets: batch.counters.rtp_packets,
                    video_frames: batch.counters.video_frames,
                    video_bytes: batch.frames.iter().map(|frame| frame.data.len()).sum(),
                    usb_latency_ms,
                    parse_latency_ms,
                    pipeline_latency_ms,
                    route_latency_ms,
                    decode_submit_latency_ms,
                    batch_latency_ms: batch_start.elapsed().as_secs_f64() * 1000.0,
                    rssi: quality.rssi,
                    snr: quality.snr,
                    link_score: quality.link_score,
                    decoder_drops: stats.waiting_drops
                        + stats.backpressure_drops
                        + stats.output_drops,
                    decoder_errors: stats.decode_errors,
                    fec: batch.fec_counters,
                    counters: batch.counters,
                    rtp: batch.rtp_status,
                    reorder: batch.rtp_reorder_status,
                    routes: route_updates,
                    audio: route_processor.audio_stats(),
                    vpn: tun
                        .as_ref()
                        .map_or_else(VpnMetrics::default, |tun| tun.metrics.clone()),
                })),
            );
            if stats.decode_errors > last_decode_errors {
                last_decode_errors = stats.decode_errors;
                log(
                    events,
                    context,
                    LogLevel::Warn,
                    "decoder",
                    format!("decoder errors: {last_decode_errors}"),
                );
            }
            tick_maintenance(&device, chip, &mut last_coex_ms, &mut power_tracking);
            tick_adaptive(&mut link, ep_out.as_mut(), now, events, context);
            if let (Some(tun), Some(endpoint)) = (tun.as_mut(), ep_out.as_mut()) {
                tun.tick(now, endpoint);
            }
            ep_in.submit(completion.buffer);
        }

        drop(ep_in);
        drop(ep_out);
        finish_recording(&mut recorder, events, context);
        let _ = decoder.flush();
        device
            .shutdown_monitor()
            .map_err(|error| format!("monitor shutdown failed: {error}"))?;
        send(events, context, RuntimeEvent::Stopped);
        Ok(())
    }

    #[cfg(debug_assertions)]
    pub(super) fn run_codec_mock(
        request: StartRequest,
        stop: &AtomicBool,
        audio_volume: &AtomicU8,
        recording_control: &Mutex<super::RecordingControl>,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) -> Result<(), String> {
        use crate::runtime::{
            codec_mock::MockAvStream,
            route_runtime::{configure_mock_receiver, RouteProcessor},
        };

        let mut decoder = PlatformDecoder::new(DecoderOptions::default())
            .map_err(|error| format!("native mock decoder unavailable: {error}"))?;
        let mut route_processor = RouteProcessor::new(&request)?;
        for entry in route_processor.take_startup_logs() {
            log(
                events,
                context,
                if entry.warning {
                    LogLevel::Warn
                } else {
                    LogLevel::Info
                },
                "route",
                entry.message,
            );
        }
        send(
            events,
            context,
            RuntimeEvent::Connected {
                label: "Pre-recorded 1080p H.264 + Opus".to_owned(),
                chip: "Synthetic A/V RTP".to_owned(),
                decoder: decoder_environment(decoder.capabilities()),
            },
        );
        send(events, context, RuntimeEvent::Started);
        log(
            events,
            context,
            LogLevel::Info,
            "mock",
            "Pre-recorded 1080p H.264 + Opus RTP mock started",
        );

        let mut receiver = ReceiverRuntime::with_mock_video_route(
            FrameLayout::WithFcs,
            VIDEO_ROUTE,
            ChannelId::default_video(),
            0,
        );
        receiver.set_rtp_reorder_enabled(request.rtp_reorder);
        let options = configure_mock_receiver(&mut receiver, &request);
        let runtime = receiver.video_runtime();
        let mut source = MockAvStream::new()?;
        let mut payload_sequence = 1u64;
        let mut recorder: Option<EncodedRecorder> = None;
        let mut armed_path: Option<PathBuf> = None;

        while !stop.load(Ordering::Relaxed) {
            let loop_started = Instant::now();
            let event = source.next_event();
            let mut metrics = BatchMetrics {
                transfers: 1,
                transfer_bytes: event.packets.iter().map(Vec::len).sum(),
                packets: event.packets.len(),
                rtp_packets: event.packets.len(),
                ..BatchMetrics::default()
            };
            for packet in event.packets {
                let batch = receiver
                    .push_mock_payload(runtime, payload_sequence, &packet, &options)
                    .map_err(|error| format!("mock payload route failed: {error}"))?;
                payload_sequence = payload_sequence.wrapping_add(1);
                metrics.video_frames += batch.frames.len();
                metrics.video_bytes = metrics
                    .video_bytes
                    .saturating_add(batch.frames.iter().map(|frame| frame.data.len()).sum());
                update_recording(
                    &batch.frames,
                    recording_control,
                    &mut armed_path,
                    &mut recorder,
                    events,
                    context,
                );
                route_processor.set_audio_volume(audio_volume.load(Ordering::Relaxed));
                let (route_updates, route_logs) = route_processor.process(&batch.raw_payloads);
                metrics.merge(BatchMetrics {
                    routes: route_updates,
                    counters: batch.counters,
                    rtp: batch.rtp_status,
                    reorder: batch.rtp_reorder_status,
                    ..BatchMetrics::default()
                });
                for entry in route_logs {
                    log(
                        events,
                        context,
                        if entry.warning {
                            LogLevel::Warn
                        } else {
                            LogLevel::Info
                        },
                        "route",
                        entry.message,
                    );
                }
                for frame in batch
                    .frames
                    .into_iter()
                    .filter(|frame| request.codec_preference.accepts(frame.codec))
                {
                    decoder
                        .submit(frame.into())
                        .map_err(|error| format!("mock decode submit failed: {error}"))?;
                }
            }
            if let Some(decoded) = decoder.latest_frame() {
                send_decoded(events, context, decoded, &decoder);
            }
            let stats = decoder.stats();
            metrics.pipeline_latency_ms = loop_started.elapsed().as_secs_f64() * 1_000.0;
            metrics.batch_latency_ms = metrics.pipeline_latency_ms;
            metrics.decoder_drops =
                stats.waiting_drops + stats.backpressure_drops + stats.output_drops;
            metrics.decoder_errors = stats.decode_errors;
            metrics.audio = route_processor.audio_stats();
            send(events, context, RuntimeEvent::Batch(Box::new(metrics)));
            if let Some(remaining) =
                Duration::from_micros(event.delay_micros).checked_sub(loop_started.elapsed())
            {
                std::thread::sleep(remaining);
            }
        }

        let _ = decoder.flush();
        finish_recording(&mut recorder, events, context);
        log(
            events,
            context,
            LogLevel::Info,
            "mock",
            "Codec mock stopped",
        );
        send(events, context, RuntimeEvent::Stopped);
        Ok(())
    }

    fn update_recording(
        frames: &[openipc_core::DepacketizedFrame],
        control: &Mutex<super::RecordingControl>,
        armed_path: &mut Option<PathBuf>,
        recorder: &mut Option<EncodedRecorder>,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) {
        let (start, stop) = {
            let mut control = control.lock().expect("recording control poisoned");
            (control.start.take(), std::mem::take(&mut control.stop))
        };
        if let Some(path) = start {
            finish_recording(recorder, events, context);
            *armed_path = Some(path);
        }
        if stop {
            *armed_path = None;
            finish_recording(recorder, events, context);
        }

        for frame in frames {
            if recorder.is_none() && frame.is_keyframe {
                let Some(path) = armed_path.take() else {
                    continue;
                };
                match EncodedRecorder::start(path, frame) {
                    Ok(started) => {
                        send(
                            events,
                            context,
                            RuntimeEvent::RecordingStarted {
                                path: started.path.display().to_string(),
                                codec: format!("{:?}", started.codec),
                            },
                        );
                        *recorder = Some(started);
                    }
                    Err(error) => send(events, context, RuntimeEvent::RecordingFailed(error)),
                }
                continue;
            }
            if let Some(active) = recorder.as_mut() {
                if let Err(error) = active.write(frame) {
                    send(events, context, RuntimeEvent::RecordingFailed(error));
                    *recorder = None;
                }
            }
        }
    }

    fn decoder_environment(
        capabilities: openipc_video::DecoderCapabilities,
    ) -> crate::runtime::DecoderEnvironment {
        let codec = |wanted| capabilities.codec(wanted);
        let h264 = codec(openipc_video::VideoCodec::H264);
        let h265 = codec(openipc_video::VideoCodec::H265);
        crate::runtime::DecoderEnvironment {
            backend: capabilities.backend.to_owned(),
            h264_supported: h264.is_some_and(|entry| entry.supported),
            h265_supported: h265.is_some_and(|entry| entry.supported),
            h264_hardware: h264.and_then(|entry| {
                entry
                    .hardware_acceleration_known
                    .then_some(entry.hardware_accelerated)
            }),
            h265_hardware: h265.and_then(|entry| {
                entry
                    .hardware_acceleration_known
                    .then_some(entry.hardware_accelerated)
            }),
            native_surfaces: capabilities.native_surfaces,
        }
    }

    fn finish_recording(
        recorder: &mut Option<EncodedRecorder>,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) {
        let Some(active) = recorder.take() else {
            return;
        };
        match active.finish() {
            Ok((path, bytes)) => send(
                events,
                context,
                RuntimeEvent::RecordingStopped { path, bytes },
            ),
            Err(error) => send(events, context, RuntimeEvent::RecordingFailed(error)),
        }
    }

    impl LinkRuntime {
        fn record_rx(&mut self, now: u64, rssi: [u8; 4], snr: [i8; 4]) {
            self.quality.record_rx_paths(now, rssi, snr);
            if let Some(sender) = self.sender.as_mut() {
                sender.record_rx_paths(now, rssi, snr);
            }
        }

        fn record_fec(&mut self, now: u64, counters: FecCounters) {
            let total = counters
                .total_packets
                .saturating_sub(self.last_fec.total_packets);
            let recovered = counters
                .recovered_packets
                .saturating_sub(self.last_fec.recovered_packets);
            let lost = counters
                .lost_packets
                .saturating_sub(self.last_fec.lost_packets);
            self.last_fec = counters;
            let total = total.min(u64::from(u32::MAX)) as u32;
            let recovered = recovered.min(u64::from(u32::MAX)) as u32;
            let lost = lost.min(u64::from(u32::MAX)) as u32;
            self.quality.record_fec(now, total, recovered, lost);
            if let Some(sender) = self.sender.as_mut() {
                sender.record_fec(now, total, recovered, lost);
            }
        }
    }

    fn build_link(
        request: &StartRequest,
        chip: ChipFamily,
        fec: FecCounters,
        device: &RealtekDevice,
    ) -> Result<LinkRuntime, String> {
        let sender = if request.adaptive_link {
            device
                .set_tx_power_override(request.channel, request.tx_power)
                .map_err(|error| error.to_string())?;
            let keypair = WfbTxKeypair::from_bytes(&request.key_bytes)
                .map_err(|error| format!("adaptive-link key is invalid: {error}"))?;
            Some(
                AdaptiveLinkSender::new(request.channel_id >> 8, keypair, 0, 1, 5)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        Ok(LinkRuntime {
            quality: AdaptiveLink::new(),
            sender,
            last_fec: fec,
            tx_options: RealtekTxOptions {
                current_channel: request.channel,
                descriptor: RealtekTxDescriptor::for_chip_family(chip),
                ..RealtekTxOptions::default()
            },
        })
    }

    fn tick_adaptive(
        runtime: &mut LinkRuntime,
        ep_out: Option<&mut nusb::Endpoint<nusb::transfer::Bulk, nusb::transfer::Out>>,
        now: u64,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) {
        let (Some(sender), Some(ep_out)) = (runtime.sender.as_mut(), ep_out) else {
            return;
        };
        match sender.tick(now) {
            Ok(frames) => {
                for frame in frames {
                    if let Err(error) =
                        RealtekDevice::send_packet_on(ep_out, &frame, runtime.tx_options)
                    {
                        log(
                            events,
                            context,
                            LogLevel::Warn,
                            "adaptive",
                            error.to_string(),
                        );
                    }
                }
            }
            Err(error) => log(
                events,
                context,
                LogLevel::Warn,
                "adaptive",
                error.to_string(),
            ),
        }
    }

    fn tick_maintenance(
        device: &RealtekDevice,
        chip: ChipFamily,
        last_coex_ms: &mut u64,
        power_tracking: &mut Jaguar3PowerTrackingState,
    ) {
        let now = now_ms();
        if !chip.is_jaguar3() || now.saturating_sub(*last_coex_ms) < 2_000 {
            return;
        }
        *last_coex_ms = now;
        let _ = device.run_jaguar3_coex_keepalive();
        let _ = device.tick_jaguar3_power_tracking(power_tracking);
    }

    fn channel_width(width: u16) -> Result<ChannelWidth, String> {
        match width {
            5 => Ok(ChannelWidth::Mhz5),
            10 => Ok(ChannelWidth::Mhz10),
            20 => Ok(ChannelWidth::Mhz20),
            40 => Ok(ChannelWidth::Mhz40),
            80 => Ok(ChannelWidth::Mhz80),
            _ => Err(format!("unsupported channel width {width} MHz")),
        }
    }

    fn parse_device_id(value: &str) -> Option<(u16, u16)> {
        let (vendor, product) = value.split_once(':')?;
        Some((
            u16::from_str_radix(vendor, 16).ok()?,
            u16::from_str_radix(product, 16).ok()?,
        ))
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }

    fn send(events: &super::EventQueue, context: &eframe::egui::Context, event: RuntimeEvent) {
        super::queue(events, event);
        context.request_repaint();
    }

    fn send_decoded(
        events: &super::EventQueue,
        context: &eframe::egui::Context,
        frame: openipc_video::DecodedFrame<<PlatformDecoder as VideoDecoder>::Surface>,
        decoder: &PlatformDecoder,
    ) {
        send(
            events,
            context,
            RuntimeEvent::NativeVideo {
                frame,
                decode_latency_ms: decoder.stats().last_decode_latency_us as f64 / 1_000.0,
            },
        );
    }

    fn log(
        events: &super::EventQueue,
        context: &eframe::egui::Context,
        level: LogLevel,
        target: &'static str,
        message: impl Into<String>,
    ) {
        send(
            events,
            context,
            RuntimeEvent::Log {
                level,
                target,
                message: message.into(),
            },
        );
    }
}
