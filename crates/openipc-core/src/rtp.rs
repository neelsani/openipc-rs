/// Error returned while parsing or depacketizing RTP video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtpError {
    /// Packet is shorter than the RTP fixed header or declared extension.
    TooShort,
    /// RTP version was not 2.
    InvalidVersion(u8),
    /// RTP extension header length is malformed.
    InvalidExtension,
    /// RTP padding length is malformed.
    InvalidPadding,
    /// Packet has no payload after header/extension/padding.
    EmptyPayload,
    /// Payload could not be interpreted as H.264 or H.265.
    UnsupportedPayload,
    /// Fragmented access unit exceeded the configured size guard.
    FragmentOverflow,
}

/// Encoded video codec carried by a depacketized frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// H.264/AVC video.
    H264,
    /// H.265/HEVC video.
    H265,
}

/// Dynamic RTP payload type used by OpenIPC for H.264.
pub const RTP_PAYLOAD_TYPE_H264: u8 = 96;
/// Dynamic RTP payload type used by OpenIPC for H.265.
pub const RTP_PAYLOAD_TYPE_H265: u8 = 97;
/// Dynamic RTP payload type used by OpenIPC/Majestic for Opus audio.
pub const RTP_PAYLOAD_TYPE_OPUS: u8 = 98;

/// Parsed RTP header metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpHeader {
    /// RTP marker bit, usually set at an access-unit boundary.
    pub marker: bool,
    /// RTP payload type.
    pub payload_type: u8,
    /// RTP sequence number.
    pub sequence_number: u16,
    /// RTP timestamp.
    pub timestamp: u32,
    /// RTP synchronization source.
    pub ssrc: u32,
    /// Number of CSRC entries.
    pub csrc_count: u8,
    /// True if the packet has an RTP header extension.
    pub has_extension: bool,
    /// Header length including CSRC and extension bytes.
    pub header_len: usize,
    /// Payload length after header and padding removal.
    pub payload_len: usize,
}

impl RtpHeader {
    /// Parse an RTP header and validate extension/padding bounds.
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

    /// Borrow this packet's payload using the parsed header offsets.
    pub fn payload<'a>(&self, packet: &'a [u8]) -> &'a [u8] {
        &packet[self.header_len..self.header_len + self.payload_len]
    }
}

/// Encoded Annex-B H.264/H.265 access unit emitted by [`RtpDepacketizer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepacketizedFrame {
    /// Annex-B byte stream data including start codes.
    pub data: Vec<u8>,
    /// RTP timestamp associated with the access unit.
    pub timestamp: u32,
    /// True when the frame contains an IDR/keyframe entry point.
    pub is_keyframe: bool,
    /// Video codec for this frame.
    pub codec: Codec,
}

#[derive(Debug, Default, Clone)]
struct FragmentState {
    data: Vec<u8>,
    timestamp: u32,
    next_sequence: Option<u16>,
    corrupted: bool,
}

/// Stateful RTP depacketizer for OpenIPC H.264/H.265 video.
///
/// The depacketizer buffers fragmented NAL units, drops incomplete fragments
/// across sequence gaps, and emits complete Annex-B access units.
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
    /// Create a depacketizer with the default fragment-size guard.
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

    /// Push one RTP packet and return a complete frame when one is ready.
    pub fn push(&mut self, packet: &[u8]) -> Result<Option<DepacketizedFrame>, RtpError> {
        let header = RtpHeader::parse(packet)?;
        let payload = header.payload(packet);
        if header.payload_type == RTP_PAYLOAD_TYPE_OPUS {
            return Err(RtpError::UnsupportedPayload);
        }
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
            28 => self.h264_fu_a(payload, header),
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
            49 => self.h265_fu(payload, header),
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
        header: RtpHeader,
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
            self.h264.timestamp = header.timestamp;
            self.h264.next_sequence = Some(header.sequence_number.wrapping_add(1));
            self.h264.corrupted = false;
            self.h264.data.push((fu_indicator & 0xe0) | nal_type);
        } else if !self.accept_fragment_sequence(Codec::H264, header.sequence_number) {
            return Ok(None);
        }
        if !self.h264.corrupted {
            self.append_fragment(Codec::H264, &payload[2..])?;
        }
        if end {
            if self.h264.corrupted || !self.has_decoder_config(Codec::H264) {
                self.reset_fragment(Codec::H264);
                return Ok(None);
            }
            let data = self.h264.data.clone();
            let frame =
                self.frame_with_prefix(&data, self.h264.timestamp, nal_type == 5, Codec::H264);
            self.reset_fragment(Codec::H264);
            Ok(Some(frame))
        } else {
            Ok(None)
        }
    }

    fn h265_fu(
        &mut self,
        payload: &[u8],
        header: RtpHeader,
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
            self.h265.timestamp = header.timestamp;
            self.h265.next_sequence = Some(header.sequence_number.wrapping_add(1));
            self.h265.corrupted = false;
            self.h265.data.push((nal_type << 1) | (payload[0] & 0x01));
            self.h265.data.push(payload[1]);
        } else if !self.accept_fragment_sequence(Codec::H265, header.sequence_number) {
            return Ok(None);
        }
        if !self.h265.corrupted {
            self.append_fragment(Codec::H265, &payload[3..])?;
        }
        if end {
            if self.h265.corrupted || !self.has_decoder_config(Codec::H265) {
                self.reset_fragment(Codec::H265);
                return Ok(None);
            }
            let data = self.h265.data.clone();
            let frame = self.frame_with_prefix(
                &data,
                self.h265.timestamp,
                (16..=23).contains(&nal_type),
                Codec::H265,
            );
            self.reset_fragment(Codec::H265);
            Ok(Some(frame))
        } else {
            Ok(None)
        }
    }

    fn accept_fragment_sequence(&mut self, codec: Codec, sequence_number: u16) -> bool {
        let state = match codec {
            Codec::H264 => &mut self.h264,
            Codec::H265 => &mut self.h265,
        };
        let Some(expected) = state.next_sequence else {
            return false;
        };
        state.next_sequence = Some(sequence_number.wrapping_add(1));
        if sequence_number != expected {
            state.data.clear();
            state.corrupted = true;
            return false;
        }
        true
    }

    fn reset_fragment(&mut self, codec: Codec) {
        let state = match codec {
            Codec::H264 => &mut self.h264,
            Codec::H265 => &mut self.h265,
        };
        state.data.clear();
        state.next_sequence = None;
        state.corrupted = false;
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
    fn opus_payload_type_is_not_sniffed_as_video() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        let err = depay
            .push(&rtp_with_payload_type(
                &[0x65, 0xaa],
                RTP_PAYLOAD_TYPE_OPUS,
                true,
                1,
                42,
            ))
            .unwrap_err();
        assert_eq!(err, RtpError::UnsupportedPayload);
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

    #[test]
    fn drops_h264_fu_a_after_sequence_gap() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        assert!(depay
            .push(&rtp(&[0x7c, 0x85, 1, 2], false, 10, 99))
            .unwrap()
            .is_none());
        assert!(depay
            .push(&rtp(&[0x7c, 0x45, 3, 4], true, 12, 99))
            .unwrap()
            .is_none());

        assert!(depay
            .push(&rtp(&[0x7c, 0x85, 5, 6], false, 13, 100))
            .unwrap()
            .is_none());
        let frame = depay
            .push(&rtp(&[0x7c, 0x45, 7, 8], true, 14, 100))
            .unwrap()
            .unwrap();
        assert!(frame.data.ends_with(&[0, 0, 0, 1, 0x65, 5, 6, 7, 8]));
    }

    #[test]
    fn drops_fragment_end_without_start() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        assert!(depay
            .push(&rtp(&[0x7c, 0x45, 1, 2], true, 10, 99))
            .unwrap()
            .is_none());
    }
}
