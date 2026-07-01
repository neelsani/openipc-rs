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
        payload_type: frame.payload_type,
        sequence_number: frame.sequence_number,
        nal_type: frame.nal_type,
        decoder_config_complete: frame.codec_config.is_complete_for(frame.codec),
        codec_config: codec_config_payload(frame.codec_config),
    }
}

pub(crate) fn video_frame_binary(frame: &DepacketizedFrame) -> Vec<u8> {
    let codec_string = codec_string(frame);
    let config = codec_config_payload(frame.codec_config);
    let mut config_flags = 0u8;
    config_flags |= u8::from(config.h264_sps);
    config_flags |= u8::from(config.h264_pps) << 1;
    config_flags |= u8::from(config.h265_vps) << 2;
    config_flags |= u8::from(config.h265_sps) << 3;
    config_flags |= u8::from(config.h265_pps) << 4;
    let codec_bytes = codec_string.as_bytes();
    let codec_len = codec_bytes.len().min(u8::MAX as usize);
    let mut output = Vec::with_capacity(17 + codec_len + frame.data.len());
    output.extend_from_slice(b"OIPC");
    output.push(1);
    output.push(match frame.codec {
        Codec::H264 => 0,
        Codec::H265 => 1,
    });
    output.push(u8::from(frame.is_keyframe));
    output.push(frame.payload_type);
    output.extend_from_slice(&frame.sequence_number.to_be_bytes());
    output.extend_from_slice(&frame.timestamp.to_be_bytes());
    output.push(frame.nal_type);
    output.push(config_flags);
    output.push(codec_len as u8);
    output.extend_from_slice(&codec_bytes[..codec_len]);
    output.extend_from_slice(&frame.data);
    output
}

pub(crate) fn rtp_status_payload(
    status: RtpDepacketizerStatus,
    reorder: RtpReorderStatus,
) -> RtpStatusPayload {
    RtpStatusPayload {
        packets: status.packets,
        frames_emitted: status.frames_emitted,
        config_wait_drops: status.config_wait_drops,
        keyframes_with_prepended_config: status.keyframes_with_prepended_config,
        parameter_sets_prepended: status.parameter_sets_prepended,
        fragment_sequence_gaps: status.fragment_sequence_gaps,
        fragment_overflows: status.fragment_overflows,
        unsupported_payloads: status.unsupported_payloads,
        malformed_packets: status.malformed_packets,
        last_payload_type: status.last_payload_type,
        last_sequence_number: status.last_sequence_number,
        last_timestamp: status.last_timestamp,
        last_codec: status.last_codec.map(codec_name),
        last_nal_type: status.last_nal_type,
        codec_config: codec_config_payload(status.codec_config),
        h264_config_complete: status.codec_config.is_complete_for(Codec::H264),
        h265_config_complete: status.codec_config.is_complete_for(Codec::H265),
        reorder_buffered_packets: reorder.buffered_packets,
        reordered_packets: reorder.reordered_packets,
        late_packets: reorder.late_packets,
        forced_flushes: reorder.forced_flushes,
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

fn codec_config_payload(config: CodecConfigState) -> CodecConfigPayload {
    CodecConfigPayload {
        h264_sps: config.h264_sps,
        h264_pps: config.h264_pps,
        h265_vps: config.h265_vps,
        h265_sps: config.h265_sps,
        h265_pps: config.h265_pps,
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
