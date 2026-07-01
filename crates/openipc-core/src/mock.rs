use crate::rtp::{RtpError, RtpHeader};

const MOCK_RTP_PAYLOAD_TYPE: u8 = 120;
const MOCK_RTP_SSRC: u32 = 0x4f49_5043;
const MOCK_PAYLOAD_MAGIC: &[u8; 4] = b"ORMF";
const MOCK_PAYLOAD_HEADER_LEN: usize = 24;
const MOCK_RTP_PAYLOAD_BYTES: usize = 1_100;

/// One RGBA frame recovered from the Rust mock RTP pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockRtpFrame {
    /// Frame width in pixels.
    pub width: u16,
    /// Frame height in pixels.
    pub height: u16,
    /// Monotonic mock frame index.
    pub frame_index: u64,
    /// RTP timestamp used for this frame.
    pub timestamp: u32,
    /// RGBA pixels in row-major order.
    pub rgba: Vec<u8>,
    /// Number of RTP packets generated and consumed for this frame.
    pub rtp_packets: usize,
    /// Total RTP bytes generated for this frame.
    pub rtp_bytes: usize,
}

/// Synthetic RTP source for no-hardware development.
///
/// This is intentionally not an H.264 encoder. It is a deterministic RTP
/// packet generator plus RTP reassembler that produces RGBA test frames for
/// frontend development, layout work, and metrics plumbing without a USB radio.
#[derive(Debug, Clone)]
pub struct MockRtpPipeline {
    width: u16,
    height: u16,
    fps: u16,
    frame_index: u64,
    sequence: u16,
    timestamp: u32,
}

impl Default for MockRtpPipeline {
    fn default() -> Self {
        Self::new(320, 180, 30)
    }
}

impl MockRtpPipeline {
    /// Create a mock RTP pipeline.
    pub fn new(width: u16, height: u16, fps: u16) -> Self {
        Self {
            width: width.clamp(16, 1_920),
            height: height.clamp(16, 1_080),
            fps: fps.clamp(1, 120),
            frame_index: 0,
            sequence: 1,
            timestamp: 0,
        }
    }

    /// Generate RTP packets, consume them with the mock RTP assembler, and
    /// return the recovered RGBA frame.
    pub fn next_frame(&mut self) -> Result<MockRtpFrame, RtpError> {
        let rgba = render_mock_rgba(self.width, self.height, self.frame_index);
        let packets = self.packetize(&rgba);
        let rtp_packets = packets.len();
        let rtp_bytes = packets.iter().map(Vec::len).sum();
        let mut assembler = MockRtpAssembler::default();
        let mut frame = None;
        for packet in packets {
            if let Some(recovered) = assembler.push(&packet)? {
                frame = Some(recovered);
            }
        }

        self.frame_index = self.frame_index.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(90_000u32 / u32::from(self.fps));

        let mut frame = frame.ok_or(RtpError::EmptyPayload)?;
        frame.rtp_packets = rtp_packets;
        frame.rtp_bytes = rtp_bytes;
        Ok(frame)
    }

    fn packetize(&mut self, rgba: &[u8]) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        let total_len = rgba.len() as u32;
        let mut offset = 0usize;
        while offset < rgba.len() {
            let remaining = rgba.len() - offset;
            let chunk_len = remaining.min(MOCK_RTP_PAYLOAD_BYTES);
            let marker = offset + chunk_len == rgba.len();
            let mut packet = Vec::with_capacity(12 + MOCK_PAYLOAD_HEADER_LEN + chunk_len);
            packet.push(0x80);
            packet.push((if marker { 0x80 } else { 0x00 }) | MOCK_RTP_PAYLOAD_TYPE);
            packet.extend_from_slice(&self.sequence.to_be_bytes());
            packet.extend_from_slice(&self.timestamp.to_be_bytes());
            packet.extend_from_slice(&MOCK_RTP_SSRC.to_be_bytes());
            packet.extend_from_slice(MOCK_PAYLOAD_MAGIC);
            packet.extend_from_slice(&self.frame_index.to_be_bytes());
            packet.extend_from_slice(&self.width.to_be_bytes());
            packet.extend_from_slice(&self.height.to_be_bytes());
            packet.extend_from_slice(&total_len.to_be_bytes());
            packet.extend_from_slice(&(offset as u32).to_be_bytes());
            packet.extend_from_slice(&rgba[offset..offset + chunk_len]);
            packets.push(packet);
            self.sequence = self.sequence.wrapping_add(1);
            offset += chunk_len;
        }
        packets
    }
}

#[derive(Debug, Default)]
struct MockRtpAssembler {
    frame_index: Option<u64>,
    timestamp: u32,
    width: u16,
    height: u16,
    total_len: usize,
    rgba: Vec<u8>,
    received: Vec<bool>,
}

impl MockRtpAssembler {
    fn push(&mut self, packet: &[u8]) -> Result<Option<MockRtpFrame>, RtpError> {
        let header = RtpHeader::parse(packet)?;
        if header.payload_type != MOCK_RTP_PAYLOAD_TYPE {
            return Err(RtpError::UnsupportedPayload);
        }
        let payload = header.payload(packet);
        if payload.len() < MOCK_PAYLOAD_HEADER_LEN || &payload[..4] != MOCK_PAYLOAD_MAGIC {
            return Err(RtpError::UnsupportedPayload);
        }
        let frame_index = u64::from_be_bytes(payload[4..12].try_into().unwrap());
        let width = u16::from_be_bytes(payload[12..14].try_into().unwrap());
        let height = u16::from_be_bytes(payload[14..16].try_into().unwrap());
        let total_len = u32::from_be_bytes(payload[16..20].try_into().unwrap()) as usize;
        let offset = u32::from_be_bytes(payload[20..24].try_into().unwrap()) as usize;
        let bytes = &payload[24..];
        if total_len == 0 || offset + bytes.len() > total_len {
            return Err(RtpError::InvalidPadding);
        }
        if self.frame_index != Some(frame_index) {
            self.frame_index = Some(frame_index);
            self.timestamp = header.timestamp;
            self.width = width;
            self.height = height;
            self.total_len = total_len;
            self.rgba = vec![0; total_len];
            self.received = vec![false; total_len];
        }
        self.rgba[offset..offset + bytes.len()].copy_from_slice(bytes);
        self.received[offset..offset + bytes.len()].fill(true);

        if header.marker && self.received.iter().all(|received| *received) {
            Ok(Some(MockRtpFrame {
                width: self.width,
                height: self.height,
                frame_index,
                timestamp: self.timestamp,
                rgba: self.rgba.clone(),
                rtp_packets: 0,
                rtp_bytes: 0,
            }))
        } else {
            Ok(None)
        }
    }
}

fn render_mock_rgba(width: u16, height: u16, frame_index: u64) -> Vec<u8> {
    let width = usize::from(width);
    let height = usize::from(height);
    let mut rgba = vec![0; width * height * 4];
    let phase = (frame_index as usize * 3) % width.max(1);
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            let bar = ((x + phase) * 6 / width.max(1)) as u8;
            let grid = x % 32 == 0 || y % 32 == 0;
            let pulse = ((frame_index * 5 + y as u64) & 0xff) as u8;
            let (r, g, b): (u8, u8, u8) = match bar {
                0 => (236, 72, 85),
                1 => (245, 158, 11),
                2 => (34, 197, 94),
                3 => (20, 184, 166),
                4 => (59, 130, 246),
                _ => (168, 85, 247),
            };
            rgba[i] = if grid { 245 } else { r };
            rgba[i + 1] = if grid {
                245
            } else {
                g.saturating_add(pulse / 12)
            };
            rgba[i + 2] = if grid { 245 } else { b };
            rgba[i + 3] = 255;
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_pipeline_roundtrips_rgba_through_rtp_packets() {
        let mut mock = MockRtpPipeline::new(64, 36, 30);
        let frame = mock.next_frame().unwrap();
        assert_eq!(frame.width, 64);
        assert_eq!(frame.height, 36);
        assert_eq!(frame.frame_index, 0);
        assert_eq!(frame.rgba.len(), 64 * 36 * 4);
        assert!(frame.rtp_packets > 1);
        assert!(frame.rtp_bytes > frame.rgba.len());
    }
}
