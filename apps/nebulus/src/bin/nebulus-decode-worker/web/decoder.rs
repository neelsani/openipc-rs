use std::cell::RefCell;

use js_sys::{Array, Date, Object, Reflect, Uint8Array};
use openipc_video::{
    DecodedFrame, DecoderOptions, DecoderStats, EncodedAccessUnit, PlatformDecoder, SubmitOutcome,
    VideoCodec, VideoDecoder as _, VideoTimestamp, WebVideoFrame,
};
use wasm_bindgen::{closure::Closure, JsCast as _, JsValue};

use crate::low_latency_queue::LowLatencyQueue;

use super::{
    bool_field, number_field, post_error, set_number, set_string, set_value, string_field,
    worker_scope,
};

const MAX_QUEUED_ACCESS_UNITS: usize = 8;
const DECODE_SUBMIT_HIGH_WATER: usize = 2;

thread_local! {
    static RUNTIME: RefCell<Option<DecodeRuntime>> = const { RefCell::new(None) };
}

pub(super) fn start() -> Result<(), String> {
    let runtime = DecodeRuntime::new()?;
    RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
    Ok(())
}

pub(super) fn handle_message(message: JsValue) -> Result<(), String> {
    match string_field(&message, "kind").as_deref() {
        Some("configure") => RUNTIME.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .ok_or_else(|| "decoder worker is not initialized".to_owned())?
                .configure(&message)
        }),
        Some("frame-ack") => {
            RUNTIME.with(|slot| {
                if let Some(runtime) = slot.borrow_mut().as_mut() {
                    runtime.acknowledge_frame();
                }
            });
            Ok(())
        }
        Some("stop") => {
            RUNTIME.with(|slot| {
                if let Some(mut runtime) = slot.borrow_mut().take() {
                    let _ = runtime.decoder.flush();
                    runtime.send_stats("stopped");
                }
            });
            worker_scope().close();
            Ok(())
        }
        Some(other) => Err(format!("unknown decoder worker message {other}")),
        None => Err("decoder worker message has no kind".to_owned()),
    }
}

struct DecodeRuntime {
    decoder: PlatformDecoder,
    input_port: Option<web_sys::MessagePort>,
    access_units: LowLatencyQueue<EncodedAccessUnit>,
    decode_pump_scheduled: bool,
    frame_in_flight: bool,
    pending_frame: Option<DecodedFrame<WebVideoFrame>>,
    last_stats_emit_ms: f64,
}

impl DecodeRuntime {
    fn new() -> Result<Self, String> {
        let decoder = PlatformDecoder::new(DecoderOptions {
            max_frames_in_flight: 3,
            ..DecoderOptions::default()
        })
        .map_err(|error| format!("WebCodecs worker unavailable: {error}"))?;
        Ok(Self {
            decoder,
            input_port: None,
            access_units: LowLatencyQueue::new(MAX_QUEUED_ACCESS_UNITS),
            decode_pump_scheduled: false,
            frame_in_flight: false,
            pending_frame: None,
            last_stats_emit_ms: Date::now(),
        })
    }

    fn configure(&mut self, message: &JsValue) -> Result<(), String> {
        let port = Reflect::get(message, &JsValue::from_str("port"))
            .map_err(|error| format!("decoder worker configure has no input port: {error:?}"))?
            .dyn_into::<web_sys::MessagePort>()
            .map_err(|_| "decoder worker configure port is invalid".to_owned())?;
        let acknowledgement_port = port.clone();
        let onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
            move |event: web_sys::MessageEvent| {
                let result = parse_access_unit(&event.data()).and_then(|unit| {
                    let should_pump = RUNTIME.with(|slot| -> Result<bool, String> {
                        let mut slot = slot.borrow_mut();
                        let runtime = slot
                            .as_mut()
                            .ok_or_else(|| "decoder worker is not initialized".to_owned())?;
                        Ok(runtime.push_access_unit(unit))
                    })?;
                    let ack = Object::new();
                    set_string(&ack, "kind", "access-unit-ack");
                    acknowledgement_port
                        .post_message(&ack)
                        .map_err(|error| format!("acknowledge access unit: {error:?}"))?;
                    if should_pump {
                        schedule_decode_pump();
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    post_error(error);
                    let ack = Object::new();
                    set_string(&ack, "kind", "access-unit-ack");
                    let _ = acknowledgement_port.post_message(&ack);
                }
            },
        );
        port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        port.start();
        onmessage.forget();
        self.input_port = Some(port);
        Ok(())
    }

    fn push_access_unit(&mut self, unit: EncodedAccessUnit) -> bool {
        let is_keyframe = unit.keyframe;
        self.access_units.push(unit, is_keyframe);
        self.arm_decode_pump()
    }

    fn arm_decode_pump(&mut self) -> bool {
        if self.decode_pump_scheduled || self.access_units.is_empty() {
            return false;
        }
        self.decode_pump_scheduled = true;
        true
    }

    fn pump_decoder(&mut self) -> bool {
        self.decode_pump_scheduled = false;
        self.poll_decoder();
        while self.decoder_has_capacity() && self.submit_next_access_unit() {
            self.poll_decoder();
        }
        if Date::now() - self.last_stats_emit_ms >= 50.0 {
            self.send_stats("decode-stats");
        }
        self.arm_decode_pump()
    }

    fn decoder_has_capacity(&self) -> bool {
        self.decoder.stats().frames_in_flight < DECODE_SUBMIT_HIGH_WATER
            && (self.decoder.decode_queue_size() as usize) < DECODE_SUBMIT_HIGH_WATER
    }

    fn submit_next_access_unit(&mut self) -> bool {
        let Some(unit) = self.access_units.pop_front() else {
            return false;
        };
        match self.decoder.submit(unit) {
            Ok(SubmitOutcome::DroppedForBackpressure) => self.access_units.force_resync(),
            Ok(_) => {}
            Err(error) => {
                post_error(format!("decode submit failed: {error}"));
                self.access_units.force_resync();
            }
        }
        true
    }

    fn poll_decoder(&mut self) {
        let Some(frame) = self.decoder.latest_frame() else {
            return;
        };
        if self.frame_in_flight {
            self.pending_frame = Some(frame);
        } else {
            self.send_frame(frame);
        }
    }

    fn acknowledge_frame(&mut self) {
        self.frame_in_flight = false;
        if let Some(frame) = self.decoder.latest_frame() {
            self.pending_frame = Some(frame);
        }
        if let Some(frame) = self.pending_frame.take() {
            self.send_frame(frame);
        }
    }

    fn send_frame(&mut self, frame: DecodedFrame<WebVideoFrame>) {
        let object = self.snapshot("frame");
        set_number(&object, "timestampValue", frame.timestamp.value as f64);
        set_number(
            &object,
            "timestampTimescale",
            f64::from(frame.timestamp.timescale),
        );
        if let Some(duration) = frame.duration {
            set_number(&object, "durationValue", duration.value as f64);
            set_number(&object, "durationTimescale", f64::from(duration.timescale));
        }
        let transferable = frame.surface.clone_video_frame();
        set_value(&object, "frame", transferable.as_ref());
        let transfer = Array::new();
        transfer.push(transferable.as_ref());
        match worker_scope().post_message_with_transfer(&object, &transfer) {
            Ok(()) => {
                self.frame_in_flight = true;
                self.last_stats_emit_ms = Date::now();
            }
            Err(error) => post_error(format!("transfer decoded frame failed: {error:?}")),
        }
    }

    fn send_stats(&mut self, kind: &str) {
        let object = self.snapshot(kind);
        let _ = worker_scope().post_message(&object);
        self.last_stats_emit_ms = Date::now();
    }

    fn snapshot(&self, kind: &str) -> Object {
        let object = Object::new();
        set_string(&object, "kind", kind);
        set_number(
            &object,
            "decodeQueueDrops",
            self.access_units.dropped() as f64,
        );
        set_number(&object, "decodeQueueDepth", self.access_units.len() as f64);
        write_decoder_stats(&object, self.decoder.stats());
        object
    }
}

fn parse_access_unit(message: &JsValue) -> Result<EncodedAccessUnit, String> {
    if string_field(message, "kind").as_deref() != Some("access-unit") {
        return Err("decoder port received an unknown message".to_owned());
    }
    let codec = match number_field(message, "codec").unwrap_or_default() as u16 {
        264 => VideoCodec::H264,
        265 => VideoCodec::H265,
        value => return Err(format!("decoder port received unknown codec {value}")),
    };
    let timestamp = number_field(message, "timestamp").unwrap_or_default() as u32;
    let data = Reflect::get(message, &JsValue::from_str("data"))
        .map_err(|error| format!("decoder access unit has no data: {error:?}"))?;
    let view = Uint8Array::new(&data);
    let mut payload = vec![0; view.length() as usize];
    view.copy_to(&mut payload);
    let mut unit = EncodedAccessUnit::new(
        codec,
        payload,
        VideoTimestamp::from_rtp(timestamp),
        bool_field(message, "keyframe"),
    );
    unit.sequence_number = number_field(message, "sequence").map(|value| value as u16);
    Ok(unit)
}

fn schedule_decode_pump() {
    let callback = Closure::once_into_js(move || {
        let schedule_again = RUNTIME.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .is_some_and(DecodeRuntime::pump_decoder)
        });
        if schedule_again {
            schedule_decode_pump();
        }
    });
    let scope: web_sys::WorkerGlobalScope = worker_scope().unchecked_into();
    if scope
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 0)
        .is_err()
    {
        scope.queue_microtask(callback.unchecked_ref());
    }
}

fn write_decoder_stats(object: &Object, stats: DecoderStats) {
    set_number(
        object,
        "accessUnitsReceived",
        stats.access_units_received as f64,
    );
    set_number(
        object,
        "accessUnitsSubmitted",
        stats.access_units_submitted as f64,
    );
    set_number(object, "waitingDrops", stats.waiting_drops as f64);
    set_number(object, "backpressureDrops", stats.backpressure_drops as f64);
    set_number(object, "framesDecoded", stats.frames_decoded as f64);
    set_number(object, "outputDrops", stats.output_drops as f64);
    set_number(object, "decodeErrors", stats.decode_errors as f64);
    set_number(object, "reconfigurations", stats.reconfigurations as f64);
    set_number(object, "framesInFlight", stats.frames_in_flight as f64);
    set_number(
        object,
        "lastDecodeLatencyUs",
        stats.last_decode_latency_us as f64,
    );
    set_number(
        object,
        "maxDecodeLatencyUs",
        stats.max_decode_latency_us as f64,
    );
}
