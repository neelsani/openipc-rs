use std::{cell::RefCell, rc::Rc};

use futures_channel::oneshot;
use js_sys::{Array, Object, Reflect, Uint8Array};
use openipc_core::{Codec, CodecConfigState, RtpDepacketizerStatus, RtpReorderStatus};
use openipc_video::{DecodedFrame, DecoderStats, VideoTimestamp, WebVideoFrame};
use wasm_bindgen::{closure::Closure, JsCast as _, JsValue};

use super::{queue_event, RuntimeEvent};
use crate::{model::LogLevel, settings::CodecPreference};

// Keep transport buffering below one typical frame interval. Worker overload
// must drop and resynchronize instead of displaying an old queue.
const MAX_RTP_BATCHES_IN_FLIGHT: usize = 3;

/// Latest cumulative state reported by the RTP and WebCodecs workers.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DecodeWorkerSnapshot {
    pub(super) decoder: DecoderStats,
    pub(super) rtp: RtpDepacketizerStatus,
    pub(super) reorder: RtpReorderStatus,
    pub(super) encoded_bytes: u64,
    pub(super) rtp_queue_drops: u64,
    pub(super) decode_queue_drops: u64,
    pub(super) transport_dropped_batches: u64,
}

impl DecodeWorkerSnapshot {
    pub(super) fn access_unit_queue_drops(self) -> u64 {
        self.rtp_queue_drops.saturating_add(self.decode_queue_drops)
    }
}

#[derive(Default)]
struct RtpTransferState {
    in_flight: usize,
    submitted_batches: u64,
    acknowledged_batches: u64,
    dropped_batches: u64,
    dropped_packets: u64,
}

struct StartupState {
    rtp_ready: bool,
    decoder_ready: bool,
    configured: bool,
    rtp_port: Option<web_sys::MessagePort>,
    decoder_port: Option<web_sys::MessagePort>,
    sender: Option<oneshot::Sender<Result<(), String>>>,
}

/// Owns the isolated browser RTP and WebCodecs workers.
pub(super) struct WebDecodeWorker {
    rtp_worker: web_sys::Worker,
    decoder_worker: web_sys::Worker,
    snapshot: Rc<RefCell<DecodeWorkerSnapshot>>,
    rtp_transfer: Rc<RefCell<RtpTransferState>>,
    rtp_payload: RefCell<Vec<u8>>,
    ready: RefCell<Option<oneshot::Receiver<Result<(), String>>>>,
    _rtp_onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _decoder_onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _rtp_onerror: Closure<dyn FnMut(web_sys::ErrorEvent)>,
    _decoder_onerror: Closure<dyn FnMut(web_sys::ErrorEvent)>,
}

impl WebDecodeWorker {
    pub(super) fn new(
        reorder: bool,
        codec: CodecPreference,
        events: Rc<RefCell<std::collections::VecDeque<RuntimeEvent>>>,
        context: eframe::egui::Context,
    ) -> Result<Self, String> {
        let build = crate::build_info::current();
        let cache_key = build.commit.unwrap_or(build.version);
        let worker_url = format!("./nebulus-decode-worker_loader.js?v={cache_key}");
        let rtp_worker = spawn_worker(&worker_url, "nebulus-rtp")?;
        let decoder_worker = spawn_worker(&worker_url, "nebulus-video-decode")?;
        let channel = web_sys::MessageChannel::new()
            .map_err(|error| format!("create RTP/decode MessageChannel: {error:?}"))?;
        let snapshot = Rc::new(RefCell::new(DecodeWorkerSnapshot::default()));
        let rtp_transfer = Rc::new(RefCell::new(RtpTransferState::default()));
        let (ready_sender, ready_receiver) = oneshot::channel();
        let startup = Rc::new(RefCell::new(StartupState {
            rtp_ready: false,
            decoder_ready: false,
            configured: false,
            rtp_port: Some(channel.port1()),
            decoder_port: Some(channel.port2()),
            sender: Some(ready_sender),
        }));
        let accept_h264 = matches!(codec, CodecPreference::Auto | CodecPreference::H264);
        let accept_h265 = matches!(codec, CodecPreference::Auto | CodecPreference::H265);

        let rtp_snapshot = Rc::clone(&snapshot);
        let rtp_transfer_events = Rc::clone(&rtp_transfer);
        let rtp_startup = Rc::clone(&startup);
        let rtp_events = Rc::clone(&events);
        let rtp_context = context.clone();
        let rtp_config_worker = rtp_worker.clone();
        let rtp_config_decoder = decoder_worker.clone();
        let rtp_onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
            move |event: web_sys::MessageEvent| {
                let message = event.data();
                let kind = string_field(&message, "kind").unwrap_or_default();
                match kind.as_str() {
                    "ready" => {
                        if let Some(result) = mark_ready_and_configure(
                            WorkerKind::Rtp,
                            &rtp_startup,
                            &rtp_config_worker,
                            &rtp_config_decoder,
                            reorder,
                            accept_h264,
                            accept_h265,
                        ) {
                            emit_startup_result(&rtp_events, &rtp_context, result);
                        }
                    }
                    "rtp-stats" | "stopped" => {
                        merge_rtp_snapshot(&mut rtp_snapshot.borrow_mut(), &message);
                    }
                    "rtp-ack" => {
                        let first_acknowledgement = {
                            let mut transfer = rtp_transfer_events.borrow_mut();
                            transfer.in_flight = transfer.in_flight.saturating_sub(1);
                            transfer.acknowledged_batches =
                                transfer.acknowledged_batches.saturating_add(1);
                            transfer.acknowledged_batches == 1
                        };
                        if first_acknowledgement {
                            emit(
                                &rtp_events,
                                &rtp_context,
                                RuntimeEvent::Log {
                                    level: LogLevel::Info,
                                    target: "decoder",
                                    message: "RTP worker acknowledged its first batch".to_owned(),
                                },
                            );
                        }
                    }
                    "error" => emit_worker_error(&rtp_events, &rtp_context, &message),
                    _ => emit_unknown(&rtp_events, &rtp_context, "RTP", &kind),
                }
            },
        );
        rtp_worker.set_onmessage(Some(rtp_onmessage.as_ref().unchecked_ref()));

        let decoder_snapshot = Rc::clone(&snapshot);
        let decoder_startup = Rc::clone(&startup);
        let decoder_events = Rc::clone(&events);
        let decoder_context = context.clone();
        let decoder_config_rtp = rtp_worker.clone();
        let decoder_config_worker = decoder_worker.clone();
        let frame_ack_worker = decoder_worker.clone();
        let decoder_onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
            move |event: web_sys::MessageEvent| {
                let message = event.data();
                let kind = string_field(&message, "kind").unwrap_or_default();
                match kind.as_str() {
                    "ready" => {
                        if let Some(result) = mark_ready_and_configure(
                            WorkerKind::Decoder,
                            &decoder_startup,
                            &decoder_config_rtp,
                            &decoder_config_worker,
                            reorder,
                            accept_h264,
                            accept_h265,
                        ) {
                            emit_startup_result(&decoder_events, &decoder_context, result);
                        }
                    }
                    "frame" => {
                        merge_decoder_snapshot(&mut decoder_snapshot.borrow_mut(), &message);
                        match decoded_frame(&message) {
                            Ok(frame) => {
                                let decode_latency_ms =
                                    number_field(&message, "lastDecodeLatencyUs")
                                        .unwrap_or_default()
                                        / 1_000.0;
                                emit(
                                    &decoder_events,
                                    &decoder_context,
                                    RuntimeEvent::NativeVideo {
                                        frame,
                                        decode_latency_ms,
                                        ready_at: web_time::Instant::now(),
                                    },
                                );
                                acknowledge_on_next_paint(frame_ack_worker.clone());
                            }
                            Err(error) => emit(
                                &decoder_events,
                                &decoder_context,
                                RuntimeEvent::Log {
                                    level: LogLevel::Warn,
                                    target: "decoder",
                                    message: error,
                                },
                            ),
                        }
                    }
                    "decode-stats" | "stopped" => {
                        merge_decoder_snapshot(&mut decoder_snapshot.borrow_mut(), &message);
                    }
                    "error" => emit_worker_error(&decoder_events, &decoder_context, &message),
                    _ => emit_unknown(&decoder_events, &decoder_context, "decoder", &kind),
                }
            },
        );
        decoder_worker.set_onmessage(Some(decoder_onmessage.as_ref().unchecked_ref()));

        let rtp_error = worker_error_handler(
            "RTP",
            Rc::clone(&startup),
            Rc::clone(&events),
            context.clone(),
        );
        rtp_worker.set_onerror(Some(rtp_error.as_ref().unchecked_ref()));
        let decoder_error = worker_error_handler("decoder", startup, events, context);
        decoder_worker.set_onerror(Some(decoder_error.as_ref().unchecked_ref()));

        Ok(Self {
            rtp_worker,
            decoder_worker,
            snapshot,
            rtp_transfer,
            rtp_payload: RefCell::new(Vec::with_capacity(16 * 1024)),
            ready: RefCell::new(Some(ready_receiver)),
            _rtp_onmessage: rtp_onmessage,
            _decoder_onmessage: decoder_onmessage,
            _rtp_onerror: rtp_error,
            _decoder_onerror: decoder_error,
        })
    }

    pub(super) async fn wait_until_ready(&self) -> Result<(), String> {
        let receiver = self
            .ready
            .borrow_mut()
            .take()
            .ok_or_else(|| "RTP/WebCodecs worker readiness was already consumed".to_owned())?;
        receiver
            .await
            .map_err(|_| "RTP/WebCodecs workers stopped before becoming ready".to_owned())?
    }

    /// Transfer recovered RTP without allowing decoder pressure to stall RF reception.
    pub(super) fn submit_rtp_batch<'a>(
        &self,
        packets: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<(), String> {
        let mut payload = self.rtp_payload.borrow_mut();
        payload.clear();
        let mut count = 0u32;
        for packet in packets {
            let length = u32::try_from(packet.len()).map_err(|_| "RTP packet is too large")?;
            payload.extend_from_slice(&length.to_le_bytes());
            payload.extend_from_slice(packet);
            count = count
                .checked_add(1)
                .ok_or_else(|| "RTP batch has too many packets".to_owned())?;
        }
        if count == 0 {
            return Ok(());
        }

        let mut transfer = self.rtp_transfer.borrow_mut();
        if transfer.in_flight >= MAX_RTP_BATCHES_IN_FLIGHT {
            transfer.dropped_batches = transfer.dropped_batches.saturating_add(1);
            transfer.dropped_packets = transfer.dropped_packets.saturating_add(u64::from(count));
            if transfer.dropped_batches == 1 || transfer.dropped_batches.is_power_of_two() {
                log::warn!(
                    target: "decoder",
                    "RTP worker overloaded; dropped {} batches / {} packets to preserve live latency",
                    transfer.dropped_batches,
                    transfer.dropped_packets
                );
            }
            return Ok(());
        }

        post_rtp_batch(&self.rtp_worker, &payload)?;
        transfer.in_flight += 1;
        transfer.submitted_batches = transfer.submitted_batches.saturating_add(1);
        if transfer.submitted_batches == 1 {
            log::info!(
                target: "decoder",
                "transferred first RTP batch packets={count} bytes={}",
                payload.len()
            );
        }
        Ok(())
    }

    pub(super) fn snapshot(&self) -> DecodeWorkerSnapshot {
        let mut snapshot = *self.snapshot.borrow();
        snapshot.transport_dropped_batches = self.rtp_transfer.borrow().dropped_batches;
        snapshot
    }
}

impl Drop for WebDecodeWorker {
    fn drop(&mut self) {
        self.rtp_worker.set_onmessage(None);
        self.rtp_worker.set_onerror(None);
        self.decoder_worker.set_onmessage(None);
        self.decoder_worker.set_onerror(None);
        self.rtp_worker.terminate();
        self.decoder_worker.terminate();
    }
}

#[derive(Clone, Copy)]
enum WorkerKind {
    Rtp,
    Decoder,
}

fn spawn_worker(url: &str, name: &str) -> Result<web_sys::Worker, String> {
    let options = web_sys::WorkerOptions::new();
    options.set_type(web_sys::WorkerType::Module);
    options.set_name(name);
    web_sys::Worker::new_with_options(url, &options)
        .map_err(|error| format!("start {name} worker: {error:?}"))
}

fn mark_ready_and_configure(
    kind: WorkerKind,
    startup: &Rc<RefCell<StartupState>>,
    rtp_worker: &web_sys::Worker,
    decoder_worker: &web_sys::Worker,
    reorder: bool,
    accept_h264: bool,
    accept_h265: bool,
) -> Option<Result<(), String>> {
    let mut startup = startup.borrow_mut();
    match kind {
        WorkerKind::Rtp => startup.rtp_ready = true,
        WorkerKind::Decoder => startup.decoder_ready = true,
    }
    if !startup.rtp_ready || !startup.decoder_ready || startup.configured {
        return None;
    }
    startup.configured = true;
    let result = (|| {
        let rtp_port = startup
            .rtp_port
            .take()
            .ok_or_else(|| "RTP worker MessagePort was already consumed".to_owned())?;
        let decoder_port = startup
            .decoder_port
            .take()
            .ok_or_else(|| "decoder worker MessagePort was already consumed".to_owned())?;
        post_rtp_configuration(rtp_worker, &rtp_port, reorder, accept_h264, accept_h265)?;
        post_decoder_configuration(decoder_worker, &decoder_port)?;
        Ok(())
    })();
    if let Some(sender) = startup.sender.take() {
        let _ = sender.send(result.clone());
    }
    Some(result)
}

fn post_rtp_configuration(
    worker: &web_sys::Worker,
    port: &web_sys::MessagePort,
    reorder: bool,
    accept_h264: bool,
    accept_h265: bool,
) -> Result<(), String> {
    let configure = Object::new();
    set_string(&configure, "kind", "configure");
    set_bool(&configure, "reorder", reorder);
    set_bool(&configure, "acceptH264", accept_h264);
    set_bool(&configure, "acceptH265", accept_h265);
    set_value(&configure, "port", port.as_ref());
    let transfer = Array::new();
    transfer.push(port.as_ref());
    worker
        .post_message_with_transfer(&configure, &transfer)
        .map_err(|error| format!("configure RTP worker: {error:?}"))
}

fn post_decoder_configuration(
    worker: &web_sys::Worker,
    port: &web_sys::MessagePort,
) -> Result<(), String> {
    let configure = Object::new();
    set_string(&configure, "kind", "configure");
    set_value(&configure, "port", port.as_ref());
    let transfer = Array::new();
    transfer.push(port.as_ref());
    worker
        .post_message_with_transfer(&configure, &transfer)
        .map_err(|error| format!("configure decoder worker: {error:?}"))
}

fn post_rtp_batch(worker: &web_sys::Worker, payload: &[u8]) -> Result<(), String> {
    let length = u32::try_from(payload.len()).map_err(|_| "RTP batch is too large")?;
    let bytes = Uint8Array::new_with_length(length);
    bytes.copy_from(payload);
    let message = Object::new();
    set_string(&message, "kind", "rtp-batch");
    set_value(&message, "data", bytes.as_ref());
    let transfer = Array::new();
    transfer.push(bytes.buffer().as_ref());
    worker
        .post_message_with_transfer(&message, &transfer)
        .map_err(|error| format!("transfer RTP batch to RTP worker: {error:?}"))
}

fn worker_error_handler(
    label: &'static str,
    startup: Rc<RefCell<StartupState>>,
    events: Rc<RefCell<std::collections::VecDeque<RuntimeEvent>>>,
    context: eframe::egui::Context,
) -> Closure<dyn FnMut(web_sys::ErrorEvent)> {
    Closure::new(move |error: web_sys::ErrorEvent| {
        let message = format!(
            "{label} worker failed at {}:{}: {}",
            error.filename(),
            error.lineno(),
            error.message()
        );
        if let Some(sender) = startup.borrow_mut().sender.take() {
            let _ = sender.send(Err(message.clone()));
        }
        emit(
            &events,
            &context,
            RuntimeEvent::Log {
                level: LogLevel::Error,
                target: "decoder",
                message,
            },
        );
    })
}

fn emit_startup_result(
    events: &Rc<RefCell<std::collections::VecDeque<RuntimeEvent>>>,
    context: &eframe::egui::Context,
    result: Result<(), String>,
) {
    let (level, message) = match result {
        Ok(()) => (
            LogLevel::Info,
            "Dedicated RTP and WebCodecs workers ready".to_owned(),
        ),
        Err(error) => (LogLevel::Error, error),
    };
    emit(
        events,
        context,
        RuntimeEvent::Log {
            level,
            target: "decoder",
            message,
        },
    );
}

fn emit_worker_error(
    events: &Rc<RefCell<std::collections::VecDeque<RuntimeEvent>>>,
    context: &eframe::egui::Context,
    message: &JsValue,
) {
    emit(
        events,
        context,
        RuntimeEvent::Log {
            level: LogLevel::Warn,
            target: "decoder",
            message: string_field(message, "message").unwrap_or_else(|| "worker error".to_owned()),
        },
    );
}

fn emit_unknown(
    events: &Rc<RefCell<std::collections::VecDeque<RuntimeEvent>>>,
    context: &eframe::egui::Context,
    worker: &str,
    kind: &str,
) {
    emit(
        events,
        context,
        RuntimeEvent::Log {
            level: LogLevel::Warn,
            target: "decoder",
            message: format!("unknown {worker} worker response {kind}"),
        },
    );
}

fn acknowledge_on_next_paint(worker: web_sys::Worker) {
    let fallback = worker.clone();
    let callback = Closure::once_into_js(move |_timestamp: f64| {
        let message = Object::new();
        set_string(&message, "kind", "frame-ack");
        let _ = worker.post_message(&message);
    });
    let scheduled = web_sys::window().is_some_and(|window| {
        window
            .request_animation_frame(callback.unchecked_ref())
            .is_ok()
    });
    if !scheduled {
        let message = Object::new();
        set_string(&message, "kind", "frame-ack");
        let _ = fallback.post_message(&message);
    }
}

fn decoded_frame(message: &JsValue) -> Result<DecodedFrame<WebVideoFrame>, String> {
    let frame = Reflect::get(message, &JsValue::from_str("frame"))
        .map_err(|error| format!("decoder worker frame missing: {error:?}"))?
        .dyn_into::<web_sys::VideoFrame>()
        .map_err(|_| "decoder worker returned a non-VideoFrame value".to_owned())?;
    let timestamp = video_timestamp(message, "timestampValue", "timestampTimescale")
        .ok_or_else(|| "decoder worker returned an invalid timestamp".to_owned())?;
    let duration = video_timestamp(message, "durationValue", "durationTimescale");
    Ok(DecodedFrame {
        surface: WebVideoFrame::from_video_frame(frame),
        timestamp,
        duration,
    })
}

fn video_timestamp(message: &JsValue, value: &str, timescale: &str) -> Option<VideoTimestamp> {
    let value = number_field(message, value)? as i64;
    let timescale = number_field(message, timescale)? as i32;
    VideoTimestamp::new(value, timescale)
}

fn merge_rtp_snapshot(snapshot: &mut DecodeWorkerSnapshot, message: &JsValue) {
    snapshot.rtp = RtpDepacketizerStatus {
        packets: u64_field(message, "rtpPackets"),
        frames_emitted: u64_field(message, "framesEmitted"),
        config_wait_drops: u64_field(message, "configWaitDrops"),
        keyframes_with_prepended_config: u64_field(message, "keyframesWithPrependedConfig"),
        parameter_sets_prepended: u64_field(message, "parameterSetsPrepended"),
        fragment_sequence_gaps: u64_field(message, "fragmentSequenceGaps"),
        damaged_frames_forwarded: u64_field(message, "damagedFramesForwarded"),
        damaged_frames_dropped: u64_field(message, "damagedFramesDropped"),
        fragment_overflows: u64_field(message, "fragmentOverflows"),
        unsupported_payloads: u64_field(message, "unsupportedPayloads"),
        malformed_packets: u64_field(message, "malformedPackets"),
        last_payload_type: optional_u8_field(message, "lastPayloadType"),
        last_sequence_number: optional_u16_field(message, "lastSequenceNumber"),
        last_timestamp: optional_u32_field(message, "lastTimestamp"),
        last_codec: number_field(message, "lastCodec").and_then(|value| match value as u16 {
            264 => Some(Codec::H264),
            265 => Some(Codec::H265),
            _ => None,
        }),
        last_nal_type: optional_u8_field(message, "lastNalType"),
        codec_config: CodecConfigState {
            h264_sps: bool_field(message, "h264Sps"),
            h264_pps: bool_field(message, "h264Pps"),
            h265_vps: bool_field(message, "h265Vps"),
            h265_sps: bool_field(message, "h265Sps"),
            h265_pps: bool_field(message, "h265Pps"),
        },
    };
    snapshot.reorder = RtpReorderStatus {
        buffered_packets: usize_field(message, "bufferedPackets"),
        reordered_packets: u64_field(message, "reorderedPackets"),
        late_packets: u64_field(message, "latePackets"),
        forced_flushes: u64_field(message, "forcedFlushes"),
    };
    snapshot.encoded_bytes = u64_field(message, "encodedBytes");
    snapshot.rtp_queue_drops = u64_field(message, "rtpQueueDrops");
}

fn merge_decoder_snapshot(snapshot: &mut DecodeWorkerSnapshot, message: &JsValue) {
    snapshot.decoder = DecoderStats {
        access_units_received: u64_field(message, "accessUnitsReceived"),
        access_units_submitted: u64_field(message, "accessUnitsSubmitted"),
        waiting_drops: u64_field(message, "waitingDrops"),
        backpressure_drops: u64_field(message, "backpressureDrops"),
        frames_decoded: u64_field(message, "framesDecoded"),
        output_drops: u64_field(message, "outputDrops"),
        decode_errors: u64_field(message, "decodeErrors"),
        reconfigurations: u64_field(message, "reconfigurations"),
        frames_in_flight: usize_field(message, "framesInFlight"),
        last_decode_latency_us: u64_field(message, "lastDecodeLatencyUs"),
        max_decode_latency_us: u64_field(message, "maxDecodeLatencyUs"),
        last_platform_status: None,
    };
    snapshot.decode_queue_drops = u64_field(message, "decodeQueueDrops");
}

fn emit(
    events: &Rc<RefCell<std::collections::VecDeque<RuntimeEvent>>>,
    context: &eframe::egui::Context,
    event: RuntimeEvent,
) {
    queue_event(&mut events.borrow_mut(), event);
    context.request_repaint();
}

fn set_value(object: &Object, name: &str, value: &JsValue) {
    let _ = Reflect::set(object, &JsValue::from_str(name), value);
}

fn set_string(object: &Object, name: &str, value: &str) {
    set_value(object, name, &JsValue::from_str(value));
}

fn set_bool(object: &Object, name: &str, value: bool) {
    set_value(object, name, &JsValue::from_bool(value));
}

fn string_field(value: &JsValue, name: &str) -> Option<String> {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_string())
}

fn number_field(value: &JsValue, name: &str) -> Option<f64> {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_f64())
}

fn bool_field(value: &JsValue, name: &str) -> bool {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn u64_field(value: &JsValue, name: &str) -> u64 {
    number_field(value, name).unwrap_or_default().max(0.0) as u64
}

fn usize_field(value: &JsValue, name: &str) -> usize {
    u64_field(value, name).min(usize::MAX as u64) as usize
}

fn optional_u8_field(value: &JsValue, name: &str) -> Option<u8> {
    number_field(value, name).and_then(|value| u8::try_from(value as u64).ok())
}

fn optional_u16_field(value: &JsValue, name: &str) -> Option<u16> {
    number_field(value, name).and_then(|value| u16::try_from(value as u64).ok())
}

fn optional_u32_field(value: &JsValue, name: &str) -> Option<u32> {
    number_field(value, name).and_then(|value| u32::try_from(value as u64).ok())
}
