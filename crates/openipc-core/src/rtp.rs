use std::collections::BTreeMap;

const DEFAULT_RTP_REORDER_WINDOW: usize = 15;
const DEFAULT_MAX_ACCESS_UNIT_SIZE: usize = 8 * 1024 * 1024;

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

/// Decoder configuration NAL units observed by the RTP depacketizer.
///
/// H.264 needs SPS and PPS before a decoder can be configured. H.265 needs
/// VPS, SPS and PPS. PixelPilot starts its decoder as soon as these parameter
/// sets have been observed, then feeds subsequent NAL units without requiring a
/// fresh IDR for every startup path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodecConfigState {
    /// H.264 sequence parameter set has been seen.
    pub h264_sps: bool,
    /// H.264 picture parameter set has been seen.
    pub h264_pps: bool,
    /// H.265 video parameter set has been seen.
    pub h265_vps: bool,
    /// H.265 sequence parameter set has been seen.
    pub h265_sps: bool,
    /// H.265 picture parameter set has been seen.
    pub h265_pps: bool,
}

impl CodecConfigState {
    /// Return true when all parameter sets required for `codec` are cached.
    pub const fn is_complete_for(self, codec: Codec) -> bool {
        match codec {
            Codec::H264 => self.h264_sps && self.h264_pps,
            Codec::H265 => self.h265_vps && self.h265_sps && self.h265_pps,
        }
    }
}

/// Cumulative RTP depacketizer diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RtpDepacketizerStatus {
    /// RTP packets submitted to the depacketizer.
    pub packets: u64,
    /// Annex-B video frames emitted.
    pub frames_emitted: u64,
    /// Video NAL units dropped because decoder config was not complete yet.
    pub config_wait_drops: u64,
    /// Keyframes emitted with cached decoder config prepended.
    pub keyframes_with_prepended_config: u64,
    /// Cached SPS/PPS/VPS parameter-set NAL units prepended to keyframes.
    pub parameter_sets_prepended: u64,
    /// Fragment chains dropped because an RTP sequence gap was observed.
    pub fragment_sequence_gaps: u64,
    /// Fragment chains that exceeded the configured size guard.
    pub fragment_overflows: u64,
    /// Packets rejected because they were not H.264/H.265 video.
    pub unsupported_payloads: u64,
    /// Packets rejected because the RTP header or payload was malformed.
    pub malformed_packets: u64,
    /// Most recent RTP payload type.
    pub last_payload_type: Option<u8>,
    /// Most recent RTP sequence number.
    pub last_sequence_number: Option<u16>,
    /// Most recent RTP timestamp.
    pub last_timestamp: Option<u32>,
    /// Most recent detected video codec.
    pub last_codec: Option<Codec>,
    /// Most recent H.264/H.265 NAL unit type.
    pub last_nal_type: Option<u8>,
    /// Current decoder configuration state.
    pub codec_config: CodecConfigState,
}

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

/// Cumulative status for [`RtpReorderBuffer`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RtpReorderStatus {
    /// Packets currently held while waiting for missing sequence numbers.
    pub buffered_packets: usize,
    /// Out-of-order packets accepted into the reorder window.
    pub reordered_packets: u64,
    /// Packets dropped because their sequence number was older than the window.
    pub late_packets: u64,
    /// Times the window flushed ahead after the missing packet did not arrive.
    pub forced_flushes: u64,
}

/// Small RTP sequence reorder buffer.
///
/// PixelPilot keeps a short queue before its RTP parser so FU-A/FU fragments
/// survive small USB/radio delivery inversions. This buffer does the same for
/// the shared Rust receiver runtime while keeping the in-order path immediate.
#[derive(Debug, Clone)]
pub struct RtpReorderBuffer {
    next_sequence: Option<u16>,
    pending: BTreeMap<u16, Vec<u8>>,
    max_depth: usize,
    status: RtpReorderStatus,
}

impl Default for RtpReorderBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_RTP_REORDER_WINDOW)
    }
}

impl RtpReorderBuffer {
    /// Create a reorder buffer with a maximum pending packet depth.
    pub fn new(max_depth: usize) -> Self {
        Self {
            next_sequence: None,
            pending: BTreeMap::new(),
            max_depth: max_depth.max(1),
            status: RtpReorderStatus::default(),
        }
    }

    /// Push one RTP packet and return packets that are ready in sequence order.
    pub fn push(&mut self, packet: &[u8]) -> Result<Vec<Vec<u8>>, RtpError> {
        let header = RtpHeader::parse(packet)?;
        let sequence = header.sequence_number;
        let mut ready = Vec::new();

        let Some(expected) = self.next_sequence else {
            self.next_sequence = Some(sequence.wrapping_add(1));
            ready.push(packet.to_vec());
            return Ok(ready);
        };

        if sequence == expected {
            self.next_sequence = Some(expected.wrapping_add(1));
            ready.push(packet.to_vec());
            self.drain_ready(&mut ready);
            return Ok(ready);
        }

        if sequence_is_before(sequence, expected) {
            self.status.late_packets = self.status.late_packets.saturating_add(1);
            return Ok(ready);
        }

        if self.pending.insert(sequence, packet.to_vec()).is_none() {
            self.status.reordered_packets = self.status.reordered_packets.saturating_add(1);
        }
        if self.pending.len() >= self.max_depth {
            self.force_flush(expected, &mut ready);
        }
        self.status.buffered_packets = self.pending.len();
        Ok(ready)
    }

    /// Return current reorder-buffer status.
    pub fn status(&self) -> RtpReorderStatus {
        RtpReorderStatus {
            buffered_packets: self.pending.len(),
            ..self.status
        }
    }

    fn drain_ready(&mut self, ready: &mut Vec<Vec<u8>>) {
        while let Some(expected) = self.next_sequence {
            let Some(packet) = self.pending.remove(&expected) else {
                break;
            };
            self.next_sequence = Some(expected.wrapping_add(1));
            ready.push(packet);
        }
        self.status.buffered_packets = self.pending.len();
    }

    fn force_flush(&mut self, expected: u16, ready: &mut Vec<Vec<u8>>) {
        let Some(sequence) = self
            .pending
            .keys()
            .copied()
            .min_by_key(|sequence| sequence.wrapping_sub(expected))
        else {
            return;
        };
        if let Some(packet) = self.pending.remove(&sequence) {
            self.status.forced_flushes = self.status.forced_flushes.saturating_add(1);
            self.next_sequence = Some(sequence.wrapping_add(1));
            ready.push(packet);
            self.drain_ready(ready);
        }
    }
}

fn sequence_is_before(sequence: u16, expected: u16) -> bool {
    let backward = expected.wrapping_sub(sequence);
    backward != 0 && backward < 0x8000
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
    /// RTP payload type that produced this frame.
    pub payload_type: u8,
    /// RTP sequence number of the packet that completed this frame.
    pub sequence_number: u16,
    /// H.264/H.265 NAL unit type for the frame payload.
    pub nal_type: u8,
    /// Decoder parameter-set state at the time this frame was emitted.
    pub codec_config: CodecConfigState,
}

#[derive(Debug, Default, Clone)]
struct FragmentState {
    data: Vec<u8>,
    timestamp: u32,
    next_sequence: Option<u16>,
    corrupted: bool,
}

#[derive(Debug, Default, Clone)]
struct AccessUnitState {
    data: Vec<u8>,
    timestamp: Option<u32>,
    next_sequence: Option<u16>,
    corrupted: bool,
    is_keyframe: bool,
    has_decoder_config: bool,
    nal_type: u8,
}

#[derive(Debug, Clone, Copy)]
struct FrameMeta {
    timestamp: u32,
    is_keyframe: bool,
    codec: Codec,
    payload_type: u8,
    sequence_number: u16,
    nal_type: u8,
}

/// Stateful RTP depacketizer for OpenIPC H.264/H.265 video.
///
/// The depacketizer buffers fragmented NAL units, drops incomplete fragments
/// across sequence gaps, and emits complete Annex-B access units.
#[derive(Debug, Clone)]
pub struct RtpDepacketizer {
    h264: FragmentState,
    h265: FragmentState,
    h264_access_unit: AccessUnitState,
    h265_access_unit: AccessUnitState,
    h264_sps: Option<Vec<u8>>,
    h264_pps: Option<Vec<u8>>,
    h265_vps: Option<Vec<u8>>,
    h265_sps: Option<Vec<u8>>,
    h265_pps: Option<Vec<u8>>,
    max_fragment: usize,
    status: RtpDepacketizerStatus,
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
            h264_access_unit: AccessUnitState::default(),
            h265_access_unit: AccessUnitState::default(),
            h264_sps: None,
            h264_pps: None,
            h265_vps: None,
            h265_sps: None,
            h265_pps: None,
            max_fragment: DEFAULT_MAX_ACCESS_UNIT_SIZE,
            status: RtpDepacketizerStatus::default(),
        }
    }

    /// Return cumulative depacketizer status and codec configuration state.
    pub fn status(&self) -> RtpDepacketizerStatus {
        RtpDepacketizerStatus {
            codec_config: self.codec_config(),
            ..self.status
        }
    }

    /// Return the current decoder parameter-set state.
    pub fn codec_config(&self) -> CodecConfigState {
        CodecConfigState {
            h264_sps: self.h264_sps.is_some(),
            h264_pps: self.h264_pps.is_some(),
            h265_vps: self.h265_vps.is_some(),
            h265_sps: self.h265_sps.is_some(),
            h265_pps: self.h265_pps.is_some(),
        }
    }

    /// Push one RTP packet and return a complete frame when one is ready.
    pub fn push(&mut self, packet: &[u8]) -> Result<Option<DepacketizedFrame>, RtpError> {
        self.status.packets = self.status.packets.saturating_add(1);
        let header = match RtpHeader::parse(packet) {
            Ok(header) => header,
            Err(err) => {
                self.record_error(err);
                return Err(err);
            }
        };
        self.status.last_payload_type = Some(header.payload_type);
        self.status.last_sequence_number = Some(header.sequence_number);
        self.status.last_timestamp = Some(header.timestamp);
        let payload = header.payload(packet);
        log::trace!(
            target: "openipc_core::rtp",
            "received RTP packet sequence={} timestamp={} pt={} marker={} bytes={}",
            header.sequence_number,
            header.timestamp,
            header.payload_type,
            header.marker,
            payload.len()
        );
        if header.payload_type == RTP_PAYLOAD_TYPE_OPUS {
            self.record_error(RtpError::UnsupportedPayload);
            return Err(RtpError::UnsupportedPayload);
        }
        let Some(codec) =
            codec_from_payload_type(header.payload_type).or_else(|| detect_codec(payload))
        else {
            self.record_error(RtpError::UnsupportedPayload);
            return Err(RtpError::UnsupportedPayload);
        };
        self.status.last_codec = Some(codec);
        self.observe_access_unit_packet(codec, header);
        let result = match codec {
            Codec::H264 => self.push_h264(payload, header),
            Codec::H265 => self.push_h265(payload, header),
        };
        match &result {
            Ok(Some(_)) => {
                self.status.frames_emitted = self.status.frames_emitted.saturating_add(1)
            }
            Err(err) => {
                log::debug!(
                    target: "openipc_core::rtp",
                    "RTP packet rejected sequence={}: {err:?}",
                    header.sequence_number
                );
                self.record_error(*err);
            }
            _ => {}
        }
        result
    }

    fn push_h264(
        &mut self,
        payload: &[u8],
        header: RtpHeader,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let nal_type = payload[0] & 0x1f;
        self.status.last_nal_type = Some(nal_type);
        match nal_type {
            7 => {
                self.h264_sps = Some(payload.to_vec());
                Ok(None)
            }
            8 => {
                self.h264_pps = Some(payload.to_vec());
                Ok(None)
            }
            24 => self.h264_stap_a(payload, header),
            28 => self.h264_fu_a(payload, header),
            _ if self.has_decoder_config(Codec::H264) && is_h264_vcl_nal(nal_type) => self
                .push_complete_nalu(
                    payload,
                    FrameMeta {
                        timestamp: header.timestamp,
                        is_keyframe: nal_type == 5,
                        codec: Codec::H264,
                        payload_type: header.payload_type,
                        sequence_number: header.sequence_number,
                        nal_type,
                    },
                    header.marker,
                ),
            _ if !is_h264_vcl_nal(nal_type) => Ok(None),
            _ => {
                self.status.config_wait_drops = self.status.config_wait_drops.saturating_add(1);
                Ok(None)
            }
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
        self.status.last_nal_type = Some(nal_type);
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
            48 => self.h265_ap(payload, header),
            49 => self.h265_fu(payload, header),
            _ if self.has_decoder_config(Codec::H265) && is_h265_vcl_nal(nal_type) => self
                .push_complete_nalu(
                    payload,
                    FrameMeta {
                        timestamp: header.timestamp,
                        is_keyframe: (16..=23).contains(&nal_type),
                        codec: Codec::H265,
                        payload_type: header.payload_type,
                        sequence_number: header.sequence_number,
                        nal_type,
                    },
                    header.marker,
                ),
            _ if !is_h265_vcl_nal(nal_type) => Ok(None),
            _ => {
                self.status.config_wait_drops = self.status.config_wait_drops.saturating_add(1);
                Ok(None)
            }
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
            if !is_h264_vcl_nal(nal_type) {
                self.reset_fragment(Codec::H264);
                return Ok(None);
            }
            if self.h264.corrupted || !self.has_decoder_config(Codec::H264) {
                if !self.has_decoder_config(Codec::H264) {
                    self.status.config_wait_drops = self.status.config_wait_drops.saturating_add(1);
                }
                self.reset_fragment(Codec::H264);
                return Ok(None);
            }
            let data = std::mem::take(&mut self.h264.data);
            let meta = FrameMeta {
                timestamp: self.h264.timestamp,
                is_keyframe: nal_type == 5,
                codec: Codec::H264,
                payload_type: header.payload_type,
                sequence_number: header.sequence_number,
                nal_type,
            };
            self.reset_fragment(Codec::H264);
            self.push_complete_owned_nalu(data, meta, header.marker)
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
            if !is_h265_vcl_nal(nal_type) {
                self.reset_fragment(Codec::H265);
                return Ok(None);
            }
            if self.h265.corrupted || !self.has_decoder_config(Codec::H265) {
                if !self.has_decoder_config(Codec::H265) {
                    self.status.config_wait_drops = self.status.config_wait_drops.saturating_add(1);
                }
                self.reset_fragment(Codec::H265);
                return Ok(None);
            }
            let data = std::mem::take(&mut self.h265.data);
            let meta = FrameMeta {
                timestamp: self.h265.timestamp,
                is_keyframe: (16..=23).contains(&nal_type),
                codec: Codec::H265,
                payload_type: header.payload_type,
                sequence_number: header.sequence_number,
                nal_type,
            };
            self.reset_fragment(Codec::H265);
            self.push_complete_owned_nalu(data, meta, header.marker)
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
            self.status.fragment_sequence_gaps =
                self.status.fragment_sequence_gaps.saturating_add(1);
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
        header: RtpHeader,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let mut out = Vec::new();
        let mut offset = 1;
        let mut keyframe = false;
        let mut has_slice = false;
        let mut has_sps = false;
        let mut has_pps = false;
        let mut last_slice_type = 0;
        while offset + 2 <= payload.len() {
            let len = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
            offset += 2;
            if len == 0 || offset.saturating_add(len) > payload.len() {
                return Err(RtpError::UnsupportedPayload);
            }
            let nalu = &payload[offset..offset + len];
            let nal_type = nalu.first().map(|b| b & 0x1f).unwrap_or(0);
            self.status.last_nal_type = Some(nal_type);
            match nal_type {
                7 => {
                    has_sps = true;
                    self.h264_sps = Some(nalu.to_vec());
                }
                8 => {
                    has_pps = true;
                    self.h264_pps = Some(nalu.to_vec());
                }
                _ => {}
            }
            if is_h264_vcl_nal(nal_type) {
                has_slice = true;
                keyframe |= nal_type == 5;
                last_slice_type = nal_type;
            }
            append_annex_b(&mut out, nalu);
            offset += len;
        }
        if offset != payload.len() {
            return Err(RtpError::UnsupportedPayload);
        }
        if !has_slice || !self.has_decoder_config(Codec::H264) {
            if has_slice {
                self.status.config_wait_drops = self.status.config_wait_drops.saturating_add(1);
            }
            return Ok(None);
        }
        self.push_complete_owned_annex_b(
            out,
            FrameMeta {
                timestamp: header.timestamp,
                is_keyframe: keyframe,
                codec: Codec::H264,
                payload_type: header.payload_type,
                sequence_number: header.sequence_number,
                nal_type: last_slice_type,
            },
            header.marker,
            has_sps && has_pps,
        )
    }

    fn h265_ap(
        &mut self,
        payload: &[u8],
        header: RtpHeader,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let mut out = Vec::new();
        let mut offset = 2;
        let mut keyframe = false;
        let mut has_slice = false;
        let mut has_vps = false;
        let mut has_sps = false;
        let mut has_pps = false;
        let mut last_slice_type = 0;
        while offset + 2 <= payload.len() {
            let len = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
            offset += 2;
            if len == 0 || offset.saturating_add(len) > payload.len() {
                return Err(RtpError::UnsupportedPayload);
            }
            let nalu = &payload[offset..offset + len];
            let nal_type = nalu.first().map(|b| (b >> 1) & 0x3f).unwrap_or(0);
            self.status.last_nal_type = Some(nal_type);
            match nal_type {
                32 => {
                    has_vps = true;
                    self.h265_vps = Some(nalu.to_vec());
                }
                33 => {
                    has_sps = true;
                    self.h265_sps = Some(nalu.to_vec());
                }
                34 => {
                    has_pps = true;
                    self.h265_pps = Some(nalu.to_vec());
                }
                _ => {}
            }
            if is_h265_vcl_nal(nal_type) {
                has_slice = true;
                keyframe |= (16..=23).contains(&nal_type);
                last_slice_type = nal_type;
            }
            append_annex_b(&mut out, nalu);
            offset += len;
        }
        if offset != payload.len() {
            return Err(RtpError::UnsupportedPayload);
        }
        if !has_slice || !self.has_decoder_config(Codec::H265) {
            if has_slice {
                self.status.config_wait_drops = self.status.config_wait_drops.saturating_add(1);
            }
            return Ok(None);
        }
        self.push_complete_owned_annex_b(
            out,
            FrameMeta {
                timestamp: header.timestamp,
                is_keyframe: keyframe,
                codec: Codec::H265,
                payload_type: header.payload_type,
                sequence_number: header.sequence_number,
                nal_type: last_slice_type,
            },
            header.marker,
            has_vps && has_sps && has_pps,
        )
    }

    fn append_fragment(&mut self, codec: Codec, bytes: &[u8]) -> Result<(), RtpError> {
        let state = match codec {
            Codec::H264 => &mut self.h264,
            Codec::H265 => &mut self.h265,
        };
        if state.data.len() + bytes.len() > self.max_fragment {
            self.status.fragment_overflows = self.status.fragment_overflows.saturating_add(1);
            return Err(RtpError::FragmentOverflow);
        }
        state.data.extend_from_slice(bytes);
        Ok(())
    }

    fn push_complete_nalu(
        &mut self,
        nalu: &[u8],
        meta: FrameMeta,
        marker: bool,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let mut owned = Vec::with_capacity(nalu.len());
        owned.extend_from_slice(nalu);
        self.push_complete_owned_nalu(owned, meta, marker)
    }

    fn push_complete_owned_nalu(
        &mut self,
        nalu: Vec<u8>,
        meta: FrameMeta,
        marker: bool,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let mut data = Vec::with_capacity(nalu.len().saturating_add(4));
        append_annex_b(&mut data, &nalu);
        self.push_complete_owned_annex_b(data, meta, marker, false)
    }

    fn push_complete_owned_annex_b(
        &mut self,
        annex_b: Vec<u8>,
        meta: FrameMeta,
        marker: bool,
        has_decoder_config: bool,
    ) -> Result<Option<DepacketizedFrame>, RtpError> {
        let max_fragment = self.max_fragment;
        let state = match meta.codec {
            Codec::H264 => &mut self.h264_access_unit,
            Codec::H265 => &mut self.h265_access_unit,
        };
        debug_assert_eq!(state.timestamp, Some(meta.timestamp));
        if state.corrupted {
            if marker {
                reset_access_unit_state(state);
            }
            return Ok(None);
        }
        if state.data.len().saturating_add(annex_b.len()) > max_fragment {
            reset_access_unit_state(state);
            self.status.fragment_overflows = self.status.fragment_overflows.saturating_add(1);
            return Err(RtpError::FragmentOverflow);
        }
        state.data.extend_from_slice(&annex_b);
        state.is_keyframe |= meta.is_keyframe;
        state.has_decoder_config |= has_decoder_config;
        state.nal_type = meta.nal_type;
        if !marker {
            return Ok(None);
        }

        let mut data = std::mem::take(&mut state.data);
        let is_keyframe = state.is_keyframe;
        let has_decoder_config = state.has_decoder_config;
        let nal_type = state.nal_type;
        reset_access_unit_state(state);
        if is_keyframe && !has_decoder_config {
            let mut prefixed = Vec::with_capacity(data.len() + self.cached_config_len(meta.codec));
            self.prepend_cached_config(&mut prefixed, meta.codec);
            prefixed.append(&mut data);
            data = prefixed;
        }
        Ok(Some(DepacketizedFrame {
            data,
            timestamp: meta.timestamp,
            is_keyframe,
            codec: meta.codec,
            payload_type: meta.payload_type,
            sequence_number: meta.sequence_number,
            nal_type,
            codec_config: self.codec_config(),
        }))
    }

    fn observe_access_unit_packet(&mut self, codec: Codec, header: RtpHeader) {
        let state = match codec {
            Codec::H264 => &mut self.h264_access_unit,
            Codec::H265 => &mut self.h265_access_unit,
        };
        if state
            .timestamp
            .is_some_and(|timestamp| timestamp != header.timestamp)
        {
            if !state.data.is_empty() {
                self.status.fragment_sequence_gaps =
                    self.status.fragment_sequence_gaps.saturating_add(1);
            }
            reset_access_unit_state(state);
        }
        if state.timestamp.is_none() {
            state.timestamp = Some(header.timestamp);
        } else if state
            .next_sequence
            .is_some_and(|expected| expected != header.sequence_number)
        {
            if !state.corrupted {
                self.status.fragment_sequence_gaps =
                    self.status.fragment_sequence_gaps.saturating_add(1);
            }
            state.corrupted = true;
            state.data.clear();
        }
        state.next_sequence = Some(header.sequence_number.wrapping_add(1));
    }

    fn cached_config_len(&self, codec: Codec) -> usize {
        match codec {
            Codec::H264 => {
                self.h264_sps.as_ref().map_or(0, Vec::len)
                    + self.h264_pps.as_ref().map_or(0, Vec::len)
                    + 8
            }
            Codec::H265 => {
                self.h265_vps.as_ref().map_or(0, Vec::len)
                    + self.h265_sps.as_ref().map_or(0, Vec::len)
                    + self.h265_pps.as_ref().map_or(0, Vec::len)
                    + 12
            }
        }
    }

    fn prepend_cached_config(&mut self, data: &mut Vec<u8>, codec: Codec) {
        let mut prepended = 0u64;
        match codec {
            Codec::H264 => {
                if let Some(sps) = &self.h264_sps {
                    append_annex_b(data, sps);
                    prepended += 1;
                }
                if let Some(pps) = &self.h264_pps {
                    append_annex_b(data, pps);
                    prepended += 1;
                }
            }
            Codec::H265 => {
                if let Some(vps) = &self.h265_vps {
                    append_annex_b(data, vps);
                    prepended += 1;
                }
                if let Some(sps) = &self.h265_sps {
                    append_annex_b(data, sps);
                    prepended += 1;
                }
                if let Some(pps) = &self.h265_pps {
                    append_annex_b(data, pps);
                    prepended += 1;
                }
            }
        }
        if prepended > 0 {
            self.status.keyframes_with_prepended_config = self
                .status
                .keyframes_with_prepended_config
                .saturating_add(1);
            self.status.parameter_sets_prepended = self
                .status
                .parameter_sets_prepended
                .saturating_add(prepended);
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

    fn record_error(&mut self, err: RtpError) {
        match err {
            RtpError::UnsupportedPayload => {
                self.status.unsupported_payloads =
                    self.status.unsupported_payloads.saturating_add(1);
            }
            RtpError::FragmentOverflow => {}
            _ => {
                self.status.malformed_packets = self.status.malformed_packets.saturating_add(1);
            }
        }
    }
}

fn reset_access_unit_state(state: &mut AccessUnitState) {
    state.data.clear();
    state.timestamp = None;
    state.next_sequence = None;
    state.corrupted = false;
    state.is_keyframe = false;
    state.has_decoder_config = false;
    state.nal_type = 0;
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

fn is_h264_vcl_nal(nal_type: u8) -> bool {
    (1..=5).contains(&nal_type)
}

fn is_h265_vcl_nal(nal_type: u8) -> bool {
    nal_type <= 31
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

    fn h265_ap(units: &[&[u8]]) -> Vec<u8> {
        let mut payload = vec![0x60, 0x01];
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
    fn combines_same_timestamp_h264_slices_until_marker() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        assert!(depay
            .push(&rtp(&[0x41, 0x80, 0xaa], false, 3, 42))
            .unwrap()
            .is_none());
        let frame = depay
            .push(&rtp(&[0x41, 0x40, 0xbb], true, 4, 42))
            .unwrap()
            .unwrap();

        assert_eq!(
            frame.data,
            [
                &[0, 0, 0, 1, 0x41, 0x80, 0xaa][..],
                &[0, 0, 0, 1, 0x41, 0x40, 0xbb][..],
            ]
            .concat()
        );
        assert_eq!(frame.timestamp, 42);
        assert!(!frame.is_keyframe);
    }

    #[test]
    fn drops_access_unit_after_sequence_gap() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        assert!(depay
            .push(&rtp(&[0x41, 0x80, 0xaa], false, 3, 42))
            .unwrap()
            .is_none());
        assert!(depay
            .push(&rtp(&[0x41, 0x40, 0xbb], true, 5, 42))
            .unwrap()
            .is_none());

        assert_eq!(depay.status().fragment_sequence_gaps, 1);
        assert!(depay
            .push(&rtp(&[0x41, 0xcc], true, 6, 43))
            .unwrap()
            .is_some());
    }

    #[test]
    fn drops_h264_video_until_sps_and_pps_are_seen() {
        let mut depay = RtpDepacketizer::new();
        assert!(depay
            .push(&rtp(&[0x65, 0xaa], true, 1, 42))
            .unwrap()
            .is_none());
        let status = depay.status();
        assert_eq!(status.config_wait_drops, 1);
        assert!(!status.codec_config.is_complete_for(Codec::H264));
        assert_eq!(status.last_nal_type, Some(5));
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
    fn h264_non_vcl_nal_is_not_emitted_as_video_frame() {
        let mut depay = RtpDepacketizer::new();
        prime_h264(&mut depay);
        assert!(depay
            .push(&rtp(&[0x06, 0x05, 0xff], true, 3, 42))
            .unwrap()
            .is_none());
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
    fn h265_non_vcl_nal_is_not_emitted_as_video_frame() {
        let mut depay = RtpDepacketizer::new();
        prime_h265(&mut depay);
        assert!(depay
            .push(&rtp_with_payload_type(
                &[0x4e, 0x01, 0xff],
                RTP_PAYLOAD_TYPE_H265,
                true,
                4,
                42,
            ))
            .unwrap()
            .is_none());
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
    fn h264_stap_a_prepends_cached_parameter_sets_for_idr_without_inband_config() {
        let mut depay = RtpDepacketizer::new();
        let sps = &[0x67, 0x64, 0x00, 0x1f][..];
        let pps = &[0x68, 0xee][..];
        depay.push(&rtp(&stap_a(&[sps, pps]), true, 1, 10)).unwrap();

        let frame = depay
            .push(&rtp(&stap_a(&[&[0x65, 0xaa, 0xbb]]), true, 2, 20))
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
                &[0, 0, 0, 1, 0x65, 0xaa, 0xbb][..],
            ]
            .concat()
        );
        let status = depay.status();
        assert_eq!(status.keyframes_with_prepended_config, 1);
        assert_eq!(status.parameter_sets_prepended, 2);
    }

    #[test]
    fn h264_stap_a_does_not_duplicate_inband_parameter_sets() {
        let mut depay = RtpDepacketizer::new();
        let sps = &[0x67, 0x64, 0x00, 0x1f][..];
        let pps = &[0x68, 0xee][..];
        let frame = depay
            .push(&rtp(&stap_a(&[sps, pps, &[0x65, 0xaa]]), true, 1, 20))
            .unwrap()
            .unwrap();

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
        let status = depay.status();
        assert_eq!(status.keyframes_with_prepended_config, 0);
        assert_eq!(status.parameter_sets_prepended, 0);
    }

    #[test]
    fn h264_stap_a_waits_for_the_access_unit_marker() {
        let mut depay = RtpDepacketizer::new();
        let sps = &[0x67, 0x64, 0x00, 0x1f][..];
        let pps = &[0x68, 0xee][..];
        assert!(depay
            .push(&rtp(&stap_a(&[sps, pps, &[0x61, 0xaa]]), false, 1, 20,))
            .unwrap()
            .is_none());

        let frame = depay
            .push(&rtp(&[0x61, 0xbb], true, 2, 20))
            .unwrap()
            .unwrap();
        assert_eq!(
            frame.data,
            [
                &[0, 0, 0, 1][..],
                sps,
                &[0, 0, 0, 1][..],
                pps,
                &[0, 0, 0, 1, 0x61, 0xaa][..],
                &[0, 0, 0, 1, 0x61, 0xbb][..],
            ]
            .concat()
        );
    }

    #[test]
    fn malformed_stap_a_length_is_rejected() {
        let mut depay = RtpDepacketizer::new();
        let malformed = [24, 0, 8, 0x67, 0x64];
        assert_eq!(
            depay.push(&rtp(&malformed, true, 1, 20)),
            Err(RtpError::UnsupportedPayload)
        );
        assert_eq!(depay.status().unsupported_payloads, 1);
    }

    #[test]
    fn h265_ap_prepends_cached_parameter_sets_for_keyframe_without_inband_config() {
        let mut depay = RtpDepacketizer::new();
        prime_h265(&mut depay);
        let frame = depay
            .push(&rtp_with_payload_type(
                &h265_ap(&[&[0x26, 0x01, 0xaa]]),
                RTP_PAYLOAD_TYPE_H265,
                true,
                4,
                20,
            ))
            .unwrap()
            .unwrap();

        assert!(frame.is_keyframe);
        assert_eq!(
            frame.data,
            [
                &[0, 0, 0, 1, 0x40, 0x01, 0xaa][..],
                &[0, 0, 0, 1, 0x42, 0x01, 0xbb][..],
                &[0, 0, 0, 1, 0x44, 0x01, 0xcc][..],
                &[0, 0, 0, 1, 0x26, 0x01, 0xaa][..],
            ]
            .concat()
        );
        let status = depay.status();
        assert_eq!(status.keyframes_with_prepended_config, 1);
        assert_eq!(status.parameter_sets_prepended, 3);
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

    #[test]
    fn status_tracks_h264_decoder_config() {
        let mut depay = RtpDepacketizer::new();
        depay
            .push(&rtp(&[0x67, 0x64, 0x00, 0x1f], true, 1, 10))
            .unwrap();
        let status = depay.status();
        assert!(status.codec_config.h264_sps);
        assert!(!status.codec_config.h264_pps);
        assert!(!status.codec_config.is_complete_for(Codec::H264));

        depay.push(&rtp(&[0x68, 0xee], true, 2, 10)).unwrap();
        let status = depay.status();
        assert!(status.codec_config.is_complete_for(Codec::H264));
    }

    #[test]
    fn reorder_buffer_restores_short_out_of_order_burst() {
        let mut reorder = RtpReorderBuffer::default();
        let first = rtp(&[0x61, 1], true, 10, 90);
        let second = rtp(&[0x61, 2], true, 11, 90);
        let third = rtp(&[0x61, 3], true, 12, 90);

        assert_eq!(reorder.push(&first).unwrap(), vec![first.clone()]);
        assert!(reorder.push(&third).unwrap().is_empty());
        assert_eq!(reorder.status().buffered_packets, 1);
        assert_eq!(reorder.status().reordered_packets, 1);

        let ready = reorder.push(&second).unwrap();
        assert_eq!(ready, vec![second, third]);
        assert_eq!(reorder.status().buffered_packets, 0);
    }
}
