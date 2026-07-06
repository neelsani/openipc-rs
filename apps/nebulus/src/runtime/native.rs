use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc, Arc, Mutex, OnceLock,
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
    vtx_commands: Arc<Mutex<Option<mpsc::Sender<super::VtxControlRequest>>>>,
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
            vtx_commands: Arc::new(Mutex::new(None)),
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
        let (vtx_sender, vtx_receiver) = mpsc::channel();
        *self
            .vtx_commands
            .lock()
            .expect("VTX command state poisoned") = Some(vtx_sender);
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
                            vtx_receiver,
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
        self.vtx_commands
            .lock()
            .expect("VTX command state poisoned")
            .take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        reap_recording_finalizers(true);
        *self.recording.lock().expect("recording control poisoned") = RecordingControl::default();
    }

    pub(crate) fn set_audio_volume(&self, volume: u8) {
        self.audio_volume.store(volume.min(100), Ordering::Relaxed);
    }

    pub(crate) fn request_vtx(&self, request: super::VtxControlRequest) -> Result<(), String> {
        self.vtx_commands
            .lock()
            .map_err(|_| "VTX command state poisoned".to_owned())?
            .as_ref()
            .ok_or_else(|| "VTX controller is not running".to_owned())?
            .send(request)
            .map_err(|_| "VTX controller stopped".to_owned())
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

    pub(crate) fn drain_into(&self, output: &mut Vec<RuntimeEvent>) {
        reap_recording_finalizers(false);
        output.clear();
        output.extend(
            self.events
                .lock()
                .expect("Nebulus event queue poisoned")
                .drain(..),
        );
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
            atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
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
        AdaptiveLink, ChannelId, DiversityCombiner, DiversityDecision, DiversitySourceId,
        DiversityStats, FecCounters, FrameLayout, PayloadRouteId, RadioPort, ReceiverRuntime,
        TxRadioParams, WfbKeypair, WfbTransmitter, WfbTxKeypair, WifiFrame,
    };
    use openipc_rtl88xx::{
        ChannelWidth, ChipFamily, DriverOptions, Jaguar3PowerTrackingState, MonitorOptions,
        RadioConfig, RealtekDevice, RealtekTxDescriptor, RealtekTxOptions,
    };
    use openipc_uplink::{NetworkConfig, UserspaceNetwork};
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
                let retune = if index > 0 {
                    let retune_started = Instant::now();
                    let report = device
                        .fast_retune(channel, true)
                        .map_err(|error| format!("retune channel {channel} failed: {error}"))?;
                    let elapsed = retune_started.elapsed();
                    std::thread::sleep(Duration::from_millis(5));
                    Some((elapsed, report.used_fast_path))
                } else {
                    None
                };
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
                        result: observed.finish(channel, started.elapsed(), retune),
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
    const CAPTURE_QUEUE_PER_ADAPTER: usize = 2;

    struct CaptureEvent {
        source_id: u16,
        buffer: Buffer,
        actual_len: usize,
        usb_latency_ms: f64,
    }

    struct CaptureWorker {
        return_buffer: mpsc::Sender<Buffer>,
        metrics: Arc<CaptureMetrics>,
        worker: Option<JoinHandle<()>>,
    }

    struct CaptureMetrics {
        source_id: u16,
        device_id: String,
        label: String,
        online: AtomicBool,
        transfers: AtomicU64,
        transfer_bytes: AtomicU64,
        queue_drops: AtomicU64,
        usb_errors: AtomicU64,
    }

    impl CaptureMetrics {
        fn new(source_id: u16, device_id: String, label: String) -> Self {
            Self {
                source_id,
                device_id,
                label,
                online: AtomicBool::new(true),
                transfers: AtomicU64::new(0),
                transfer_bytes: AtomicU64::new(0),
                queue_drops: AtomicU64::new(0),
                usb_errors: AtomicU64::new(0),
            }
        }

        fn snapshot(&self) -> AdapterRuntimeMetrics {
            AdapterRuntimeMetrics {
                source_id: self.source_id,
                device_id: self.device_id.clone(),
                label: self.label.clone(),
                online: self.online.load(Ordering::Relaxed),
                transfers: self.transfers.load(Ordering::Relaxed),
                transfer_bytes: self.transfer_bytes.load(Ordering::Relaxed),
                queue_drops: self.queue_drops.load(Ordering::Relaxed),
                usb_errors: self.usb_errors.load(Ordering::Relaxed),
                ..AdapterRuntimeMetrics::default()
            }
        }
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
            let metrics = Arc::new(CaptureMetrics::new(source_id, device_id, label));
            let thread_metrics = Arc::clone(&metrics);
            let (return_buffer, returned_buffers) = mpsc::channel();
            let worker = std::thread::Builder::new()
                .name(format!("nebulus-usb-rx-{source_id}"))
                .spawn(move || {
                    crate::low_latency::tune_receiver_thread();
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
                                thread_metrics.transfers.fetch_add(1, Ordering::Relaxed);
                                thread_metrics
                                    .transfer_bytes
                                    .fetch_add(actual_len as u64, Ordering::Relaxed);
                                let event = CaptureEvent {
                                    source_id,
                                    buffer: completion.buffer,
                                    actual_len,
                                    usb_latency_ms,
                                };
                                match completions.try_send(event) {
                                    Ok(()) => {}
                                    Err(TrySendError::Full(event)) => {
                                        thread_metrics.queue_drops.fetch_add(1, Ordering::Relaxed);
                                        endpoint.submit(event.buffer);
                                    }
                                    Err(TrySendError::Disconnected(event)) => {
                                        endpoint.submit(event.buffer);
                                        break;
                                    }
                                }
                            }
                            Err(TransferError::Disconnected) => {
                                thread_metrics.usb_errors.fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                            Err(TransferError::Stall) => {
                                thread_metrics.usb_errors.fetch_add(1, Ordering::Relaxed);
                                let _ = endpoint.clear_halt().wait();
                                endpoint.submit(completion.buffer);
                                consecutive_errors = 0;
                            }
                            Err(_) => {
                                thread_metrics.usb_errors.fetch_add(1, Ordering::Relaxed);
                                endpoint.submit(completion.buffer);
                                consecutive_errors = consecutive_errors.saturating_add(1);
                                if consecutive_errors >= 8 {
                                    break;
                                }
                            }
                        }
                    }
                    thread_metrics.online.store(false, Ordering::Relaxed);
                })
                .map_err(|error| format!("start USB capture worker failed: {error}"))?;
            Ok(Self {
                return_buffer,
                metrics,
                worker: Some(worker),
            })
        }

        fn snapshot(&self, diversity: &DiversityStats) -> AdapterRuntimeMetrics {
            let mut snapshot = self.metrics.snapshot();
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
                .filter(|worker| worker.metrics.online.load(Ordering::Relaxed))
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
        adaptive_enabled: bool,
        last_feedback_ms: Option<u64>,
        last_fec: FecCounters,
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

    struct UplinkRuntime {
        bridge: Option<crate::tun_bridge::TunBridge>,
        network: Arc<Mutex<UserspaceNetwork>>,
        transmitter: WfbTransmitter,
        tx_options: RealtekTxOptions,
        tx_params: TxRadioParams,
        last_session_ms: Option<u64>,
        metrics: VpnMetrics,
    }

    impl UplinkRuntime {
        fn new(request: &StartRequest, chip: ChipFamily) -> Result<Self, String> {
            let bridge = request
                .vpn_enabled
                .then(crate::tun_bridge::TunBridge::open_default)
                .transpose()?;
            let interface_name = bridge
                .as_ref()
                .map(|bridge| bridge.name().to_owned())
                .unwrap_or_default();
            let keypair = WfbTxKeypair::from_bytes(&request.key_bytes)
                .map_err(|error| format!("uplink transmit key is invalid: {error}"))?;
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
                network: Arc::new(Mutex::new(
                    UserspaceNetwork::new(NetworkConfig::default())
                        .map_err(|error| error.to_string())?,
                )),
                transmitter,
                tx_options: RealtekTxOptions {
                    current_channel: request.channel,
                    configured_channel_width: channel_width(request.channel_width_mhz)?,
                    descriptor: RealtekTxDescriptor::for_chip_family(chip),
                    ..RealtekTxOptions::default()
                },
                tx_params: TxRadioParams::openipc_uplink_default(),
                last_session_ms: None,
                metrics: VpnMetrics {
                    active: request.vpn_enabled,
                    interface_name,
                    ..VpnMetrics::default()
                },
            })
        }

        fn write_downlink(&mut self, payload: &[u8]) {
            if self
                .network
                .lock()
                .map_err(|_| ())
                .and_then(|mut network| network.ingest_tunnel_payload(payload).map_err(|_| ()))
                .is_err()
            {
                self.metrics.errors += 1;
            }
            if let Some(bridge) = self.bridge.as_mut() {
                match bridge.write_downlink(payload) {
                    Ok(0) => {}
                    Ok(bytes) => {
                        self.metrics.downlink_packets += 1;
                        self.metrics.downlink_bytes += bytes as u64;
                    }
                    Err(_) => self.metrics.errors += 1,
                }
            }
        }

        fn network(&self) -> Arc<Mutex<UserspaceNetwork>> {
            Arc::clone(&self.network)
        }

        fn network_metrics(&self) -> openipc_uplink::NetworkMetrics {
            self.network.lock().map_or_else(
                |_| openipc_uplink::NetworkMetrics::default(),
                |network| network.metrics(),
            )
        }

        fn tick(&mut self, now: u64, radio: &RadioWorker, adaptive: Option<Vec<u8>>) {
            let mut payloads = Vec::new();
            match self.network.lock() {
                Ok(mut network) => {
                    network.poll(now);
                    payloads.extend(network.drain_outbound());
                }
                Err(_) => self.metrics.errors += 1,
            }
            if let Some(payload) = adaptive {
                payloads.push(payload);
            }
            if let Some(bridge) = self.bridge.as_mut() {
                for _ in 0..32 {
                    match bridge.read_uplink() {
                        Ok(Some(payload)) => payloads.push(payload),
                        Ok(None) => break,
                        Err(_) => {
                            self.metrics.errors += 1;
                            break;
                        }
                    }
                }
            }
            if payloads.is_empty() {
                return;
            }
            let session_due = self
                .last_session_ms
                .is_none_or(|last| now.saturating_sub(last) >= 1_000);
            if session_due {
                let frame = self.transmitter.session_radio_packet(self.tx_params);
                if radio.enqueue(frame, self.tx_options) {
                    self.last_session_ms = Some(now);
                } else {
                    self.metrics.errors += 1;
                    return;
                }
            }
            for payload in payloads {
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

    struct VtxControlWorker {
        stop: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    impl VtxControlWorker {
        fn start(
            network: Arc<Mutex<UserspaceNetwork>>,
            credentials: openipc_uplink::SshCredentials,
            commands: mpsc::Receiver<crate::runtime::VtxControlRequest>,
            events: &super::EventQueue,
            context: &eframe::egui::Context,
        ) -> Result<Self, String> {
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let events = Arc::clone(events);
            let context = context.clone();
            let worker = std::thread::Builder::new()
                .name("nebulus-vtx-control".to_owned())
                .spawn(move || {
                    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().build() else {
                        super::queue(
                            &events,
                            RuntimeEvent::VtxControl(crate::runtime::VtxControlEvent::Failed(
                                "could not start VTX control executor".to_owned(),
                            )),
                        );
                        context.request_repaint();
                        return;
                    };
                    let mut controller = None;
                    while !worker_stop.load(Ordering::Relaxed) {
                        let request = match commands.recv_timeout(Duration::from_millis(50)) {
                            Ok(request) => request,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        };
                        runtime.block_on(crate::runtime::uplink_control::process_request(
                            &mut controller,
                            &network,
                            &credentials,
                            request,
                            |event| {
                                super::queue(&events, RuntimeEvent::VtxControl(event));
                                context.request_repaint();
                            },
                        ));
                    }
                })
                .map_err(|error| format!("start VTX control worker failed: {error}"))?;
            Ok(Self {
                stop,
                worker: Some(worker),
            })
        }
    }

    impl Drop for VtxControlWorker {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    pub(super) fn run(
        request: StartRequest,
        stop: &AtomicBool,
        audio_volume: &AtomicU8,
        recording_control: &Mutex<super::RecordingControl>,
        vtx_commands: mpsc::Receiver<crate::runtime::VtxControlRequest>,
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
        let mut uplink =
            (request.vpn_enabled || request.vtx_control_enabled || request.adaptive_link)
                .then(|| UplinkRuntime::new(&request, chip))
                .transpose()?;
        if let Some(uplink) = uplink.as_ref().filter(|_| request.vpn_enabled) {
            log(
                events,
                context,
                LogLevel::Info,
                "vpn",
                format!(
                    "VPN active on {} at {}/{}",
                    uplink
                        .bridge
                        .as_ref()
                        .map_or("OpenIPC VPN", crate::tun_bridge::TunBridge::name),
                    crate::tun_bridge::ADDRESS,
                    crate::tun_bridge::PREFIX_LENGTH
                ),
            );
        }
        let _vtx_control = if request.vtx_control_enabled {
            Some(VtxControlWorker::start(
                uplink
                    .as_ref()
                    .expect("VTX control requires an uplink runtime")
                    .network(),
                request.vtx_credentials.clone(),
                vtx_commands,
                events,
                context,
            )?)
        } else {
            drop(vtx_commands);
            None
        };
        let radio =
            RadioWorker::start(Arc::clone(&device), chip, uplink.is_some(), events, context)?;
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
            .saturating_mul(CAPTURE_QUEUE_PER_ADAPTER)
            .max(CAPTURE_QUEUE_PER_ADAPTER);
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
        let mut last_decoded_frames = 0;
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
            let wait = if uplink.is_some() {
                Duration::from_millis(10)
            } else {
                Duration::from_millis(50)
            };
            let event = match capture_events.recv_timeout(wait) {
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
                    let now = now_ms();
                    let adaptive = link.feedback_due(now);
                    if let (Some(uplink), Some(radio)) = (uplink.as_mut(), radio.as_ref()) {
                        uplink.tick(now, radio, adaptive);
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
            let source = DiversitySourceId::new(event.source_id);
            let selected_packets = packets.into_iter().filter_map(|packet| {
                if packet.attrib.crc_err
                    || packet.attrib.icv_err
                    || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
                {
                    return Some((packet, None));
                }
                let frame = WifiFrame::parse(packet.data, FrameLayout::WithFcs).ok();
                let is_video = frame.is_some_and(|frame| {
                    frame.matches_channel_id(ChannelId::new(request.channel_id))
                });
                if is_video {
                    source_quality[source_index].record_rx_paths(
                        now,
                        packet.attrib.rssi,
                        packet.attrib.snr,
                    );
                }
                let decision = match (diversity_enabled, frame) {
                    (true, Some(frame)) => diversity.observe_wifi_frame(source, frame),
                    _ => DiversityDecision::Passthrough,
                };
                if decision != DiversityDecision::Duplicate && is_video {
                    link.record_rx(now, packet.attrib.rssi, packet.attrib.snr);
                }
                decision.should_forward().then_some((packet, frame))
            });
            let pipeline_start = Instant::now();
            let mut batch = receiver.push_parsed_rx_packets(selected_packets, &options);
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
            let stats = decoder.stats();
            if let Some(decoded) = decoder.latest_frame() {
                presenter.submit(decoded, stats.last_decode_latency_us as f64 / 1_000.0);
            }

            if let Some(uplink) = uplink.as_mut() {
                for payload in &batch.raw_payloads {
                    if payload.route_id == VPN_ROUTE_ID {
                        uplink.write_downlink(&payload.data);
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
            let decoder_frames = stats.frames_decoded.saturating_sub(last_decoded_frames);
            last_decoded_frames = stats.frames_decoded;
            if let Some(metrics) = metrics_throttle.push(BatchMetrics {
                transfers: 1,
                transfer_bytes: event.actual_len,
                packets: batch.counters.packets,
                rtp_packets: batch.counters.rtp_packets,
                video_frames: batch.counters.video_frames,
                decoder_frames,
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
                uplink: uplink.as_ref().map_or_else(
                    openipc_uplink::NetworkMetrics::default,
                    UplinkRuntime::network_metrics,
                ),
                routes: route_updates,
                telemetry,
                audio: route_processor.audio_stats(),
                vpn: uplink
                    .as_ref()
                    .map_or_else(VpnMetrics::default, |uplink| uplink.metrics.clone()),
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
            let adaptive = link.feedback_due(now);
            if let (Some(uplink), Some(radio)) = (uplink.as_mut(), radio.as_ref()) {
                uplink.tick(now, radio, adaptive);
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
        let mock_codec = request.codec_preference.mock_codec();
        let mock_codec_label = match mock_codec {
            openipc_core::Codec::H264 => "H.264",
            openipc_core::Codec::H265 => "H.265",
        };
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
                receivers: vec![crate::runtime::ReceiverInfo::codec_mock(mock_codec)],
                decoder: decoder_environment(decoder.capabilities()),
            },
        );
        send(events, context, RuntimeEvent::Started);
        log(
            events,
            context,
            LogLevel::Info,
            "mock",
            format!("Pre-recorded 1080p {mock_codec_label} + Opus RTP mock started"),
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
        let mut source = MockAvStream::new(mock_codec)?;
        let mock_started = Instant::now();
        let mut payload_sequence = 1u64;
        let mut recorder: Option<EncodedRecorder> = None;
        let mut armed_path: Option<PathBuf> = None;
        let mut last_decoded_frames = 0;

        while !stop.load(Ordering::Relaxed) {
            source.rebase_timing_if_late(
                mock_started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                50_000,
            );
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
            let stats = decoder.stats();
            if let Some(decoded) = decoder.latest_frame() {
                presenter.submit(decoded, stats.last_decode_latency_us as f64 / 1_000.0);
            }
            metrics.decoder_frames = stats.frames_decoded.saturating_sub(last_decoded_frames);
            last_decoded_frames = stats.frames_decoded;
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
        }

        fn feedback_due(&mut self, now: u64) -> Option<Vec<u8>> {
            if !self.adaptive_enabled
                || self
                    .last_feedback_ms
                    .is_some_and(|last| now.saturating_sub(last) < 100)
            {
                return None;
            }
            self.last_feedback_ms = Some(now);
            Some(self.quality.feedback_ip_packet(now))
        }
    }

    fn build_link(
        request: &StartRequest,
        _chip: ChipFamily,
        fec: FecCounters,
        device: &RealtekDevice,
    ) -> Result<LinkRuntime, String> {
        if request.adaptive_link {
            device
                .set_tx_power_override(request.channel, request.tx_power)
                .map_err(|error| error.to_string())?;
        }
        Ok(LinkRuntime {
            quality: AdaptiveLink::new(),
            adaptive_enabled: request.adaptive_link,
            last_feedback_ms: None,
            last_fec: fec,
        })
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
