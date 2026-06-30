use super::*;

pub(crate) fn init_report_payload(report: InitReport) -> InitReportPayload {
    let status = match report.status {
        InitStatus::AlreadyRunning => "already_running",
        InitStatus::Initialized => "initialized",
    };
    InitReportPayload {
        chip: report.chip.family.name().to_owned(),
        rf_paths: report.chip.total_rf_paths(),
        cut_version: report.chip.cut_version,
        status: status.to_owned(),
        firmware_downloaded: report.firmware_downloaded,
    }
}

pub(crate) fn fec_counters_payload(counters: FecCounters) -> FecCountersPayload {
    FecCountersPayload {
        total_packets: counters.total_packets,
        recovered_packets: counters.recovered_packets,
        lost_packets: counters.lost_packets,
        bad_packets: counters.bad_packets,
    }
}

pub(crate) fn video_frame_payload(frame: DepacketizedFrame) -> VideoFramePayload {
    let codec_string = codec_string(&frame);
    VideoFramePayload {
        data_base64: BASE64.encode(&frame.data),
        codec: codec_name(frame.codec),
        codec_string,
        is_key_frame: frame.is_keyframe,
        timestamp: frame.timestamp,
    }
}

pub(crate) fn raw_payload_payload(payload: openipc_core::RoutePayload) -> RawPayloadPayload {
    RawPayloadPayload {
        data_base64: BASE64.encode(&payload.data),
        packet_seq: payload.packet_seq.to_string(),
        route_id: payload.route_id.raw(),
        channel_id: payload.channel_id.raw(),
    }
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

fn h264_codec_string(frame: &[u8]) -> Option<String> {
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
    let mut units = Vec::new();
    for (position, start_code) in starts.iter().enumerate() {
        let start = start_code + start_code_len(frame, *start_code);
        let end = starts.get(position + 1).copied().unwrap_or(frame.len());
        if start < end {
            units.push(AnnexBUnit { start, end });
        }
    }
    units
}

fn start_code_len(frame: &[u8], index: usize) -> usize {
    if index + 4 <= frame.len() && frame[index..index + 4] == [0, 0, 0, 1] {
        4
    } else if index + 3 <= frame.len() && frame[index..index + 3] == [0, 0, 1] {
        3
    } else {
        0
    }
}

fn hex_byte(byte: u8) -> String {
    format!("{byte:02X}")
}
