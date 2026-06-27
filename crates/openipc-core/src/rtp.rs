#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtpError {
    TooShort,
    InvalidVersion(u8),
    InvalidExtension,
    InvalidPadding,
    EmptyPayload,
    UnsupportedPayload,
    FragmentOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
}

pub const RTP_PAYLOAD_TYPE_H264: u8 = 96;
pub const RTP_PAYLOAD_TYPE_H265: u8 = 97;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpHeader {
    pub marker: bool,
    pub payload_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub csrc_count: u8,
    pub has_extension: bool,
    pub header_len: usize,
    pub payload_len: usize,
}

impl RtpHeader {
    pub fn parse(packet: &[u8]) -> Result<Self, RtpError> {
        if packet.len() < 12 {
            return Err(RtpError::TooShort);
        }
        let version = packet[0] >> 6;
        if version != 2 {
            return Err(RtpError::InvalidVersion(version));
        }

        let padding = packet[0] & 0x20 != 0;
        let extension = packet[0] & 0x10 != 0;
        let csrc_count = packet[0] & 0x0f;
        let mut header_len = 12 + csrc_count as usize * 4;
        if packet.len() < header_len {
            return Err(RtpError::TooShort);
        }

        if extension {
            if packet.len() < header_len + 4 {
                return Err(RtpError::InvalidExtension);
            }
            let ext_words =
                u16::from_be_bytes([packet[header_len + 2], packet[header_len + 3]]) as usize;
            header_len += 4 + ext_words * 4;
            if packet.len() < header_len {
                return Err(RtpError::InvalidExtension);
            }
        }

        let padding_len = if padding {
            let len = *packet.last().ok_or(RtpError::InvalidPadding)? as usize;
            if len == 0 || len > packet.len() - header_len {
                return Err(RtpError::InvalidPadding);
            }
            len
        } else {
            0
        };

        let payload_len = packet.len() - header_len - padding_len;
        if payload_len == 0 {
            return Err(RtpError::EmptyPayload);
        }

        Ok(Self {
            marker: packet[1] & 0x80 != 0,
            payload_type: packet[1] & 0x7f,
            sequence_number: u16::from_be_bytes([packet[2], packet[3]]),
            timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
            ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
            csrc_count,
            has_extension: extension,
            header_len,
            payload_len,
        })
    }

    pub fn payload<'a>(&self, packet: &'a [u8]) -> &'a [u8] {
        &packet[self.header_len..self.header_len + self.payload_len]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepacketizedFrame {
    pub data: Vec<u8>,
    pub timestamp: u32,
    pub is_keyframe: bool,
    pub codec: Codec,
}

#[derive(Debug, Default, Clone)]
struct FragmentState {
    data: Vec<u8>,
    timestamp: u32,
}

#[derive(Debug, Clone)]
pub struct RtpDepacketizer {
    h264: FragmentState,
    h265: FragmentState,
    h264_sps: Option<Vec<u8>>,
    h264_pps: Option<Vec<u8>>,
    h265_vps: Option<Vec<u8>>,
    h265_sps: Option<Vec<u8>>,
    h265_pps: Option<Vec<u8>>,
    max_fragment: usize,
}

impl Default for RtpDepacketizer {
    fn default() -> Self {
        Self::new()
    }
}

impl RtpDepacketizer {
    pub fn new() -> Self {
        Self {
            h264: FragmentState::default(),
            h265: FragmentState::default(),
            h264_sps: None,
            h264_pps: None,
            h265_vps: None,
            h265_sps: None,
            h265_pps: None,
            max_fragment: 1024 * 1024,
        }
    }

    pub fn push(&mut self, packet: &[u8]) -> Result<Option<DepacketizedFrame>, RtpError> {
        let header = RtpHeader::parse(packet)?;
        let payload = header.payload(packet);
        let codec = codec_from_payload_type(header.payload_type)
            .or_else(|| detect_codec(payload))
            .ok_or(RtpError::UnsupportedPayload)?;
        match codec {
            Codec::H264 => self.push_h264(payload, header),
            Codec::H265 => self.push_h265(payload, header),
        }
    }

    fn push_h264(
        &mut self,
        payload: &[u8],
        header: RtpHeader,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let nal_type = payload[0] & 0x1f;
        match nal_type {
            7 => {
                self.h264_sps = Some(payload.to_vec());
                Ok(None)
            }
            8 => {
                self.h264_pps = Some(payload.to_vec());
                Ok(None)
            }
            24 => self.h264_stap_a(payload, header.timestamp),
            28 => self.h264_fu_a(payload, header.timestamp),
            _ if self.has_decoder_config(Codec::H264) => Ok(Some(self.frame_with_prefix(
                payload,
                header.timestamp,
                nal_type == 5,
                Codec::H264,
            ))),
            _ => Ok(None),
        }
    }

    fn push_h265(
        &mut self,
        payload: &[u8],
        header: RtpHeader,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        if payload.len() < 2 {
            return Err(RtpError::UnsupportedPayload);
        }
        let nal_type = (payload[0] >> 1) & 0x3f;
        match nal_type {
            32 => {
                self.h265_vps = Some(payload.to_vec());
                Ok(None)
            }
            33 => {
                self.h265_sps = Some(payload.to_vec());
                Ok(None)
            }
            34 => {
                self.h265_pps = Some(payload.to_vec());
                Ok(None)
            }
            48 => self.h265_ap(payload, header.timestamp),
            49 => self.h265_fu(payload, header.timestamp),
            _ if self.has_decoder_config(Codec::H265) => Ok(Some(self.frame_with_prefix(
                payload,
                header.timestamp,
                (16..=23).contains(&nal_type),
                Codec::H265,
            ))),
            _ => Ok(None),
        }
    }

    fn h264_fu_a(
        &mut self,
        payload: &[u8],
        timestamp: u32,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        if payload.len() < 2 {
            return Err(RtpError::UnsupportedPayload);
        }
        let fu_indicator = payload[0];
        let fu_header = payload[1];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        let nal_type = fu_header & 0x1f;
        if start {
            self.h264.data.clear();
            self.h264.timestamp = timestamp;
            self.h264.data.push((fu_indicator & 0xe0) | nal_type);
        }
        self.append_fragment(Codec::H264, &payload[2..])?;
        if end {
            if !self.has_decoder_config(Codec::H264) {
                self.h264.data.clear();
                return Ok(None);
            }
            let data = self.h264.data.clone();
            Ok(Some(self.frame_with_prefix(
                &data,
                self.h264.timestamp,
                nal_type == 5,
                Codec::H264,
            )))
        } else {
            Ok(None)
        }
    }

    fn h265_fu(
        &mut self,
        payload: &[u8],
        timestamp: u32,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        if payload.len() < 3 {
            return Err(RtpError::UnsupportedPayload);
        }
        let fu_header = payload[2];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        let nal_type = fu_header & 0x3f;
        if start {
            self.h265.data.clear();
            self.h265.timestamp = timestamp;
            self.h265.data.push((nal_type << 1) | (payload[0] & 0x01));
            self.h265.data.push(payload[1]);
        }
        self.append_fragment(Codec::H265, &payload[3..])?;
        if end {
            if !self.has_decoder_config(Codec::H265) {
                self.h265.data.clear();
                return Ok(None);
            }
            let data = self.h265.data.clone();
            Ok(Some(self.frame_with_prefix(
                &data,
                self.h265.timestamp,
                (16..=23).contains(&nal_type),
                Codec::H265,
            )))
        } else {
            Ok(None)
        }
    }

    fn h264_stap_a(
        &mut self,
        payload: &[u8],
        timestamp: u32,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let mut out = Vec::new();
        let mut offset = 1;
        let mut keyframe = false;
        let mut has_slice = false;
        while offset + 2 <= payload.len() {
            let len = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
            offset += 2;
            if offset + len > payload.len() {
                break;
            }
            let nalu = &payload[offset..offset + len];
            let nal_type = nalu.first().map(|b| b & 0x1f).unwrap_or(0);
            match nal_type {
                7 => self.h264_sps = Some(nalu.to_vec()),
                8 => self.h264_pps = Some(nalu.to_vec()),
                _ => {}
            }
            has_slice |= (1..=5).contains(&nal_type);
            keyframe |= nal_type == 5;
            append_annex_b(&mut out, nalu);
            offset += len;
        }
        if !has_slice || !self.has_decoder_config(Codec::H264) {
            return Ok(None);
        }
        Ok(Some(DepacketizedFrame {
            data: out,
            timestamp,
            is_keyframe: keyframe,
            codec: Codec::H264,
        }))
    }

    fn h265_ap(
        &mut self,
        payload: &[u8],
        timestamp: u32,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let mut out = Vec::new();
        let mut offset = 2;
        let mut keyframe = false;
        let mut has_slice = false;
        while offset + 2 <= payload.len() {
            let len = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
            offset += 2;
            if offset + len > payload.len() {
                break;
            }
            let nalu = &payload[offset..offset + len];
            let nal_type = nalu.first().map(|b| (b >> 1) & 0x3f).unwrap_or(0);
            match nal_type {
                32 => self.h265_vps = Some(nalu.to_vec()),
                33 => self.h265_sps = Some(nalu.to_vec()),
                34 => self.h265_pps = Some(nalu.to_vec()),
                _ => {}
            }
            has_slice |= !nalu.is_empty() && nal_type <= 31;
            keyframe |= (16..=23).contains(&nal_type);
            append_annex_b(&mut out, nalu);
            offset += len;
        }
        if !has_slice || !self.has_decoder_config(Codec::H265) {
            return Ok(None);
        }
        Ok(Some(DepacketizedFrame {
            data: out,
            timestamp,
            is_keyframe: keyframe,
            codec: Codec::H265,
        }))
    }

    fn append_fragment(&mut self, codec: Codec, bytes: &[u8]) -> Result<(), RtpError> {
        let state = match codec {
            Codec::H264 => &mut self.h264,
            Codec::H265 => &mut self.h265,
        };
        if state.data.len() + bytes.len() > self.max_fragment {
            return Err(RtpError::FragmentOverflow);
        }
        state.data.extend_from_slice(bytes);
        Ok(())
    }

    fn frame_with_prefix(
        &self,
        nalu: &[u8],
        timestamp: u32,
        is_keyframe: bool,
        codec: Codec,
    ) -> DepacketizedFrame {
        let mut data = Vec::new();
        if is_keyframe {
            match codec {
                Codec::H264 => {
                    if let Some(sps) = &self.h264_sps {
                        append_annex_b(&mut data, sps);
                    }
                    if let Some(pps) = &self.h264_pps {
                        append_annex_b(&mut data, pps);
                    }
                }
                Codec::H265 => {
                    if let Some(vps) = &self.h265_vps {
                        append_annex_b(&mut data, vps);
                    }
                    if let Some(sps) = &self.h265_sps {
                        append_annex_b(&mut data, sps);
                    }
                    if let Some(pps) = &self.h265_pps {
                        append_annex_b(&mut data, pps);
                    }
                }
            }
        }
        append_annex_b(&mut data, nalu);
        DepacketizedFrame {
            data,
            timestamp,
            is_keyframe,
            codec,
        }
    }

    fn has_decoder_config(&self, codec: Codec) -> bool {
        match codec {
            Codec::H264 => self.h264_sps.is_some() && self.h264_pps.is_some(),
            Codec::H265 => {
                self.h265_vps.is_some() && self.h265_sps.is_some() && self.h265_pps.is_some()
            }
        }
    }
}

fn codec_from_payload_type(payload_type: u8) -> Option<Codec> {
    match payload_type {
        RTP_PAYLOAD_TYPE_H264 => Some(Codec::H264),
        RTP_PAYLOAD_TYPE_H265 => Some(Codec::H265),
        _ => None,
    }
}

fn detect_codec(payload: &[u8]) -> Option<Codec> {
    if payload.is_empty() {
        return None;
    }
    if payload.len() >= 2 {
        let h265_nal_type = (payload[0] >> 1) & 0x3f;
        if h265_nal_type == 48 || h265_nal_type == 49 || (32..=40).contains(&h265_nal_type) {
            return Some(Codec::H265);
        }
    }
    let h264_nal_type = payload[0] & 0x1f;
    if h264_nal_type == 24 || h264_nal_type == 28 || (1..=12).contains(&h264_nal_type) {
        return Some(Codec::H264);
    }
    None
}

fn append_annex_b(out: &mut Vec<u8>, nalu: &[u8]) {
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(nalu);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtp(payload: &[u8], marker: bool, seq: u16, timestamp: u32) -> Vec<u8> {
        rtp_with_payload_type(payload, RTP_PAYLOAD_TYPE_H264, marker, seq, timestamp)
    }

    fn rtp_with_payload_type(
        payload: &[u8],
        payload_type: u8,
        marker: bool,
        seq: u16,
        timestamp: u32,
    ) -> Vec<u8> {
        let mut packet = vec![
            0x80,
            (if marker { 0x80 } else { 0x00 }) | (payload_type & 0x7f),
        ];
        packet.extend_from_slice(&seq.to_be_bytes());
        packet.extend_from_slice(&timestamp.to_be_bytes());
        packet.extend_from_slice(&0x1122_3344u32.to_be_bytes());
        packet.extend_from_slice(payload);
        packet
    }

    fn stap_a(units: &[&[u8]]) -> Vec<u8> {
        let mut payload = vec![24];
        for unit in units {
            payload.extend_from_slice(&(unit.len() as u16).to_be_bytes());
            payload.extend_from_slice(unit);
        }
        payload
    }

    fn prime_h264(depay: &mut RtpDepacketizer) {
        assert!(depay
            .push(&rtp(&[0x67, 0x64, 0x00, 0x1f], true, 1, 10))
            .unwrap()
            .is_none());
        assert!(depay
            .push(&rtp(&[0x68, 0xee], true, 2, 10))
            .unwrap()
            .is_none());
    }

    fn prime_h265(depay: &mut RtpDepacketizer) {
        for (seq, payload) in [
            (1, &[0x40, 0x01, 0xaa][..]),
            (2, &[0x42, 0x01, 0xbb][..]),
            (3, &[0x44, 0x01, 0xcc][..]),
        ] {
            assert!(depay
                .push(&rtp_with_payload_type(
                    payload,
                    RTP_PAYLOAD_TYPE_H265,
                    true,
                    seq,
                    10,
                ))
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn parses_rtp_header() {
        let packet = rtp(&[0x65, 1, 2], true, 7, 1234);
        let header = RtpHeader::parse(&packet).unwrap();
        assert!(header.marker);
        assert_eq!(header.payload_type, 96);
        assert_eq!(header.sequence_number, 7);
        assert_eq!(header.timestamp, 1234);
        assert_eq!(header.payload(&packet), &[0x65, 1, 2]);
    }

    #[test]
    fn depacketizes_h264_single_nalu_as_annex_b() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        let frame = depay
            .push(&rtp(&[0x65, 0xaa], true, 1, 42))
            .unwrap()
            .unwrap();
        assert_eq!(frame.codec, Codec::H264);
        assert!(frame.is_keyframe);
        assert_eq!(
            frame.data,
            [
                &[0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f][..],
                &[0, 0, 0, 1, 0x68, 0xee][..],
                &[0, 0, 0, 1, 0x65, 0xaa][..],
            ]
            .concat()
        );
    }

    #[test]
    fn drops_h264_video_until_sps_and_pps_are_seen() {
        let mut depay = RtpDepacketizer::new();
        assert!(depay
            .push(&rtp(&[0x65, 0xaa], true, 1, 42))
            .unwrap()
            .is_none());
    }

    #[test]
    fn h264_payload_type_prevents_h265_false_positive() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        let frame = depay
            .push(&rtp(&[0x41, 0xaa], true, 1, 42))
            .unwrap()
            .unwrap();
        assert_eq!(frame.codec, Codec::H264);
        assert!(!frame.is_keyframe);
        assert_eq!(frame.data, &[0, 0, 0, 1, 0x41, 0xaa]);
    }

    #[test]
    fn depacketizes_h265_single_nalu_by_payload_type() {
        let mut depay = RtpDepacketizer::new();
        prime_h265(&mut depay);
        let frame = depay
            .push(&rtp_with_payload_type(
                &[0x02, 0x01, 0xaa],
                RTP_PAYLOAD_TYPE_H265,
                true,
                1,
                42,
            ))
            .unwrap()
            .unwrap();
        assert_eq!(frame.codec, Codec::H265);
        assert!(!frame.is_keyframe);
        assert_eq!(frame.data, &[0, 0, 0, 1, 0x02, 0x01, 0xaa]);
    }

    #[test]
    fn h264_stap_a_caches_parameter_sets_for_later_keyframe() {
        let mut depay = RtpDepacketizer::new();
        let sps = &[0x67, 0x64, 0x00, 0x1f][..];
        let pps = &[0x68, 0xee][..];
        let aggregate = depay.push(&rtp(&stap_a(&[sps, pps]), true, 1, 10)).unwrap();
        assert!(aggregate.is_none());

        let frame = depay
            .push(&rtp(&[0x65, 0xaa], true, 2, 20))
            .unwrap()
            .unwrap();
        assert!(frame.is_keyframe);
        assert_eq!(
            frame.data,
            [
                &[0, 0, 0, 1][..],
                sps,
                &[0, 0, 0, 1][..],
                pps,
                &[0, 0, 0, 1, 0x65, 0xaa][..],
            ]
            .concat()
        );
    }

    #[test]
    fn depacketizes_h264_fu_a() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        assert!(depay
            .push(&rtp(&[0x7c, 0x85, 1, 2], false, 1, 99))
            .unwrap()
            .is_none());
        let frame = depay
            .push(&rtp(&[0x7c, 0x45, 3, 4], true, 2, 99))
            .unwrap()
            .unwrap();
        assert_eq!(
            frame.data,
            [
                &[0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f][..],
                &[0, 0, 0, 1, 0x68, 0xee][..],
                &[0, 0, 0, 1, 0x65, 1, 2, 3, 4][..],
            ]
            .concat()
        );
    }
}
