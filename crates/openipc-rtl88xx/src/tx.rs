use openipc_core::radiotap::{
    parse_radiotap_tx_mode, radiotap_len, RadiotapError, TxMode, TxModeKind,
};

use crate::types::ChipFamily;

/// Size of the Realtek USB TX descriptor prepended before injected frames.
pub const TX_DESC_SIZE: usize = 40;
/// Size of the Jaguar3/RTL8822C USB TX descriptor.
pub const TX_DESC_SIZE_8822C: usize = 48;

const MGN_1M: u8 = 0x02;
const MGN_2M: u8 = 0x04;
const MGN_5_5M: u8 = 0x0b;
const MGN_6M: u8 = 0x0c;
const MGN_9M: u8 = 0x12;
const MGN_11M: u8 = 0x16;
const MGN_12M: u8 = 0x18;
const MGN_18M: u8 = 0x24;
const MGN_24M: u8 = 0x30;
const MGN_36M: u8 = 0x48;
const MGN_48M: u8 = 0x60;
const MGN_54M: u8 = 0x6c;
const MGN_MCS0: u8 = 0x80;
const MGN_VHT1SS_MCS0: u8 = 0xa0;

/// Realtek TX descriptor layout to prepend before an injected 802.11 frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RealtekTxDescriptor {
    /// RTL8812AU/RTL8821AU-style 40-byte descriptor.
    #[default]
    Jaguar1,
    /// RTL8814AU 40-byte descriptor variant.
    Rtl8814,
    /// RTL8812CU/RTL8822CU 48-byte Jaguar3 descriptor with descriptor checksum.
    Jaguar3,
}

impl RealtekTxDescriptor {
    /// Select the USB TX descriptor layout used by a supported chip family.
    pub const fn for_chip_family(family: ChipFamily) -> Self {
        match family {
            ChipFamily::Rtl8814 => Self::Rtl8814,
            ChipFamily::Rtl8822c => Self::Jaguar3,
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => Self::Jaguar1,
        }
    }
}

/// Options used while building a Realtek USB TX frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtekTxOptions {
    /// Current RF channel, used to avoid CCK rates on 5 GHz channels.
    pub current_channel: u8,
    /// TX descriptor layout to emit.
    pub descriptor: RealtekTxDescriptor,
    /// Force the legacy RTL8812 descriptor shape for compatibility testing.
    pub legacy_8812_descriptor: bool,
    /// Default TX mode when the radiotap header does not provide one.
    pub tx_mode_default: Option<TxMode>,
}

impl Default for RealtekTxOptions {
    fn default() -> Self {
        Self {
            current_channel: 36,
            descriptor: RealtekTxDescriptor::Jaguar1,
            legacy_8812_descriptor: false,
            tx_mode_default: None,
        }
    }
}

/// Error returned while constructing a Realtek TX USB frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtekTxError {
    /// Radiotap parsing failed.
    Radiotap(RadiotapError),
    /// 802.11 payload does not fit in the descriptor length field.
    PayloadTooLarge,
}

impl std::fmt::Display for RealtekTxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Radiotap(err) => write!(f, "{err}"),
            Self::PayloadTooLarge => write!(f, "802.11 TX payload is too large for descriptor"),
        }
    }
}

impl std::error::Error for RealtekTxError {}

impl From<RadiotapError> for RealtekTxError {
    fn from(value: RadiotapError) -> Self {
        Self::Radiotap(value)
    }
}

/// Convert a radiotap+802.11 packet into a Realtek USB TX frame.
pub fn build_usb_tx_frame(
    radiotap_packet: &[u8],
    options: RealtekTxOptions,
) -> Result<Vec<u8>, RealtekTxError> {
    let rtap_len = radiotap_len(radiotap_packet)?;
    let tx_mode = parse_radiotap_tx_mode(radiotap_packet)?
        .or(options.tx_mode_default)
        .unwrap_or_else(TxMode::legacy_1m);
    let payload = &radiotap_packet[rtap_len..];
    if payload.len() > u16::MAX as usize {
        return Err(RealtekTxError::PayloadTooLarge);
    }

    let mut fixed_rate = tx_mode_fixed_rate(tx_mode);
    if options.current_channel > 14 && is_cck_rate(fixed_rate) {
        fixed_rate = MGN_6M;
    }

    if options.descriptor == RealtekTxDescriptor::Jaguar3 {
        return build_usb_tx_frame_jaguar3(payload, tx_mode, fixed_rate);
    }

    let mut out = vec![0; TX_DESC_SIZE + payload.len()];
    set_bits_le32(
        &mut out,
        20,
        5,
        2,
        tx_mode.bandwidth.realtek_desc_bits() as u32,
    );
    let use_8814_descriptor_exceptions =
        options.descriptor == RealtekTxDescriptor::Rtl8814 && !options.legacy_8812_descriptor;

    set_bits_le32(&mut out, 0, 26, 1, 1);
    if !use_8814_descriptor_exceptions {
        set_bits_le32(&mut out, 0, 31, 1, 1);
    }
    set_bits_le32(&mut out, 0, 0, 16, payload.len() as u32);
    set_bits_le32(&mut out, 0, 16, 8, TX_DESC_SIZE as u32);
    set_bits_le32(&mut out, 4, 0, 7, 0x01);
    set_bits_le32(&mut out, 0, 24, 1, 1);
    set_bits_le32(
        &mut out,
        4,
        16,
        5,
        if tx_mode.kind == TxModeKind::Vht {
            9
        } else {
            8
        },
    );
    set_bits_le32(&mut out, 4, 8, 5, 0x12);
    if use_8814_descriptor_exceptions {
        set_bits_le32(&mut out, 24, 0, 12, 0x001);
        set_bits_le32(&mut out, 32, 15, 1, 1);
    } else {
        set_bits_le32(&mut out, 8, 24, 6, 0x3f);
        set_bits_le32(&mut out, 16, 18, 6, 12);
        set_bits_le32(&mut out, 24, 0, 12, 0x001);
        set_bits_le32(&mut out, 32, 15, 1, 1);
    }
    set_bits_le32(&mut out, 16, 17, 1, 1);
    if tx_mode.short_gi {
        set_bits_le32(&mut out, 20, 4, 1, 1);
    }
    set_bits_le32(&mut out, 12, 8, 1, 1);
    set_bits_le32(&mut out, 16, 0, 7, mrate_to_hw_rate(fixed_rate) as u32);
    if tx_mode.ldpc {
        set_bits_le32(&mut out, 20, 7, 1, 1);
    }
    set_bits_le32(&mut out, 20, 8, 2, u32::from(tx_mode.stbc));
    tx_desc_checksum(&mut out[..TX_DESC_SIZE]);
    out[TX_DESC_SIZE..].copy_from_slice(payload);
    Ok(out)
}

fn build_usb_tx_frame_jaguar3(
    payload: &[u8],
    tx_mode: TxMode,
    fixed_rate: u8,
) -> Result<Vec<u8>, RealtekTxError> {
    let mut out = vec![0; TX_DESC_SIZE_8822C + payload.len()];
    set_bits_le32(&mut out, 0, 0, 16, payload.len() as u32);
    set_bits_le32(&mut out, 0, 16, 8, TX_DESC_SIZE_8822C as u32);
    set_bits_le32(&mut out, 0, 26, 1, 1);
    set_bits_le32(&mut out, 4, 0, 7, 0x01);
    set_bits_le32(&mut out, 4, 8, 5, 0x12);
    set_bits_le32(
        &mut out,
        4,
        16,
        5,
        if tx_mode.kind == TxModeKind::Vht {
            9
        } else {
            8
        },
    );
    set_bits_le32(&mut out, 12, 8, 1, 1);
    set_bits_le32(&mut out, 12, 10, 1, 1);
    set_bits_le32(&mut out, 16, 0, 7, mrate_to_hw_rate(fixed_rate) as u32);
    if tx_mode.short_gi {
        set_bits_le32(&mut out, 20, 4, 1, 1);
    }
    set_bits_le32(
        &mut out,
        20,
        5,
        2,
        tx_mode.bandwidth.realtek_desc_bits() as u32,
    );
    if tx_mode.ldpc {
        set_bits_le32(&mut out, 20, 7, 1, 1);
    }
    set_bits_le32(&mut out, 20, 8, 2, u32::from(tx_mode.stbc));
    set_bits_le32(&mut out, 32, 15, 1, 1);
    tx_desc_checksum_8822c(&mut out[..TX_DESC_SIZE_8822C]);
    out[TX_DESC_SIZE_8822C..].copy_from_slice(payload);
    Ok(out)
}

fn tx_mode_fixed_rate(mode: TxMode) -> u8 {
    match mode.kind {
        TxModeKind::Legacy => mode.legacy_rate_500kbps,
        TxModeKind::Ht => MGN_MCS0 + mode.ht_mcs,
        TxModeKind::Vht => {
            MGN_VHT1SS_MCS0 + ((mode.vht_nss.clamp(1, 4) - 1) * 10 + mode.vht_mcs.min(9))
        }
    }
}

fn set_bits_le32(bytes: &mut [u8], offset: usize, bit_offset: u8, bit_len: u8, value: u32) {
    let mut word = u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("descriptor offset is in range"),
    );
    let mask = if bit_len == 32 {
        u32::MAX
    } else {
        ((1u32 << bit_len) - 1) << bit_offset
    };
    word = (word & !mask) | ((value << bit_offset) & mask);
    bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
}

fn tx_desc_checksum(desc: &mut [u8]) {
    set_bits_le32(desc, 28, 0, 16, 0);
    let mut checksum = 0u16;
    for idx in 0..16 {
        let offset = idx * 2;
        checksum ^= u16::from_le_bytes([desc[offset], desc[offset + 1]]);
    }
    set_bits_le32(desc, 28, 0, 16, checksum as u32);
}

fn tx_desc_checksum_8822c(desc: &mut [u8]) {
    set_bits_le32(desc, 28, 0, 16, 0);
    let pkt_offset = bits(le32(desc, 4), 24, 5) as usize;
    let pairs = (pkt_offset + (TX_DESC_SIZE_8822C >> 3)) << 1;
    let mut checksum = 0u16;
    for idx in 0..pairs {
        checksum ^= le16(desc, 2 * idx) ^ le16(desc, 2 * idx + 1);
    }
    set_bits_le32(desc, 28, 0, 16, checksum as u32);
}

const fn is_cck_rate(rate: u8) -> bool {
    matches!(rate, MGN_1M | MGN_2M | MGN_5_5M | MGN_11M)
}

const fn mrate_to_hw_rate(rate: u8) -> u8 {
    match rate {
        MGN_1M => 0x00,
        MGN_2M => 0x01,
        MGN_5_5M => 0x02,
        MGN_11M => 0x03,
        MGN_6M => 0x04,
        MGN_9M => 0x05,
        MGN_12M => 0x06,
        MGN_18M => 0x07,
        MGN_24M => 0x08,
        MGN_36M => 0x09,
        MGN_48M => 0x0a,
        MGN_54M => 0x0b,
        MGN_MCS0..=0x9f => 0x0c + (rate - MGN_MCS0),
        MGN_VHT1SS_MCS0..=0xc7 => 0x2c + (rate - MGN_VHT1SS_MCS0),
        _ => 0x00,
    }
}

fn le16(bytes: &[u8], word: usize) -> u16 {
    let offset = word * 2;
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn le32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("descriptor offset is in range"),
    )
}

fn bits(word: u32, offset: u8, len: u8) -> u32 {
    (word >> offset) & ((1u32 << len) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openipc_core::ieee80211::build_wfb_header_with_frame_type;
    use openipc_core::radiotap::{
        build_radiotap_header, ChannelBandwidth, TxRadioParams, FRAME_TYPE_RTS,
    };
    use openipc_core::ChannelId;

    fn read_le32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn test_bits(word: u32, offset: u8, len: u8) -> u32 {
        (word >> offset) & ((1u32 << len) - 1)
    }

    #[test]
    fn builds_descriptor_and_strips_radiotap() {
        let params = TxRadioParams::default();
        let mut packet = build_radiotap_header(params);
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));
        packet.extend_from_slice(&[1, 2, 3]);

        let usb = build_usb_tx_frame(&packet, RealtekTxOptions::default()).unwrap();
        assert_eq!(usb.len(), TX_DESC_SIZE + 27);
        assert_eq!(test_bits(read_le32(&usb, 0), 0, 16), 27);
        assert_eq!(test_bits(read_le32(&usb, 0), 16, 8), TX_DESC_SIZE as u32);
        assert_eq!(test_bits(read_le32(&usb, 0), 26, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 0), 27, 1), 0);
        assert_eq!(test_bits(read_le32(&usb, 0), 31, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 4), 16, 5), 8);
        assert_eq!(test_bits(read_le32(&usb, 8), 24, 6), 0x3f);
        assert_eq!(test_bits(read_le32(&usb, 16), 18, 6), 12);
        assert_eq!(test_bits(read_le32(&usb, 16), 0, 7), 0x0c);
        assert_eq!(test_bits(read_le32(&usb, 24), 0, 12), 1);
        assert_eq!(test_bits(read_le32(&usb, 32), 15, 1), 1);
        assert_eq!(
            &usb[TX_DESC_SIZE..TX_DESC_SIZE + 2],
            &[FRAME_TYPE_RTS, 0x01]
        );
    }

    #[test]
    fn rtl8812_descriptor_copies_80211_sequence() {
        let params = TxRadioParams::default();
        let mut packet = build_radiotap_header(params);
        let mut frame = build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        );
        frame[22] = 0x50;
        frame[23] = 0x12;
        packet.extend_from_slice(&frame);

        let usb = build_usb_tx_frame(&packet, RealtekTxOptions::default()).unwrap();
        assert_eq!(test_bits(read_le32(&usb, 36), 12, 12), 0);
        assert_eq!(test_bits(read_le32(&usb, 32), 15, 1), 1);
    }

    #[test]
    fn rtl8814_descriptor_keeps_kernel_matching_exceptions() {
        let params = TxRadioParams::default();
        let mut packet = build_radiotap_header(params);
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));

        let usb = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                descriptor: RealtekTxDescriptor::Rtl8814,
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(test_bits(read_le32(&usb, 0), 27, 1), 0);
        assert_eq!(test_bits(read_le32(&usb, 0), 31, 1), 0);
        assert_eq!(test_bits(read_le32(&usb, 24), 0, 12), 1);
        assert_eq!(test_bits(read_le32(&usb, 32), 15, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 8), 24, 6), 0);
        assert_eq!(test_bits(read_le32(&usb, 16), 18, 6), 0);
    }

    #[test]
    fn rtl8814_descriptor_can_use_legacy_8812_shape() {
        let params = TxRadioParams::default();
        let mut packet = build_radiotap_header(params);
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));

        let usb = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                descriptor: RealtekTxDescriptor::Rtl8814,
                legacy_8812_descriptor: true,
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(test_bits(read_le32(&usb, 0), 31, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 8), 24, 6), 0x3f);
        assert_eq!(test_bits(read_le32(&usb, 16), 18, 6), 12);
    }

    #[test]
    fn rateless_radiotap_uses_default_tx_mode() {
        let mut packet = vec![
            0x00, 0x00, 0x0c, 0x00, 0x00, 0x80, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
        ];
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));

        let mut mode = TxMode::ht(5);
        mode.bandwidth = ChannelBandwidth::Mhz40;
        mode.short_gi = true;
        let usb = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                tx_mode_default: Some(mode),
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(test_bits(read_le32(&usb, 16), 0, 7), 0x0c + 5);
        assert_eq!(test_bits(read_le32(&usb, 20), 5, 2), 1);
        assert_eq!(test_bits(read_le32(&usb, 20), 4, 1), 1);
    }

    #[test]
    fn jaguar3_descriptor_matches_devourer_offsets() {
        let params = TxRadioParams::default();
        let mut packet = build_radiotap_header(params);
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));

        let usb = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                descriptor: RealtekTxDescriptor::Jaguar3,
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(usb.len(), TX_DESC_SIZE_8822C + 24);
        assert_eq!(test_bits(read_le32(&usb, 0), 0, 16), 24);
        assert_eq!(
            test_bits(read_le32(&usb, 0), 16, 8),
            TX_DESC_SIZE_8822C as u32
        );
        assert_eq!(test_bits(read_le32(&usb, 0), 26, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 4), 0, 7), 1);
        assert_eq!(test_bits(read_le32(&usb, 4), 8, 5), 0x12);
        assert_eq!(test_bits(read_le32(&usb, 4), 16, 5), 8);
        assert_eq!(test_bits(read_le32(&usb, 12), 8, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 12), 10, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 16), 0, 7), 0x0c);
        assert_eq!(test_bits(read_le32(&usb, 32), 15, 1), 1);
        assert_ne!(test_bits(read_le32(&usb, 28), 0, 16), 0);
    }
}
