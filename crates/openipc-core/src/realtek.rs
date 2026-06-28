pub const RX_DESC_SIZE: usize = 24;
pub const DEFAULT_RX_TRANSFER_SIZE: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxPacketType {
    NormalRx,
    C2hPacket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxPacketAttrib {
    pub pkt_len: u16,
    pub physt: bool,
    pub drvinfo_sz: u8,
    pub shift_sz: u8,
    pub qos: bool,
    pub priority: u8,
    pub mdata: bool,
    pub seq_num: u16,
    pub frag_num: u8,
    pub mfrag: bool,
    pub bdecrypted: bool,
    pub encrypt: u8,
    pub crc_err: bool,
    pub icv_err: bool,
    pub tsfl: u32,
    pub data_rate: u8,
    pub bw: u8,
    pub stbc: u8,
    pub ldpc: u8,
    pub sgi: u8,
    pub scrambler: u8,
    pub rssi: [u8; 4],
    pub snr: [i8; 4],
    pub evm: [i8; 4],
    pub pkt_rpt_type: RxPacketType,
}

impl Default for RxPacketAttrib {
    fn default() -> Self {
        Self {
            pkt_len: 0,
            physt: false,
            drvinfo_sz: 0,
            shift_sz: 0,
            qos: false,
            priority: 0,
            mdata: false,
            seq_num: 0,
            frag_num: 0,
            mfrag: false,
            bdecrypted: false,
            encrypt: 0,
            crc_err: false,
            icv_err: false,
            tsfl: 0,
            data_rate: 0,
            bw: 0,
            stbc: 0,
            ldpc: 0,
            sgi: 0,
            scrambler: 0,
            rssi: [0; 4],
            snr: [0; 4],
            evm: [0; 4],
            pkt_rpt_type: RxPacketType::NormalRx,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RealtekRxPacket<'a> {
    pub attrib: RxPacketAttrib,
    pub data: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct C2hPacket<'a> {
    pub cmd_id: u8,
    pub seq: Option<u8>,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxStatusReport8814 {
    pub header_offset: usize,
    pub queue_select: u8,
    pub packet_broadcast: bool,
    pub lifetime_over: bool,
    pub retry_over: bool,
    pub mac_id: u8,
    pub data_retry_count: u8,
    pub queue_time_us: u32,
    pub final_data_rate: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateError {
    DescriptorTooShort,
    InvalidPacketLength {
        pkt_len: u16,
        pkt_offset: usize,
        remaining: usize,
    },
}

impl std::fmt::Display for AggregateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DescriptorTooShort => write!(f, "RX descriptor is shorter than {RX_DESC_SIZE} bytes"),
            Self::InvalidPacketLength {
                pkt_len,
                pkt_offset,
                remaining,
            } => write!(
                f,
                "invalid RX packet length: pkt_len={pkt_len}, pkt_offset={pkt_offset}, remaining={remaining}"
            ),
        }
    }
}

impl std::error::Error for AggregateError {}

pub fn parse_rx_descriptor(desc: &[u8]) -> Result<RxPacketAttrib, AggregateError> {
    if desc.len() < RX_DESC_SIZE {
        return Err(AggregateError::DescriptorTooShort);
    }

    let d0 = le32(desc, 0);
    let d1 = le32(desc, 4);
    let d2 = le32(desc, 8);
    let d3 = le32(desc, 12);
    let d4 = le32(desc, 16);
    let d5 = le32(desc, 20);

    Ok(RxPacketAttrib {
        pkt_len: bits(d0, 0, 14) as u16,
        crc_err: bits(d0, 14, 1) != 0,
        icv_err: bits(d0, 15, 1) != 0,
        drvinfo_sz: (bits(d0, 16, 4) * 8) as u8,
        encrypt: bits(d0, 20, 3) as u8,
        qos: bits(d0, 23, 1) != 0,
        shift_sz: bits(d0, 24, 2) as u8,
        physt: bits(d0, 26, 1) != 0,
        bdecrypted: bits(d0, 27, 1) == 0,
        priority: bits(d1, 8, 4) as u8,
        mdata: bits(d1, 26, 1) != 0,
        mfrag: bits(d1, 27, 1) != 0,
        seq_num: bits(d2, 0, 12) as u16,
        frag_num: bits(d2, 12, 4) as u8,
        pkt_rpt_type: if bits(d2, 28, 1) != 0 {
            RxPacketType::C2hPacket
        } else {
            RxPacketType::NormalRx
        },
        data_rate: bits(d3, 0, 7) as u8,
        sgi: bits(d4, 0, 1) as u8,
        ldpc: bits(d4, 1, 1) as u8,
        stbc: bits(d4, 2, 1) as u8,
        bw: bits(d4, 4, 2) as u8,
        scrambler: bits(d4, 9, 7) as u8,
        tsfl: d5,
        ..Default::default()
    })
}

pub fn parse_rx_aggregate(buf: &[u8]) -> Result<Vec<RealtekRxPacket<'_>>, AggregateError> {
    let mut packets = Vec::new();
    let mut offset = 0usize;

    while offset < buf.len() {
        let remaining = buf.len() - offset;
        if remaining < RX_DESC_SIZE {
            break;
        }

        let desc = &buf[offset..offset + RX_DESC_SIZE];
        let mut attrib = parse_rx_descriptor(desc)?;
        let data_start =
            offset + RX_DESC_SIZE + attrib.drvinfo_sz as usize + attrib.shift_sz as usize;
        let pkt_offset = RX_DESC_SIZE
            + attrib.drvinfo_sz as usize
            + attrib.shift_sz as usize
            + attrib.pkt_len as usize;
        if attrib.pkt_len == 0 || pkt_offset > remaining {
            return Err(AggregateError::InvalidPacketLength {
                pkt_len: attrib.pkt_len,
                pkt_offset,
                remaining,
            });
        }

        if attrib.pkt_rpt_type == RxPacketType::NormalRx {
            let phy_start = offset + RX_DESC_SIZE;
            let phy_end = phy_start + attrib.drvinfo_sz as usize;
            parse_phy_status(&mut attrib, &buf[phy_start..phy_end]);
        }

        let data_end = data_start + attrib.pkt_len as usize;
        packets.push(RealtekRxPacket {
            attrib,
            data: &buf[data_start..data_end],
        });

        let aligned = round_up_8(pkt_offset);
        if aligned >= remaining {
            break;
        }
        offset += aligned;
    }

    Ok(packets)
}

pub fn parse_c2h_packet(data: &[u8]) -> Option<C2hPacket<'_>> {
    let (&cmd_id, rest) = data.split_first()?;
    let seq = rest.first().copied();
    let payload = if rest.len() > 1 { &rest[1..] } else { rest };
    Some(C2hPacket {
        cmd_id,
        seq,
        payload,
    })
}

pub fn parse_8814_tx_status_reports(data: &[u8]) -> Vec<TxStatusReport8814> {
    [1usize, 2usize]
        .into_iter()
        .filter_map(|offset| parse_8814_tx_status_report_at(data, offset))
        .collect()
}

pub fn parse_8814_tx_status_report_at(
    data: &[u8],
    header_offset: usize,
) -> Option<TxStatusReport8814> {
    let h = data.get(header_offset..header_offset + 6)?;
    let queue_time_raw = u16::from_le_bytes([h[3], h[4]]);
    Some(TxStatusReport8814 {
        header_offset,
        queue_select: h[0] & 0x1f,
        packet_broadcast: h[0] & (1 << 5) != 0,
        lifetime_over: h[0] & (1 << 6) != 0,
        retry_over: h[0] & (1 << 7) != 0,
        mac_id: h[1],
        data_retry_count: h[2] & 0x3f,
        queue_time_us: u32::from(queue_time_raw) * 256,
        final_data_rate: h[5],
    })
}

const fn round_up_8(value: usize) -> usize {
    (value + 7) & !7
}

fn le32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("descriptor length checked"),
    )
}

const fn bits(word: u32, offset: u8, len: u8) -> u32 {
    if len == 32 {
        word
    } else {
        (word >> offset) & ((1u32 << len) - 1)
    }
}

fn parse_phy_status(attrib: &mut RxPacketAttrib, phy: &[u8]) {
    if phy.len() < 2 {
        return;
    }

    attrib.rssi[0] = phy[0];
    attrib.rssi[1] = phy[1];

    if phy.len() < 28 {
        return;
    }

    attrib.rssi[2] = phy[23];
    attrib.rssi[3] = phy[24];
    attrib.snr[0] = phy[15] as i8;
    attrib.snr[1] = phy[16] as i8;
    attrib.snr[2] = phy[21] as i8;
    attrib.snr[3] = phy[22] as i8;
    attrib.evm[0] = phy[13] as i8;
    attrib.evm[1] = phy[14] as i8;
    attrib.evm[2] = phy[19] as i8;
    attrib.evm[3] = phy[20] as i8;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_bits(word: &mut u32, offset: u8, len: u8, value: u32) {
        let mask = ((1u32 << len) - 1) << offset;
        *word = (*word & !mask) | ((value << offset) & mask);
    }

    fn descriptor(pkt_len: u16, drvinfo_units: u8, shift: u8, seq: u16) -> [u8; RX_DESC_SIZE] {
        let mut desc = [0; RX_DESC_SIZE];
        let mut d0 = 0u32;
        put_bits(&mut d0, 0, 14, pkt_len as u32);
        put_bits(&mut d0, 16, 4, drvinfo_units as u32);
        put_bits(&mut d0, 24, 2, shift as u32);
        let mut d2 = 0u32;
        put_bits(&mut d2, 0, 12, seq as u32);
        desc[0..4].copy_from_slice(&d0.to_le_bytes());
        desc[8..12].copy_from_slice(&d2.to_le_bytes());
        desc
    }

    #[test]
    fn parses_single_rx_packet() {
        let mut aggregate = Vec::new();
        aggregate.extend_from_slice(&descriptor(4, 0, 0, 77));
        aggregate.extend_from_slice(&[1, 2, 3, 4]);

        let packets = parse_rx_aggregate(&aggregate).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].attrib.pkt_len, 4);
        assert_eq!(packets[0].attrib.seq_num, 77);
        assert_eq!(packets[0].data, &[1, 2, 3, 4]);
    }

    #[test]
    fn parses_c2h_packet_header() {
        let packet = parse_c2h_packet(&[0x42, 0x7f, 1, 2]).unwrap();
        assert_eq!(packet.cmd_id, 0x42);
        assert_eq!(packet.seq, Some(0x7f));
        assert_eq!(packet.payload, &[1, 2]);
    }

    #[test]
    fn parses_8814_tx_status_at_devourer_offsets() {
        let bytes = [0xaa, 0x83, 0x09, 0x25, 0x34, 0x12, 0x6c, 0xbb];
        let reports = parse_8814_tx_status_reports(&bytes);
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].header_offset, 1);
        assert_eq!(reports[0].queue_select, 3);
        assert!(reports[0].retry_over);
        assert_eq!(reports[0].mac_id, 9);
        assert_eq!(reports[0].data_retry_count, 0x25);
        assert_eq!(reports[0].queue_time_us, 0x1234 * 256);
        assert_eq!(reports[0].final_data_rate, 0x6c);
    }

    #[test]
    fn advances_by_jaguar_eight_byte_alignment() {
        let mut aggregate = Vec::new();
        aggregate.extend_from_slice(&descriptor(5, 0, 0, 1));
        aggregate.extend_from_slice(&[1, 2, 3, 4, 5]);
        aggregate.extend_from_slice(&[0, 0, 0]);
        aggregate.extend_from_slice(&descriptor(3, 0, 0, 2));
        aggregate.extend_from_slice(&[6, 7, 8]);

        let packets = parse_rx_aggregate(&aggregate).unwrap();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].data, &[1, 2, 3, 4, 5]);
        assert_eq!(packets[1].data, &[6, 7, 8]);
    }
}
