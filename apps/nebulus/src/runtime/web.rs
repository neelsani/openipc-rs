use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

use super::{RuntimeEvent, StartRequest, UsbDeviceInfo};

#[derive(Default)]
struct RecordingControl {
    start: bool,
    stop: bool,
}

/// Browser receiver owner. Work stays on the browser's local executor because
/// WebUSB and WebCodecs handles are intentionally not `Send`.
pub(crate) struct Runtime {
    events: Rc<RefCell<VecDeque<RuntimeEvent>>>,
    cancel: Option<Rc<Cell<bool>>>,
    audio_volume: Rc<Cell<u8>>,
    recording: Rc<RefCell<RecordingControl>>,
    context: eframe::egui::Context,
}

impl Runtime {
    pub(crate) fn new(context: eframe::egui::Context) -> Self {
        Self {
            events: Rc::new(RefCell::new(VecDeque::new())),
            cancel: None,
            audio_volume: Rc::new(Cell::new(100)),
            recording: Rc::new(RefCell::new(RecordingControl::default())),
            context,
        }
    }

    pub(crate) fn refresh_devices(&self) {
        let events = Rc::clone(&self.events);
        let context = self.context.clone();
        spawn_local(async move {
            let event = match nusb::list_devices().await {
                Ok(devices) => RuntimeEvent::Devices(
                    devices
                        .filter(|device| {
                            openipc_rtl88xx::is_supported_id(
                                device.vendor_id(),
                                device.product_id(),
                            )
                        })
                        .map(|device| UsbDeviceInfo {
                            id: format!("{:04x}:{:04x}", device.vendor_id(), device.product_id()),
                            label: device
                                .product_string()
                                .or(device.manufacturer_string())
                                .map(str::to_owned)
                                .unwrap_or_else(|| {
                                    format!(
                                        "{:04x}:{:04x}",
                                        device.vendor_id(),
                                        device.product_id()
                                    )
                                }),
                            vendor_id: device.vendor_id(),
                            product_id: device.product_id(),
                        })
                        .collect(),
                ),
                Err(error) => {
                    RuntimeEvent::DiscoveryFailed(format!("WebUSB discovery failed: {error}"))
                }
            };
            emit(&events, &context, event);
        });
    }

    pub(crate) fn start(&mut self, request: StartRequest, context: eframe::egui::Context) {
        if let Some(cancel) = self.cancel.take() {
            cancel.set(true);
        }
        *self.recording.borrow_mut() = RecordingControl::default();

        let route_processor = match super::route_runtime::RouteProcessor::new(&request) {
            Ok(processor) => processor,
            Err(error) => {
                self.events
                    .borrow_mut()
                    .push_back(RuntimeEvent::Failed(error));
                context.request_repaint();
                return;
            }
        };

        // AudioContext and requestDevice must be created synchronously inside
        // the button event so browser user-gesture requirements remain valid.
        // The requestDevice call itself must happen synchronously inside the
        // button event so the browser still considers it a user gesture.
        let promise = match request_device(request.device_id.as_deref()) {
            Ok(promise) => promise,
            Err(error) => {
                self.events
                    .borrow_mut()
                    .push_back(RuntimeEvent::Failed(js_error(error)));
                context.request_repaint();
                return;
            }
        };
        let cancel = Rc::new(Cell::new(false));
        self.cancel = Some(Rc::clone(&cancel));
        self.audio_volume.set(request.audio_volume.min(100));
        let audio_volume = Rc::clone(&self.audio_volume);
        let events = Rc::clone(&self.events);
        let recording = Rc::clone(&self.recording);
        emit(&events, &context, RuntimeEvent::Connecting);

        spawn_local(async move {
            let completion_events = Rc::clone(&events);
            let completion_context = context.clone();
            let handles = worker::WorkerHandles {
                cancel,
                audio_volume,
                recording,
                events,
                context,
            };
            let result = worker::run(promise, request, route_processor, handles).await;
            if let Err(error) = result {
                emit(
                    &completion_events,
                    &completion_context,
                    RuntimeEvent::Failed(error),
                );
            } else {
                emit(
                    &completion_events,
                    &completion_context,
                    RuntimeEvent::Stopped,
                );
            }
        });
    }

    #[cfg(debug_assertions)]
    pub(crate) fn start_codec_mock(
        &mut self,
        request: StartRequest,
        context: eframe::egui::Context,
    ) {
        if let Some(cancel) = self.cancel.take() {
            cancel.set(true);
        }
        *self.recording.borrow_mut() = RecordingControl::default();
        let route_processor = match super::route_runtime::RouteProcessor::new(&request) {
            Ok(processor) => processor,
            Err(error) => {
                self.events
                    .borrow_mut()
                    .push_back(RuntimeEvent::Failed(error));
                context.request_repaint();
                return;
            }
        };
        let cancel = Rc::new(Cell::new(false));
        self.cancel = Some(Rc::clone(&cancel));
        self.audio_volume.set(request.audio_volume.min(100));
        let audio_volume = Rc::clone(&self.audio_volume);
        let events = Rc::clone(&self.events);
        let recording = Rc::clone(&self.recording);
        emit(&events, &context, RuntimeEvent::Connecting);
        spawn_local(async move {
            let completion_events = Rc::clone(&events);
            let completion_context = context.clone();
            let handles = worker::WorkerHandles {
                cancel,
                audio_volume,
                recording,
                events,
                context,
            };
            let result = worker::run_codec_mock(request, route_processor, handles).await;
            if let Err(error) = result {
                emit(
                    &completion_events,
                    &completion_context,
                    RuntimeEvent::Failed(error),
                );
            } else {
                emit(
                    &completion_events,
                    &completion_context,
                    RuntimeEvent::Stopped,
                );
            }
        });
    }

    pub(crate) fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.set(true);
        } else {
            self.events.borrow_mut().push_back(RuntimeEvent::Stopped);
        }
        *self.recording.borrow_mut() = RecordingControl::default();
    }

    pub(crate) fn set_audio_volume(&self, volume: u8) {
        self.audio_volume.set(volume.min(100));
    }

    pub(crate) fn start_recording(&self) {
        let mut control = self.recording.borrow_mut();
        control.start = true;
        control.stop = false;
        drop(control);
        emit(
            &self.events,
            &self.context,
            RuntimeEvent::RecordingArmed("Browser download".to_owned()),
        );
    }

    pub(crate) fn stop_recording(&self) {
        self.recording.borrow_mut().stop = true;
    }

    pub(crate) fn drain(&mut self) -> impl Iterator<Item = RuntimeEvent> {
        self.events
            .borrow_mut()
            .drain(..)
            .collect::<Vec<_>>()
            .into_iter()
    }
}

fn request_device(selected: Option<&str>) -> Result<js_sys::Promise<web_sys::UsbDevice>, JsValue> {
    let filters = selected
        .and_then(parse_device_id)
        .map(|(vendor_id, product_id)| vec![(vendor_id, product_id)])
        .unwrap_or_else(|| {
            openipc_rtl88xx::SUPPORTED_DEVICES
                .iter()
                .map(|device| (device.vendor_id, device.product_id))
                .collect()
        })
        .into_iter()
        .map(|(vendor_id, product_id)| {
            let filter = web_sys::UsbDeviceFilter::new();
            filter.set_vendor_id(vendor_id);
            filter.set_product_id(product_id);
            filter
        })
        .collect::<Vec<_>>();
    let options = web_sys::UsbDeviceRequestOptions::new(&filters);
    let usb = web_sys::window()
        .ok_or_else(|| JsValue::from_str("browser window is unavailable"))?
        .navigator()
        .usb();
    Ok(usb.request_device(&options))
}

fn parse_device_id(value: &str) -> Option<(u16, u16)> {
    let (vendor, product) = value.split_once(':')?;
    Some((
        u16::from_str_radix(vendor, 16).ok()?,
        u16::from_str_radix(product, 16).ok()?,
    ))
}

fn emit(
    events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
    context: &eframe::egui::Context,
    event: RuntimeEvent,
) {
    super::queue_event(&mut events.borrow_mut(), event);
    context.request_repaint();
}

fn js_error(error: JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

mod worker {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
        time::Duration,
    };

    use futures_util::future::{select, Either};
    use nusb::transfer::{Bulk, Completion, In, TransferError};
    use openipc_core::{
        realtek::{parse_rx_aggregate_with_kind, RxPacketType},
        AdaptiveLink, AdaptiveLinkSender, ChannelId, FecCounters, FrameLayout, PayloadRouteId,
        ReceiverRuntime, WfbKeypair, WfbTxKeypair,
    };
    use openipc_rtl88xx::{
        ChannelWidth, ChipFamily, Jaguar3PowerTrackingState, RadioConfig, RealtekDevice,
        RealtekTxDescriptor, RealtekTxOptions,
    };
    use openipc_video::{DecoderOptions, PlatformDecoder, VideoDecoder as _};
    use wasm_bindgen::JsValue;
    use wasm_bindgen_futures::JsFuture;
    use web_time::Instant;

    use crate::{
        model::LogLevel,
        runtime::{
            route_runtime::{configure_receiver, RouteProcessor},
            BatchMetrics, RuntimeEvent, StartRequest,
        },
    };

    const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);
    const RX_TRANSFERS_IN_FLIGHT: usize = 4;
    const MAX_BROWSER_RECORDING_BYTES: usize = 512 * 1024 * 1024;

    struct BrowserRecorder {
        codec: openipc_core::Codec,
        config: crate::recording::Mp4TrackConfig,
        audio_config: Option<crate::recording::AudioTrackConfig>,
        frames: Vec<crate::recording::RecordedAccessUnit>,
        audio_packets: Vec<crate::recording::RecordedAudioPacket>,
        bytes: usize,
    }

    impl BrowserRecorder {
        fn new(
            frame: &openipc_core::DepacketizedFrame,
            audio_config: Option<crate::recording::AudioTrackConfig>,
        ) -> Result<Self, String> {
            let config = crate::recording::Mp4TrackConfig::from_keyframe(frame)?;
            let mut recorder = Self {
                codec: frame.codec,
                config,
                audio_config,
                frames: Vec::new(),
                audio_packets: Vec::new(),
                bytes: 0,
            };
            if recorder.append(frame) {
                Ok(recorder)
            } else {
                Err("The first encoded frame exceeds the browser recording limit".to_owned())
            }
        }

        fn append(&mut self, frame: &openipc_core::DepacketizedFrame) -> bool {
            let Some(total) = self.bytes.checked_add(frame.data.len()) else {
                return false;
            };
            if total > MAX_BROWSER_RECORDING_BYTES {
                return false;
            }
            self.frames.push(frame.into());
            self.bytes = total;
            true
        }

        fn append_audio(&mut self, packet: crate::recording::RecordedAudioPacket) -> bool {
            let Some(total) = self.bytes.checked_add(packet.data.len()) else {
                return false;
            };
            if total > MAX_BROWSER_RECORDING_BYTES {
                return false;
            }
            self.audio_packets.push(packet);
            self.bytes = total;
            true
        }

        fn finish(self) -> Result<Vec<u8>, String> {
            let mut output = std::io::Cursor::new(Vec::new());
            crate::recording::mux_mp4(
                &mut output,
                &self.config,
                &self.frames,
                self.audio_config,
                &self.audio_packets,
            )?;
            Ok(output.into_inner())
        }
    }

    pub(super) struct WorkerHandles {
        pub(super) cancel: Rc<Cell<bool>>,
        pub(super) audio_volume: Rc<Cell<u8>>,
        pub(super) recording: Rc<RefCell<super::RecordingControl>>,
        pub(super) events: Rc<RefCell<VecDeque<RuntimeEvent>>>,
        pub(super) context: eframe::egui::Context,
    }
    const MAINTENANCE_INTERVAL_MS: u64 = 2_000;

    struct LinkRuntime {
        quality: AdaptiveLink,
        sender: Option<AdaptiveLinkSender>,
        last_fec: FecCounters,
        tx_options: RealtekTxOptions,
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

    pub(super) async fn run(
        permission: js_sys::Promise<web_sys::UsbDevice>,
        request: StartRequest,
        mut route_processor: RouteProcessor,
        handles: WorkerHandles,
    ) -> Result<(), String> {
        let cancel = &handles.cancel;
        let audio_volume = &handles.audio_volume;
        let recording_control = &handles.recording;
        let events = &handles.events;
        let context = &handles.context;
        let recording_audio_config = route_processor.recording_audio_config();
        let web_device = JsFuture::from(permission).await.map_err(super::js_error)?;
        let label = web_device.product_name().unwrap_or_else(|| {
            format!(
                "{:04x}:{:04x}",
                web_device.vendor_id(),
                web_device.product_id()
            )
        });
        let device = RealtekDevice::from_web_usb_device(web_device)
            .await
            .map_err(|error| error.to_string())?;
        let report = device
            .initialize_monitor_async(
                RadioConfig {
                    channel: request.channel,
                    channel_width: channel_width(request.channel_width_mhz)?,
                    channel_offset: request.channel_offset,
                },
                false,
            )
            .await
            .map_err(|error| error.to_string())?;
        let chip = report.chip.family;
        let receiver_info = crate::runtime::ReceiverInfo::initialized(label, &device, &report);
        let mut decoder = PlatformDecoder::new(DecoderOptions::default())
            .map_err(|error| format!("WebCodecs decoder unavailable: {error}"))?;
        super::emit(
            events,
            context,
            RuntimeEvent::Connected {
                receiver: receiver_info,
                decoder: decoder_environment(decoder.capabilities()),
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
        let mut link = build_link(&request, chip, receiver.video_fec_counters(), &device).await?;
        let mut endpoint = device
            .open_bulk_in_endpoint()
            .map_err(|error| error.to_string())?;
        endpoint
            .clear_halt()
            .await
            .map_err(|error| format!("clear bulk-IN halt failed: {error}"))?;
        while endpoint.pending() < RX_TRANSFERS_IN_FLIGHT {
            endpoint.submit(endpoint.allocate(request.transfer_size));
        }
        super::emit(events, context, RuntimeEvent::Started);
        log(
            events,
            context,
            LogLevel::Info,
            "rx",
            "WebUSB receiver started",
        );

        let descriptor = device.rx_descriptor_kind();
        let mut last_maintenance = 0;
        let mut power_tracking = Jaguar3PowerTrackingState::default();
        let mut last_decode_errors = 0;
        let mut recorder: Option<BrowserRecorder> = None;
        let mut recording_armed = false;
        while !cancel.get() {
            let usb_start = Instant::now();
            let Some(completion) = next_with_timeout(&mut endpoint).await else {
                update_recording(
                    &[],
                    recording_control,
                    &mut recording_armed,
                    &mut recorder,
                    recording_audio_config,
                    events,
                    context,
                );
                tick_maintenance(&device, chip, &mut last_maintenance, &mut power_tracking).await;
                tick_adaptive(&device, &mut link, now_ms(), events, context).await;
                continue;
            };
            let usb_latency_ms = usb_start.elapsed().as_secs_f64() * 1_000.0;
            let actual_len = completion.actual_len;
            if let Err(error) = completion.status {
                if error == TransferError::Stall {
                    let _ = endpoint.clear_halt().await;
                }
                log(
                    events,
                    context,
                    LogLevel::Warn,
                    "usb",
                    format!("bulk IN failed: {error}"),
                );
                endpoint.submit(completion.buffer);
                continue;
            }

            let batch_start = Instant::now();
            let parse_start = Instant::now();
            let packets =
                match parse_rx_aggregate_with_kind(&completion.buffer[..actual_len], descriptor) {
                    Ok(packets) => packets,
                    Err(error) => {
                        log(
                            events,
                            context,
                            LogLevel::Warn,
                            "usb",
                            format!("RX aggregate rejected: {error}"),
                        );
                        endpoint.submit(completion.buffer);
                        continue;
                    }
                };
            let parse_latency_ms = parse_start.elapsed().as_secs_f64() * 1_000.0;
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
            let pipeline_latency_ms = pipeline_start.elapsed().as_secs_f64() * 1_000.0;
            update_recording(
                &batch.frames,
                recording_control,
                &mut recording_armed,
                &mut recorder,
                recording_audio_config,
                events,
                context,
            );
            route_processor.set_audio_volume(audio_volume.get());
            let route_start = Instant::now();
            let (route_updates, route_logs, recorded_audio) =
                route_processor.process(&batch.raw_payloads, recorder.is_some());
            append_recorded_audio(&mut recorder, recorded_audio, events, context);
            let route_latency_ms = route_start.elapsed().as_secs_f64() * 1_000.0;
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
            let decode_submit_latency_ms = decode_submit_start.elapsed().as_secs_f64() * 1_000.0;
            if let Some(decoded) = decoder.latest_frame() {
                let decode_latency_ms = decoder.stats().last_decode_latency_us as f64 / 1_000.0;
                super::emit(
                    events,
                    context,
                    RuntimeEvent::NativeVideo {
                        frame: decoded,
                        decode_latency_ms,
                    },
                );
            }
            let stats = decoder.stats();
            super::emit(
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
                    batch_latency_ms: batch_start.elapsed().as_secs_f64() * 1_000.0,
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
                    ..BatchMetrics::default()
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
            tick_maintenance(&device, chip, &mut last_maintenance, &mut power_tracking).await;
            tick_adaptive(&device, &mut link, now, events, context).await;
            endpoint.submit(completion.buffer);
        }

        drop(endpoint);
        finish_recording(&mut recorder, events, context);
        let _ = decoder.flush();
        device
            .shutdown_monitor_async()
            .await
            .map_err(|error| format!("monitor shutdown failed: {error}"))?;
        Ok(())
    }

    #[cfg(debug_assertions)]
    pub(super) async fn run_codec_mock(
        request: StartRequest,
        mut route_processor: RouteProcessor,
        handles: WorkerHandles,
    ) -> Result<(), String> {
        let cancel = &handles.cancel;
        let audio_volume = &handles.audio_volume;
        let recording_control = &handles.recording;
        let events = &handles.events;
        let context = &handles.context;
        let recording_audio_config = route_processor.recording_audio_config();
        use crate::runtime::{codec_mock::MockAvStream, route_runtime::configure_mock_receiver};
        use openipc_video::{DecoderOptions, PlatformDecoder, VideoDecoder as _};

        let mut decoder = PlatformDecoder::new(DecoderOptions::default())
            .map_err(|error| format!("WebCodecs mock decoder unavailable: {error}"))?;
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
        super::emit(
            events,
            context,
            RuntimeEvent::Connected {
                receiver: crate::runtime::ReceiverInfo::codec_mock(),
                decoder: decoder_environment(decoder.capabilities()),
            },
        );
        super::emit(events, context, RuntimeEvent::Started);
        log(
            events,
            context,
            LogLevel::Info,
            "mock",
            "Pre-recorded 1080p H.264 + Opus RTP/WebCodecs mock started",
        );

        let channel = ChannelId::default_video();
        let mut receiver =
            ReceiverRuntime::with_mock_video_route(FrameLayout::WithFcs, VIDEO_ROUTE, channel, 0);
        receiver.set_rtp_reorder_enabled(request.rtp_reorder);
        let options = configure_mock_receiver(&mut receiver, &request);
        let runtime = receiver.video_runtime();
        let mut source = MockAvStream::new()?;
        let mock_started = Instant::now();
        let mut payload_sequence = 1u64;
        let mut recorder: Option<BrowserRecorder> = None;
        let mut recording_armed = false;

        while !cancel.get() {
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
                    &mut recording_armed,
                    &mut recorder,
                    recording_audio_config,
                    events,
                    context,
                );
                route_processor.set_audio_volume(audio_volume.get());
                let (route_updates, route_logs, recorded_audio) =
                    route_processor.process(&batch.raw_payloads, recorder.is_some());
                append_recorded_audio(&mut recorder, recorded_audio, events, context);
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
                let decode_latency_ms = decoder.stats().last_decode_latency_us as f64 / 1_000.0;
                super::emit(
                    events,
                    context,
                    RuntimeEvent::NativeVideo {
                        frame: decoded,
                        decode_latency_ms,
                    },
                );
            }
            let stats = decoder.stats();
            metrics.pipeline_latency_ms = loop_started.elapsed().as_secs_f64() * 1_000.0;
            metrics.batch_latency_ms = metrics.pipeline_latency_ms;
            metrics.decoder_drops =
                stats.waiting_drops + stats.backpressure_drops + stats.output_drops;
            metrics.decoder_errors = stats.decode_errors;
            metrics.audio = route_processor.audio_stats();
            super::emit(events, context, RuntimeEvent::Batch(Box::new(metrics)));
            let remaining_ms = Duration::from_micros(event.next_due_micros)
                .checked_sub(mock_started.elapsed())
                .map_or(0, |remaining| {
                    remaining.as_millis().min(i32::MAX as u128) as i32
                });
            sleep_ms(remaining_ms).await;
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
        Ok(())
    }

    fn update_recording(
        frames: &[openipc_core::DepacketizedFrame],
        control: &Rc<RefCell<super::RecordingControl>>,
        armed: &mut bool,
        recorder: &mut Option<BrowserRecorder>,
        audio_config: Option<crate::recording::AudioTrackConfig>,
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
    ) {
        let (start, stop) = {
            let mut control = control.borrow_mut();
            (
                std::mem::take(&mut control.start),
                std::mem::take(&mut control.stop),
            )
        };
        if start {
            finish_recording(recorder, events, context);
            *armed = true;
        }
        if stop {
            *armed = false;
            finish_recording(recorder, events, context);
        }

        for frame in frames {
            if recorder.is_none() && *armed && frame.is_keyframe {
                let started = match BrowserRecorder::new(frame, audio_config) {
                    Ok(started) => started,
                    Err(error) => {
                        *armed = false;
                        super::emit(events, context, RuntimeEvent::RecordingFailed(error));
                        continue;
                    }
                };
                *armed = false;
                super::emit(
                    events,
                    context,
                    RuntimeEvent::RecordingStarted {
                        path: "Browser download".to_owned(),
                        codec: format!("{:?}", frame.codec),
                    },
                );
                *recorder = Some(started);
                continue;
            }
            let Some(active) = recorder.as_mut() else {
                continue;
            };
            if frame.codec == active.codec && !active.append(frame) {
                log(
                    events,
                    context,
                    LogLevel::Warn,
                    "record",
                    "Browser recording reached 512 MiB and was finalized",
                );
                finish_recording(recorder, events, context);
                break;
            }
        }
    }

    fn append_recorded_audio(
        recorder: &mut Option<BrowserRecorder>,
        packets: Vec<crate::recording::RecordedAudioPacket>,
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
    ) {
        for packet in packets {
            let Some(active) = recorder.as_mut() else {
                break;
            };
            if !active.append_audio(packet) {
                log(
                    events,
                    context,
                    LogLevel::Warn,
                    "record",
                    "Browser recording reached 512 MiB and was finalized",
                );
                finish_recording(recorder, events, context);
                break;
            }
        }
    }

    fn finish_recording(
        recorder: &mut Option<BrowserRecorder>,
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
    ) {
        let Some(active) = recorder.take() else {
            return;
        };
        let filename = "openipc-recording.mp4".to_owned();
        let byte_count = active.bytes as u64;
        let result = active
            .finish()
            .and_then(|bytes| download_recording(&filename, &bytes));
        match result {
            Ok(()) => super::emit(
                events,
                context,
                RuntimeEvent::RecordingStopped {
                    path: filename,
                    bytes: byte_count,
                },
            ),
            Err(error) => super::emit(events, context, RuntimeEvent::RecordingFailed(error)),
        }
    }

    fn download_recording(filename: &str, bytes: &[u8]) -> Result<(), String> {
        use wasm_bindgen::JsCast as _;

        let parts = js_sys::Array::new();
        let bytes = js_sys::Uint8Array::from(bytes);
        parts.push(&bytes.buffer());
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("video/mp4");
        let blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &options)
            .map_err(super::js_error)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(super::js_error)?;
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| "browser document is unavailable".to_owned())?;
        let anchor = document
            .create_element("a")
            .map_err(super::js_error)?
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(|_| "could not create browser recording download link".to_owned())?;
        anchor.set_href(&url);
        anchor.set_download(filename);
        anchor.click();
        web_sys::Url::revoke_object_url(&url).map_err(super::js_error)
    }

    fn decoder_environment(
        capabilities: openipc_video::DecoderCapabilities,
    ) -> crate::runtime::DecoderEnvironment {
        let h264 = capabilities.codec(openipc_video::VideoCodec::H264);
        let h265 = capabilities.codec(openipc_video::VideoCodec::H265);
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

    async fn build_link(
        request: &StartRequest,
        chip: ChipFamily,
        fec: FecCounters,
        device: &RealtekDevice,
    ) -> Result<LinkRuntime, String> {
        let sender = if request.adaptive_link {
            device
                .set_tx_power_override_async(request.channel, request.tx_power)
                .await
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

    async fn tick_adaptive(
        device: &RealtekDevice,
        runtime: &mut LinkRuntime,
        now: u64,
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
    ) {
        let Some(sender) = runtime.sender.as_mut() else {
            return;
        };
        match sender.tick(now) {
            Ok(frames) => {
                for frame in frames {
                    if let Err(error) = device.send_packet_async(&frame, runtime.tx_options).await {
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

    async fn tick_maintenance(
        device: &RealtekDevice,
        chip: ChipFamily,
        last_tick: &mut u64,
        power_tracking: &mut Jaguar3PowerTrackingState,
    ) {
        let now = now_ms();
        if !chip.is_jaguar3() || now.saturating_sub(*last_tick) < MAINTENANCE_INTERVAL_MS {
            return;
        }
        *last_tick = now;
        let _ = device.run_jaguar3_coex_keepalive_async().await;
        let _ = device
            .tick_jaguar3_power_tracking_async(power_tracking)
            .await;
    }

    async fn next_with_timeout(endpoint: &mut nusb::Endpoint<Bulk, In>) -> Option<Completion> {
        let completion = Box::pin(endpoint.next_complete());
        let timeout = Box::pin(sleep_ms(100));
        match select(completion, timeout).await {
            Either::Left((completion, _)) => Some(completion),
            Either::Right(((), _)) => None,
        }
    }

    async fn sleep_ms(milliseconds: i32) {
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            if let Some(window) = web_sys::window() {
                let _ = window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, milliseconds);
            } else {
                let _ = resolve.call0(&JsValue::UNDEFINED);
            }
        });
        let _ = JsFuture::from(promise).await;
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

    fn now_ms() -> u64 {
        js_sys::Date::now().max(0.0).min(u64::MAX as f64) as u64
    }

    fn log(
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
        level: LogLevel,
        target: &'static str,
        message: impl Into<String>,
    ) {
        super::emit(
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
