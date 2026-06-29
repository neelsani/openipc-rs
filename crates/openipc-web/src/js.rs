use js_sys::{Array, Object, Reflect, Uint8Array};
use openipc_core::realtek::RxPacketType;
use openipc_core::{ChannelId, FecCounters, PayloadPipelineEvent, PipelineEvent};
use wasm_bindgen::prelude::*;

use crate::video::video_frame_object;

pub(crate) fn video_frames_from_events(events: Vec<PipelineEvent>) -> Array {
    let frames = Array::new();
    append_video_frames(&frames, events);
    frames
}

pub(crate) fn append_video_frames(frames: &Array, events: Vec<PipelineEvent>) {
    for event in events {
        if let PipelineEvent::VideoFrame(frame) = event {
            frames.push(&Uint8Array::from(frame.data.as_slice()));
        }
    }
}

pub(crate) fn append_video_frame_objects(
    frames: &Array,
    events: Vec<PipelineEvent>,
) -> Result<(), JsValue> {
    for event in events {
        if let PipelineEvent::VideoFrame(frame) = event {
            frames.push(&video_frame_object(frame)?.into());
        }
    }
    Ok(())
}

pub(crate) fn append_payload_objects(
    payloads: &Array,
    events: Vec<PayloadPipelineEvent>,
    channel_id: ChannelId,
    payload_count: &mut usize,
    payload_bytes: &mut usize,
) -> Result<(), JsValue> {
    for event in events {
        if let PayloadPipelineEvent::Payload(payload) = event {
            *payload_count += 1;
            *payload_bytes += payload.data.len();
            payloads.push(&raw_payload_object(payload, channel_id)?.into());
        }
    }
    Ok(())
}

pub(crate) fn raw_payload_object(
    payload: openipc_core::RecoveredPayload,
    channel_id: ChannelId,
) -> Result<Object, JsValue> {
    let object = Object::new();
    Reflect::set(
        &object,
        &JsValue::from_str("data"),
        &Uint8Array::from(payload.data.as_slice()),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("packetSeq"),
        &JsValue::from_str(&payload.packet_seq.to_string()),
    )?;
    set_number(&object, "channelId", channel_id.raw() as f64)?;
    Ok(object)
}

pub(crate) fn accept_rx_packet(
    attrib: openipc_core::realtek::RxPacketAttrib,
    keep_corrupted: bool,
) -> bool {
    attrib.pkt_rpt_type == RxPacketType::NormalRx
        && (keep_corrupted || (!attrib.crc_err && !attrib.icv_err))
}

pub(crate) fn set_number(object: &Object, key: &str, value: f64) -> Result<(), JsValue> {
    Reflect::set(object, &JsValue::from_str(key), &JsValue::from_f64(value))?;
    Ok(())
}

pub(crate) fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.performance())
            .map(|performance| performance.now())
            .unwrap_or_else(js_sys::Date::now)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}

pub(crate) fn elapsed_ms(start_ms: f64) -> f64 {
    let elapsed = now_ms() - start_ms;
    if elapsed.is_finite() && elapsed >= 0.0 {
        elapsed
    } else {
        0.0
    }
}

pub(crate) fn counters_json(counters: FecCounters) -> String {
    format!(
        r#"{{"totalPackets":{},"recoveredPackets":{},"lostPackets":{},"badPackets":{}}}"#,
        counters.total_packets,
        counters.recovered_packets,
        counters.lost_packets,
        counters.bad_packets
    )
}

pub(crate) fn escape_json_str(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn ms_from_js(now_ms: f64) -> u64 {
    if now_ms.is_finite() && now_ms > 0.0 {
        now_ms.min(u64::MAX as f64) as u64
    } else {
        0
    }
}

pub(crate) fn parse_hex_u64(input: &str) -> Result<u64, JsValue> {
    let trimmed = input.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u64::from_str_radix(hex, 16)
        .map_err(|err| JsValue::from_str(&format!("invalid nonce hex: {err}")))
}
