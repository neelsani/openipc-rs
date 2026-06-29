use js_sys::{Object, Reflect, Uint8Array};
use openipc_core::{Codec, DepacketizedFrame};
use wasm_bindgen::prelude::*;

pub(crate) fn video_frame_object(frame: DepacketizedFrame) -> Result<Object, JsValue> {
    let object = Object::new();
    let codec_string = codec_string(&frame);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h264_codec_string_comes_from_annex_b_sps() {
        let frame = [0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f, 0xac, 0xd9];
        assert_eq!(h264_codec_string(&frame).as_deref(), Some("avc1.64001F"));
    }
}
