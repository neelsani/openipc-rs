use std::ops::Range;

use openipc_core::rtp::{
    Codec, RTP_PAYLOAD_TYPE_H264, RTP_PAYLOAD_TYPE_H265, RTP_PAYLOAD_TYPE_OPUS,
};

pub(crate) const MOCK_FPS: u32 = 60;
pub(crate) const MOCK_FPS_OPTIONS: [u32; 4] = [30, 60, 120, 240];
#[cfg(test)]
const MOCK_WIDTH: u32 = 3_840;
#[cfg(test)]
const MOCK_HEIGHT: u32 = 2_160;
#[cfg(test)]
const MOCK_FRAMES_PER_LOOP: usize = 300;

const CLOCK_RATE: u32 = 90_000;
const AUDIO_CLOCK_RATE: u32 = 48_000;
const AUDIO_FRAME_SAMPLES: u32 = AUDIO_CLOCK_RATE / 50;
const AUDIO_PACKETS_PER_LOOP: usize = 250;
const RTP_PAYLOAD_BYTES: usize = 1_100;
const VIDEO_SSRC: u32 = 0x4f49_5043;
const AUDIO_SSRC: u32 = 0x4f49_5041;
const OPUS_OGG: &[u8] = include_bytes!("../../assets/mock.opus.ogg");

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MockResolution {
    Hd720,
    FullHd1080,
    #[default]
    UltraHd2160,
}

impl MockResolution {
    pub(crate) const ALL: [Self; 3] = [Self::Hd720, Self::FullHd1080, Self::UltraHd2160];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Hd720 => "720p",
            Self::FullHd1080 => "1080p",
            Self::UltraHd2160 => "2160p (4K)",
        }
    }

    pub(crate) const fn short_label(self) -> &'static str {
        match self {
            Self::Hd720 => "720p",
            Self::FullHd1080 => "1080p",
            Self::UltraHd2160 => "4K",
        }
    }

    #[cfg(test)]
    const fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Hd720 => (1_280, 720),
            Self::FullHd1080 => (1_920, 1_080),
            Self::UltraHd2160 => (3_840, 2_160),
        }
    }

    pub(crate) const fn fixture_name(self, codec: Codec) -> &'static str {
        match (self, codec) {
            (Self::Hd720, Codec::H264) => "mock-720p.h264",
            (Self::Hd720, Codec::H265) => "mock-720p.h265",
            (Self::FullHd1080, Codec::H264) => "mock-1080p.h264",
            (Self::FullHd1080, Codec::H265) => "mock-1080p.h265",
            (Self::UltraHd2160, Codec::H264) => "mock.h264",
            (Self::UltraHd2160, Codec::H265) => "mock.h265",
        }
    }

    pub(crate) const fn fixture_sha256(self, codec: Codec) -> &'static str {
        match (self, codec) {
            (Self::Hd720, Codec::H264) => {
                "a996a0b35fe71c18ca7e59546557a804b0231c9ef2bfecaea74a2bbfc089b06c"
            }
            (Self::Hd720, Codec::H265) => {
                "c092ebf17b327f1f5abdc65f7d9c6224e45e484693807344d2122b620ed8ac09"
            }
            (Self::FullHd1080, Codec::H264) => {
                "e1a22b9ef65aa25034381f658f09ef58cd9c00b092d85fecf93a7b1830c46b80"
            }
            (Self::FullHd1080, Codec::H265) => {
                "127c2344662257266113c30fa794189e171f370520813a34c4c0699195c8c4c6"
            }
            (Self::UltraHd2160, Codec::H264) => {
                "7cee9eb9a146a4dc86e323d77d9141f3cce6e8ca21c469863e6c608cad6b8e51"
            }
            (Self::UltraHd2160, Codec::H265) => {
                "098df0d672df6b50d0c787c177c9edcf885c800d35db9536768d50bfa75f2eac"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MockVideoConfig {
    pub(crate) resolution: MockResolution,
    pub(crate) fps: u32,
}

impl MockVideoConfig {
    pub(crate) fn label(self) -> String {
        format!("{}{}", self.resolution.short_label(), self.fps)
    }

    fn validate(self) -> Result<Self, String> {
        if MOCK_FPS_OPTIONS.contains(&self.fps) {
            Ok(self)
        } else {
            Err(format!("unsupported mock frame rate: {} FPS", self.fps))
        }
    }
}

impl Default for MockVideoConfig {
    fn default() -> Self {
        Self {
            resolution: MockResolution::UltraHd2160,
            fps: MOCK_FPS,
        }
    }
}

pub(crate) struct MockRtpFrame {
    pub(crate) packets: Vec<Vec<u8>>,
}

pub(crate) struct MockRtpEvent {
    pub(crate) packets: Vec<Vec<u8>>,
    /// Absolute mock-stream deadline for the event after this one.
    pub(crate) next_due_micros: u64,
}

/// Interleaves the pre-encoded video and audio fixtures on their RTP clocks.
pub(crate) struct MockAvStream {
    video: MockVideoStream,
    fps: u32,
    audio_packets: Vec<Vec<u8>>,
    audio_index: usize,
    audio_timestamp: u32,
    audio_sequence: u16,
    next_video_micros: u64,
    next_audio_micros: u64,
}

impl MockAvStream {
    #[cfg(test)]
    pub(crate) fn new(codec: Codec) -> Result<Self, String> {
        let config = MockVideoConfig::default();
        Self::new_with_config(codec, config, test_fixture(config.resolution, codec)?)
    }

    pub(crate) fn new_with_config(
        codec: Codec,
        config: MockVideoConfig,
        video_data: Vec<u8>,
    ) -> Result<Self, String> {
        let config = config.validate()?;
        let mut audio_packets = ogg_packets(OPUS_OGG)?;
        audio_packets
            .retain(|packet| !packet.starts_with(b"OpusHead") && !packet.starts_with(b"OpusTags"));
        if audio_packets.len() < AUDIO_PACKETS_PER_LOOP {
            return Err(format!(
                "embedded Opus mock contains {} packets; expected at least {AUDIO_PACKETS_PER_LOOP}",
                audio_packets.len()
            ));
        }
        audio_packets.truncate(AUDIO_PACKETS_PER_LOOP);
        Ok(Self {
            video: MockVideoStream::new_with_config(codec, config, video_data)?,
            fps: config.fps,
            audio_packets,
            audio_index: 0,
            audio_timestamp: 0,
            audio_sequence: 1,
            next_video_micros: 0,
            next_audio_micros: 0,
        })
    }

    pub(crate) fn next_event(&mut self) -> MockRtpEvent {
        let due = self.next_video_micros.min(self.next_audio_micros);
        let mut packets = Vec::new();
        if self.next_video_micros == due {
            packets.extend(self.video.next_frame().packets);
            self.next_video_micros += 1_000_000 / u64::from(self.fps);
        }
        if self.next_audio_micros == due {
            let payload = &self.audio_packets[self.audio_index];
            packets.push(rtp_packet(
                payload,
                self.audio_timestamp,
                self.audio_sequence,
                false,
                RTP_PAYLOAD_TYPE_OPUS,
                AUDIO_SSRC,
            ));
            self.audio_index = (self.audio_index + 1) % self.audio_packets.len();
            self.audio_timestamp = self.audio_timestamp.wrapping_add(AUDIO_FRAME_SAMPLES);
            self.audio_sequence = self.audio_sequence.wrapping_add(1);
            self.next_audio_micros += 20_000;
        }
        let next_due = self.next_video_micros.min(self.next_audio_micros);
        MockRtpEvent {
            packets,
            next_due_micros: next_due,
        }
    }

    /// Rebase the synthetic clock after a debugger or expensive UI pass stalls it.
    ///
    /// The mock is a live-source simulator, so replaying an arbitrarily large
    /// backlog is less useful than preserving A/V order and resuming at "now".
    pub(crate) fn rebase_timing_if_late(&mut self, now_micros: u64, max_lag_micros: u64) -> bool {
        let next_due = self.next_video_micros.min(self.next_audio_micros);
        let lag = now_micros.saturating_sub(next_due);
        if lag <= max_lag_micros {
            return false;
        }
        self.next_video_micros = self.next_video_micros.saturating_add(lag);
        self.next_audio_micros = self.next_audio_micros.saturating_add(lag);
        true
    }
}

pub(crate) enum MockVideoStream {
    H264(MockH264Stream),
    H265(MockH265Stream),
}

impl MockVideoStream {
    pub(crate) fn new_with_config(
        codec: Codec,
        config: MockVideoConfig,
        video_data: Vec<u8>,
    ) -> Result<Self, String> {
        let config = config.validate()?;
        match codec {
            Codec::H264 => MockH264Stream::new_with_data(video_data, config.fps).map(Self::H264),
            Codec::H265 => MockH265Stream::new_with_data(video_data, config.fps).map(Self::H265),
        }
    }

    pub(crate) fn next_frame(&mut self) -> MockRtpFrame {
        match self {
            Self::H264(stream) => stream.next_frame(),
            Self::H265(stream) => stream.next_frame(),
        }
    }
}

/// Loops a pre-encoded Annex-B fixture and packetizes each access unit as RTP.
pub(crate) struct MockH264Stream {
    data: Vec<u8>,
    access_units: Vec<Vec<Range<usize>>>,
    frame_index: usize,
    timestamp: u32,
    timestamp_step: u32,
    sequence: u16,
}

impl MockH264Stream {
    #[cfg(test)]
    pub(crate) fn new() -> Result<Self, String> {
        Self::new_with_data(
            test_fixture(MockResolution::UltraHd2160, Codec::H264)?,
            MOCK_FPS,
        )
    }

    fn new_with_data(data: Vec<u8>, fps: u32) -> Result<Self, String> {
        let access_units = access_units(&data);
        if access_units.is_empty() {
            return Err("embedded H.264 mock contains no access units".to_owned());
        }
        if !access_units[0]
            .iter()
            .any(|range| nal_type(&data[range.clone()]) == 5)
        {
            return Err("embedded H.264 mock does not begin with an IDR".to_owned());
        }
        Ok(Self {
            data,
            access_units,
            frame_index: 0,
            timestamp: 0,
            timestamp_step: CLOCK_RATE / fps,
            sequence: 1,
        })
    }

    pub(crate) fn next_frame(&mut self) -> MockRtpFrame {
        let access_unit = &self.access_units[self.frame_index];
        let mut packets = Vec::new();
        for (index, range) in access_unit.iter().enumerate() {
            let nalu = &self.data[range.clone()];
            let last_nalu = index + 1 == access_unit.len();
            packetize_h264_nalu(
                nalu,
                self.timestamp,
                last_nalu,
                &mut self.sequence,
                &mut packets,
            );
        }
        self.frame_index = (self.frame_index + 1) % self.access_units.len();
        self.timestamp = self.timestamp.wrapping_add(self.timestamp_step);
        MockRtpFrame { packets }
    }
}

/// Loops a pre-encoded Annex-B HEVC fixture and packetizes it as RFC 7798 RTP.
pub(crate) struct MockH265Stream {
    data: Vec<u8>,
    access_units: Vec<Vec<Range<usize>>>,
    frame_index: usize,
    timestamp: u32,
    timestamp_step: u32,
    sequence: u16,
}

impl MockH265Stream {
    #[cfg(test)]
    pub(crate) fn new() -> Result<Self, String> {
        Self::new_with_data(
            test_fixture(MockResolution::UltraHd2160, Codec::H265)?,
            MOCK_FPS,
        )
    }

    fn new_with_data(data: Vec<u8>, fps: u32) -> Result<Self, String> {
        let access_units = h265_access_units(&data);
        if access_units.is_empty() {
            return Err("embedded H.265 mock contains no access units".to_owned());
        }
        let first = &access_units[0];
        for (kind, label) in [(32, "VPS"), (33, "SPS"), (34, "PPS")] {
            if !first
                .iter()
                .any(|range| h265_nal_type(&data[range.clone()]) == kind)
            {
                return Err(format!("embedded H.265 mock does not begin with a {label}"));
            }
        }
        if !first
            .iter()
            .any(|range| matches!(h265_nal_type(&data[range.clone()]), 16..=23))
        {
            return Err("embedded H.265 mock does not begin with an IRAP picture".to_owned());
        }
        Ok(Self {
            data,
            access_units,
            frame_index: 0,
            timestamp: 0,
            timestamp_step: CLOCK_RATE / fps,
            sequence: 1,
        })
    }

    pub(crate) fn next_frame(&mut self) -> MockRtpFrame {
        let access_unit = &self.access_units[self.frame_index];
        let mut packets = Vec::new();
        for (index, range) in access_unit.iter().enumerate() {
            let nalu = &self.data[range.clone()];
            packetize_h265_nalu(
                nalu,
                self.timestamp,
                index + 1 == access_unit.len(),
                &mut self.sequence,
                &mut packets,
            );
        }
        self.frame_index = (self.frame_index + 1) % self.access_units.len();
        self.timestamp = self.timestamp.wrapping_add(self.timestamp_step);
        MockRtpFrame { packets }
    }
}

fn access_units(data: &[u8]) -> Vec<Vec<Range<usize>>> {
    let mut units = Vec::new();
    let mut pending = Vec::new();
    let mut has_video_slice = false;
    for range in annex_b_nalus(data) {
        let nalu = &data[range.clone()];
        let kind = nal_type(nalu);
        let is_video_slice = matches!(kind, 1 | 5);
        let starts_picture = is_video_slice && first_mb_in_slice(nalu) == Some(0);
        let prefixes_picture = matches!(kind, 6..=9);
        if has_video_slice && (starts_picture || prefixes_picture) {
            units.push(std::mem::take(&mut pending));
            has_video_slice = false;
        }
        pending.push(range);
        if is_video_slice {
            has_video_slice = true;
        }
    }
    if has_video_slice {
        units.push(pending);
    }
    units
}

fn h265_access_units(data: &[u8]) -> Vec<Vec<Range<usize>>> {
    let mut units = Vec::new();
    let mut pending = Vec::new();
    let mut has_video_slice = false;
    for range in annex_b_nalus(data) {
        let nalu = &data[range.clone()];
        let kind = h265_nal_type(nalu);
        let is_video_slice = kind <= 31;
        let starts_picture = is_video_slice && h265_first_slice_segment(nalu);
        let prefixes_picture = matches!(kind, 32..=35 | 39);
        if has_video_slice && (starts_picture || prefixes_picture) {
            units.push(std::mem::take(&mut pending));
            has_video_slice = false;
        }
        pending.push(range);
        if is_video_slice {
            has_video_slice = true;
        }
    }
    if has_video_slice {
        units.push(pending);
    }
    units
}

fn h265_first_slice_segment(nalu: &[u8]) -> bool {
    nalu.get(2).is_some_and(|byte| byte & 0x80 != 0)
}

fn first_mb_in_slice(nalu: &[u8]) -> Option<u32> {
    let mut rbsp = Vec::with_capacity(nalu.len().saturating_sub(1));
    let mut zeros = 0u8;
    for &byte in nalu.get(1..)? {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        rbsp.push(byte);
        zeros = if byte == 0 {
            zeros.saturating_add(1)
        } else {
            0
        };
    }
    read_unsigned_exp_golomb(&rbsp)
}

fn read_unsigned_exp_golomb(data: &[u8]) -> Option<u32> {
    let bit_len = data.len().checked_mul(8)?;
    let mut leading_zeros = 0usize;
    while leading_zeros < bit_len && bit(data, leading_zeros)? == 0 {
        leading_zeros += 1;
    }
    if leading_zeros >= 32 || leading_zeros >= bit_len {
        return None;
    }
    let mut value = 1u32.checked_shl(leading_zeros as u32)?;
    for suffix_index in 0..leading_zeros {
        value |= u32::from(bit(data, leading_zeros + 1 + suffix_index)?)
            << (leading_zeros - suffix_index - 1);
    }
    value.checked_sub(1)
}

fn bit(data: &[u8], index: usize) -> Option<u8> {
    data.get(index / 8)
        .map(|byte| (byte >> (7 - index % 8)) & 1)
}

fn annex_b_nalus(data: &[u8]) -> Vec<Range<usize>> {
    let mut starts = Vec::new();
    let mut offset = 0usize;
    while offset + 3 <= data.len() {
        let length = start_code_len(data, offset);
        if length > 0 {
            starts.push((offset, length));
            offset += length;
        } else {
            offset += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .filter_map(|(index, &(offset, length))| {
            let start = offset + length;
            let end = starts.get(index + 1).map_or(data.len(), |next| next.0);
            (start < end).then_some(start..end)
        })
        .collect()
}

fn start_code_len(data: &[u8], offset: usize) -> usize {
    match data.get(offset..) {
        Some([0, 0, 1, ..]) => 3,
        Some([0, 0, 0, 1, ..]) => 4,
        _ => 0,
    }
}

fn nal_type(nalu: &[u8]) -> u8 {
    nalu.first().copied().unwrap_or_default() & 0x1f
}

fn h265_nal_type(nalu: &[u8]) -> u8 {
    nalu.first().copied().unwrap_or_default() >> 1 & 0x3f
}

fn packetize_h264_nalu(
    nalu: &[u8],
    timestamp: u32,
    marker: bool,
    sequence: &mut u16,
    packets: &mut Vec<Vec<u8>>,
) {
    if nalu.len() <= RTP_PAYLOAD_BYTES {
        packets.push(rtp_packet(
            nalu,
            timestamp,
            *sequence,
            marker,
            RTP_PAYLOAD_TYPE_H264,
            VIDEO_SSRC,
        ));
        *sequence = sequence.wrapping_add(1);
        return;
    }

    let nal_header = nalu[0];
    let nal_type = nal_header & 0x1f;
    let fu_indicator = (nal_header & 0xe0) | 28;
    let mut offset = 1usize;
    let mut first = true;
    while offset < nalu.len() {
        let chunk_len = (nalu.len() - offset).min(RTP_PAYLOAD_BYTES - 2);
        let end = offset + chunk_len == nalu.len();
        let mut payload = Vec::with_capacity(chunk_len + 2);
        payload.push(fu_indicator);
        payload.push((u8::from(first) << 7) | (u8::from(end) << 6) | nal_type);
        payload.extend_from_slice(&nalu[offset..offset + chunk_len]);
        packets.push(rtp_packet(
            &payload,
            timestamp,
            *sequence,
            end && marker,
            RTP_PAYLOAD_TYPE_H264,
            VIDEO_SSRC,
        ));
        *sequence = sequence.wrapping_add(1);
        offset += chunk_len;
        first = false;
    }
}

fn packetize_h265_nalu(
    nalu: &[u8],
    timestamp: u32,
    marker: bool,
    sequence: &mut u16,
    packets: &mut Vec<Vec<u8>>,
) {
    if nalu.len() < 2 {
        return;
    }
    if nalu.len() <= RTP_PAYLOAD_BYTES {
        packets.push(rtp_packet(
            nalu,
            timestamp,
            *sequence,
            marker,
            RTP_PAYLOAD_TYPE_H265,
            VIDEO_SSRC,
        ));
        *sequence = sequence.wrapping_add(1);
        return;
    }

    let nal_type = h265_nal_type(nalu);
    let fu_indicator = [(nalu[0] & 0x81) | (49 << 1), nalu[1]];
    let mut offset = 2usize;
    let mut first = true;
    while offset < nalu.len() {
        let chunk_len = (nalu.len() - offset).min(RTP_PAYLOAD_BYTES - 3);
        let end = offset + chunk_len == nalu.len();
        let mut payload = Vec::with_capacity(chunk_len + 3);
        payload.extend_from_slice(&fu_indicator);
        payload.push((u8::from(first) << 7) | (u8::from(end) << 6) | nal_type);
        payload.extend_from_slice(&nalu[offset..offset + chunk_len]);
        packets.push(rtp_packet(
            &payload,
            timestamp,
            *sequence,
            end && marker,
            RTP_PAYLOAD_TYPE_H265,
            VIDEO_SSRC,
        ));
        *sequence = sequence.wrapping_add(1);
        offset += chunk_len;
        first = false;
    }
}

fn rtp_packet(
    payload: &[u8],
    timestamp: u32,
    sequence: u16,
    marker: bool,
    payload_type: u8,
    ssrc: u32,
) -> Vec<u8> {
    let mut packet = Vec::with_capacity(12 + payload.len());
    packet.push(0x80);
    packet.push((u8::from(marker) << 7) | payload_type);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(payload);
    packet
}

fn ogg_packets(data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut packets = Vec::new();
    let mut pending = Vec::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let header = data
            .get(offset..offset + 27)
            .ok_or_else(|| "truncated Ogg page header".to_owned())?;
        if &header[..4] != b"OggS" || header[4] != 0 {
            return Err(format!("invalid Ogg page at byte {offset}"));
        }
        let segment_count = usize::from(header[26]);
        let lacing = data
            .get(offset + 27..offset + 27 + segment_count)
            .ok_or_else(|| "truncated Ogg lacing table".to_owned())?;
        let body_start = offset + 27 + segment_count;
        let body_len = lacing
            .iter()
            .map(|length| usize::from(*length))
            .sum::<usize>();
        let body = data
            .get(body_start..body_start + body_len)
            .ok_or_else(|| "truncated Ogg page body".to_owned())?;
        let mut body_offset = 0usize;
        for &length in lacing {
            let length = usize::from(length);
            pending.extend_from_slice(&body[body_offset..body_offset + length]);
            body_offset += length;
            if length < 255 {
                packets.push(std::mem::take(&mut pending));
            }
        }
        offset = body_start + body_len;
    }
    if !pending.is_empty() {
        return Err("Ogg stream ends in a continued packet".to_owned());
    }
    Ok(packets)
}

#[cfg(test)]
fn test_fixture(resolution: MockResolution, codec: Codec) -> Result<Vec<u8>, String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(resolution.fixture_name(codec));
    std::fs::read(&path)
        .map_err(|error| format!("could not read mock fixture {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_contains_decodable_access_units() {
        let stream = MockH264Stream::new().unwrap();
        assert_eq!(stream.access_units.len(), MOCK_FRAMES_PER_LOOP);
        assert!(stream.access_units[0]
            .iter()
            .any(|range| nal_type(&stream.data[range.clone()]) == 7));
        assert!(stream.access_units[0]
            .iter()
            .any(|range| nal_type(&stream.data[range.clone()]) == 8));

        let sps = stream.access_units[0]
            .iter()
            .find(|range| nal_type(&stream.data[(*range).clone()]) == 7)
            .map(|range| &stream.data[range.clone()])
            .unwrap();
        let pps = stream.access_units[0]
            .iter()
            .find(|range| nal_type(&stream.data[(*range).clone()]) == 8)
            .map(|range| &stream.data[range.clone()])
            .unwrap();
        let info =
            openipc_video::CodecConfig::H264(openipc_video::H264Config::new(sps, pps).unwrap())
                .stream_info()
                .unwrap();
        assert_eq!(info.visible_dimensions.width, MOCK_WIDTH);
        assert_eq!(info.visible_dimensions.height, MOCK_HEIGHT);
        assert_eq!(info.bit_depth, 8);
    }

    #[test]
    fn h265_fixture_contains_parameter_sets_and_irap() {
        let stream = MockH265Stream::new().unwrap();
        assert_eq!(stream.access_units.len(), MOCK_FRAMES_PER_LOOP);
        for kind in [32, 33, 34] {
            assert!(stream.access_units[0]
                .iter()
                .any(|range| h265_nal_type(&stream.data[range.clone()]) == kind));
        }
        assert!(stream.access_units[0]
            .iter()
            .any(|range| matches!(h265_nal_type(&stream.data[range.clone()]), 16..=23)));

        let parameter_set = |kind| {
            stream.access_units[0]
                .iter()
                .find(|range| h265_nal_type(&stream.data[(*range).clone()]) == kind)
                .map(|range| &stream.data[range.clone()])
                .unwrap()
        };
        let info = openipc_video::CodecConfig::H265(
            openipc_video::H265Config::new(parameter_set(32), parameter_set(33), parameter_set(34))
                .unwrap(),
        )
        .stream_info()
        .unwrap();
        assert_eq!(info.visible_dimensions.width, MOCK_WIDTH);
        assert_eq!(info.visible_dimensions.height, MOCK_HEIGHT);
        assert_eq!(info.bit_depth, 8);
    }

    #[test]
    fn selectable_fixtures_match_their_advertised_resolution() {
        for resolution in MockResolution::ALL {
            let expected = resolution.dimensions();
            let h264 = MockH264Stream::new_with_data(
                test_fixture(resolution, Codec::H264).unwrap(),
                MOCK_FPS,
            )
            .unwrap();
            assert_eq!(h264.access_units.len(), MOCK_FRAMES_PER_LOOP);
            let h264_parameter_set = |kind| {
                h264.access_units[0]
                    .iter()
                    .find(|range| nal_type(&h264.data[(*range).clone()]) == kind)
                    .map(|range| &h264.data[range.clone()])
                    .unwrap()
            };
            let h264_info = openipc_video::CodecConfig::H264(
                openipc_video::H264Config::new(h264_parameter_set(7), h264_parameter_set(8))
                    .unwrap(),
            )
            .stream_info()
            .unwrap();
            assert_eq!(
                (
                    h264_info.visible_dimensions.width,
                    h264_info.visible_dimensions.height
                ),
                expected
            );

            let h265 = MockH265Stream::new_with_data(
                test_fixture(resolution, Codec::H265).unwrap(),
                MOCK_FPS,
            )
            .unwrap();
            assert_eq!(h265.access_units.len(), MOCK_FRAMES_PER_LOOP);
            let h265_parameter_set = |kind| {
                h265.access_units[0]
                    .iter()
                    .find(|range| h265_nal_type(&h265.data[(*range).clone()]) == kind)
                    .map(|range| &h265.data[range.clone()])
                    .unwrap()
            };
            let h265_info = openipc_video::CodecConfig::H265(
                openipc_video::H265Config::new(
                    h265_parameter_set(32),
                    h265_parameter_set(33),
                    h265_parameter_set(34),
                )
                .unwrap(),
            )
            .stream_info()
            .unwrap();
            assert_eq!(
                (
                    h265_info.visible_dimensions.width,
                    h265_info.visible_dimensions.height
                ),
                expected
            );
        }
    }

    #[test]
    fn selected_frame_rate_controls_rtp_clock_and_pacing() {
        let config = MockVideoConfig {
            resolution: MockResolution::Hd720,
            fps: 240,
        };
        let mut stream = MockAvStream::new_with_config(
            Codec::H264,
            config,
            test_fixture(config.resolution, Codec::H264).unwrap(),
        )
        .unwrap();
        let first = stream.next_event();
        assert_eq!(first.next_due_micros, 1_000_000 / 240);
        let second = stream.next_event();
        let packet = second
            .packets
            .iter()
            .find(|packet| packet[1] & 0x7f == RTP_PAYLOAD_TYPE_H264)
            .unwrap();
        assert_eq!(u32::from_be_bytes(packet[4..8].try_into().unwrap()), 375);

        let invalid = MockVideoConfig {
            resolution: MockResolution::Hd720,
            fps: 0,
        };
        assert!(MockAvStream::new_with_config(Codec::H264, invalid, Vec::new()).is_err());
    }

    #[test]
    fn h265_rtp_round_trips_through_production_depacketizer() {
        let mut stream = MockH265Stream::new().unwrap();
        let mut depacketizer = openipc_core::RtpDepacketizer::new();
        let mut frame = None;
        for packet in stream.next_frame().packets {
            if let Some(output) = depacketizer.push(&packet).unwrap() {
                frame = Some(output);
            }
        }
        let frame = frame.expect("first H.265 access unit should be emitted");
        assert_eq!(frame.codec, Codec::H265);
        assert!(frame.is_keyframe);
        assert!(depacketizer.codec_config().h265_vps);
        assert!(depacketizer.codec_config().h265_sps);
        assert!(depacketizer.codec_config().h265_pps);
    }

    #[test]
    fn h265_mock_uses_waybeam_rtp_wire_shape() {
        let mut stream = MockH265Stream::new().unwrap();
        let packets = stream.next_frame().packets;
        let mut saw_single_nalu = false;
        let mut saw_fragmentation_unit = false;

        for packet in &packets {
            let header = openipc_core::RtpHeader::parse(packet).unwrap();
            assert_eq!(header.payload_type, RTP_PAYLOAD_TYPE_H265);
            assert_eq!(header.ssrc, VIDEO_SSRC);
            assert!(header.payload_len <= RTP_PAYLOAD_BYTES);

            let payload = &packet[header.header_len..header.header_len + header.payload_len];
            let nal_type = h265_nal_type(payload);
            assert_ne!(nal_type, 48, "Waybeam does not emit aggregation packets");
            if nal_type == 49 {
                saw_fragmentation_unit = true;
            } else {
                saw_single_nalu = true;
            }
        }

        assert!(saw_single_nalu, "expected separate VPS/SPS/PPS packets");
        assert!(saw_fragmentation_unit, "expected an RFC 7798 type-49 FU");
        assert!(packets.last().is_some_and(|packet| packet[1] & 0x80 != 0));
        assert!(packets[..packets.len() - 1]
            .iter()
            .all(|packet| packet[1] & 0x80 == 0));
    }

    #[test]
    fn loop_preserves_monotonic_rtp_timing_and_sequence() {
        let mut stream = MockH264Stream::new().unwrap();
        let first = stream.next_frame();
        for _ in 1..MOCK_FRAMES_PER_LOOP {
            stream.next_frame();
        }
        let looped = stream.next_frame();

        assert_eq!(
            u16::from_be_bytes([first.packets[0][2], first.packets[0][3]]),
            1
        );
        assert_eq!(
            u32::from_be_bytes(looped.packets[0][4..8].try_into().unwrap()),
            MOCK_FRAMES_PER_LOOP as u32 * (CLOCK_RATE / MOCK_FPS)
        );
        assert!(looped
            .packets
            .iter()
            .any(|packet| packet.get(12).is_some_and(|byte| byte & 0x1f == 7)));
    }

    #[test]
    fn av_fixture_interleaves_h264_and_opus_rtp() {
        let mut stream = MockAvStream::new(Codec::H264).unwrap();
        let first = stream.next_event();
        assert!(first
            .packets
            .iter()
            .any(|packet| packet[1] & 0x7f == RTP_PAYLOAD_TYPE_H264));
        assert!(first
            .packets
            .iter()
            .any(|packet| packet[1] & 0x7f == RTP_PAYLOAD_TYPE_OPUS));
        assert_eq!(
            first.next_due_micros,
            (1_000_000 / u64::from(MOCK_FPS)).min(20_000)
        );
    }

    #[test]
    fn av_fixture_interleaves_h265_and_opus_rtp() {
        let mut stream = MockAvStream::new(Codec::H265).unwrap();
        let first = stream.next_event();
        assert!(first
            .packets
            .iter()
            .any(|packet| packet[1] & 0x7f == RTP_PAYLOAD_TYPE_H265));
        assert!(first
            .packets
            .iter()
            .any(|packet| packet[1] & 0x7f == RTP_PAYLOAD_TYPE_OPUS));
    }

    #[test]
    fn late_live_mock_rebases_without_changing_av_order() {
        let mut stream = MockAvStream::new(Codec::H265).unwrap();
        let first = stream.next_event();
        let before_gap = stream
            .next_audio_micros
            .saturating_sub(stream.next_video_micros);

        assert!(stream.rebase_timing_if_late(500_000, 50_000));
        assert_eq!(
            stream
                .next_audio_micros
                .saturating_sub(stream.next_video_micros),
            before_gap
        );
        assert_eq!(
            stream.next_video_micros.min(stream.next_audio_micros),
            500_000
        );
        assert_eq!(first.next_due_micros, 1_000_000 / u64::from(MOCK_FPS));
    }

    #[test]
    fn opus_fixture_has_audible_signal_level() {
        let stream = MockAvStream::new(Codec::H265).unwrap();
        let mut decoder = ropus::Decoder::new(48_000, ropus::Channels::Mono).unwrap();
        let mut pcm = vec![0.0; 5_760];
        let frames = decoder
            .decode_float(
                &stream.audio_packets[0],
                &mut pcm,
                ropus::DecodeMode::Normal,
            )
            .unwrap();
        let peak = pcm[..frames]
            .iter()
            .copied()
            .map(f32::abs)
            .fold(0.0, f32::max);
        assert!(peak > 0.1, "mock Opus peak is too quiet: {peak}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "exercises the host VideoToolbox service"]
    fn fixture_decodes_with_macos_backend() {
        use std::{thread, time::Duration};

        use openipc_core::{
            ChannelId, FrameLayout, PayloadRouteId, ReceiverBatchOptions, ReceiverRuntime,
        };
        use openipc_video::{DecoderOptions, PlatformDecoder, VideoDecoder as _};

        for codec in [Codec::H264, Codec::H265] {
            let config = MockVideoConfig::default();
            let mut source = MockVideoStream::new_with_config(
                codec,
                config,
                test_fixture(config.resolution, codec).unwrap(),
            )
            .unwrap();
            let mut receiver = ReceiverRuntime::with_mock_video_route(
                FrameLayout::WithFcs,
                PayloadRouteId::new(1),
                ChannelId::default_video(),
                0,
            );
            let runtime = receiver.video_runtime();
            let options = ReceiverBatchOptions::default();
            let mut decoder = PlatformDecoder::new(DecoderOptions::default()).unwrap();
            let mut payload_sequence = 1u64;
            let mut decoded = None;

            for _ in 0..60 {
                for packet in source.next_frame().packets {
                    let batch = receiver
                        .push_mock_payload(runtime, payload_sequence, &packet, &options)
                        .unwrap();
                    payload_sequence = payload_sequence.wrapping_add(1);
                    for frame in batch.frames {
                        decoder.submit(frame.into()).unwrap_or_else(|error| {
                            panic!("VideoToolbox rejected the {codec:?} fixture: {error}")
                        });
                    }
                }
                for _ in 0..10 {
                    if let Some(frame) = decoder.latest_frame() {
                        decoded = Some(frame);
                        break;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                if decoded.is_some() {
                    break;
                }
            }

            let frame = decoded.unwrap_or_else(|| {
                panic!(
                    "VideoToolbox returned no {codec:?} frame: {:?}",
                    decoder.stats()
                )
            });
            let presented = crate::video::present_frame(frame, &decoder).unwrap();
            assert_eq!(
                presented.rgba.len(),
                presented.dimensions.width as usize * presented.dimensions.height as usize * 4
            );
        }
    }
}
