use js_sys::{Object, Reflect, Uint8Array};
use openipc_core::{
    Codec, CodecConfigState, DepacketizedFrame, FrameDamage, RtpDepacketizerStatus,
    RtpReorderStatus,
};
use wasm_bindgen::prelude::*;

pub(crate) fn video_frame_object(frame: DepacketizedFrame) -> Result<Object, JsValue> {
    let object = Object::new();
    let codec_string = codec_string(&frame);
    let codec_config = JsValue::from(codec_config_object(frame.codec_config)?);
    Reflect::set(
        &object,
        &JsValue::from_str("data"),
        &Uint8Array::from(frame.data.as_slice()),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("codec"),
        &JsValue::from_str(codec_name(frame.codec)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("codecString"),
        &JsValue::from_str(&codec_string),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("isKeyFrame"),
        &JsValue::from_bool(frame.is_keyframe),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("timestamp"),
        &JsValue::from_f64(f64::from(frame.timestamp)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("payloadType"),
        &JsValue::from_f64(f64::from(frame.payload_type)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("sequenceNumber"),
        &JsValue::from_f64(f64::from(frame.sequence_number)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("nalType"),
        &JsValue::from_f64(f64::from(frame.nal_type)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("decoderConfigComplete"),
        &JsValue::from_bool(frame.codec_config.is_complete_for(frame.codec)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("damaged"),
        &JsValue::from_bool(frame.damaged),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("damageKind"),
        &JsValue::from_str(match frame.damage {
            FrameDamage::None => "none",
            FrameDamage::MissingSlice => "missing-slice",
            FrameDamage::TruncatedFragment => "truncated-fragment",
        }),
    )?;
    Reflect::set(&object, &JsValue::from_str("codecConfig"), &codec_config)?;
    Ok(object)
}

pub(crate) fn rtp_status_object(
    status: RtpDepacketizerStatus,
    reorder: RtpReorderStatus,
) -> Result<Object, JsValue> {
    let object = Object::new();
    let codec_config = JsValue::from(codec_config_object(status.codec_config)?);
    set_u64(&object, "packets", status.packets)?;
    set_u64(&object, "framesEmitted", status.frames_emitted)?;
    set_u64(&object, "configWaitDrops", status.config_wait_drops)?;
    set_u64(
        &object,
        "keyframesWithPrependedConfig",
        status.keyframes_with_prepended_config,
    )?;
    set_u64(
        &object,
        "parameterSetsPrepended",
        status.parameter_sets_prepended,
    )?;
    set_u64(
        &object,
        "fragmentSequenceGaps",
        status.fragment_sequence_gaps,
    )?;
    set_u64(
        &object,
        "damagedFramesForwarded",
        status.damaged_frames_forwarded,
    )?;
    set_u64(
        &object,
        "damagedFramesDropped",
        status.damaged_frames_dropped,
    )?;
    set_u64(&object, "fragmentOverflows", status.fragment_overflows)?;
    set_u64(&object, "unsupportedPayloads", status.unsupported_payloads)?;
    set_u64(&object, "malformedPackets", status.malformed_packets)?;
    set_option_u8(&object, "lastPayloadType", status.last_payload_type)?;
    set_option_u16(&object, "lastSequenceNumber", status.last_sequence_number)?;
    set_option_u32(&object, "lastTimestamp", status.last_timestamp)?;
    Reflect::set(
        &object,
        &JsValue::from_str("lastCodec"),
        &status
            .last_codec
            .map(codec_name)
            .map(JsValue::from_str)
            .unwrap_or(JsValue::NULL),
    )?;
    set_option_u8(&object, "lastNalType", status.last_nal_type)?;
    Reflect::set(&object, &JsValue::from_str("codecConfig"), &codec_config)?;
    Reflect::set(
        &object,
        &JsValue::from_str("h264ConfigComplete"),
        &JsValue::from_bool(status.codec_config.is_complete_for(Codec::H264)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("h265ConfigComplete"),
        &JsValue::from_bool(status.codec_config.is_complete_for(Codec::H265)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("reorderBufferedPackets"),
        &JsValue::from_f64(reorder.buffered_packets as f64),
    )?;
    set_u64(&object, "reorderedPackets", reorder.reordered_packets)?;
    set_u64(&object, "latePackets", reorder.late_packets)?;
    set_u64(&object, "forcedFlushes", reorder.forced_flushes)?;
    Ok(object)
}

fn codec_config_object(config: CodecConfigState) -> Result<Object, JsValue> {
    let object = Object::new();
    Reflect::set(
        &object,
        &JsValue::from_str("h264Sps"),
        &JsValue::from_bool(config.h264_sps),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("h264Pps"),
        &JsValue::from_bool(config.h264_pps),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("h265Vps"),
        &JsValue::from_bool(config.h265_vps),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("h265Sps"),
        &JsValue::from_bool(config.h265_sps),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("h265Pps"),
        &JsValue::from_bool(config.h265_pps),
    )?;
    Ok(object)
}

fn codec_name(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "h264",
        Codec::H265 => "h265",
    }
}

fn codec_string(frame: &DepacketizedFrame) -> String {
    match frame.codec {
        Codec::H264 => h264_codec_string(&frame.data).unwrap_or_else(|| "avc1.42E01E".to_owned()),
        Codec::H265 => "hev1.1.6.L93.B0".to_owned(),
    }
}

pub(crate) fn h264_codec_string(frame: &[u8]) -> Option<String> {
    for unit in annex_b_units(frame) {
        let nalu = &frame[unit.start..unit.end];
        if nalu.len() >= 4 && nalu[0] & 0x1f == 7 {
            return Some(format!(
                "avc1.{}{}{}",
                hex_byte(nalu[1]),
                hex_byte(nalu[2]),
                hex_byte(nalu[3])
            ));
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct AnnexBUnit {
    start: usize,
    end: usize,
}

fn annex_b_units(frame: &[u8]) -> Vec<AnnexBUnit> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 < frame.len() {
        let len = start_code_len(frame, index);
        if len > 0 {
            starts.push(index);
            index += len;
        } else {
            index += 1;
        }
    }
    if starts.is_empty() && !frame.is_empty() {
        return vec![AnnexBUnit {
            start: 0,
            end: frame.len(),
        }];
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| AnnexBUnit {
            start: start + start_code_len(frame, *start),
            end: starts.get(index + 1).copied().unwrap_or(frame.len()),
        })
        .collect()
}

fn start_code_len(frame: &[u8], offset: usize) -> usize {
    if frame.get(offset) != Some(&0) || frame.get(offset + 1) != Some(&0) {
        return 0;
    }
    if frame.get(offset + 2) == Some(&1) {
        return 3;
    }
    if frame.get(offset + 2) == Some(&0) && frame.get(offset + 3) == Some(&1) {
        return 4;
    }
    0
}

fn hex_byte(value: u8) -> String {
    format!("{value:02X}")
}

fn set_u64(object: &Object, key: &str, value: u64) -> Result<(), JsValue> {
    Reflect::set(
        object,
        &JsValue::from_str(key),
        &JsValue::from_f64(value as f64),
    )?;
    Ok(())
}

fn set_option_u8(object: &Object, key: &str, value: Option<u8>) -> Result<(), JsValue> {
    Reflect::set(
        object,
        &JsValue::from_str(key),
        &value
            .map(|value| JsValue::from_f64(f64::from(value)))
            .unwrap_or(JsValue::NULL),
    )?;
    Ok(())
}

fn set_option_u16(object: &Object, key: &str, value: Option<u16>) -> Result<(), JsValue> {
    Reflect::set(
        object,
        &JsValue::from_str(key),
        &value
            .map(|value| JsValue::from_f64(f64::from(value)))
            .unwrap_or(JsValue::NULL),
    )?;
    Ok(())
}

fn set_option_u32(object: &Object, key: &str, value: Option<u32>) -> Result<(), JsValue> {
    Reflect::set(
        object,
        &JsValue::from_str(key),
        &value
            .map(|value| JsValue::from_f64(f64::from(value)))
            .unwrap_or(JsValue::NULL),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h264_codec_string_comes_from_annex_b_sps() {
        let frame = [0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f, 0xac, 0xd9];
        assert_eq!(h264_codec_string(&frame).as_deref(), Some("avc1.64001F"));
    }
}
