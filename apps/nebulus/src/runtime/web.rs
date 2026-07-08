use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use futures_channel::oneshot;
use futures_util::future::{select as select_future, Either as FutureEither};
use wasm_bindgen::{closure::Closure, JsCast as _, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};

use super::{RuntimeEvent, ScanRequest, StartRequest, UsbDeviceInfo};

#[derive(Default)]
struct RecordingControl {
    start: bool,
    stop: bool,
}

enum PermissionOutcome {
    Granted(web_sys::UsbDevice),
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerExit {
    Focused,
    TimedOut,
}

/// Owns the WebUSB chooser promise and a focus listener installed before the
/// chooser opens. Some Chromium builds can leave `requestDevice()` pending
/// when the chooser is dismissed immediately; focus returning to the page is
/// the fallback signal that selection ended.
struct WebUsbPermissionRequest {
    permission: Option<JsFuture<web_sys::UsbDevice>>,
    focus: Option<oneshot::Receiver<()>>,
    window: web_sys::Window,
    focus_listener: Closure<dyn FnMut()>,
}

impl WebUsbPermissionRequest {
    async fn resolve(mut self) -> Result<PermissionOutcome, String> {
        let permission = Box::pin(
            self.permission
                .take()
                .expect("WebUSB permission future is present"),
        );
        let picker_exit = Box::pin(wait_for_picker_exit(
            self.focus.take().expect("WebUSB focus receiver is present"),
        ));

        match select_future(permission, picker_exit).await {
            FutureEither::Left((result, _)) => permission_result(result),
            FutureEither::Right((PickerExit::Focused | PickerExit::TimedOut, permission)) => {
                // Chromium normally settles requestDevice before or directly
                // after restoring focus. Give that microtask a short grace
                // period before treating the focus return as dismissal.
                let grace = Box::pin(permission_delay(500));
                match select_future(permission, grace).await {
                    FutureEither::Left((result, _)) => permission_result(result),
                    FutureEither::Right(((), _)) => Ok(PermissionOutcome::Dismissed),
                }
            }
        }
    }
}

impl Drop for WebUsbPermissionRequest {
    fn drop(&mut self) {
        let _ = self.window.remove_event_listener_with_callback(
            "focus",
            self.focus_listener.as_ref().unchecked_ref(),
        );
    }
}

/// Browser receiver owner. WebUSB/WFB work stays on the app's local executor;
/// recovered RTP and WebCodecs run in a dedicated WASM worker.
pub(crate) struct Runtime {
    events: Rc<RefCell<VecDeque<RuntimeEvent>>>,
    cancel: Option<Rc<Cell<bool>>>,
    audio_volume: Rc<Cell<u8>>,
    recording: Rc<RefCell<RecordingControl>>,
    vtx_commands: Rc<RefCell<VecDeque<super::VtxControlRequest>>>,
    context: eframe::egui::Context,
}

impl Runtime {
    pub(crate) fn new(context: eframe::egui::Context) -> Self {
        Self {
            events: Rc::new(RefCell::new(VecDeque::new())),
            cancel: None,
            audio_volume: Rc::new(Cell::new(100)),
            recording: Rc::new(RefCell::new(RecordingControl::default())),
            vtx_commands: Rc::new(RefCell::new(VecDeque::new())),
            context,
        }
    }

    pub(crate) fn refresh_devices(&self) {
        let events = Rc::clone(&self.events);
        let context = self.context.clone();
        spawn_local(async move {
            let event = match discover_web_devices().await {
                Ok(devices) => RuntimeEvent::Devices(devices),
                Err(error) => RuntimeEvent::DiscoveryFailed(error),
            };
            emit(&events, &context, event);
        });
    }

    pub(crate) fn authorize_device(&self) {
        let permission = match request_device(None) {
            Ok(permission) => permission,
            Err(error) => {
                emit(
                    &self.events,
                    &self.context,
                    RuntimeEvent::DiscoveryFailed(js_error(error)),
                );
                return;
            }
        };
        let events = Rc::clone(&self.events);
        let context = self.context.clone();
        spawn_local(async move {
            let event = match permission.resolve().await {
                Ok(PermissionOutcome::Granted(_)) => discover_web_devices()
                    .await
                    .map(RuntimeEvent::Devices)
                    .unwrap_or_else(RuntimeEvent::DiscoveryFailed),
                Ok(PermissionOutcome::Dismissed) => return,
                Err(error) => RuntimeEvent::DiscoveryFailed(error),
            };
            emit(&events, &context, event);
        });
    }

    pub(crate) fn start(&mut self, request: StartRequest, context: eframe::egui::Context) {
        if let Some(cancel) = self.cancel.take() {
            cancel.set(true);
        }
        *self.recording.borrow_mut() = RecordingControl::default();

        if request.receiver_source == crate::settings::ReceiverSource::UdpRtp {
            self.events.borrow_mut().push_back(RuntimeEvent::Failed(
                "Direct UDP RTP input is unavailable in browsers; use WebUSB or the native app"
                    .to_owned(),
            ));
            context.request_repaint();
            return;
        }

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
        let permission = if request.device_ids.is_empty() {
            match request_device(request.primary_device_id.as_deref()) {
                Ok(promise) => Some(promise),
                Err(error) => {
                    self.events
                        .borrow_mut()
                        .push_back(RuntimeEvent::Failed(js_error(error)));
                    context.request_repaint();
                    return;
                }
            }
        } else {
            None
        };
        let cancel = Rc::new(Cell::new(false));
        self.cancel = Some(Rc::clone(&cancel));
        self.audio_volume.set(request.audio_volume.min(100));
        let audio_volume = Rc::clone(&self.audio_volume);
        let events = Rc::clone(&self.events);
        let recording = Rc::clone(&self.recording);
        let vtx_commands = Rc::clone(&self.vtx_commands);
        let completion_cancel = Rc::clone(&cancel);

        spawn_local(async move {
            let completion_events = Rc::clone(&events);
            let completion_context = context.clone();
            let handles = worker::WorkerHandles {
                cancel,
                audio_volume,
                recording,
                vtx_commands,
                events,
                context,
            };
            let result = worker::run(permission, request, route_processor, handles).await;
            if completion_cancel.get() {
                return;
            }
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

    pub(crate) fn start_scan(&mut self, request: ScanRequest, context: eframe::egui::Context) {
        if let Some(cancel) = self.cancel.take() {
            cancel.set(true);
        }
        let permission = match request_device(request.device_id.as_deref()) {
            Ok(permission) => permission,
            Err(error) => {
                emit(
                    &self.events,
                    &context,
                    RuntimeEvent::ScanFailed(js_error(error)),
                );
                return;
            }
        };
        let cancel = Rc::new(Cell::new(false));
        self.cancel = Some(Rc::clone(&cancel));
        let events = Rc::clone(&self.events);
        spawn_local(async move {
            if let Err(error) = worker::scan(permission, request, cancel, &events, &context).await {
                emit(&events, &context, RuntimeEvent::ScanFailed(error));
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
        let completion_cancel = Rc::clone(&cancel);
        let recording = Rc::clone(&self.recording);
        let vtx_commands = Rc::clone(&self.vtx_commands);
        emit(&events, &context, RuntimeEvent::Connecting);
        spawn_local(async move {
            let completion_events = Rc::clone(&events);
            let completion_context = context.clone();
            let handles = worker::WorkerHandles {
                cancel,
                audio_volume,
                recording,
                vtx_commands,
                events,
                context,
            };
            let result = worker::run_codec_mock(request, route_processor, handles).await;
            if completion_cancel.get() {
                return;
            }
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
        }
        emit(&self.events, &self.context, RuntimeEvent::Stopped);
        *self.recording.borrow_mut() = RecordingControl::default();
    }

    pub(crate) fn set_audio_volume(&self, volume: u8) {
        self.audio_volume.set(volume.min(100));
    }

    pub(crate) fn request_vtx(&self, request: super::VtxControlRequest) -> Result<(), String> {
        if self.cancel.is_none() {
            return Err("VTX controller is not running".to_owned());
        }
        self.vtx_commands.borrow_mut().push_back(request);
        Ok(())
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

    pub(crate) fn drain_into(&self, output: &mut Vec<RuntimeEvent>) {
        output.clear();
        output.extend(self.events.borrow_mut().drain(..));
    }
}

async fn discover_web_devices() -> Result<Vec<UsbDeviceInfo>, String> {
    let devices = nusb::list_devices()
        .await
        .map_err(|error| format!("WebUSB discovery failed: {error}"))?;
    Ok(devices
        .filter(|device| openipc_rtl88xx::is_supported_id(device.vendor_id(), device.product_id()))
        .enumerate()
        .map(|(index, device)| UsbDeviceInfo {
            id: web_device_info_id(&device, index),
            label: device
                .product_string()
                .or(device.manufacturer_string())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!("{:04x}:{:04x}", device.vendor_id(), device.product_id())
                }),
            vendor_id: device.vendor_id(),
            product_id: device.product_id(),
            location: device
                .serial_number()
                .map(|serial| format!("serial {serial}"))
                .unwrap_or_else(|| format!("WebUSB device {}", index + 1)),
        })
        .collect())
}

fn request_device(selected: Option<&str>) -> Result<WebUsbPermissionRequest, JsValue> {
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
    let window =
        web_sys::window().ok_or_else(|| JsValue::from_str("browser window is unavailable"))?;
    let (focus_sender, focus) = oneshot::channel();
    let focus_sender = Rc::new(RefCell::new(Some(focus_sender)));
    let focus_listener = Closure::wrap(Box::new(move || {
        if let Some(sender) = focus_sender.borrow_mut().take() {
            let _ = sender.send(());
        }
    }) as Box<dyn FnMut()>);
    window.add_event_listener_with_callback("focus", focus_listener.as_ref().unchecked_ref())?;
    let permission = JsFuture::from(window.navigator().usb().request_device(&options));
    Ok(WebUsbPermissionRequest {
        permission: Some(permission),
        focus: Some(focus),
        window,
        focus_listener,
    })
}

async fn wait_for_picker_exit(focus: oneshot::Receiver<()>) -> PickerExit {
    // The absolute bound prevents a browser bug from retaining a dead chooser
    // forever even when it does not restore a focus event.
    let focus = Box::pin(focus);
    let timeout = Box::pin(permission_delay(60_000));
    match select_future(focus, timeout).await {
        FutureEither::Left((_result, _)) => PickerExit::Focused,
        FutureEither::Right(((), _)) => PickerExit::TimedOut,
    }
}

async fn permission_delay(milliseconds: i32) {
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

fn permission_result(
    result: Result<web_sys::UsbDevice, JsValue>,
) -> Result<PermissionOutcome, String> {
    match result {
        Ok(device) => Ok(PermissionOutcome::Granted(device)),
        Err(error) if permission_was_dismissed(&error) => Ok(PermissionOutcome::Dismissed),
        Err(error) => Err(js_error(error)),
    }
}

fn permission_was_dismissed(error: &JsValue) -> bool {
    js_sys::Reflect::get(error, &JsValue::from_str("name"))
        .ok()
        .and_then(|name| name.as_string())
        .is_some_and(|name| super::is_webusb_dismissal_name(&name))
}

fn web_device_info_id(device: &nusb::DeviceInfo, index: usize) -> String {
    web_device_id(
        device.vendor_id(),
        device.product_id(),
        device.serial_number(),
        index,
    )
}

fn web_device_id(
    vendor_id: u16,
    product_id: u16,
    serial_number: Option<&str>,
    index: usize,
) -> String {
    serial_number.map_or_else(
        || format!("{vendor_id:04x}:{product_id:04x}@web-{index}"),
        |serial| format!("{vendor_id:04x}:{product_id:04x}@serial-{serial}"),
    )
}

fn parse_device_id(value: &str) -> Option<(u16, u16)> {
    let (vendor, product) = value.split('@').next()?.split_once(':')?;
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
    #[cfg(debug_assertions)]
    use std::time::Duration;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    use futures_channel::oneshot;
    use futures_util::{
        future::{select, select_all, Either, LocalBoxFuture},
        FutureExt as _,
    };
    use nusb::transfer::{Buffer, Bulk, Completion, In, Out, TransferError};
    use openipc_core::{
        realtek::{parse_rx_aggregate_with_kind, RxPacketType},
        AdaptiveLink, ChannelId, DiversityCombiner, DiversityDecision, DiversitySourceId,
        FecCounters, FrameLayout, PayloadRouteId, ReceiverRuntime, TxRadioParams, WfbKeypair,
        WfbTransmitter, WfbTxKeypair,
    };
    use openipc_rtl88xx::{
        build_usb_tx_frame, ChannelWidth, ChipFamily, DriverOptions, Jaguar3PowerTrackingState,
        RadioConfig, RealtekDevice, RealtekTxDescriptor, RealtekTxOptions,
    };
    use openipc_uplink::{NetworkConfig, UserspaceNetwork, VtxController};
    use wasm_bindgen::JsValue;
    use wasm_bindgen_futures::{spawn_local, JsFuture};
    use web_time::Instant;

    use crate::{
        model::LogLevel,
        runtime::{
            route_runtime::{configure_receiver, RouteProcessor},
            web_decode::{DecodeWorkerSnapshot, WebDecodeWorker},
            AdapterRuntimeMetrics, BatchMetrics, ChannelScanAccumulator, MetricsThrottle,
            RuntimeEvent, ScanRequest, StartRequest,
        },
    };

    const VIDEO_ROUTE: PayloadRouteId = PayloadRouteId::new(1);
    const RX_TRANSFERS_IN_FLIGHT: usize = 8;
    const TX_TRANSFERS_IN_FLIGHT: usize = 8;
    const FIRST_RX_WARNING_AFTER_MS: u128 = 2_000;
    const MAX_BROWSER_RECORDING_BYTES: usize = 512 * 1024 * 1024;

    struct WebRadio {
        source_id: u16,
        descriptor: openipc_core::RxDescriptorKind,
        endpoint_address: u8,
        endpoint: nusb::Endpoint<Bulk, In>,
        consecutive_errors: u8,
        metrics: Rc<RefCell<AdapterRuntimeMetrics>>,
    }

    type WebRadioCompletion = (WebRadio, Option<Completion>, f64);

    fn wait_for_radio(radio: WebRadio) -> LocalBoxFuture<'static, WebRadioCompletion> {
        async move {
            let mut radio = radio;
            let started = Instant::now();
            let completion = next_with_timeout(&mut radio.endpoint).await;
            let latency_ms = started.elapsed().as_secs_f64() * 1_000.0;
            (radio, completion, latency_ms)
        }
        .boxed_local()
    }

    pub(super) async fn scan(
        permission: super::WebUsbPermissionRequest,
        request: ScanRequest,
        cancel: Rc<Cell<bool>>,
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
    ) -> Result<(), String> {
        if request.channels.is_empty() {
            return Err("Select at least one channel to scan".to_owned());
        }
        let super::PermissionOutcome::Granted(web_device) = permission.resolve().await? else {
            return Ok(());
        };
        if cancel.get() {
            return Ok(());
        }
        let device = RealtekDevice::from_web_usb_device(web_device)
            .await
            .map_err(|error| error.to_string())?;
        let width = channel_width(request.channel_width_mhz)?;
        device
            .initialize_monitor_async(
                RadioConfig {
                    channel: request.channels[0],
                    channel_width: width,
                    channel_offset: request.channel_offset,
                },
                false,
            )
            .await
            .map_err(|error| error.to_string())?;
        let mut endpoint = device
            .open_bulk_in_endpoint()
            .map_err(|error| error.to_string())?;
        endpoint
            .clear_halt()
            .await
            .map_err(|error| format!("clear bulk-IN halt failed: {error}"))?;
        while endpoint.pending() < 2 {
            endpoint.submit(endpoint.allocate(request.transfer_size));
        }
        let descriptor = device.rx_descriptor_kind();
        super::emit(
            events,
            context,
            RuntimeEvent::ScanStarted {
                total: request.channels.len(),
            },
        );
        let scan_result = async {
            for (index, channel) in request.channels.iter().copied().enumerate() {
                if cancel.get() {
                    break;
                }
                let retune = if index > 0 {
                    let retune_started = Instant::now();
                    let report = device
                        .fast_retune_async(channel, true)
                        .await
                        .map_err(|error| format!("retune channel {channel} failed: {error}"))?;
                    let elapsed = retune_started.elapsed();
                    sleep_ms(5).await;
                    Some((elapsed, report.used_fast_path))
                } else {
                    None
                };
                let started = Instant::now();
                let mut observed = ChannelScanAccumulator::default();
                while started.elapsed() < request.dwell && !cancel.get() {
                    let Some(completion) = next_with_timeout_ms(&mut endpoint, 25).await else {
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
                            let _ = endpoint.clear_halt().await;
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
                super::emit(
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
        }
        .await;
        drop(endpoint);
        let shutdown = device
            .shutdown_monitor_async()
            .await
            .map_err(|error| format!("monitor shutdown failed after scan: {error}"));
        scan_result?;
        shutdown?;
        super::emit(events, context, RuntimeEvent::ScanCompleted);
        Ok(())
    }

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
        pub(super) vtx_commands: Rc<RefCell<VecDeque<crate::runtime::VtxControlRequest>>>,
        pub(super) events: Rc<RefCell<VecDeque<RuntimeEvent>>>,
        pub(super) context: eframe::egui::Context,
    }
    struct LinkRuntime {
        quality: AdaptiveLink,
        adaptive_enabled: bool,
        last_feedback_ms: Option<u64>,
        last_fec: FecCounters,
    }

    struct UplinkRuntime {
        network: Arc<Mutex<UserspaceNetwork>>,
        transmitter: WfbTransmitter,
        tx_options: RealtekTxOptions,
        tx_params: TxRadioParams,
        last_session_ms: Option<u64>,
    }

    impl UplinkRuntime {
        fn new(request: &StartRequest, chip: ChipFamily) -> Result<Self, String> {
            let keypair = WfbTxKeypair::from_bytes(&request.key_bytes)
                .map_err(|error| format!("uplink transmit key is invalid: {error}"))?;
            Ok(Self {
                network: Arc::new(Mutex::new(
                    UserspaceNetwork::new(NetworkConfig::default())
                        .map_err(|error| error.to_string())?,
                )),
                transmitter: WfbTransmitter::new(
                    ChannelId::from_link_port(
                        request.channel_id >> 8,
                        openipc_core::RadioPort::TunnelTx,
                    ),
                    keypair,
                    0,
                    1,
                    5,
                )
                .map_err(|error| error.to_string())?,
                tx_options: RealtekTxOptions {
                    current_channel: request.channel,
                    configured_channel_width: channel_width(request.channel_width_mhz)?,
                    configured_channel_offset: request.channel_offset,
                    descriptor: RealtekTxDescriptor::for_chip_family(chip),
                    ..RealtekTxOptions::default()
                },
                tx_params: TxRadioParams::openipc_uplink_default(),
                last_session_ms: None,
            })
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

        fn write_downlink(&mut self, payload: &[u8]) -> Result<(), String> {
            self.network
                .lock()
                .map_err(|_| "userspace network state poisoned".to_owned())?
                .ingest_tunnel_payload(payload)
                .map_err(|error| error.to_string())
        }

        fn tick(
            &mut self,
            now: u64,
            tx_queue: &mut WebTxQueue,
            adaptive: Option<Vec<u8>>,
        ) -> Result<(), String> {
            let mut payloads = Vec::new();
            {
                let mut network = self
                    .network
                    .lock()
                    .map_err(|_| "userspace network state poisoned".to_owned())?;
                network.poll(now);
                payloads.extend(network.drain_outbound());
            }
            if let Some(payload) = adaptive {
                payloads.push(payload);
            }
            if payloads.is_empty() {
                return Ok(());
            }
            if self
                .last_session_ms
                .is_none_or(|last| now.saturating_sub(last) >= 1_000)
            {
                if !tx_queue.enqueue(
                    self.transmitter.session_radio_packet(self.tx_params),
                    self.tx_options,
                )? {
                    return Err("WebUSB TX queue full before uplink session packet".to_owned());
                }
                self.last_session_ms = Some(now);
            }
            for payload in payloads {
                for frame in self
                    .transmitter
                    .radio_packets_for_payload(&payload, self.tx_params)
                    .map_err(|error| error.to_string())?
                {
                    if !tx_queue.enqueue(frame, self.tx_options)? {
                        return Err("WebUSB TX queue full; dropped uplink payload".to_owned());
                    }
                }
            }
            Ok(())
        }
    }

    type SharedVtxController = Rc<futures_util::lock::Mutex<Option<VtxController>>>;

    fn service_vtx_commands(
        commands: &Rc<RefCell<VecDeque<crate::runtime::VtxControlRequest>>>,
        controller: &SharedVtxController,
        network: &Arc<Mutex<UserspaceNetwork>>,
        credentials: &openipc_uplink::SshCredentials,
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
    ) {
        let pending = commands.borrow_mut().drain(..).collect::<Vec<_>>();
        for request in pending {
            let controller = Rc::clone(controller);
            let network = Arc::clone(network);
            let credentials = credentials.clone();
            let events = Rc::clone(events);
            let context = context.clone();
            spawn_local(async move {
                let mut controller = controller.lock().await;
                crate::runtime::uplink_control::process_request(
                    &mut controller,
                    &network,
                    &credentials,
                    request,
                    |event| super::emit(&events, &context, RuntimeEvent::VtxControl(event)),
                )
                .await;
            });
        }
    }

    struct WebTxQueue {
        device: Rc<RealtekDevice>,
        chip: ChipFamily,
        endpoint: nusb::Endpoint<Bulk, Out>,
        stalled: bool,
        last_error: Option<String>,
    }

    impl WebTxQueue {
        async fn new(device: Rc<RealtekDevice>, chip: ChipFamily) -> Result<Self, String> {
            let mut endpoint = device
                .open_bulk_out_endpoint()
                .map_err(|error| error.to_string())?;
            if RealtekTxDescriptor::for_chip_family(chip).uses_terminated_bulk_out() {
                endpoint
                    .clear_halt()
                    .await
                    .map_err(|error| format!("clear Jaguar1 bulk-OUT halt failed: {error}"))?;
            }
            Ok(Self {
                device,
                chip,
                endpoint,
                stalled: false,
                last_error: None,
            })
        }

        fn enqueue(&mut self, frame: Vec<u8>, options: RealtekTxOptions) -> Result<bool, String> {
            self.drain_ready();
            let options = self
                .device
                .apply_persistent_tx_options(self.chip, options)
                .map_err(|error| error.to_string())?;
            let usb_frame =
                build_usb_tx_frame(&frame, options).map_err(|error| error.to_string())?;
            let terminate = options.descriptor.uses_terminated_bulk_out()
                && !usb_frame.is_empty()
                && usb_frame
                    .len()
                    .is_multiple_of(self.endpoint.max_packet_size());
            let required = if terminate { 2 } else { 1 };
            if self.stalled || self.endpoint.pending() + required > TX_TRANSFERS_IN_FLIGHT {
                return Ok(false);
            }
            self.endpoint.submit(Buffer::from(usb_frame));
            if terminate {
                self.endpoint.submit(Buffer::new(0));
            }
            Ok(true)
        }

        fn drain_ready(&mut self) {
            while self.endpoint.pending() > 0 {
                let Some(completion) = self.endpoint.next_complete().now_or_never() else {
                    break;
                };
                if let Err(error) = completion.status {
                    self.stalled = true;
                    self.last_error = Some(format!("WebUSB bulk OUT failed: {error}"));
                    if self.stalled {
                        break;
                    }
                }
            }
        }

        async fn service(&mut self) -> Option<String> {
            self.drain_ready();
            if self.stalled {
                if let Err(error) = self.endpoint.clear_halt().await {
                    self.last_error = Some(format!("clear WebUSB bulk-OUT halt failed: {error}"));
                } else {
                    self.stalled = false;
                }
            }
            self.last_error.take()
        }
    }

    struct WebMaintenance {
        stop: Option<oneshot::Sender<()>>,
        done: oneshot::Receiver<()>,
    }

    impl WebMaintenance {
        fn start(device: Rc<RealtekDevice>, chip: ChipFamily) -> Option<Self> {
            if !chip.is_jaguar2() && !chip.is_jaguar3() {
                return None;
            }
            let (stop, stop_receiver) = oneshot::channel();
            let (done_sender, done) = oneshot::channel();
            spawn_local(async move {
                let mut stop_receiver = Box::pin(stop_receiver);
                let mut power_tracking = Jaguar3PowerTrackingState::default();
                loop {
                    let timer = Box::pin(sleep_ms(if chip.is_jaguar2() { 100 } else { 2_000 }));
                    match select(timer, stop_receiver).await {
                        Either::Left(((), remaining_stop)) => {
                            stop_receiver = remaining_stop;
                            if chip.is_jaguar2() && device.jaguar2_dig_enabled() {
                                let _ = device.run_jaguar2_dig_step_async().await;
                            } else if chip.is_jaguar3() {
                                let _ = device.run_jaguar3_coex_keepalive_async().await;
                                let _ = device
                                    .tick_jaguar3_power_tracking_async(&mut power_tracking)
                                    .await;
                            }
                        }
                        Either::Right((_stop, _timer)) => break,
                    }
                }
                let _ = done_sender.send(());
            });
            Some(Self {
                stop: Some(stop),
                done,
            })
        }

        async fn stop(mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            let _ = self.done.await;
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

    pub(super) async fn run(
        permission: Option<super::WebUsbPermissionRequest>,
        request: StartRequest,
        mut route_processor: RouteProcessor,
        handles: WorkerHandles,
    ) -> Result<(), String> {
        let cancel = &handles.cancel;
        let audio_volume = &handles.audio_volume;
        let recording_control = &handles.recording;
        let vtx_commands = &handles.vtx_commands;
        let events = &handles.events;
        let context = &handles.context;
        let recording_audio_config = route_processor.recording_audio_config();
        if let Some(permission) = permission {
            match permission.resolve().await? {
                super::PermissionOutcome::Granted(_) => {}
                super::PermissionOutcome::Dismissed => {
                    log(
                        events,
                        context,
                        LogLevel::Info,
                        "usb",
                        "WebUSB device selection dismissed",
                    );
                    return Ok(());
                }
            }
        }
        if cancel.get() {
            return Ok(());
        }
        super::emit(events, context, RuntimeEvent::Connecting);
        let mut discovered = nusb::list_devices()
            .await
            .map_err(|error| format!("WebUSB discovery failed: {error}"))?
            .filter(|info| openipc_rtl88xx::is_supported_id(info.vendor_id(), info.product_id()))
            .enumerate()
            .map(|(index, info)| {
                let id = super::web_device_info_id(&info, index);
                let label = info
                    .product_string()
                    .or(info.manufacturer_string())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!("{:04x}:{:04x}", info.vendor_id(), info.product_id())
                    });
                (id, label, info)
            })
            .collect::<Vec<_>>();
        if request.device_ids.is_empty() {
            discovered.truncate(1);
        } else {
            let mut selected = Vec::with_capacity(request.device_ids.len());
            for requested in &request.device_ids {
                if let Some(index) = discovered.iter().position(|(id, _, _)| {
                    id == requested || (!requested.contains('@') && id.starts_with(requested))
                }) {
                    selected.push(discovered.remove(index));
                }
            }
            discovered = selected;
        }
        if discovered.is_empty() {
            return Err(
                "No selected WebUSB adapter is authorized. Use Add adapter, then select it."
                    .to_owned(),
            );
        }

        let radio_config = RadioConfig {
            channel: request.channel,
            channel_width: channel_width(request.channel_width_mhz)?,
            channel_offset: request.channel_offset,
        };
        let mut devices = Vec::new();
        let mut radios = Vec::new();
        let mut receiver_infos = Vec::new();
        let mut metric_handles = Vec::new();
        let mut adapter_errors = Vec::new();
        for (id, label, info) in discovered {
            let opened = match info.open().await {
                Ok(opened) => opened,
                Err(error) => {
                    adapter_errors.push(format!("{id}: open failed: {error}"));
                    continue;
                }
            };
            let device =
                match RealtekDevice::from_nusb_device_async(opened, DriverOptions::default()).await
                {
                    Ok(device) => Rc::new(device),
                    Err(error) => {
                        adapter_errors.push(format!("{id}: claim failed: {error}"));
                        continue;
                    }
                };
            let report = match device.initialize_monitor_async(radio_config, false).await {
                Ok(report) => report,
                Err(error) => {
                    adapter_errors.push(format!("{id}: initialization failed: {error}"));
                    continue;
                }
            };
            log_rx_register_snapshot(events, context, &id, &device).await;
            let source_id = u16::try_from(devices.len())
                .map_err(|_| "too many diversity adapters selected".to_owned())?;
            let mut endpoint = device
                .open_bulk_in_endpoint()
                .map_err(|error| error.to_string())?;
            endpoint
                .clear_halt()
                .await
                .map_err(|error| format!("clear {id} bulk-IN halt failed: {error}"))?;
            while endpoint.pending() < RX_TRANSFERS_IN_FLIGHT {
                endpoint.submit(endpoint.allocate(request.transfer_size));
            }
            log(
                events,
                context,
                LogLevel::Info,
                "usb",
                format!(
                    "{id}: bulk-IN queue armed endpoint=0x{:02x} pending={} transfer_size={} max_packet_size={}",
                    device.bulk_in_endpoint_address(),
                    endpoint.pending(),
                    request.transfer_size,
                    endpoint.max_packet_size(),
                ),
            );
            let metrics = Rc::new(RefCell::new(AdapterRuntimeMetrics {
                source_id,
                device_id: id.clone(),
                label: label.clone(),
                online: true,
                ..AdapterRuntimeMetrics::default()
            }));
            receiver_infos.push(crate::runtime::ReceiverInfo::initialized(
                id, source_id, label, &device, &report,
            ));
            radios.push(WebRadio {
                source_id,
                descriptor: device.rx_descriptor_kind(),
                endpoint_address: device.bulk_in_endpoint_address(),
                endpoint,
                consecutive_errors: 0,
                metrics: Rc::clone(&metrics),
            });
            metric_handles.push(metrics);
            devices.push((device, report.chip.family));
        }
        if devices.is_empty() {
            return Err(format!(
                "No WebUSB adapter could be initialized: {}",
                adapter_errors.join("; ")
            ));
        }
        for error in adapter_errors {
            log(events, context, LogLevel::Warn, "diversity", error);
        }
        let device = Rc::clone(&devices[0].0);
        let chip = devices[0].1;
        let decoder = WebDecodeWorker::new(
            request.rtp_reorder,
            request.codec_preference,
            Rc::clone(events),
            context.clone(),
        )?;
        decoder.wait_until_ready().await?;
        super::emit(
            events,
            context,
            RuntimeEvent::Connected {
                receivers: receiver_infos,
                decoder: worker_decoder_environment(),
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
        let mut options = configure_receiver(&mut receiver, &request)?;
        options.depacketize_video = false;
        if !options.raw_payload_routes.contains(&VIDEO_ROUTE) {
            options.raw_payload_routes.push(VIDEO_ROUTE);
        }
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
        let mut uplink = (request.vtx_control_enabled || request.adaptive_link)
            .then(|| UplinkRuntime::new(&request, chip))
            .transpose()?;
        let vtx_controller = Rc::new(futures_util::lock::Mutex::new(None));
        let mut tx_queue = if uplink.is_some() {
            Some(WebTxQueue::new(Rc::clone(&device), chip).await?)
        } else {
            None
        };
        let mut maintenance = devices
            .iter()
            .filter_map(|(device, chip)| WebMaintenance::start(Rc::clone(device), *chip))
            .collect::<Vec<_>>();
        let mut radio_futures = radios.into_iter().map(wait_for_radio).collect::<Vec<_>>();
        super::emit(events, context, RuntimeEvent::Started);
        log(
            events,
            context,
            LogLevel::Info,
            "rx",
            format!("WebUSB receiver started with {} adapter(s)", devices.len()),
        );

        let mut last_decode_errors = 0;
        let mut last_worker_snapshot = DecodeWorkerSnapshot::default();
        let mut recorder: Option<BrowserRecorder> = None;
        let mut recording_armed = false;
        let mut metrics_throttle = MetricsThrottle::new();
        let mut diversity = DiversityCombiner::default();
        let diversity_enabled = devices.len() > 1;
        let mut source_quality = (0..devices.len())
            .map(|_| AdaptiveLink::new())
            .collect::<Vec<_>>();
        let receive_started = Instant::now();
        let mut first_rx_warning_emitted = false;
        while !cancel.get() {
            if radio_futures.is_empty() {
                return Err("all WebUSB receive adapters disconnected".to_owned());
            }
            let ((mut radio, completion, usb_latency_ms), _, remaining) =
                select_all(radio_futures).await;
            radio_futures = remaining;
            let Some(completion) = completion else {
                if !first_rx_warning_emitted
                    && receive_started.elapsed().as_millis() >= FIRST_RX_WARNING_AFTER_MS
                    && metric_handles
                        .iter()
                        .all(|metrics| metrics.borrow().transfers == 0)
                {
                    first_rx_warning_emitted = true;
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "usb",
                        format!(
                            "no bulk-IN completion after {} ms; radio {} still has {} WebUSB reads pending on endpoint 0x{:02x}. No data has reached 802.11, WFB, RTP, or the decoder; verify the VTX channel/width and power-cycle the adapter if another receiver used it first",
                            receive_started.elapsed().as_millis(),
                            radio.source_id + 1,
                            radio.endpoint.pending(),
                            radio.endpoint_address,
                        ),
                    );
                    emit_diversity_update(
                        events,
                        context,
                        &diversity,
                        &metric_handles,
                        &mut source_quality,
                    );
                }
                radio_futures.push(wait_for_radio(radio));
                if let Some(metrics) = metrics_throttle.flush() {
                    emit_metrics(
                        events,
                        context,
                        metrics,
                        &diversity,
                        &metric_handles,
                        &mut source_quality,
                    );
                }
                update_recording(
                    &[],
                    recording_control,
                    &mut recording_armed,
                    &mut recorder,
                    recording_audio_config,
                    events,
                    context,
                );
                let now = now_ms();
                if request.vtx_control_enabled {
                    service_vtx_commands(
                        vtx_commands,
                        &vtx_controller,
                        &uplink
                            .as_ref()
                            .expect("VTX commands require an uplink runtime")
                            .network(),
                        &request.vtx_credentials,
                        events,
                        context,
                    );
                }
                let adaptive = link.feedback_due(now);
                if let (Some(uplink), Some(tx_queue)) = (uplink.as_mut(), tx_queue.as_mut()) {
                    if let Err(error) = uplink.tick(now, tx_queue, adaptive) {
                        log(events, context, LogLevel::Warn, "uplink", error);
                    }
                }
                if let Some(tx_queue) = tx_queue.as_mut() {
                    if let Some(error) = tx_queue.service().await {
                        log(events, context, LogLevel::Warn, "adaptive", error);
                    }
                }
                continue;
            };
            let actual_len = completion.actual_len;
            if let Err(error) = completion.status {
                radio.metrics.borrow_mut().usb_errors += 1;
                if error == TransferError::Disconnected {
                    radio.metrics.borrow_mut().online = false;
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "diversity",
                        format!(
                            "Radio {} disconnected; {}/{} WebUSB adapters remain",
                            radio.source_id + 1,
                            radio_futures.len(),
                            devices.len()
                        ),
                    );
                    emit_diversity_update(
                        events,
                        context,
                        &diversity,
                        &metric_handles,
                        &mut source_quality,
                    );
                    continue;
                }
                log(
                    events,
                    context,
                    LogLevel::Warn,
                    "usb",
                    format!("radio {} bulk IN failed: {error}", radio.source_id + 1),
                );
                if error == TransferError::Stall {
                    let _ = radio.endpoint.clear_halt().await;
                    radio.consecutive_errors = 0;
                } else {
                    radio.consecutive_errors = radio.consecutive_errors.saturating_add(1);
                }
                radio.endpoint.submit(completion.buffer);
                if radio.consecutive_errors < 8 {
                    radio_futures.push(wait_for_radio(radio));
                } else {
                    radio.metrics.borrow_mut().online = false;
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "diversity",
                        format!(
                            "Radio {} stopped after repeated USB errors",
                            radio.source_id + 1
                        ),
                    );
                    emit_diversity_update(
                        events,
                        context,
                        &diversity,
                        &metric_handles,
                        &mut source_quality,
                    );
                }
                continue;
            }
            radio.consecutive_errors = 0;
            {
                let mut metrics = radio.metrics.borrow_mut();
                metrics.transfers = metrics.transfers.saturating_add(1);
                metrics.transfer_bytes = metrics.transfer_bytes.saturating_add(actual_len as u64);
            }

            let batch_start = Instant::now();
            let parse_start = Instant::now();
            let packets = match parse_rx_aggregate_with_kind(
                &completion.buffer[..actual_len],
                radio.descriptor,
            ) {
                Ok(packets) => packets,
                Err(error) => {
                    log(
                        events,
                        context,
                        LogLevel::Warn,
                        "usb",
                        format!(
                            "radio {} RX aggregate rejected: {error}",
                            radio.source_id + 1
                        ),
                    );
                    radio.endpoint.submit(completion.buffer);
                    radio_futures.push(wait_for_radio(radio));
                    continue;
                }
            };
            let parse_latency_ms = parse_start.elapsed().as_secs_f64() * 1_000.0;
            let now = now_ms();
            let source_index = usize::from(radio.source_id);
            let source = DiversitySourceId::new(radio.source_id);
            let selected_packets = packets.into_iter().filter_map(|packet| {
                if packet.attrib.crc_err
                    || packet.attrib.icv_err
                    || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
                {
                    return Some((packet, None));
                }
                let frame = openipc_core::WifiFrame::parse(packet.data, FrameLayout::WithFcs).ok();
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
            options.depacketize_video =
                recorder.is_some() || recording_armed || recording_control.borrow().start;
            let mut batch = receiver.push_parsed_rx_packets(selected_packets, &options);
            let pipeline_latency_ms = pipeline_start.elapsed().as_secs_f64() * 1_000.0;

            // Re-arm WebUSB as soon as parsing and WFB recovery no longer
            // borrow this transfer. Browser rendering and route work must not
            // create a gap in the USB receive queue.
            radio.endpoint.submit(completion.buffer);
            radio_futures.push(wait_for_radio(radio));

            update_recording(
                &batch.frames,
                recording_control,
                &mut recording_armed,
                &mut recorder,
                recording_audio_config,
                events,
                context,
            );
            let decode_submit_start = Instant::now();
            decoder.submit_rtp_batch(
                batch
                    .raw_payloads
                    .iter()
                    .filter(|payload| payload.route_id == VIDEO_ROUTE)
                    .map(|payload| payload.data.as_slice()),
            )?;
            batch.frames.clear();
            let decode_submit_latency_ms = decode_submit_start.elapsed().as_secs_f64() * 1_000.0;
            let video_submit_path_ms = batch_start.elapsed().as_secs_f64() * 1_000.0;

            if let Some(uplink) = uplink.as_mut() {
                for payload in &batch.raw_payloads {
                    if payload.route_id == crate::runtime::route_runtime::VPN_ROUTE_ID {
                        if let Err(error) = uplink.write_downlink(&payload.data) {
                            log(events, context, LogLevel::Warn, "uplink", error);
                        }
                    }
                }
            }

            route_processor.set_audio_volume(audio_volume.get());
            let route_start = Instant::now();
            let (route_updates, route_logs, recorded_audio, telemetry) =
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
            let worker_snapshot = decoder.snapshot();
            let video_frames = worker_snapshot
                .rtp
                .frames_emitted
                .saturating_sub(last_worker_snapshot.rtp.frames_emitted);
            let video_bytes = worker_snapshot
                .encoded_bytes
                .saturating_sub(last_worker_snapshot.encoded_bytes);
            let decoder_frames = worker_snapshot
                .decoder
                .frames_decoded
                .saturating_sub(last_worker_snapshot.decoder.frames_decoded);
            last_worker_snapshot = worker_snapshot;
            batch.counters.video_frames = video_frames.min(usize::MAX as u64) as usize;
            let stats = worker_snapshot.decoder;
            if let Some(metrics) = metrics_throttle.push(BatchMetrics {
                transfers: 1,
                transfer_bytes: actual_len,
                packets: batch.counters.packets,
                rtp_packets: batch.counters.rtp_packets,
                video_frames: batch.counters.video_frames,
                decoder_frames,
                video_bytes: video_bytes.min(usize::MAX as u64) as usize,
                usb_latency_ms,
                parse_latency_ms,
                pipeline_latency_ms,
                route_latency_ms,
                decode_submit_latency_ms,
                video_submit_path_ms,
                batch_latency_ms: batch_start.elapsed().as_secs_f64() * 1_000.0,
                rssi: quality.rssi,
                snr: quality.snr,
                link_score: quality.link_score,
                decoder_drops: stats
                    .waiting_drops
                    .saturating_add(stats.backpressure_drops)
                    .saturating_add(stats.output_drops)
                    .saturating_add(worker_snapshot.access_unit_queue_drops())
                    .saturating_add(worker_snapshot.transport_dropped_batches),
                decoder_errors: stats.decode_errors,
                fec: batch.fec_counters,
                counters: batch.counters,
                rtp: worker_snapshot.rtp,
                reorder: worker_snapshot.reorder,
                uplink: uplink.as_ref().map_or_else(
                    openipc_uplink::NetworkMetrics::default,
                    UplinkRuntime::network_metrics,
                ),
                routes: route_updates,
                telemetry,
                audio: route_processor.audio_stats(),
                ..BatchMetrics::default()
            }) {
                emit_metrics(
                    events,
                    context,
                    metrics,
                    &diversity,
                    &metric_handles,
                    &mut source_quality,
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
            if request.vtx_control_enabled {
                service_vtx_commands(
                    vtx_commands,
                    &vtx_controller,
                    &uplink
                        .as_ref()
                        .expect("VTX commands require an uplink runtime")
                        .network(),
                    &request.vtx_credentials,
                    events,
                    context,
                );
            }
            let adaptive = link.feedback_due(now);
            if let (Some(uplink), Some(tx_queue)) = (uplink.as_mut(), tx_queue.as_mut()) {
                if let Err(error) = uplink.tick(now, tx_queue, adaptive) {
                    log(events, context, LogLevel::Warn, "uplink", error);
                }
            }
            if let Some(tx_queue) = tx_queue.as_mut() {
                if let Some(error) = tx_queue.service().await {
                    log(events, context, LogLevel::Warn, "adaptive", error);
                }
            }
        }

        if let Some(metrics) = metrics_throttle.flush() {
            emit_metrics(
                events,
                context,
                metrics,
                &diversity,
                &metric_handles,
                &mut source_quality,
            );
        }
        drop(radio_futures);
        drop(tx_queue);
        for task in maintenance.drain(..) {
            task.stop().await;
        }
        finish_recording(&mut recorder, events, context);
        drop(decoder);
        let mut shutdown_errors = Vec::new();
        for (device, _) in devices {
            if let Err(error) = device.shutdown_monitor_async().await {
                shutdown_errors.push(error.to_string());
            }
        }
        if !shutdown_errors.is_empty() {
            return Err(format!(
                "monitor shutdown failed: {}",
                shutdown_errors.join("; ")
            ));
        }
        Ok(())
    }

    fn emit_diversity_update(
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
        diversity: &DiversityCombiner,
        metric_handles: &[Rc<RefCell<AdapterRuntimeMetrics>>],
        source_quality: &mut [AdaptiveLink],
    ) {
        let (stats, adapters) = diversity_snapshot(diversity, metric_handles, source_quality);
        super::emit(
            events,
            context,
            RuntimeEvent::DiversityUpdate { stats, adapters },
        );
    }

    fn emit_metrics(
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
        mut metrics: BatchMetrics,
        diversity: &DiversityCombiner,
        metric_handles: &[Rc<RefCell<AdapterRuntimeMetrics>>],
        source_quality: &mut [AdaptiveLink],
    ) {
        (metrics.diversity, metrics.adapters) =
            diversity_snapshot(diversity, metric_handles, source_quality);
        super::emit(events, context, RuntimeEvent::Batch(Box::new(metrics)));
    }

    fn diversity_snapshot(
        diversity: &DiversityCombiner,
        metric_handles: &[Rc<RefCell<AdapterRuntimeMetrics>>],
        source_quality: &mut [AdaptiveLink],
    ) -> (openipc_core::DiversityStats, Vec<AdapterRuntimeMetrics>) {
        let now = now_ms();
        let stats = diversity.stats();
        let mut adapters = metric_handles
            .iter()
            .map(|metrics| metrics.borrow().clone())
            .collect::<Vec<_>>();
        for (snapshot, quality_tracker) in adapters.iter_mut().zip(source_quality) {
            if let Some(source) = stats
                .sources
                .get(&DiversitySourceId::new(snapshot.source_id))
            {
                snapshot.accepted = source.accepted;
                snapshot.duplicates = source.duplicates;
            }
            let quality = quality_tracker.quality(now);
            snapshot.rssi[0] = quality.rssi[0];
            snapshot.rssi[1] = quality.rssi[1];
            snapshot.snr[0] = quality.snr[0];
            snapshot.snr[1] = quality.snr[1];
        }
        (stats, adapters)
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
        let mock_codec = request.codec_preference.mock_codec();
        let mock_codec_label = match mock_codec {
            openipc_core::Codec::H264 => "H.264",
            openipc_core::Codec::H265 => "H.265",
        };

        let decoder = WebDecodeWorker::new(
            request.rtp_reorder,
            request.codec_preference,
            Rc::clone(events),
            context.clone(),
        )?;
        decoder.wait_until_ready().await?;
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
                receivers: vec![crate::runtime::ReceiverInfo::codec_mock(mock_codec)],
                decoder: worker_decoder_environment(),
            },
        );
        super::emit(events, context, RuntimeEvent::Started);
        log(
            events,
            context,
            LogLevel::Info,
            "mock",
            format!("Pre-recorded 1080p {mock_codec_label} + Opus RTP/WebCodecs mock started"),
        );

        let channel = ChannelId::default_video();
        let mut receiver =
            ReceiverRuntime::with_mock_video_route(FrameLayout::WithFcs, VIDEO_ROUTE, channel, 0);
        receiver.set_rtp_reorder_enabled(request.rtp_reorder);
        let mut options = configure_mock_receiver(&mut receiver, &request);
        options.depacketize_video = false;
        if !options.raw_payload_routes.contains(&VIDEO_ROUTE) {
            options.raw_payload_routes.push(VIDEO_ROUTE);
        }
        let runtime = receiver.video_runtime();
        let mut source = MockAvStream::new(mock_codec)?;
        let mock_started = Instant::now();
        let mut payload_sequence = 1u64;
        let mut recorder: Option<BrowserRecorder> = None;
        let mut recording_armed = false;
        let mut last_worker_snapshot = DecodeWorkerSnapshot::default();
        let mut metrics_throttle = MetricsThrottle::new();

        while !cancel.get() {
            source.rebase_timing_if_late(
                mock_started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                50_000,
            );
            let loop_started = Instant::now();
            let mut metrics = BatchMetrics::default();
            let mut next_due_micros;
            let mut catch_up_events = 0usize;
            loop {
                let event = source.next_event();
                next_due_micros = event.next_due_micros;
                metrics.transfers = metrics.transfers.saturating_add(1);
                metrics.transfer_bytes = metrics
                    .transfer_bytes
                    .saturating_add(event.packets.iter().map(Vec::len).sum::<usize>());
                metrics.packets = metrics.packets.saturating_add(event.packets.len());
                metrics.rtp_packets = metrics.rtp_packets.saturating_add(event.packets.len());
                let mut event_video_packets = Vec::new();
                for packet in event.packets {
                    options.depacketize_video =
                        recorder.is_some() || recording_armed || recording_control.borrow().start;
                    let mut batch = receiver
                        .push_mock_payload(runtime, payload_sequence, &packet, &options)
                        .map_err(|error| format!("mock payload route failed: {error}"))?;
                    payload_sequence = payload_sequence.wrapping_add(1);
                    update_recording(
                        &batch.frames,
                        recording_control,
                        &mut recording_armed,
                        &mut recorder,
                        recording_audio_config,
                        events,
                        context,
                    );
                    batch.frames.clear();
                    route_processor.set_audio_volume(audio_volume.get());
                    let (route_updates, route_logs, recorded_audio, telemetry) =
                        route_processor.process(&batch.raw_payloads, recorder.is_some());
                    append_recorded_audio(&mut recorder, recorded_audio, events, context);
                    metrics.merge(BatchMetrics {
                        routes: route_updates,
                        counters: batch.counters,
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
                    event_video_packets.extend(
                        batch
                            .raw_payloads
                            .into_iter()
                            .filter(|payload| payload.route_id == VIDEO_ROUTE)
                            .map(|payload| payload.data),
                    );
                }
                decoder.submit_rtp_batch(event_video_packets.iter().map(Vec::as_slice))?;
                catch_up_events += 1;
                if next_due_micros > mock_started.elapsed().as_micros() as u64
                    || catch_up_events >= 16
                    || cancel.get()
                {
                    break;
                }
            }

            let worker_snapshot = decoder.snapshot();
            let video_frames = worker_snapshot
                .rtp
                .frames_emitted
                .saturating_sub(last_worker_snapshot.rtp.frames_emitted);
            let decoder_frames = worker_snapshot
                .decoder
                .frames_decoded
                .saturating_sub(last_worker_snapshot.decoder.frames_decoded);
            let video_bytes = worker_snapshot
                .encoded_bytes
                .saturating_sub(last_worker_snapshot.encoded_bytes);
            last_worker_snapshot = worker_snapshot;
            metrics.video_frames = video_frames.min(usize::MAX as u64) as usize;
            metrics.decoder_frames = decoder_frames;
            metrics.video_bytes = video_bytes.min(usize::MAX as u64) as usize;
            metrics.counters.video_frames = metrics.video_frames;
            metrics.rtp = worker_snapshot.rtp;
            metrics.reorder = worker_snapshot.reorder;
            let stats = worker_snapshot.decoder;
            metrics.pipeline_latency_ms = loop_started.elapsed().as_secs_f64() * 1_000.0;
            metrics.decode_submit_latency_ms = metrics.pipeline_latency_ms;
            metrics.video_submit_path_ms = metrics.pipeline_latency_ms;
            metrics.batch_latency_ms = metrics.pipeline_latency_ms;
            metrics.decoder_drops = stats
                .waiting_drops
                .saturating_add(stats.backpressure_drops)
                .saturating_add(stats.output_drops)
                .saturating_add(worker_snapshot.access_unit_queue_drops())
                .saturating_add(worker_snapshot.transport_dropped_batches);
            metrics.decoder_errors = stats.decode_errors;
            metrics.audio = route_processor.audio_stats();
            if let Some(metrics) = metrics_throttle.push(metrics) {
                super::emit(events, context, RuntimeEvent::Batch(Box::new(metrics)));
            }
            let remaining_ms = Duration::from_micros(next_due_micros)
                .checked_sub(mock_started.elapsed())
                .map_or(0, |remaining| {
                    remaining.as_millis().min(i32::MAX as u128) as i32
                });
            sleep_ms(remaining_ms).await;
        }
        if let Some(metrics) = metrics_throttle.flush() {
            super::emit(events, context, RuntimeEvent::Batch(Box::new(metrics)));
        }
        drop(decoder);
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

    fn worker_decoder_environment() -> crate::runtime::DecoderEnvironment {
        let mut environment = decoder_environment(openipc_video::WebDecoder::probe_capabilities());
        environment.backend = "webcodecs-worker".to_owned();
        environment
    }

    async fn build_link(
        request: &StartRequest,
        _chip: ChipFamily,
        fec: FecCounters,
        device: &RealtekDevice,
    ) -> Result<LinkRuntime, String> {
        if request.adaptive_link {
            device
                .set_tx_power_override_async(request.channel, request.tx_power)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(LinkRuntime {
            quality: AdaptiveLink::new(),
            adaptive_enabled: request.adaptive_link,
            last_feedback_ms: None,
            last_fec: fec,
        })
    }

    async fn next_with_timeout(endpoint: &mut nusb::Endpoint<Bulk, In>) -> Option<Completion> {
        next_with_timeout_ms(endpoint, 10).await
    }

    async fn log_rx_register_snapshot(
        events: &Rc<RefCell<VecDeque<RuntimeEvent>>>,
        context: &eframe::egui::Context,
        device_id: &str,
        device: &RealtekDevice,
    ) {
        // These are the minimum registers needed to distinguish a bad chip
        // dispatch or disabled RX DMA from a healthy radio on a quiet channel.
        let chip_id = device.read_u8_async(0x00fc).await;
        let firmware = device.read_u32_async(0x0080).await;
        let command = device.read_u16_async(0x0100).await;
        let receive_config = device.read_u32_async(0x0608).await;
        let receive_filter = device.read_u16_async(0x06a4).await;
        let receive_dma = device.read_u32_async(0x0288).await;
        log(
            events,
            context,
            LogLevel::Info,
            "usb",
            format!(
                "{device_id}: post-init RX registers SYS_CFG2={} MCUFWDL={} CR={} RCR={} RXFLTMAP2={} RXDMA_STATUS={}",
                register_u8(chip_id),
                register_u32(firmware),
                register_u16(command),
                register_u32(receive_config),
                register_u16(receive_filter),
                register_u32(receive_dma),
            ),
        );
    }

    fn register_u8(value: Result<u8, openipc_rtl88xx::DriverError>) -> String {
        value.map_or_else(
            |error| format!("error({error})"),
            |value| format!("0x{value:02x}"),
        )
    }

    fn register_u16(value: Result<u16, openipc_rtl88xx::DriverError>) -> String {
        value.map_or_else(
            |error| format!("error({error})"),
            |value| format!("0x{value:04x}"),
        )
    }

    fn register_u32(value: Result<u32, openipc_rtl88xx::DriverError>) -> String {
        value.map_or_else(
            |error| format!("error({error})"),
            |value| format!("0x{value:08x}"),
        )
    }

    async fn next_with_timeout_ms(
        endpoint: &mut nusb::Endpoint<Bulk, In>,
        milliseconds: i32,
    ) -> Option<Completion> {
        let completion = Box::pin(endpoint.next_complete());
        let timeout = Box::pin(sleep_ms(milliseconds));
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
