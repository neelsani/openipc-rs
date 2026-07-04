use std::cell::RefCell;

use js_sys::{Array, Date, Object, Reflect, Uint8Array};
use openipc_core::{
    Codec, DepacketizedFrame, RtpDepacketizer, RtpDepacketizerStatus, RtpReorderBuffer,
    RtpReorderStatus,
};
use wasm_bindgen::{closure::Closure, JsCast as _, JsValue};

use crate::{batch::visit_rtp_batch, low_latency_queue::LowLatencyQueue};

use super::{
    bool_field, post_error, post_kind, set_bool, set_number, set_optional_number, set_string,
    set_value, string_field, worker_scope,
};

const MAX_ACCESS_UNITS_IN_FLIGHT: usize = 8;
const MAX_QUEUED_ACCESS_UNITS: usize = 8;

thread_local! {
    static RUNTIME: RefCell<Option<RtpRuntime>> = const { RefCell::new(None) };
}

pub(super) fn start() {
    RUNTIME.with(|slot| *slot.borrow_mut() = Some(RtpRuntime::new()));
}

pub(super) fn handle_message(message: JsValue) -> Result<(), String> {
    match string_field(&message, "kind").as_deref() {
        Some("configure") => RUNTIME.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .ok_or_else(|| "RTP worker is not initialized".to_owned())?
                .configure(&message)
        }),
        Some("rtp-batch") => {
            let data = Reflect::get(&message, &JsValue::from_str("data"))
                .map_err(|error| format!("RTP worker batch has no data: {error:?}"))?;
            let view = Uint8Array::new(&data);
            let mut payload = vec![0; view.length() as usize];
            view.copy_to(&mut payload);
            RUNTIME.with(|slot| -> Result<(), String> {
                let mut slot = slot.borrow_mut();
                let runtime = slot
                    .as_mut()
                    .ok_or_else(|| "RTP worker is not initialized".to_owned())?;
                visit_rtp_batch(&payload, |packet| runtime.push_rtp(packet))
                    .map_err(str::to_owned)?;
                runtime.finish_batch();
                Ok(())
            })?;
            post_kind("rtp-ack");
            Ok(())
        }
        Some("stop") => {
            RUNTIME.with(|slot| {
                if let Some(mut runtime) = slot.borrow_mut().take() {
                    runtime.send_stats("stopped");
                }
            });
            worker_scope().close();
            Ok(())
        }
        Some(other) => Err(format!("unknown RTP worker message {other}")),
        None => Err("RTP worker message has no kind".to_owned()),
    }
}

struct RtpRuntime {
    depacketizer: RtpDepacketizer,
    reorder: Option<RtpReorderBuffer>,
    accept_h264: bool,
    accept_h265: bool,
    decoder_port: Option<web_sys::MessagePort>,
    pending: LowLatencyQueue<DepacketizedFrame>,
    access_units_in_flight: usize,
    encoded_bytes: u64,
    last_stats_emit_ms: f64,
}

impl RtpRuntime {
    fn new() -> Self {
        Self {
            depacketizer: RtpDepacketizer::new(),
            reorder: None,
            accept_h264: true,
            accept_h265: true,
            decoder_port: None,
            pending: LowLatencyQueue::new(MAX_QUEUED_ACCESS_UNITS),
            access_units_in_flight: 0,
            encoded_bytes: 0,
            last_stats_emit_ms: Date::now(),
        }
    }

    fn configure(&mut self, message: &JsValue) -> Result<(), String> {
        self.reorder = bool_field(message, "reorder").then(RtpReorderBuffer::default);
        self.accept_h264 = bool_field(message, "acceptH264");
        self.accept_h265 = bool_field(message, "acceptH265");
        let port = Reflect::get(message, &JsValue::from_str("port"))
            .map_err(|error| format!("RTP worker configure has no decoder port: {error:?}"))?
            .dyn_into::<web_sys::MessagePort>()
            .map_err(|_| "RTP worker configure port is invalid".to_owned())?;
        let onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
            move |event: web_sys::MessageEvent| {
                if string_field(&event.data(), "kind").as_deref() != Some("access-unit-ack") {
                    return;
                }
                RUNTIME.with(|slot| {
                    if let Some(runtime) = slot.borrow_mut().as_mut() {
                        runtime.access_units_in_flight =
                            runtime.access_units_in_flight.saturating_sub(1);
                        runtime.drain_pending();
                    }
                });
            },
        );
        port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        port.start();
        onmessage.forget();
        self.decoder_port = Some(port);
        self.drain_pending();
        Ok(())
    }

    fn push_rtp(&mut self, packet: &[u8]) {
        if let Some(reorder) = self.reorder.as_mut() {
            if let Ok(packets) = reorder.push(packet) {
                for packet in packets {
                    self.push_ordered_rtp(&packet);
                }
            }
        } else {
            self.push_ordered_rtp(packet);
        }
    }

    fn push_ordered_rtp(&mut self, packet: &[u8]) {
        let Ok(Some(frame)) = self.depacketizer.push(packet) else {
            return;
        };
        if !self.accepts(frame.codec) {
            return;
        }
        self.encoded_bytes = self.encoded_bytes.saturating_add(frame.data.len() as u64);
        let is_keyframe = frame.is_keyframe;
        self.pending.push(frame, is_keyframe);
        self.drain_pending();
    }

    fn accepts(&self, codec: Codec) -> bool {
        match codec {
            Codec::H264 => self.accept_h264,
            Codec::H265 => self.accept_h265,
        }
    }

    fn drain_pending(&mut self) {
        while self.access_units_in_flight < MAX_ACCESS_UNITS_IN_FLIGHT {
            let Some(frame) = self.pending.pop_front() else {
                break;
            };
            match self.post_access_unit(frame) {
                Ok(()) => {
                    self.access_units_in_flight += 1;
                }
                Err(error) => {
                    post_error(error);
                    self.pending.force_resync();
                    break;
                }
            }
        }
    }

    fn post_access_unit(&self, frame: DepacketizedFrame) -> Result<(), String> {
        let port = self
            .decoder_port
            .as_ref()
            .ok_or_else(|| "RTP worker has no decoder port".to_owned())?;
        let length = u32::try_from(frame.data.len()).map_err(|_| "access unit is too large")?;
        let bytes = Uint8Array::new_with_length(length);
        bytes.copy_from(&frame.data);
        let message = Object::new();
        set_string(&message, "kind", "access-unit");
        set_number(
            &message,
            "codec",
            match frame.codec {
                Codec::H264 => 264.0,
                Codec::H265 => 265.0,
            },
        );
        set_number(&message, "timestamp", frame.timestamp as f64);
        set_number(&message, "sequence", f64::from(frame.sequence_number));
        set_bool(&message, "keyframe", frame.is_keyframe);
        set_value(&message, "data", bytes.as_ref());
        let transfer = Array::new();
        transfer.push(bytes.buffer().as_ref());
        port.post_message_with_transferable(&message, &transfer)
            .map_err(|error| format!("transfer access unit to decoder worker: {error:?}"))
    }

    fn finish_batch(&mut self) {
        self.drain_pending();
        if Date::now() - self.last_stats_emit_ms >= 50.0 {
            self.send_stats("rtp-stats");
        }
    }

    fn send_stats(&mut self, kind: &str) {
        let object = Object::new();
        set_string(&object, "kind", kind);
        set_number(&object, "encodedBytes", self.encoded_bytes as f64);
        set_number(&object, "rtpQueueDrops", self.pending.dropped() as f64);
        set_number(&object, "rtpQueueDepth", self.pending.len() as f64);
        write_rtp_status(&object, self.depacketizer.status());
        write_reorder_status(
            &object,
            self.reorder
                .as_ref()
                .map(RtpReorderBuffer::status)
                .unwrap_or_default(),
        );
        let _ = worker_scope().post_message(&object);
        self.last_stats_emit_ms = Date::now();
    }
}

fn write_rtp_status(object: &Object, status: RtpDepacketizerStatus) {
    set_number(object, "rtpPackets", status.packets as f64);
    set_number(object, "framesEmitted", status.frames_emitted as f64);
    set_number(object, "configWaitDrops", status.config_wait_drops as f64);
    set_number(
        object,
        "keyframesWithPrependedConfig",
        status.keyframes_with_prepended_config as f64,
    );
    set_number(
        object,
        "parameterSetsPrepended",
        status.parameter_sets_prepended as f64,
    );
    set_number(
        object,
        "fragmentSequenceGaps",
        status.fragment_sequence_gaps as f64,
    );
    set_number(
        object,
        "fragmentOverflows",
        status.fragment_overflows as f64,
    );
    set_number(
        object,
        "unsupportedPayloads",
        status.unsupported_payloads as f64,
    );
    set_number(object, "malformedPackets", status.malformed_packets as f64);
    set_optional_number(
        object,
        "lastPayloadType",
        status.last_payload_type.map(f64::from),
    );
    set_optional_number(
        object,
        "lastSequenceNumber",
        status.last_sequence_number.map(f64::from),
    );
    set_optional_number(
        object,
        "lastTimestamp",
        status.last_timestamp.map(f64::from),
    );
    set_optional_number(
        object,
        "lastCodec",
        status.last_codec.map(|codec| match codec {
            Codec::H264 => 264.0,
            Codec::H265 => 265.0,
        }),
    );
    set_optional_number(object, "lastNalType", status.last_nal_type.map(f64::from));
    set_bool(object, "h264Sps", status.codec_config.h264_sps);
    set_bool(object, "h264Pps", status.codec_config.h264_pps);
    set_bool(object, "h265Vps", status.codec_config.h265_vps);
    set_bool(object, "h265Sps", status.codec_config.h265_sps);
    set_bool(object, "h265Pps", status.codec_config.h265_pps);
}

fn write_reorder_status(object: &Object, status: RtpReorderStatus) {
    set_number(object, "bufferedPackets", status.buffered_packets as f64);
    set_number(object, "reorderedPackets", status.reordered_packets as f64);
    set_number(object, "latePackets", status.late_packets as f64);
    set_number(object, "forcedFlushes", status.forced_flushes as f64);
}
