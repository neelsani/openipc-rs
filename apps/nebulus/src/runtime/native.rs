use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
};

use super::{RuntimeEvent, ScanRequest, StartRequest, UsbDeviceInfo};

type EventQueue = Arc<Mutex<VecDeque<RuntimeEvent>>>;

#[derive(Default)]
struct RecordingControl {
    start: Option<PathBuf>,
    stop: bool,
}

static RECORDING_FINALIZERS: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();

fn recording_finalizers() -> &'static Mutex<Vec<JoinHandle<()>>> {
    RECORDING_FINALIZERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn reap_recording_finalizers(wait_for_all: bool) {
    let mut finalizers = recording_finalizers()
        .lock()
        .expect("recording finalizer list poisoned");
    let mut index = 0;
    while index < finalizers.len() {
        if wait_for_all || finalizers[index].is_finished() {
            let finalizer = finalizers.swap_remove(index);
            let _ = finalizer.join();
        } else {
            index += 1;
        }
    }
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
        self.worker = Some(
            thread::Builder::new()
                .name("nebulus-receiver".to_owned())
                .spawn(move || {
                    crate::low_latency::tune_receiver_thread();
                    queue(&events, RuntimeEvent::Connecting);
                    context.request_repaint();
                    let result = match request.receiver_source {
                        crate::settings::ReceiverSource::Usb => super::native::worker::run(
                            request,
                            &stop,
                            &audio_volume,
                            &recording,
                            &events,
                            &context,
                        ),
                        crate::settings::ReceiverSource::UdpRtp => {
                            super::native::worker::run_udp_rtp(
                                request,
                                &stop,
                                &audio_volume,
                                &recording,
                                &events,
                                &context,
                            )
                        }
                    };
                    if let Err(error) = result {
                        queue(&events, RuntimeEvent::Failed(error));
                        context.request_repaint();
                    }
                })
                .expect("spawn Nebulus receiver worker"),
        );
    }

    pub(crate) fn start_scan(&mut self, request: ScanRequest, context: eframe::egui::Context) {
        self.stop();
        self.stop = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&self.stop);
        let events = Arc::clone(&self.events);
        self.worker = Some(
            thread::Builder::new()
                .name("nebulus-channel-scan".to_owned())
                .spawn(move || {
                    crate::low_latency::tune_receiver_thread();
                    if let Err(error) =
                        super::native::worker::scan(request, &stop, &events, &context)
                    {
                        queue(&events, RuntimeEvent::ScanFailed(error));
                        context.request_repaint();
                    }
                })
                .expect("spawn Nebulus channel scanner"),
        );
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
        self.worker = Some(
            thread::Builder::new()
                .name("nebulus-codec-mock".to_owned())
                .spawn(move || {
                    crate::low_latency::tune_receiver_thread();
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
                })
                .expect("spawn Nebulus codec mock worker"),
        );
    }

    pub(crate) fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        reap_recording_finalizers(true);
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
        reap_recording_finalizers(false);
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
    let id = device.stable_id();
    let location = if device.port_chain.is_empty() {
        format!("{} address {}", device.bus_id, device.device_address)
    } else {
        format!(
            "{} port {}",
            device.bus_id,
            device
                .port_chain
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(".")
        )
    };
    UsbDeviceInfo {
        id,
        label: device
            .product
            .or(device.manufacturer)
            .unwrap_or_else(|| format!("{:04x}:{:04x}", device.vendor_id, device.product_id)),
        vendor_id: device.vendor_id,
        product_id: device.product_id,
        location,
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
        fs::{self, OpenOptions},
        io::BufWriter,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicU8, Ordering},
            mpsc::{self, SyncSender, TrySendError},
            Arc, Mutex,
        },
        thread::JoinHandle,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use nusb::transfer::{Buffer, TransferError};
    use nusb::MaybeFuture;
    use openipc_core::{
        realtek::{parse_rx_aggregate_with_kind, RxPacketType},
        AdaptiveLink, AdaptiveLinkSender, ChannelId, DiversityCombiner, DiversityDecision,
        DiversitySourceId, DiversityStats, FecCounters, FrameLayout, PayloadRouteId, RadioPort,
        ReceiverRuntime, TxRadioParams, WfbKeypair, WfbTransmitter, WfbTxKeypair,
    };
    use openipc_rtl88xx::{
        ChannelWidth, ChipFamily, DriverOptions, Jaguar3PowerTrackingState, MonitorOptions,
        RadioConfig, RealtekDevice, RealtekTxDescriptor, RealtekTxOptions,
    };
    #[cfg(target_os = "android")]
    use openipc_video::AndroidSurfaceDecoder as AppDecoder;
    #[cfg(not(target_os = "android"))]
    use openipc_video::PlatformDecoder as AppDecoder;
    use openipc_video::{DecoderOptions, VideoDecoder};

    use crate::{
        model::LogLevel,
        runtime::{
            route_runtime::{configure_receiver, RouteProcessor, VPN_ROUTE_ID},
            AdapterRuntimeMetrics, BatchMetrics, ChannelScanAccumulator, MetricsThrottle,
            RuntimeEvent, ScanRequest, StartRequest, VpnMetrics,
        },
    };

    mod udp;

    pub(super) fn run_udp_rtp(
        request: StartRequest,
        stop: &AtomicBool,
        audio_volume: &AtomicU8,
        recording_control: &Mutex<super::RecordingControl>,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) -> Result<(), String> {
        udp::run(
            request,
            stop,
            audio_volume,
            recording_control,
            events,
            context,
        )
    }

    pub(super) fn scan(
        request: ScanRequest,
        stop: &AtomicBool,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) -> Result<(), String> {
        if request.channels.is_empty() {
            return Err("Select at least one channel to scan".to_owned());
        }
        let driver = DriverOptions::default();
        #[cfg(not(target_os = "android"))]
        let device = request
            .device_id
            .as_deref()
            .map_or_else(
                || RealtekDevice::open_first(driver),
                |id| RealtekDevice::open_by_id(id, driver),
            )
            .map_err(|error| error.to_string())?;
        #[cfg(target_os = "android")]
        let (device, _android_connection) = {
            let driver = DriverOptions {
                skip_reset: true,
                ..driver
            };
            let opened = crate::android::open_device(request.device_id.as_deref())?;
            let device = RealtekDevice::from_nusb_device(opened.device, driver)
                .map_err(|error| error.to_string())?;
            (device, opened.connection)
        };
        let width = channel_width(request.channel_width_mhz)?;
        let first = request.channels[0];
        device
            .initialize_monitor_with_options(
                RadioConfig {
                    channel: first,
                    channel_width: width,
                    channel_offset: request.channel_offset,
                },
                MonitorOptions::from_env(),
            )
            .map_err(|error| error.to_string())?;
        let mut endpoint = device
            .bulk_in_endpoint()
            .map_err(|error| error.to_string())?;
        while endpoint.pending() < 2 {
            endpoint.submit(endpoint.allocate(request.transfer_size));
        }
        let descriptor = device.rx_descriptor_kind();
        send(
            events,
            context,
            RuntimeEvent::ScanStarted {
                total: request.channels.len(),
            },
        );
        let scan_result = (|| {
            for (index, channel) in request.channels.iter().copied().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if index > 0 {
                    device
                        .retune(RadioConfig {
                            channel,
                            channel_width: width,
                            channel_offset: request.channel_offset,
                        })
                        .map_err(|error| format!("retune channel {channel} failed: {error}"))?;
                    std::thread::sleep(Duration::from_millis(15));
                }
                let started = Instant::now();
                let mut observed = ChannelScanAccumulator::default();
                while started.elapsed() < request.dwell && !stop.load(Ordering::Relaxed) {
                    let remaining = request.dwell.saturating_sub(started.elapsed());
                    let Some(completion) =
                        endpoint.wait_next_complete(remaining.min(Duration::from_millis(40)))
                    else {
                        continue;
                    };
                    let actual_len = completion.actual_len;
                    match completion.status {
                        Ok(()) => {
                            if let Ok(packets) = parse_rx_aggregate_with_kind(
                                &completion.buffer[..actual_len],
                                descriptor,
                            ) {
                                for packet in &packets {
                                    observed.observe(packet);
                                }
                            }
                        }
                        Err(TransferError::Stall) => {
                            let _ = endpoint.clear_halt().wait();
                        }
                        Err(TransferError::Disconnected) => {
                            return Err("USB adapter disconnected during channel scan".to_owned());
                        }
                        Err(error) => {
                            log(
                                events,
                                context,
                                LogLevel::Warn,
                                "scanner",
                                format!("channel scan USB transfer failed: {error}"),
                            );
                        }
                    }
                    endpoint.submit(completion.buffer);
                }
                send(
                    events,
                    context,
                    RuntimeEvent::ScanProgress {
                        index: index + 1,
                        total: request.channels.len(),
                        result: observed.finish(channel, started.elapsed()),
                    },
                );
            }
            Ok::<(), String>(())
        })();
        drop(endpoint);
        let shutdown = device
            .shutdown_monitor()
            .map_err(|error| format!("monitor shutdown failed after scan: {error}"));
        scan_result?;
        shutdown?;
        send(events, context, RuntimeEvent::ScanCompleted);
        Ok(())
    }

    enum RecorderMessage {
        Video(crate::recording::RecordedAccessUnit),
        Audio(crate::recording::RecordedAudioPacket),
        Finish,
    }

    struct EncodedRecorder {
        path: PathBuf,
        codec: openipc_core::Codec,
        bytes: usize,
        sender: SyncSender<RecorderMessage>,
        worker: JoinHandle<Result<u64, String>>,
    }

    impl EncodedRecorder {
        const MAX_BYTES: usize = 512 * 1024 * 1024;

        fn start(
            path: PathBuf,
            frame: &openipc_core::DepacketizedFrame,
            audio_config: Option<crate::recording::AudioTrackConfig>,
        ) -> Result<Self, String> {
            let config = crate::recording::Mp4TrackConfig::from_keyframe(frame)?;
            let codec = frame.codec;
            let first = crate::recording::RecordedAccessUnit::from(frame);
            let (sender, receiver) = mpsc::sync_channel(64);
            let output_path = path.clone();
            let worker = std::thread::Builder::new()
                .name("nebulus-mp4-recorder".to_owned())
                .spawn(move || {
                    if let Some(directory) = output_path.parent() {
                        fs::create_dir_all(directory).map_err(|error| {
                            format!(
                                "create recording directory {} failed: {error}",
                                directory.display()
                            )
                        })?;
                    }
                    let file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&output_path)
                        .map_err(|error| {
                            format!("create recording {} failed: {error}", output_path.display())
                        })?;
                    let writer = BufWriter::with_capacity(256 * 1024, file);
                    let mut muxer = config.muxer(writer, audio_config)?;
                    muxer
                        .write_video(0.0, &first.data, first.is_keyframe)
                        .map_err(|error| format!("mux first video frame failed: {error}"))?;
                    let mut video_timestamp = first.timestamp;
                    let mut video_pts = 0u64;
                    let mut video_delta = 3_000;
                    let mut audio_timestamp = None;
                    let mut audio_pts = 0u64;
                    let mut audio_delta = audio_config
                        .map(|config| (config.sample_rate / 50).max(1))
                        .unwrap_or(960);
                    while let Ok(message) = receiver.recv() {
                        match message {
                            RecorderMessage::Video(frame) => {
                                video_delta = crate::recording::frame_delta_ticks(
                                    video_timestamp,
                                    frame.timestamp,
                                    video_delta,
                                );
                                video_pts = video_pts.saturating_add(u64::from(video_delta));
                                video_timestamp = frame.timestamp;
                                muxer
                                    .write_video(
                                        video_pts as f64 / 90_000.0,
                                        &frame.data,
                                        frame.is_keyframe,
                                    )
                                    .map_err(|error| format!("mux video frame failed: {error}"))?;
                            }
                            RecorderMessage::Audio(packet) => {
                                let Some(config) = audio_config else {
                                    continue;
                                };
                                if let Some(previous) = audio_timestamp {
                                    audio_delta = crate::recording::timestamp_delta(
                                        previous,
                                        packet.timestamp,
                                        audio_delta,
                                        config.sample_rate.saturating_mul(2),
                                    );
                                    audio_pts = audio_pts.saturating_add(u64::from(audio_delta));
                                }
                                audio_timestamp = Some(packet.timestamp);
                                muxer
                                    .write_audio(
                                        audio_pts as f64 / config.sample_rate as f64,
                                        &packet.data,
                                    )
                                    .map_err(|error| format!("mux Opus packet failed: {error}"))?;
                            }
                            RecorderMessage::Finish => break,
                        }
                    }
                    muxer
                        .finish_in_place()
                        .map_err(|error| format!("finalize MP4 recording failed: {error}"))?;
                    drop(muxer);
                    std::fs::metadata(&output_path)
                        .map(|metadata| metadata.len())
                        .map_err(|error| format!("read MP4 recording size failed: {error}"))
                })
                .map_err(|error| format!("start MP4 recorder worker failed: {error}"))?;
            let recorder = Self {
                path,
                codec,
                bytes: frame.data.len(),
                sender,
                worker,
            };
            Ok(recorder)
        }

        fn write(&mut self, frame: &openipc_core::DepacketizedFrame) -> Result<(), String> {
            if frame.codec != self.codec {
                return Ok(());
            }
            self.send(frame.data.len(), RecorderMessage::Video(frame.into()))
        }

        fn write_audio(
            &mut self,
            packet: crate::recording::RecordedAudioPacket,
        ) -> Result<(), String> {
            self.send(packet.data.len(), RecorderMessage::Audio(packet))
        }

        fn send(&mut self, bytes: usize, message: RecorderMessage) -> Result<(), String> {
            let Some(total) = self.bytes.checked_add(bytes) else {
                return Err("MP4 recording exceeded its encoded-data limit".to_owned());
            };
            if total > Self::MAX_BYTES {
                return Err("MP4 recording reached 512 MiB and was finalized".to_owned());
            }
            match self.sender.try_send(message) {
                Ok(()) => {
                    self.bytes = total;
                    Ok(())
                }
                Err(TrySendError::Full(_)) => Err(
                    "MP4 recorder could not keep up; recording stopped before affecting RX latency"
                        .to_owned(),
                ),
                Err(TrySendError::Disconnected(_)) => {
                    Err("MP4 recorder worker stopped unexpectedly".to_owned())
                }
            }
        }

        fn finish(self) -> Result<(String, u64), String> {
            let _ = self.sender.send(RecorderMessage::Finish);
            let bytes = self
                .worker
                .join()
                .map_err(|_| "MP4 recorder worker panicked".to_owned())??;
            Ok((self.path.display().to_string(), bytes))
        }
    }

    const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);
    const RX_TRANSFERS_IN_FLIGHT: usize = 4;

    struct CaptureEvent {
        source_id: u16,
        buffer: Buffer,
        actual_len: usize,
        usb_latency_ms: f64,
    }

    struct CaptureWorker {
        return_buffer: mpsc::Sender<Buffer>,
        metrics: Arc<Mutex<AdapterRuntimeMetrics>>,
        worker: Option<JoinHandle<()>>,
    }

    impl CaptureWorker {
        fn start(
            source_id: u16,
            device_id: String,
            label: String,
            device: &RealtekDevice,
            transfer_size: usize,
            stop: Arc<AtomicBool>,
            completions: SyncSender<CaptureEvent>,
        ) -> Result<Self, String> {
            let mut endpoint = device
                .bulk_in_endpoint()
                .map_err(|error| error.to_string())?;
            while endpoint.pending() < RX_TRANSFERS_IN_FLIGHT {
                endpoint.submit(endpoint.allocate(transfer_size));
            }
            let metrics = Arc::new(Mutex::new(AdapterRuntimeMetrics {
                source_id,
                device_id,
                label,
                online: true,
                ..AdapterRuntimeMetrics::default()
            }));
            let thread_metrics = Arc::clone(&metrics);
            let (return_buffer, returned_buffers) = mpsc::channel();
            let worker = std::thread::Builder::new()
                .name(format!("nebulus-usb-rx-{source_id}"))
                .spawn(move || {
                    let mut consecutive_errors = 0u8;
                    while !stop.load(Ordering::Relaxed) {
                        while let Ok(buffer) = returned_buffers.try_recv() {
                            endpoint.submit(buffer);
                        }
                        let started = Instant::now();
                        let Some(completion) =
                            endpoint.wait_next_complete(Duration::from_millis(20))
                        else {
                            continue;
                        };
                        let usb_latency_ms = started.elapsed().as_secs_f64() * 1_000.0;
                        let actual_len = completion.actual_len;
                        match completion.status {
                            Ok(()) => {
                                consecutive_errors = 0;
                                {
                                    let mut metrics =
                                        thread_metrics.lock().expect("capture metrics poisoned");
                                    metrics.transfers = metrics.transfers.saturating_add(1);
                                    metrics.transfer_bytes =
                                        metrics.transfer_bytes.saturating_add(actual_len as u64);
                                }
                                let event = CaptureEvent {
                                    source_id,
                                    buffer: completion.buffer,
                                    actual_len,
                                    usb_latency_ms,
                                };
                                match completions.try_send(event) {
                                    Ok(()) => {}
                                    Err(TrySendError::Full(event)) => {
                                        thread_metrics
                                            .lock()
                                            .expect("capture metrics poisoned")
                                            .queue_drops += 1;
                                        endpoint.submit(event.buffer);
                                    }
                                    Err(TrySendError::Disconnected(event)) => {
                                        endpoint.submit(event.buffer);
                                        break;
                                    }
                                }
                            }
                            Err(TransferError::Disconnected) => {
                                thread_metrics
                                    .lock()
                                    .expect("capture metrics poisoned")
                                    .usb_errors += 1;
                                break;
                            }
                            Err(TransferError::Stall) => {
                                thread_metrics
                                    .lock()
                                    .expect("capture metrics poisoned")
                                    .usb_errors += 1;
                                let _ = endpoint.clear_halt().wait();
                                endpoint.submit(completion.buffer);
                                consecutive_errors = 0;
                            }
                            Err(_) => {
                                thread_metrics
                                    .lock()
                                    .expect("capture metrics poisoned")
                                    .usb_errors += 1;
                                endpoint.submit(completion.buffer);
                                consecutive_errors = consecutive_errors.saturating_add(1);
                                if consecutive_errors >= 8 {
                                    break;
                                }
                            }
                        }
                    }
                    thread_metrics
                        .lock()
                        .expect("capture metrics poisoned")
                        .online = false;
                })
                .map_err(|error| format!("start USB capture worker failed: {error}"))?;
            Ok(Self {
                return_buffer,
                metrics,
                worker: Some(worker),
            })
        }

        fn snapshot(&self, diversity: &DiversityStats) -> AdapterRuntimeMetrics {
            let mut snapshot = self
                .metrics
                .lock()
                .expect("capture metrics poisoned")
                .clone();
            if let Some(source) = diversity
                .sources
                .get(&DiversitySourceId::new(snapshot.source_id))
            {
                snapshot.accepted = source.accepted;
                snapshot.duplicates = source.duplicates;
            }
            snapshot
        }
    }

    struct CaptureGroup {
        stop: Arc<AtomicBool>,
        workers: Vec<CaptureWorker>,
    }

    impl CaptureGroup {
        fn online_count(&self) -> usize {
            self.workers
                .iter()
                .filter(|worker| {
                    worker
                        .metrics
                        .lock()
                        .expect("capture metrics poisoned")
                        .online
                })
                .count()
        }

        fn snapshots(&self, diversity: &DiversityStats) -> Vec<AdapterRuntimeMetrics> {
            self.workers
                .iter()
                .map(|worker| worker.snapshot(diversity))
                .collect()
        }
    }

    fn diversity_snapshot(
        diversity: &DiversityCombiner,
        captures: &CaptureGroup,
        source_quality: &mut [AdaptiveLink],
        now: u64,
    ) -> (DiversityStats, Vec<AdapterRuntimeMetrics>) {
        let stats = diversity.stats();
        let mut adapters = captures.snapshots(&stats);
        for (snapshot, quality_tracker) in adapters.iter_mut().zip(source_quality) {
            let quality = quality_tracker.quality(now);
            snapshot.rssi[0] = quality.rssi[0];
            snapshot.rssi[1] = quality.rssi[1];
            snapshot.snr[0] = quality.snr[0];
            snapshot.snr[1] = quality.snr[1];
        }
        (stats, adapters)
    }

    fn send_metrics(
        events: &super::EventQueue,
        context: &eframe::egui::Context,
        mut metrics: BatchMetrics,
        diversity: &DiversityCombiner,
        captures: &CaptureGroup,
        source_quality: &mut [AdaptiveLink],
        now: u64,
    ) {
        (metrics.diversity, metrics.adapters) =
            diversity_snapshot(diversity, captures, source_quality, now);
        send(events, context, RuntimeEvent::Batch(Box::new(metrics)));
    }

    impl Drop for CaptureGroup {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            for worker in &mut self.workers {
                if let Some(handle) = worker.worker.take() {
                    let _ = handle.join();
                }
            }
        }
    }

    struct ActiveAdapter {
        id: String,
        label: String,
        device: Arc<RealtekDevice>,
        chip: ChipFamily,
        descriptor: openipc_core::RxDescriptorKind,
        receiver_info: crate::runtime::ReceiverInfo,
    }

    fn decoder_options() -> DecoderOptions {
        #[cfg(target_os = "android")]
        {
            if crate::android::is_probably_emulator().unwrap_or(false) {
                // Goldfish advertises an eight-frame output delay. This larger
                // allowance is strictly an emulator compatibility policy; real
                // devices retain the three-frame low-latency default.
                DecoderOptions {
                    max_frames_in_flight: 12,
                    ..DecoderOptions::default()
                }
            } else {
                DecoderOptions::default()
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            DecoderOptions::default()
        }
    }

    struct FramePresenter {
        events: super::EventQueue,
        context: eframe::egui::Context,
    }

    impl FramePresenter {
        fn new(events: &super::EventQueue, context: &eframe::egui::Context) -> Self {
            Self {
                events: Arc::clone(events),
                context: context.clone(),
            }
        }

        fn submit(
            &self,
            frame: openipc_video::DecodedFrame<
                <AppDecoder as openipc_video::VideoDecoder>::Surface,
            >,
            decode_latency_ms: f64,
        ) {
            send(
                &self.events,
                &self.context,
                RuntimeEvent::NativeVideo {
                    frame,
                    decode_latency_ms,
                    ready_at: web_time::Instant::now(),
                },
            );
        }
    }

    fn create_decoder(_request: &StartRequest) -> Result<AppDecoder, String> {
        #[cfg(target_os = "android")]
        {
            let output = _request.video_output.clone().ok_or_else(|| {
                "Android SurfaceTexture renderer is unavailable; cannot start video decoder"
                    .to_owned()
            })?;
            AppDecoder::new(decoder_options(), output)
                .map_err(|error| format!("video decoder unavailable: {error}"))
        }
        #[cfg(not(target_os = "android"))]
        {
            AppDecoder::new(decoder_options())
                .map_err(|error| format!("video decoder unavailable: {error}"))
        }
    }

    struct LinkRuntime {
        quality: AdaptiveLink,
        sender: Option<AdaptiveLinkSender>,
        last_fec: FecCounters,
        tx_options: RealtekTxOptions,
        last_tx_queue_warning_ms: Option<u64>,
    }

    enum RadioCommand {
        Transmit {
            frame: Vec<u8>,
            options: RealtekTxOptions,
        },
    }

    /// Keeps auxiliary USB OUT and Jaguar3 register work off the RX thread.
    struct RadioWorker {
        sender: Option<SyncSender<RadioCommand>>,
        worker: Option<JoinHandle<()>>,
    }

    impl RadioWorker {
        const QUEUE_CAPACITY: usize = 64;
        const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(2);

        fn start(
            device: Arc<RealtekDevice>,
            chip: ChipFamily,
            transmit: bool,
            events: &super::EventQueue,
            context: &eframe::egui::Context,
        ) -> Result<Option<Self>, String> {
            if !transmit && !chip.is_jaguar3() {
                return Ok(None);
            }
            let mut endpoint = transmit
                .then(|| {
                    device
                        .bulk_out_endpoint()
                        .map_err(|error| error.to_string())
                })
                .transpose()?;
            let (sender, receiver) = mpsc::sync_channel(Self::QUEUE_CAPACITY);
            let events = Arc::clone(events);
            let context = context.clone();
            let worker = std::thread::Builder::new()
                .name("nebulus-radio-background".to_owned())
                .spawn(move || {
                    let mut power_tracking = Jaguar3PowerTrackingState::default();
                    let mut last_maintenance = Instant::now();
                    let mut last_tx_error_log = None;
                    loop {
                        let wait = if chip.is_jaguar3() {
                            Self::MAINTENANCE_INTERVAL.saturating_sub(last_maintenance.elapsed())
                        } else {
                            Duration::from_secs(60)
                        };
                        match receiver.recv_timeout(wait) {
                            Ok(RadioCommand::Transmit { frame, options }) => {
                                let result = endpoint.as_mut().map_or_else(
                                    || Err("radio TX endpoint is unavailable".to_owned()),
                                    |endpoint| {
                                        RealtekDevice::send_packet_on(endpoint, &frame, options)
                                            .map(|_| ())
                                            .map_err(|error| error.to_string())
                                    },
                                );
                                if let Err(error) = result {
                                    let now = Instant::now();
                                    if last_tx_error_log.is_none_or(|last: Instant| {
                                        now.duration_since(last) >= Duration::from_secs(1)
                                    }) {
                                        log(&events, &context, LogLevel::Warn, "radio-tx", error);
                                        last_tx_error_log = Some(now);
                                    }
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                        if chip.is_jaguar3()
                            && last_maintenance.elapsed() >= Self::MAINTENANCE_INTERVAL
                        {
                            last_maintenance = Instant::now();
                            let _ = device.run_jaguar3_coex_keepalive();
                            let _ = device.tick_jaguar3_power_tracking(&mut power_tracking);
                        }
                    }
                })
                .map_err(|error| format!("start radio background worker failed: {error}"))?;
            Ok(Some(Self {
                sender: Some(sender),
                worker: Some(worker),
            }))
        }

        fn enqueue(&self, frame: Vec<u8>, options: RealtekTxOptions) -> bool {
            let Some(sender) = self.sender.as_ref() else {
                return false;
            };
            sender
                .try_send(RadioCommand::Transmit { frame, options })
                .is_ok()
        }
    }

    impl Drop for RadioWorker {
        fn drop(&mut self) {
            self.sender.take();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
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

        fn tick(&mut self, now: u64, radio: &RadioWorker) {
            let session_due = self
                .last_session_ms
                .is_none_or(|last| now.saturating_sub(last) >= 1_000);
            if session_due {
                let frame = self.transmitter.session_radio_packet(self.tx_params);
                if radio.enqueue(frame, self.tx_options) {
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
                            if !radio.enqueue(frame, self.tx_options) {
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
        let radio = RadioConfig {
            channel: request.channel,
            channel_width: channel_width(request.channel_width_mhz)?,
            channel_offset: request.channel_offset,
        };
        let mut adapters = Vec::new();
        let mut adapter_errors = Vec::new();
        #[cfg(target_os = "android")]
        let mut android_connections = Vec::new();
        let requested_ids = if request.device_ids.is_empty() {
            vec![request.primary_device_id.clone()]
        } else {
            request.device_ids.iter().cloned().map(Some).collect()
        };
        for requested_id in requested_ids {
            let driver = DriverOptions::default();
            #[cfg(target_os = "android")]
            let driver = DriverOptions {
                skip_reset: true,
                ..driver
            };
            #[cfg(not(target_os = "android"))]
            let (device, id, label) = {
                let opened = requested_id.as_deref().map_or_else(
                    || RealtekDevice::open_first(driver),
                    |id| RealtekDevice::open_by_id(id, driver),
                );
                let device = match opened {
                    Ok(device) => device,
                    Err(error) => {
                        log(
                            events,
                            context,
                            LogLevel::Warn,
                            "usb",
                            format!(
                                "Skipping diversity adapter {}: {error}",
                                requested_id.as_deref().unwrap_or("automatic")
                            ),
                        );
                        adapter_errors.push(format!(
                            "{}: {error}",
                            requested_id.as_deref().unwrap_or("automatic")
                        ));
                        continue;
                    }
                };
                let id = requested_id.unwrap_or_else(|| {
                    format!("{:04x}:{:04x}", device.vendor_id(), device.product_id())
                });
                let label =
                    openipc_rtl88xx::supported_device(device.vendor_id(), device.product_id())
                        .map(|supported| supported.label.to_owned())
                        .unwrap_or_else(|| id.clone());
                (device, id, label)
            };
            #[cfg(target_os = "android")]
            let (device, id, label) = {
                let opened = match crate::android::open_device(requested_id.as_deref()) {
                    Ok(opened) => opened,
                    Err(error) => {
                        log(
                            events,
                            context,
                            LogLevel::Warn,
                            "usb",
                            format!(
                                "Skipping diversity adapter {}: {error}",
                                requested_id.as_deref().unwrap_or("automatic")
                            ),
                        );
                        adapter_errors.push(format!(
                            "{}: {error}",
                            requested_id.as_deref().unwrap_or("automatic")
                        ));
                        continue;
                    }
                };
                let id = opened.info.id.clone();
                let label = opened.info.label.clone();
                let device = match RealtekDevice::from_nusb_device(opened.device, driver) {
                    Ok(device) => device,
                    Err(error) => {
                        log(
                            events,
                            context,
                            LogLevel::Warn,
                            "usb",
                            format!("Skipping diversity adapter {id}: claim failed: {error}"),
                        );
                        adapter_errors.push(format!("{id}: claim failed: {error}"));
                        continue;
                    }
                };
                android_connections.push(opened.connection);
                (device, id, label)
            };
            if adapters
                .iter()
                .any(|adapter: &ActiveAdapter| adapter.id == id)
            {
                continue;
            }
            let device = Arc::new(device);
            let report = match device
                .initialize_monitor_with_options(radio, MonitorOptions::from_env())
            {
                Ok(report) => report,
                Err(error) => {
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "usb",
                        format!("Skipping diversity adapter {id}: initialization failed: {error}"),
                    );
                    adapter_errors.push(format!("{id}: initialization failed: {error}"));
                    continue;
                }
            };
            let source_id = u16::try_from(adapters.len())
                .map_err(|_| "too many diversity adapters selected".to_owned())?;
            adapters.push(ActiveAdapter {
                descriptor: device.rx_descriptor_kind(),
                chip: report.chip.family,
                receiver_info: crate::runtime::ReceiverInfo::initialized(
                    id.clone(),
                    source_id,
                    label.clone(),
                    &device,
                    &report,
                ),
                id,
                label,
                device,
            });
        }
        let primary = adapters.first().ok_or_else(|| {
            format!(
                "no selected receive adapter could be initialized{}",
                if adapter_errors.is_empty() {
                    String::new()
                } else {
                    format!(": {}", adapter_errors.join("; "))
                }
            )
        })?;
        let chip = primary.chip;
        let device = Arc::clone(&primary.device);
        let mut decoder = create_decoder(&request)?;
        let presenter = FramePresenter::new(events, context);
        let decoder_environment = decoder_environment(decoder.capabilities());
        send(
            events,
            context,
            RuntimeEvent::Connected {
                receivers: adapters
                    .iter()
                    .map(|adapter| adapter.receiver_info.clone())
                    .collect(),
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
        let recording_audio_config = route_processor.recording_audio_config();
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
        let radio = RadioWorker::start(
            Arc::clone(&device),
            chip,
            link.sender.is_some() || tun.is_some(),
            events,
            context,
        )?;
        let diversity_radio_workers = adapters
            .iter()
            .skip(1)
            .map(|adapter| {
                RadioWorker::start(
                    Arc::clone(&adapter.device),
                    adapter.chip,
                    false,
                    events,
                    context,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let capture_stop = Arc::new(AtomicBool::new(false));
        let queue_capacity = adapters
            .len()
            .saturating_mul(RX_TRANSFERS_IN_FLIGHT)
            .saturating_mul(2)
            .max(8);
        let (capture_sender, capture_events) = mpsc::sync_channel(queue_capacity);
        let mut captures = CaptureGroup {
            stop: capture_stop,
            workers: Vec::with_capacity(adapters.len()),
        };
        for (index, adapter) in adapters.iter().enumerate() {
            captures.workers.push(CaptureWorker::start(
                index as u16,
                adapter.id.clone(),
                adapter.label.clone(),
                &adapter.device,
                request.transfer_size,
                Arc::clone(&captures.stop),
                capture_sender.clone(),
            )?);
        }
        drop(capture_sender);
        send(events, context, RuntimeEvent::Started);
        log(
            events,
            context,
            LogLevel::Info,
            "rx",
            format!(
                "Receiver started with {} adapter(s); primary TX adapter is {}",
                adapters.len(),
                adapters[0].label
            ),
        );

        let mut last_decode_errors = 0;
        let mut recorder: Option<EncodedRecorder> = None;
        let mut armed_path: Option<PathBuf> = None;
        let mut metrics_throttle = MetricsThrottle::new();
        let mut diversity = DiversityCombiner::default();
        let diversity_enabled = adapters.len() > 1;
        let mut source_quality = (0..adapters.len())
            .map(|_| AdaptiveLink::new())
            .collect::<Vec<_>>();
        let mut last_online_count = captures.online_count();
        while !stop.load(Ordering::Relaxed) {
            let event = match capture_events.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let online_count = captures.online_count();
                    if online_count != last_online_count {
                        let level = if online_count == adapters.len() {
                            LogLevel::Info
                        } else {
                            LogLevel::Warn
                        };
                        log(
                            events,
                            context,
                            level,
                            "diversity",
                            format!(
                                "Receive diversity health: {online_count}/{} adapters online",
                                adapters.len()
                            ),
                        );
                        last_online_count = online_count;
                        let now = now_ms();
                        let (stats, adapters) =
                            diversity_snapshot(&diversity, &captures, &mut source_quality, now);
                        send(
                            events,
                            context,
                            RuntimeEvent::DiversityUpdate { stats, adapters },
                        );
                    }
                    if online_count == 0 {
                        return Err("all receive adapters disconnected".to_owned());
                    }
                    if let Some(metrics) = metrics_throttle.flush() {
                        send_metrics(
                            events,
                            context,
                            metrics,
                            &diversity,
                            &captures,
                            &mut source_quality,
                            now_ms(),
                        );
                    }
                    update_recording(
                        &[],
                        recording_control,
                        &mut armed_path,
                        &mut recorder,
                        recording_audio_config,
                        events,
                        context,
                    );
                    tick_adaptive(&mut link, radio.as_ref(), now_ms(), events, context);
                    if let (Some(tun), Some(radio)) = (tun.as_mut(), radio.as_ref()) {
                        tun.tick(now_ms(), radio);
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("all USB capture workers stopped".to_owned());
                }
            };
            let source_index = usize::from(event.source_id);
            let Some(adapter) = adapters.get(source_index) else {
                continue;
            };
            let bytes = &event.buffer[..event.actual_len];
            let batch_start = Instant::now();
            let parse_start = Instant::now();
            let packets = match parse_rx_aggregate_with_kind(bytes, adapter.descriptor) {
                Ok(packets) => packets,
                Err(error) => {
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "usb",
                        format!("{} RX aggregate rejected: {error}", adapter.label),
                    );
                    let _ = captures.workers[source_index]
                        .return_buffer
                        .send(event.buffer);
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
                    source_quality[source_index].record_rx_paths(
                        now,
                        packet.attrib.rssi,
                        packet.attrib.snr,
                    );
                }
            }
            let source = DiversitySourceId::new(event.source_id);
            let selected_packets = packets.into_iter().filter(|packet| {
                if packet.attrib.crc_err
                    || packet.attrib.icv_err
                    || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
                {
                    return true;
                }
                let decision = if diversity_enabled {
                    diversity.observe_frame(source, packet.data, FrameLayout::WithFcs)
                } else {
                    DiversityDecision::Passthrough
                };
                let is_video = openipc_core::WifiFrame::parse(packet.data, FrameLayout::WithFcs)
                    .is_ok_and(|frame| {
                        frame.matches_channel_id(ChannelId::new(request.channel_id))
                    });
                if decision != DiversityDecision::Duplicate && is_video {
                    link.record_rx(now, packet.attrib.rssi, packet.attrib.snr);
                }
                decision.should_forward()
            });
            let pipeline_start = Instant::now();
            let mut batch = receiver.push_rx_packets(selected_packets, &options);
            let pipeline_latency_ms = pipeline_start.elapsed().as_secs_f64() * 1000.0;

            // Return the owned transfer buffer before decode, audio, recording,
            // diagnostics, or uplink processing can reduce USB queue depth.
            let _ = captures.workers[source_index]
                .return_buffer
                .send(event.buffer);

            let video_bytes = batch.frames.iter().map(|frame| frame.data.len()).sum();
            update_recording(
                &batch.frames,
                recording_control,
                &mut armed_path,
                &mut recorder,
                recording_audio_config,
                events,
                context,
            );

            let decode_submit_start = Instant::now();
            for frame in std::mem::take(&mut batch.frames)
                .into_iter()
                .filter(|frame| request.codec_preference.accepts(frame.codec))
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
            let video_submit_path_ms = batch_start.elapsed().as_secs_f64() * 1000.0;
            if let Some(decoded) = decoder.latest_frame() {
                presenter.submit(
                    decoded,
                    decoder.stats().last_decode_latency_us as f64 / 1_000.0,
                );
            }

            if let Some(tun) = tun.as_mut() {
                for payload in &batch.raw_payloads {
                    if payload.route_id == VPN_ROUTE_ID {
                        tun.write_downlink(&payload.data);
                    }
                }
            }
            route_processor.set_audio_volume(audio_volume.load(Ordering::Relaxed));
            let route_start = Instant::now();
            let (route_updates, route_logs, recorded_audio, telemetry) =
                route_processor.process(&batch.raw_payloads, recorder.is_some());
            record_audio_packets(&mut recorder, recorded_audio, events, context);
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
            let stats = decoder.stats();
            if let Some(metrics) = metrics_throttle.push(BatchMetrics {
                transfers: 1,
                transfer_bytes: event.actual_len,
                packets: batch.counters.packets,
                rtp_packets: batch.counters.rtp_packets,
                video_frames: batch.counters.video_frames,
                video_bytes,
                usb_latency_ms: event.usb_latency_ms,
                parse_latency_ms,
                pipeline_latency_ms,
                route_latency_ms,
                decode_submit_latency_ms,
                video_submit_path_ms,
                batch_latency_ms: batch_start.elapsed().as_secs_f64() * 1000.0,
                rssi: quality.rssi,
                snr: quality.snr,
                link_score: quality.link_score,
                decoder_drops: stats.waiting_drops + stats.backpressure_drops + stats.output_drops,
                decoder_errors: stats.decode_errors,
                fec: batch.fec_counters,
                counters: batch.counters,
                rtp: batch.rtp_status,
                reorder: batch.rtp_reorder_status,
                routes: route_updates,
                telemetry,
                audio: route_processor.audio_stats(),
                vpn: tun
                    .as_ref()
                    .map_or_else(VpnMetrics::default, |tun| tun.metrics.clone()),
                diversity: DiversityStats::default(),
                adapters: Vec::new(),
            }) {
                send_metrics(
                    events,
                    context,
                    metrics,
                    &diversity,
                    &captures,
                    &mut source_quality,
                    now,
                );
            }
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
            tick_adaptive(&mut link, radio.as_ref(), now, events, context);
            if let (Some(tun), Some(radio)) = (tun.as_mut(), radio.as_ref()) {
                tun.tick(now, radio);
            }
        }

        if let Some(metrics) = metrics_throttle.flush() {
            send_metrics(
                events,
                context,
                metrics,
                &diversity,
                &captures,
                &mut source_quality,
                now_ms(),
            );
        }
        drop(captures);
        drop(radio);
        drop(diversity_radio_workers);
        finish_recording(&mut recorder, events, context);
        let _ = decoder.flush();
        drop(presenter);
        let mut shutdown_errors = Vec::new();
        for adapter in adapters {
            if let Err(error) = adapter.device.shutdown_monitor() {
                shutdown_errors.push(format!("{}: {error}", adapter.label));
            }
        }
        if !shutdown_errors.is_empty() {
            return Err(format!(
                "monitor shutdown failed: {}",
                shutdown_errors.join("; ")
            ));
        }
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

        let mut decoder = create_decoder(&request)
            .map_err(|error| format!("native mock decoder unavailable: {error}"))?;
        let presenter = FramePresenter::new(events, context);
        let mut route_processor = RouteProcessor::new(&request)?;
        let recording_audio_config = route_processor.recording_audio_config();
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
                receivers: vec![crate::runtime::ReceiverInfo::codec_mock()],
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
        let mock_started = Instant::now();
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
                    recording_audio_config,
                    events,
                    context,
                );
                route_processor.set_audio_volume(audio_volume.load(Ordering::Relaxed));
                let (route_updates, route_logs, recorded_audio, telemetry) =
                    route_processor.process(&batch.raw_payloads, recorder.is_some());
                record_audio_packets(&mut recorder, recorded_audio, events, context);
                metrics.merge(BatchMetrics {
                    routes: route_updates,
                    counters: batch.counters,
                    rtp: batch.rtp_status,
                    reorder: batch.rtp_reorder_status,
                    telemetry,
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
                presenter.submit(
                    decoded,
                    decoder.stats().last_decode_latency_us as f64 / 1_000.0,
                );
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
                Duration::from_micros(event.next_due_micros).checked_sub(mock_started.elapsed())
            {
                std::thread::sleep(remaining);
            }
        }

        let _ = decoder.flush();
        drop(presenter);
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
        audio_config: Option<crate::recording::AudioTrackConfig>,
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

        if armed_path.is_none() && recorder.is_none() {
            return;
        }

        for frame in frames {
            if recorder.is_none() && frame.is_keyframe {
                let Some(path) = armed_path.take() else {
                    continue;
                };
                match EncodedRecorder::start(path, frame, audio_config) {
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

    fn record_audio_packets(
        recorder: &mut Option<EncodedRecorder>,
        packets: Vec<crate::recording::RecordedAudioPacket>,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) {
        for packet in packets {
            let Some(active) = recorder.as_mut() else {
                break;
            };
            if let Err(error) = active.write_audio(packet) {
                send(events, context, RuntimeEvent::RecordingFailed(error));
                finish_recording(recorder, events, context);
                break;
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
        let event_queue = Arc::clone(events);
        let repaint = context.clone();
        match std::thread::Builder::new()
            .name("nebulus-recorder-finalizer".to_owned())
            .spawn(move || match active.finish() {
                Ok((path, bytes)) => send(
                    &event_queue,
                    &repaint,
                    RuntimeEvent::RecordingStopped { path, bytes },
                ),
                Err(error) => send(&event_queue, &repaint, RuntimeEvent::RecordingFailed(error)),
            }) {
            Ok(finalizer) => super::recording_finalizers()
                .lock()
                .expect("recording finalizer list poisoned")
                .push(finalizer),
            Err(error) => send(
                events,
                context,
                RuntimeEvent::RecordingFailed(format!("start recording finalizer failed: {error}")),
            ),
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
            last_tx_queue_warning_ms: None,
        })
    }

    fn tick_adaptive(
        runtime: &mut LinkRuntime,
        radio: Option<&RadioWorker>,
        now: u64,
        events: &super::EventQueue,
        context: &eframe::egui::Context,
    ) {
        let (Some(sender), Some(radio)) = (runtime.sender.as_mut(), radio) else {
            return;
        };
        match sender.tick(now) {
            Ok(frames) => {
                let mut dropped = false;
                for frame in frames {
                    if !radio.enqueue(frame, runtime.tx_options) {
                        dropped = true;
                    }
                }
                if dropped
                    && runtime
                        .last_tx_queue_warning_ms
                        .is_none_or(|last| now.saturating_sub(last) >= 1_000)
                {
                    runtime.last_tx_queue_warning_ms = Some(now);
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "adaptive",
                        "radio TX queue full; dropped adaptive-link feedback",
                    );
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
