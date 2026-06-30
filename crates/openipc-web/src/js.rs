use js_sys::{Object, Reflect, Uint8Array};
use openipc_core::FecCounters;
use wasm_bindgen::prelude::*;

pub(crate) fn raw_payload_object(payload: openipc_core::RoutePayload) -> Result<Object, JsValue> {
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
    set_number(&object, "routeId", payload.route_id.raw() as f64)?;
    set_number(&object, "channelId", payload.channel_id.raw() as f64)?;
    Ok(object)
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
