use std::collections::BTreeMap;
#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use eframe::egui;
use web_time::Instant;

#[cfg(target_arch = "wasm32")]
type PendingKeyFile = Rc<RefCell<Option<Result<(String, Vec<u8>), String>>>>;

use crate::{
    model::{
        AudioStats, DiagnosticsState, EnvironmentDetails, LiveMetrics, LogEntry, LogLevel,
        MetricHistory, ReceiverState, RecordingState, RecordingStatus, RouteStats, VpnStatus,
    },
    runtime::{ReceiverInfo, Runtime, RuntimeEvent, StartRequest, UsbDeviceInfo},
    settings::Settings,
};

/// Main Nebulus application state.
pub struct NebulusApp {
    pub(crate) settings: Settings,
    pub(crate) state: ReceiverState,
    pub(crate) devices: Vec<UsbDeviceInfo>,
    pub(crate) receiver_info: Option<ReceiverInfo>,
    pub(crate) metrics: LiveMetrics,
    pub(crate) history: MetricHistory,
    pub(crate) logs: Vec<LogEntry>,
    pub(crate) log_filter: LogLevel,
    pub(crate) log_search: String,
    pub(crate) route_stats: BTreeMap<u64, RouteStats>,
    pub(crate) audio: AudioStats,
    pub(crate) diagnostics: DiagnosticsState,
    pub(crate) environment: EnvironmentDetails,
    pub(crate) recording: RecordingStatus,
    pub(crate) vpn: VpnStatus,
    #[cfg(target_os = "windows")]
    pub(crate) wintun_state: crate::wintun::InstallState,
    #[cfg(target_os = "windows")]
    wintun_events: Option<std::sync::mpsc::Receiver<crate::wintun::InstallEvent>>,
    pub(crate) active_tab: crate::ui::PanelTab,
    pub(crate) runtime: Runtime,
    pub(crate) texture: Option<egui::TextureHandle>,
    pub(crate) video_renderer: Option<crate::video::PlatformVideoRenderer>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) desktop_tray: Option<crate::desktop_tray::DesktopTray>,
    pub(crate) frame_size: Option<[usize; 2]>,
    pub(crate) key_name: String,
    pub(crate) key_error: Option<String>,
    #[cfg(target_arch = "wasm32")]
    key_file_result: PendingKeyFile,
    pub(crate) video_fullscreen: bool,
    pub(crate) show_about: bool,
    started_at: Instant,
    metrics_started_at: Instant,
    rate_window_started: Instant,
    rate_window_bytes: u64,
    rate_window_frames: u64,
    rate_window_decoded: u64,
    rate_window_rendered: u64,
    last_rate_fec: openipc_core::FecCounters,
    next_log_sequence: u64,
}

impl NebulusApp {
    pub(crate) fn new(context: &eframe::CreationContext<'_>) -> Self {
        let settings: Settings = context
            .storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default();
        crate::logging::set_level(settings.diagnostic_verbosity.log_level());
        let key_name = if settings.key_bytes == crate::settings::DEFAULT_KEY_BYTES {
            "Default gs.key".to_owned()
        } else {
            "Saved gs.key".to_owned()
        };
        let (video_renderer, video_renderer_error) =
            match crate::video::PlatformVideoRenderer::new(context) {
                Ok(renderer) => (Some(renderer), None),
                Err(error) => (None, Some(error)),
            };
        let mut app = Self {
            settings,
            state: ReceiverState::Idle,
            devices: Vec::new(),
            receiver_info: None,
            metrics: LiveMetrics::default(),
            history: MetricHistory::default(),
            logs: Vec::new(),
            log_filter: LogLevel::Trace,
            log_search: String::new(),
            route_stats: BTreeMap::new(),
            audio: AudioStats::default(),
            diagnostics: DiagnosticsState::default(),
            environment: EnvironmentDetails::detect(),
            recording: RecordingStatus::default(),
            vpn: VpnStatus::default(),
            #[cfg(target_os = "windows")]
            wintun_state: crate::wintun::InstallState::detect(),
            #[cfg(target_os = "windows")]
            wintun_events: None,
            active_tab: crate::ui::PanelTab::Settings,
            runtime: Runtime::new(context.egui_ctx.clone()),
            texture: None,
            video_renderer,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            desktop_tray: None,
            frame_size: None,
            key_name,
            key_error: None,
            #[cfg(target_arch = "wasm32")]
            key_file_result: Rc::new(RefCell::new(None)),
            video_fullscreen: false,
            show_about: false,
            started_at: Instant::now(),
            metrics_started_at: Instant::now(),
            rate_window_started: Instant::now(),
            rate_window_bytes: 0,
            rate_window_frames: 0,
            rate_window_decoded: 0,
            rate_window_rendered: 0,
            last_rate_fec: openipc_core::FecCounters::default(),
            next_log_sequence: 1,
        };
        crate::ui::theme::apply(&context.egui_ctx, app.settings.gui_theme);
        app.settings.interface_scale_percent = app.settings.interface_scale_percent.clamp(75, 150);
        context
            .egui_ctx
            .set_zoom_factor(f32::from(app.settings.interface_scale_percent) / 100.0);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        match crate::desktop_tray::DesktopTray::new(&context.egui_ctx) {
            Ok(tray) => app.desktop_tray = Some(tray),
            Err(error) => app.log(LogLevel::Warn, "tray", error),
        }
        app.log(LogLevel::Info, "app", "Nebulus ready");
        if let Some(error) = video_renderer_error {
            app.log(
                LogLevel::Error,
                "video",
                format!("Video renderer initialization failed: {error}"),
            );
        }
        app.log(
            LogLevel::Debug,
            "app",
            format!(
                "egui renderer scale {:.2}",
                context.egui_ctx.pixels_per_point()
            ),
        );
        app.runtime.refresh_devices();
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        if std::env::var_os("NEBULUS_CODEC_MOCK").is_some() {
            app.start_codec_mock(&context.egui_ctx);
        }
        app
    }

    pub(crate) fn start_receiver(&mut self, context: &egui::Context) {
        self.reset_runtime_metrics();
        let request = self.start_request();
        self.runtime.start(request, context.clone());
    }

    #[cfg(debug_assertions)]
    pub(crate) fn start_codec_mock(&mut self, context: &egui::Context) {
        self.reset_runtime_metrics();
        let request = self.start_request();
        self.runtime.start_codec_mock(request, context.clone());
    }

    fn start_request(&self) -> StartRequest {
        StartRequest {
            #[cfg(target_os = "android")]
            video_output: self
                .video_renderer
                .as_ref()
                .map(crate::video::PlatformVideoRenderer::output_window),
            device_id: self.settings.device_id.clone(),
            channel: self.settings.channel,
            channel_width_mhz: self.settings.channel_width_mhz,
            channel_offset: self.settings.channel_offset,
            channel_id: self.settings.video_channel().raw(),
            minimum_epoch: self.settings.minimum_epoch,
            transfer_size: self.settings.transfer_size,
            codec_preference: self.settings.codec_preference,
            rtp_reorder: self.settings.rtp_reorder,
            adaptive_link: self.settings.adaptive_link,
            tx_power: self.settings.tx_power,
            key_bytes: self.settings.key_bytes.clone(),
            audio_volume: self.settings.audio_volume,
            vpn_enabled: self.settings.vpn_enabled && self.vpn_available(),
            payload_routes: self.settings.payload_routes.clone(),
        }
    }

    fn reset_runtime_metrics(&mut self) {
        self.metrics = LiveMetrics::default();
        self.history.clear();
        self.metrics_started_at = Instant::now();
        self.rate_window_started = Instant::now();
        self.rate_window_bytes = 0;
        self.rate_window_frames = 0;
        self.rate_window_decoded = 0;
        self.rate_window_rendered = 0;
        self.last_rate_fec = openipc_core::FecCounters::default();
        self.route_stats.clear();
        self.audio = AudioStats::default();
        self.diagnostics = DiagnosticsState::default();
        self.vpn = VpnStatus::default();
    }

    pub(crate) fn stop_receiver(&mut self) {
        if self.recording.state != RecordingState::Idle {
            self.runtime.stop_recording();
        }
        self.state = ReceiverState::Stopping;
        self.runtime.stop();
    }

    pub(crate) fn toggle_recording(&mut self) {
        if self.recording.state != RecordingState::Idle {
            self.runtime.stop_recording();
            return;
        }
        if self.state != ReceiverState::Receiving {
            self.log(LogLevel::Warn, "record", "Start RX before recording");
            return;
        }
        self.start_recording();
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn start_recording(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save OpenIPC recording")
            .set_file_name("openipc-recording.mp4")
            .add_filter("MP4 video", &["mp4"])
            .save_file()
        else {
            return;
        };
        self.runtime.start_recording(path);
    }

    #[cfg(target_os = "android")]
    fn start_recording(&mut self) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        let path = std::env::temp_dir().join(format!("openipc-{timestamp}.mp4"));
        self.runtime.start_recording(path);
    }

    #[cfg(target_arch = "wasm32")]
    fn start_recording(&mut self) {
        self.runtime.start_recording();
    }

    pub(crate) fn refresh_devices(&mut self) {
        self.runtime.refresh_devices();
    }

    pub(crate) fn vpn_available(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
        #[cfg(target_os = "windows")]
        {
            self.wintun_state.is_ready()
        }
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
        {
            true
        }
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn install_wintun(&mut self, context: &egui::Context) {
        if matches!(
            self.wintun_state,
            crate::wintun::InstallState::Downloading { .. }
                | crate::wintun::InstallState::Installing
        ) {
            return;
        }
        match crate::wintun::spawn_installer(context.clone()) {
            Ok(events) => {
                self.wintun_state = crate::wintun::InstallState::Downloading {
                    downloaded: 0,
                    total: None,
                };
                self.wintun_events = Some(events);
                self.log(LogLevel::Info, "wintun", "Downloading Wintun");
            }
            Err(error) => {
                self.wintun_state = crate::wintun::InstallState::Failed(error.clone());
                self.log(LogLevel::Error, "wintun", error);
            }
        }
    }

    fn process_events(&mut self, context: &egui::Context) {
        #[cfg(target_os = "windows")]
        self.process_wintun_events();
        self.process_key_file_result();
        self.process_dropped_files(context);
        if self.video_fullscreen && context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.set_video_fullscreen(context, false);
        }
        let events = self.runtime.drain().collect::<Vec<_>>();
        for event in events {
            match event {
                RuntimeEvent::Devices(devices) => {
                    self.devices = devices;
                    if self.settings.device_id.is_none() {
                        self.settings.device_id =
                            self.devices.first().map(|device| device.id.clone());
                    }
                }
                RuntimeEvent::DiscoveryFailed(error) => {
                    self.log(LogLevel::Warn, "usb", error);
                }
                RuntimeEvent::Connecting => {
                    self.receiver_info = None;
                    self.state = ReceiverState::Connecting;
                }
                RuntimeEvent::Connected { receiver, decoder } => {
                    let label = receiver.label.clone();
                    self.receiver_info = Some(receiver);
                    self.metrics.decoder_name = decoder.backend.clone();
                    self.environment.decoder_backend = decoder.backend;
                    self.environment.h264 =
                        capability_label(decoder.h264_supported, decoder.h264_hardware);
                    self.environment.h265 =
                        capability_label(decoder.h265_supported, decoder.h265_hardware);
                    self.environment.native_surfaces = decoder.native_surfaces;
                    self.state = ReceiverState::Ready;
                    self.log(LogLevel::Info, "usb", format!("Connected to {label}"));
                }
                RuntimeEvent::Started => self.state = ReceiverState::Receiving,
                RuntimeEvent::Batch(batch) => self.apply_batch(*batch),
                RuntimeEvent::NativeVideo {
                    frame,
                    decode_latency_ms,
                    ready_at,
                } => {
                    let dimensions = frame.dimensions();
                    let uploaded = self
                        .video_renderer
                        .as_mut()
                        .is_some_and(|renderer| renderer.upload(&frame).is_ok());
                    if uploaded {
                        self.frame_size =
                            Some([dimensions.width as usize, dimensions.height as usize]);
                        self.record_presented_frame(
                            [dimensions.width, dimensions.height],
                            decode_latency_ms,
                        );
                    } else {
                        self.video_renderer = None;
                        #[cfg(not(target_arch = "wasm32"))]
                        match crate::video::fallback_rgba(&frame) {
                            Ok(rgba) => self.apply_presented_frame(
                                context,
                                crate::video::PresentedFrame {
                                    dimensions,
                                    rgba,
                                    decode_latency_ms,
                                },
                            ),
                            Err(error) => self.log(LogLevel::Warn, "video", error),
                        }
                        #[cfg(target_arch = "wasm32")]
                        self.log(
                            LogLevel::Warn,
                            "video",
                            "Direct WebCodecs/WebGL frame upload failed",
                        );
                    }
                    let presentation_queue_latency_ms = ready_at.elapsed().as_secs_f64() * 1_000.0;
                    self.metrics.presentation_queue_latency_ms = presentation_queue_latency_ms;
                    self.metrics.local_processing_latency_ms = self.metrics.video_submit_path_ms
                        + self.metrics.decode_latency_ms
                        + presentation_queue_latency_ms;
                    self.diagnostics
                        .observe("Decode to GPU upload", presentation_queue_latency_ms);
                }
                RuntimeEvent::Log {
                    level,
                    target,
                    message,
                } => self.log(level, target, message),
                RuntimeEvent::RecordingArmed(path) => {
                    self.recording.state = RecordingState::Armed;
                    self.recording.path = path;
                    self.recording.bytes = 0;
                }
                RuntimeEvent::RecordingStarted { path, codec } => {
                    self.recording.state = RecordingState::Recording;
                    self.recording.path.clone_from(&path);
                    self.recording.codec.clone_from(&codec);
                    self.log(
                        LogLevel::Info,
                        "record",
                        format!("Recording {codec} to {path}"),
                    );
                }
                RuntimeEvent::RecordingStopped { path, bytes } => {
                    self.recording.state = RecordingState::Idle;
                    self.recording.path.clone_from(&path);
                    self.recording.bytes = bytes;
                    self.log(
                        LogLevel::Info,
                        "record",
                        format!("Recording stopped: {bytes} bytes at {path}"),
                    );
                }
                RuntimeEvent::RecordingFailed(error) => {
                    self.recording.state = RecordingState::Idle;
                    self.log(LogLevel::Error, "record", error);
                }
                RuntimeEvent::Stopped => {
                    self.receiver_info = None;
                    self.state = ReceiverState::Idle;
                    self.recording.state = RecordingState::Idle;
                    self.vpn.active = false;
                    self.log(LogLevel::Info, "rx", "Receiver stopped");
                }
                RuntimeEvent::Failed(error) => {
                    self.receiver_info = None;
                    self.state = ReceiverState::Failed;
                    self.recording.state = RecordingState::Idle;
                    self.vpn.active = false;
                    self.log(LogLevel::Error, "runtime", error);
                }
            }
        }
        if self.state == ReceiverState::Receiving
            && (self.rate_window_bytes > 0 || self.rate_window_frames > 0)
        {
            self.update_rates();
        }
    }

    #[cfg(target_os = "windows")]
    fn process_wintun_events(&mut self) {
        use std::sync::mpsc::TryRecvError;

        let mut updates = Vec::new();
        let mut disconnected = false;
        if let Some(events) = self.wintun_events.as_ref() {
            loop {
                match events.try_recv() {
                    Ok(event) => updates.push(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        let mut terminal = false;
        for event in updates {
            match event {
                crate::wintun::InstallEvent::Progress { downloaded, total } => {
                    self.wintun_state =
                        crate::wintun::InstallState::Downloading { downloaded, total };
                }
                crate::wintun::InstallEvent::Installing => {
                    self.wintun_state = crate::wintun::InstallState::Installing;
                }
                crate::wintun::InstallEvent::Complete(path) => {
                    self.wintun_state = crate::wintun::InstallState::Ready;
                    self.log(
                        LogLevel::Info,
                        "wintun",
                        format!(
                            "Wintun {} installed at {}",
                            crate::wintun::VERSION,
                            path.display()
                        ),
                    );
                    terminal = true;
                }
                crate::wintun::InstallEvent::Failed(error) => {
                    self.wintun_state = crate::wintun::InstallState::Failed(error.clone());
                    self.log(LogLevel::Error, "wintun", error);
                    terminal = true;
                }
            }
        }
        if disconnected
            && !terminal
            && matches!(
                self.wintun_state,
                crate::wintun::InstallState::Downloading { .. }
                    | crate::wintun::InstallState::Installing
            )
        {
            let error = "Wintun installer stopped before completion".to_owned();
            self.wintun_state = crate::wintun::InstallState::Failed(error.clone());
            self.log(LogLevel::Error, "wintun", error);
            terminal = true;
        }
        if terminal || disconnected {
            self.wintun_events = None;
        }
    }

    fn process_dropped_files(&mut self, context: &egui::Context) {
        let dropped = context.input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
        for file in dropped {
            let name = if file.name.is_empty() {
                file.path
                    .as_deref()
                    .and_then(std::path::Path::file_name)
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or("gs.key")
                    .to_owned()
            } else {
                file.name
            };
            let bytes = file.bytes.map(|bytes| bytes.to_vec());
            #[cfg(not(target_arch = "wasm32"))]
            let bytes = bytes.or_else(|| {
                file.path
                    .as_deref()
                    .and_then(|path| std::fs::read(path).ok())
            });
            if let Some(bytes) = bytes {
                if let Err(error) = self.set_key_file(name, bytes) {
                    self.log(LogLevel::Warn, "key", error);
                }
            }
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    pub(crate) fn open_key_file(&mut self, _context: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open OpenIPC ground-station key")
            .add_filter("OpenIPC key", &["key"])
            .pick_file()
        else {
            return;
        };
        let name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("gs.key")
            .to_owned();
        match std::fs::read(&path) {
            Ok(bytes) => {
                if let Err(error) = self.set_key_file(name, bytes) {
                    self.key_error = Some(error);
                }
            }
            Err(error) => {
                self.key_error = Some(format!("Could not read {}: {error}", path.display()))
            }
        }
    }

    #[cfg(target_os = "android")]
    pub(crate) fn open_key_file(&mut self, context: &egui::Context) {
        if let Err(error) = crate::android::open_key_file(context.clone()) {
            self.key_error = Some(error);
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn open_key_file(&mut self, context: &egui::Context) {
        let result = Rc::clone(&self.key_file_result);
        let context = context.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Open OpenIPC ground-station key")
                .add_filter("OpenIPC key", &["key"])
                .pick_file()
                .await
            else {
                return;
            };
            *result.borrow_mut() = Some(Ok((file.file_name(), file.read().await)));
            context.request_repaint();
        });
    }

    pub(crate) fn reset_key(&mut self) {
        let _ = self.set_key_file(
            "Default gs.key".to_owned(),
            crate::settings::DEFAULT_KEY_BYTES.to_vec(),
        );
    }

    pub(crate) fn set_video_fullscreen(&mut self, context: &egui::Context, enabled: bool) {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            let result = web_sys::window()
                .and_then(|window| window.document())
                .ok_or_else(|| "browser document is unavailable".to_owned())
                .and_then(|document| {
                    if enabled {
                        document
                            .get_element_by_id("nebulus-canvas")
                            .ok_or_else(|| "Nebulus canvas is unavailable".to_owned())?
                            .request_fullscreen()
                            .map_err(|error| {
                                format!("browser fullscreen request failed: {error:?}")
                            })
                    } else {
                        document.exit_fullscreen();
                        Ok(())
                    }
                });
            match result {
                Ok(()) => self.video_fullscreen = enabled,
                Err(error) => {
                    self.video_fullscreen = false;
                    self.log(LogLevel::Warn, "fullscreen", error);
                }
            }
            context.request_repaint();
        }

        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            self.video_fullscreen = enabled;
            context.send_viewport_cmd(egui::ViewportCommand::Fullscreen(enabled));
        }
    }

    fn process_key_file_result(&mut self) {
        #[cfg(target_os = "android")]
        if let Some(result) = crate::android::take_key_file_result() {
            match result {
                Ok(file) => {
                    if let Err(error) = self.set_key_file(file.name, file.bytes) {
                        self.key_error = Some(error);
                    }
                }
                Err(error) => self.key_error = Some(error),
            }
        }
        #[cfg(target_arch = "wasm32")]
        let result = self.key_file_result.borrow_mut().take();
        #[cfg(target_arch = "wasm32")]
        if let Some(result) = result {
            match result {
                Ok((name, bytes)) => {
                    if let Err(error) = self.set_key_file(name, bytes) {
                        self.key_error = Some(error);
                    }
                }
                Err(error) => self.key_error = Some(error),
            }
        }
    }

    fn set_key_file(&mut self, name: String, bytes: Vec<u8>) -> Result<(), String> {
        let bytes = validate_key(bytes)?;
        self.settings.key_bytes = bytes;
        self.key_name = name;
        self.key_error = None;
        self.log(
            LogLevel::Info,
            "key",
            format!("Receiver key loaded from {}", self.key_name),
        );
        Ok(())
    }

    fn apply_batch(&mut self, batch: crate::runtime::BatchMetrics) {
        self.metrics.usb_bytes += batch.transfer_bytes as u64;
        self.metrics.usb_transfers += batch.transfers;
        self.metrics.wifi_packets += batch.packets as u64;
        self.metrics.rtp_packets += batch.rtp_packets as u64;
        self.metrics.encoded_frames += batch.video_frames as u64;
        self.metrics.fec_total_packets = batch.fec.total_packets;
        self.metrics.recovered_packets = batch.fec.recovered_packets;
        self.metrics.lost_packets = batch.fec.lost_packets;
        self.metrics.rssi = batch.rssi;
        self.metrics.snr = batch.snr;
        self.metrics.link_score = batch.link_score;
        self.metrics.decoder_drops = batch.decoder_drops;
        self.metrics.decoder_errors = batch.decoder_errors;
        self.metrics.usb_latency_ms = batch.usb_latency_ms;
        self.metrics.pipeline_latency_ms = batch.pipeline_latency_ms;
        self.metrics.batch_latency_ms = batch.batch_latency_ms;
        self.metrics.video_submit_path_ms = batch.video_submit_path_ms;
        self.metrics.local_processing_latency_ms = batch.video_submit_path_ms
            + self.metrics.decode_latency_ms
            + self.metrics.presentation_queue_latency_ms;
        accumulate_counters(&mut self.diagnostics.counters, batch.counters);
        self.diagnostics.rtp = batch.rtp;
        self.diagnostics.reorder = batch.reorder;
        self.diagnostics.observe("USB wait", batch.usb_latency_ms);
        self.diagnostics
            .observe("Realtek parse", batch.parse_latency_ms);
        self.diagnostics
            .observe("WFB + RTP", batch.pipeline_latency_ms);
        self.diagnostics.observe("Routes", batch.route_latency_ms);
        self.diagnostics
            .observe("Decode submit", batch.decode_submit_latency_ms);
        self.diagnostics.observe(
            "USB completion to decode submit",
            batch.video_submit_path_ms,
        );
        self.diagnostics
            .observe("Receive batch", batch.batch_latency_ms);
        self.vpn.active = batch.vpn.active;
        self.vpn.interface_name = batch.vpn.interface_name;
        self.vpn.downlink_packets = batch.vpn.downlink_packets;
        self.vpn.downlink_bytes = batch.vpn.downlink_bytes;
        self.vpn.uplink_packets = batch.vpn.uplink_packets;
        self.vpn.uplink_bytes = batch.vpn.uplink_bytes;
        self.vpn.errors = batch.vpn.errors;
        self.rate_window_bytes = self
            .rate_window_bytes
            .saturating_add(batch.video_bytes as u64);
        self.rate_window_frames += batch.video_frames as u64;
        for update in batch.routes {
            let stats = self.route_stats.entry(update.route_id).or_default();
            stats.packets = stats.packets.saturating_add(update.packets);
            stats.bytes = stats.bytes.saturating_add(update.bytes);
            stats.last_bytes = update.last_bytes;
            stats.errors = stats.errors.saturating_add(update.errors);
        }
        self.audio = batch.audio;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn apply_presented_frame(
        &mut self,
        context: &egui::Context,
        frame: crate::video::PresentedFrame,
    ) {
        let dimensions = [frame.dimensions.width, frame.dimensions.height];
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [dimensions[0] as usize, dimensions[1] as usize],
            &frame.rgba,
        );
        if let Some(texture) = self.texture.as_mut() {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.texture =
                Some(context.load_texture("openipc-video", image, egui::TextureOptions::LINEAR));
        }
        self.frame_size = Some([dimensions[0] as usize, dimensions[1] as usize]);
        self.record_presented_frame(dimensions, frame.decode_latency_ms);
    }

    fn record_presented_frame(&mut self, dimensions: [u32; 2], decode_latency_ms: f64) {
        self.metrics.resolution = Some(dimensions);
        self.metrics.decode_latency_ms = decode_latency_ms;
        self.metrics.local_processing_latency_ms = self.metrics.video_submit_path_ms
            + decode_latency_ms
            + self.metrics.presentation_queue_latency_ms;
        self.diagnostics
            .observe("Hardware decode", decode_latency_ms);
        self.metrics.decoded_frames += 1;
        self.metrics.render_frames += 1;
        self.rate_window_decoded += 1;
        self.rate_window_rendered += 1;
        let observed = self.environment.maximum_observed_resolution;
        if observed.is_none_or(|current| {
            u64::from(dimensions[0]) * u64::from(dimensions[1])
                > u64::from(current[0]) * u64::from(current[1])
        }) {
            self.environment.maximum_observed_resolution = Some(dimensions);
        }
    }

    fn update_rates(&mut self) {
        let elapsed = self.rate_window_started.elapsed();
        if elapsed.as_secs_f64() < 1.0 {
            return;
        }
        let seconds = elapsed.as_secs_f64();
        self.metrics.bitrate_bps = self.rate_window_bytes as f64 * 8.0 / seconds;
        self.metrics.receive_fps = self.rate_window_frames as f64 / seconds;
        self.metrics.decode_fps = self.rate_window_decoded as f64 / seconds;
        self.metrics.render_fps = self.rate_window_rendered as f64 / seconds;
        self.environment.maximum_observed_fps = self
            .environment
            .maximum_observed_fps
            .max(self.metrics.decode_fps);
        let time = self.metrics_started_at.elapsed().as_secs_f64();
        self.history.link_score.push(
            time,
            self.metrics.link_score[0].max(self.metrics.link_score[1]) as f64,
        );
        self.history
            .bitrate
            .push(time, self.metrics.bitrate_bps / 1_000_000.0);
        self.history
            .receive_fps
            .push(time, self.metrics.receive_fps);
        let fec_total = self
            .metrics
            .fec_total_packets
            .saturating_sub(self.last_rate_fec.total_packets);
        let fec_recovered = self
            .metrics
            .recovered_packets
            .saturating_sub(self.last_rate_fec.recovered_packets);
        let fec_lost = self
            .metrics
            .lost_packets
            .saturating_sub(self.last_rate_fec.lost_packets);
        self.last_rate_fec = openipc_core::FecCounters {
            total_packets: self.metrics.fec_total_packets,
            recovered_packets: self.metrics.recovered_packets,
            lost_packets: self.metrics.lost_packets,
            bad_packets: 0,
        };
        let (loss, fec_recovery) = fec_window_percentages(fec_total, fec_recovered, fec_lost);
        self.history.loss.push(time, loss);
        self.history.fec_recovery.push(time, fec_recovery);
        self.history
            .local_processing_ms
            .push(time, self.metrics.local_processing_latency_ms);
        self.rate_window_started = Instant::now();
        self.rate_window_bytes = 0;
        self.rate_window_frames = 0;
        self.rate_window_decoded = 0;
        self.rate_window_rendered = 0;
    }

    pub(crate) fn metric_view_time(&self) -> f64 {
        if self.state == ReceiverState::Receiving {
            self.metrics_started_at.elapsed().as_secs_f64()
        } else {
            self.history.latest_time()
        }
    }

    pub(crate) fn log(
        &mut self,
        level: LogLevel,
        target: impl Into<String>,
        message: impl Into<String>,
    ) {
        let visible = match self.settings.diagnostic_verbosity {
            crate::settings::DiagnosticVerbosity::Low => {
                matches!(level, LogLevel::Warn | LogLevel::Error)
            }
            crate::settings::DiagnosticVerbosity::Normal => {
                matches!(level, LogLevel::Info | LogLevel::Warn | LogLevel::Error)
            }
            crate::settings::DiagnosticVerbosity::High => !matches!(level, LogLevel::Trace),
            crate::settings::DiagnosticVerbosity::VeryHigh => true,
        };
        if !visible {
            return;
        }
        if self.logs.len() >= 1_000 {
            self.logs.drain(..100);
        }
        let sequence = self.next_log_sequence;
        self.next_log_sequence = self.next_log_sequence.wrapping_add(1).max(1);
        self.logs.push(LogEntry {
            sequence,
            elapsed_seconds: self.started_at.elapsed().as_secs_f64(),
            level,
            target: target.into(),
            message: message.into(),
        });
    }
}

fn capability_label(supported: bool, hardware: Option<bool>) -> String {
    if !supported {
        return "Unavailable".to_owned();
    }
    match hardware {
        Some(true) => "Supported, hardware accelerated",
        Some(false) => "Supported, software",
        None => "Supported, acceleration not reported",
    }
    .to_owned()
}

fn accumulate_counters(
    current: &mut openipc_core::ReceiverBatchCounters,
    batch: openipc_core::ReceiverBatchCounters,
) {
    current.packets = current.packets.saturating_add(batch.packets);
    current.accepted_packets = current
        .accepted_packets
        .saturating_add(batch.accepted_packets);
    current.dropped_packets = current
        .dropped_packets
        .saturating_add(batch.dropped_packets);
    current.crc_dropped = current.crc_dropped.saturating_add(batch.crc_dropped);
    current.icv_dropped = current.icv_dropped.saturating_add(batch.icv_dropped);
    current.report_dropped = current.report_dropped.saturating_add(batch.report_dropped);
    current.ignored_frames = current.ignored_frames.saturating_add(batch.ignored_frames);
    current.sessions = current.sessions.saturating_add(batch.sessions);
    current.wfb_payloads = current.wfb_payloads.saturating_add(batch.wfb_payloads);
    current.rtp_packets = current.rtp_packets.saturating_add(batch.rtp_packets);
    current.video_frames = current.video_frames.saturating_add(batch.video_frames);
    current.raw_payload_count = current
        .raw_payload_count
        .saturating_add(batch.raw_payload_count);
    current.raw_payload_bytes = current
        .raw_payload_bytes
        .saturating_add(batch.raw_payload_bytes);
    current.route_errors = current.route_errors.saturating_add(batch.route_errors);
}

fn validate_key(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    openipc_core::WfbKeypair::from_bytes(&bytes)
        .map_err(|error| format!("Invalid WFB key: {error}"))?;
    Ok(bytes)
}

fn fec_window_percentages(total: u64, recovered: u64, lost: u64) -> (f64, f64) {
    let expected_fragments = total.saturating_add(lost);
    let unrecoverable_loss = if expected_fragments == 0 {
        0.0
    } else {
        lost as f64 * 100.0 / expected_fragments as f64
    };
    let damaged_primary = recovered.saturating_add(lost);
    let fec_recovery = if damaged_primary == 0 {
        0.0
    } else {
        recovered as f64 * 100.0 / damaged_primary as f64
    };
    (unrecoverable_loss, fec_recovery)
}

impl eframe::App for NebulusApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        crate::logging::set_level(self.settings.diagnostic_verbosity.log_level());
        // Egui may run more than one sizing pass for a frame. All external
        // state must be applied once so widget identity and geometry remain
        // stable throughout the remaining passes.
        if context.current_pass_index() == 0 {
            for record in crate::logging::drain() {
                self.log(
                    LogLevel::from_log(record.level),
                    record.target,
                    record.message,
                );
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            self.process_tray(context);
            self.process_events(context);
        }
        // Video frames and coalesced runtime events request repaints directly.
        // A fixed 60 Hz wakeup wastes CPU/GPU time and competes with decode on
        // mobile devices when no new frame is ready.
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::ui::show(self, ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }
}

#[cfg(test)]
mod tests {
    use super::{fec_window_percentages, validate_key};

    #[test]
    fn default_key_is_valid() {
        let key = crate::settings::DEFAULT_KEY_BYTES;
        assert!(validate_key(key.to_vec()).is_ok());
    }

    #[test]
    fn short_key_is_rejected() {
        assert!(validate_key(vec![0; 8]).is_err());
    }

    #[test]
    fn fec_window_distinguishes_recovery_from_final_loss() {
        assert_eq!(fec_window_percentages(100, 0, 0), (0.0, 0.0));
        assert_eq!(fec_window_percentages(100, 5, 0), (0.0, 100.0));
        let (loss, recovery) = fec_window_percentages(98, 3, 2);
        assert_eq!(loss, 2.0);
        assert_eq!(recovery, 60.0);
    }
}
