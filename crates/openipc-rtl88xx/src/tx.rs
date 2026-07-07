use openipc_core::radiotap::{radiotap_len, ChannelBandwidth, RadiotapError, TxMode, TxModeKind};

use crate::types::{ChannelWidth, ChipFamily};
use crate::{jaguar2_packet_power_step, TxCapabilities};

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
    /// RTL8812BU/RTL8822BU 48-byte descriptor with a 32-byte checksum span.
    Jaguar2,
    /// RTL8812CU/EU and RTL8822CU/EU 48-byte Jaguar3 descriptor with checksum.
    Jaguar3,
}

impl RealtekTxDescriptor {
    /// Select the USB TX descriptor layout used by a supported chip family.
    pub const fn for_chip_family(family: ChipFamily) -> Self {
        match family {
            ChipFamily::Rtl8814 => Self::Rtl8814,
            ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => Self::Jaguar2,
            ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => Self::Jaguar3,
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => Self::Jaguar1,
        }
    }

    /// Whether Devourer's USB path requests a terminating zero-length packet.
    pub const fn uses_terminated_bulk_out(self) -> bool {
        matches!(self, Self::Jaguar1 | Self::Rtl8814)
    }
}

/// Return whether Devourer's `ADD_ZERO_PACKET` flag emits a ZLP for this frame.
pub const fn bulk_out_requires_zlp(
    descriptor: RealtekTxDescriptor,
    transfer_len: usize,
    max_packet_size: usize,
) -> bool {
    descriptor.uses_terminated_bulk_out()
        && transfer_len != 0
        && max_packet_size != 0
        && transfer_len.is_multiple_of(max_packet_size)
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
    /// RF bandwidth configured on the adapter, used for subchannel placement.
    pub configured_channel_width: ChannelWidth,
    /// Primary-channel index within a configured 40/80 MHz channel.
    pub configured_channel_offset: u8,
    /// Mark this frame as an NDPA and disable ordinary sequence/fallback handling.
    pub beamforming_ndpa: bool,
    /// Force periodic Jaguar3 sounding frames to VHT 2SS MCS0.
    pub beamforming_ndpa_periodic: bool,
    /// Session-default Jaguar2 `TXPWR_OFSET` step used when radiotap omits one.
    pub jaguar2_packet_power_step: u8,
    /// Probed hardware capabilities used to reject unsupported modulation.
    pub capabilities: Option<TxCapabilities>,
}

impl Default for RealtekTxOptions {
    fn default() -> Self {
        Self {
            current_channel: 36,
            descriptor: RealtekTxDescriptor::Jaguar1,
            legacy_8812_descriptor: false,
            tx_mode_default: None,
            configured_channel_width: ChannelWidth::Mhz20,
            configured_channel_offset: 0,
            beamforming_ndpa: false,
            beamforming_ndpa_periodic: false,
            jaguar2_packet_power_step: 0,
            capabilities: None,
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
    let metadata = openipc_core::parse_radiotap_tx_metadata(radiotap_packet)?;
    let mut tx_mode = metadata
        .mode
        .or(options.tx_mode_default)
        .unwrap_or_else(TxMode::legacy_1m);
    if options.beamforming_ndpa_periodic {
        // Devourer forces periodic Jaguar3 sounding to a two-stream NDP while
        // preserving the caller's bandwidth/coding flags.
        tx_mode.kind = TxModeKind::Vht;
        tx_mode.vht_nss = 2;
        tx_mode.vht_mcs = 0;
    }
    let payload = &radiotap_packet[rtap_len..];
    if payload.len() > u16::MAX as usize {
        return Err(RealtekTxError::PayloadTooLarge);
    }
    if tx_mode.stbc
        && options
            .capabilities
            .is_some_and(|capabilities| !capabilities.stbc)
    {
        log::warn!(target: "openipc_rtl88xx::tx", "STBC requested on a 1T1R adapter; clearing STBC so the frame remains decodable");
        tx_mode.stbc = false;
    }

    let mut fixed_rate = tx_mode_fixed_rate(tx_mode);
    if options.current_channel > 14 && is_cck_rate(fixed_rate) {
        fixed_rate = MGN_6M;
    }

    if matches!(
        options.descriptor,
        RealtekTxDescriptor::Jaguar2 | RealtekTxDescriptor::Jaguar3
    ) {
        return build_usb_tx_frame_halmac(
            payload,
            tx_mode,
            fixed_rate,
            metadata.dbm_tx_power,
            options,
        );
    }

    let mut out = vec![0; TX_DESC_SIZE + payload.len()];
    set_bits_le32(
        &mut out,
        20,
        5,
        2,
        tx_mode.bandwidth.realtek_desc_bits() as u32,
    );
    set_bits_le32(&mut out, 20, 0, 4, data_subchannel(options, tx_mode));
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
    if options.beamforming_ndpa {
        apply_ndpa_descriptor_fields(&mut out);
    }
    tx_desc_checksum(&mut out[..TX_DESC_SIZE]);
    out[TX_DESC_SIZE..].copy_from_slice(payload);
    Ok(out)
}

fn build_usb_tx_frame_halmac(
    payload: &[u8],
    tx_mode: TxMode,
    fixed_rate: u8,
    packet_power_db: Option<i8>,
    options: RealtekTxOptions,
) -> Result<Vec<u8>, RealtekTxError> {
    let mut out = vec![0; TX_DESC_SIZE_8822C + payload.len()];
    set_bits_le32(&mut out, 0, 0, 16, payload.len() as u32);
    set_bits_le32(&mut out, 0, 16, 8, TX_DESC_SIZE_8822C as u32);
    if payload.get(4).is_some_and(|octet| octet & 1 != 0) {
        set_bits_le32(&mut out, 0, 24, 1, 1);
    }
    set_bits_le32(&mut out, 0, 26, 1, 1);
    set_bits_le32(&mut out, 0, 31, 1, 1);
    set_bits_le32(&mut out, 4, 0, 7, 0x01);
    set_bits_le32(&mut out, 4, 8, 5, 0x12);
    set_bits_le32(&mut out, 4, 16, 5, 9);
    set_bits_le32(&mut out, 8, 24, 6, 0x3f);
    set_bits_le32(&mut out, 12, 8, 1, 1);
    set_bits_le32(&mut out, 16, 17, 1, 1);
    set_bits_le32(&mut out, 16, 18, 6, 12);
    set_bits_le32(&mut out, 24, 0, 12, 1);
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
    set_bits_le32(&mut out, 20, 0, 4, data_subchannel(options, tx_mode));
    if tx_mode.ldpc {
        set_bits_le32(&mut out, 20, 7, 1, 1);
    }
    set_bits_le32(&mut out, 20, 8, 2, u32::from(tx_mode.stbc));
    if options.descriptor == RealtekTxDescriptor::Jaguar2 {
        let step = packet_power_db.map_or(options.jaguar2_packet_power_step, |db| {
            jaguar2_packet_power_step(db)
        });
        set_bits_le32(&mut out, 20, 28, 3, u32::from(step & 0x07));
    }
    set_bits_le32(&mut out, 32, 15, 1, 1);
    if options.beamforming_ndpa {
        apply_ndpa_descriptor_fields(&mut out);
    }
    match options.descriptor {
        RealtekTxDescriptor::Jaguar2 => tx_desc_checksum_8822b(&mut out[..TX_DESC_SIZE_8822C]),
        RealtekTxDescriptor::Jaguar3 => tx_desc_checksum_8822c(&mut out[..TX_DESC_SIZE_8822C]),
        RealtekTxDescriptor::Jaguar1 | RealtekTxDescriptor::Rtl8814 => {
            unreachable!("HalMAC builder only accepts Jaguar2/3 descriptors")
        }
    }
    out[TX_DESC_SIZE_8822C..].copy_from_slice(payload);
    Ok(out)
}

fn data_subchannel(options: RealtekTxOptions, tx_mode: TxMode) -> u32 {
    if options.descriptor == RealtekTxDescriptor::Jaguar3 {
        return u32::from(
            options.configured_channel_width == ChannelWidth::Mhz80
                && tx_mode.bandwidth == ChannelBandwidth::Mhz40,
        ) * 10;
    }
    if options.descriptor != RealtekTxDescriptor::Jaguar2 {
        return 0;
    }
    let primary = options.configured_channel_offset;
    match (options.configured_channel_width, tx_mode.bandwidth) {
        (ChannelWidth::Mhz80, ChannelBandwidth::Mhz40) => {
            if primary <= 2 {
                10
            } else {
                9
            }
        }
        (ChannelWidth::Mhz80, ChannelBandwidth::Mhz20) => match primary {
            1 => 4,
            2 => 2,
            3 => 1,
            4 => 3,
            _ => 0,
        },
        (ChannelWidth::Mhz40, ChannelBandwidth::Mhz20) => match primary {
            1 => 2,
            2 => 1,
            _ => 0,
        },
        _ => 0,
    }
}

pub(crate) fn build_firmware_page_8822b(chunk: &[u8]) -> (Vec<u8>, usize) {
    const PACKET_OFFSET_SIZE: usize = 8;
    let packet_offset = if (TX_DESC_SIZE_8822C + chunk.len()).is_multiple_of(512) {
        PACKET_OFFSET_SIZE
    } else {
        0
    };
    let mut out = vec![0; TX_DESC_SIZE_8822C + packet_offset + chunk.len()];
    set_bits_le32(&mut out, 0, 0, 16, chunk.len() as u32);
    set_bits_le32(
        &mut out,
        0,
        16,
        8,
        (TX_DESC_SIZE_8822C + packet_offset) as u32,
    );
    set_bits_le32(&mut out, 4, 8, 5, 0x10);
    if packet_offset != 0 {
        set_bits_le32(&mut out, 4, 24, 5, 1);
    }
    tx_desc_checksum_8822b(&mut out[..TX_DESC_SIZE_8822C]);
    out[TX_DESC_SIZE_8822C + packet_offset..].copy_from_slice(chunk);
    (out, packet_offset)
}

fn apply_ndpa_descriptor_fields(desc: &mut [u8]) {
    set_bits_le32(desc, 12, 22, 2, 1);
    set_bits_le32(desc, 32, 15, 1, 0);
    set_bits_le32(desc, 0, 24, 1, 0);
    set_bits_le32(desc, 12, 15, 1, 1);
    set_bits_le32(desc, 12, 10, 1, 1);
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

fn tx_desc_checksum_8822b(desc: &mut [u8]) {
    set_bits_le32(desc, 28, 0, 16, 0);
    let mut checksum = 0u16;
    for pair in 0..8 {
        checksum ^= le16(desc, pair * 2) ^ le16(desc, pair * 2 + 1);
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

    #[test]
    fn zlp_rule_matches_devourer_add_zero_packet() {
        assert!(bulk_out_requires_zlp(
            RealtekTxDescriptor::Jaguar1,
            1024,
            512
        ));
        assert!(bulk_out_requires_zlp(
            RealtekTxDescriptor::Rtl8814,
            512,
            512
        ));
        assert!(!bulk_out_requires_zlp(
            RealtekTxDescriptor::Jaguar1,
            513,
            512
        ));
        assert!(!bulk_out_requires_zlp(
            RealtekTxDescriptor::Jaguar2,
            512,
            512
        ));
    }

    #[test]
    fn jaguar2_subchannel_mapping_matches_devourer() {
        let mut mode = TxMode::ht(0);
        mode.bandwidth = ChannelBandwidth::Mhz20;
        let options = RealtekTxOptions {
            descriptor: RealtekTxDescriptor::Jaguar2,
            configured_channel_width: ChannelWidth::Mhz80,
            configured_channel_offset: 3,
            ..RealtekTxOptions::default()
        };
        assert_eq!(data_subchannel(options, mode), 1);
        mode.bandwidth = ChannelBandwidth::Mhz40;
        assert_eq!(data_subchannel(options, mode), 9);
    }

    #[test]
    fn periodic_jaguar3_sounding_forces_vht_two_stream_mcs0() {
        let mut packet = openipc_core::build_stream_radiotap(TxMode::ht(4));
        packet.extend_from_slice(&[0u8; 24]);
        let frame = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                descriptor: RealtekTxDescriptor::Jaguar3,
                beamforming_ndpa: true,
                beamforming_ndpa_periodic: true,
                capabilities: Some(TxCapabilities::for_family(ChipFamily::Rtl8822e)),
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        // MGN_VHT2SS_MCS0 maps through MRateToHwRate to DESC_RATEVHT2SS_MCS0.
        assert_eq!(test_bits(read_le32(&frame, 16), 0, 7), 0x36);
        assert_eq!(test_bits(read_le32(&frame, 12), 22, 2), 1);
    }

    fn radiotap_with_packet_power(db: i8) -> Vec<u8> {
        let present = (1u32 << 2) | (1u32 << 10);
        let mut packet = vec![0, 0, 10, 0];
        packet.extend_from_slice(&present.to_le_bytes());
        packet.push(12);
        packet.push(db as u8);
        packet.extend_from_slice(&[0u8; 24]);
        packet
    }

    #[test]
    fn jaguar2_descriptor_honors_radiotap_packet_power_delta() {
        let frame = build_usb_tx_frame(
            &radiotap_with_packet_power(-7),
            RealtekTxOptions {
                descriptor: RealtekTxDescriptor::Jaguar2,
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        let word = u32::from_le_bytes(frame[20..24].try_into().unwrap());
        assert_eq!((word >> 28) & 0x07, 2);
    }

    #[test]
    fn one_stream_capability_clears_stbc() {
        let mut mode = TxMode::ht(0);
        mode.stbc = true;
        let mut packet = openipc_core::build_stream_radiotap(mode);
        packet.extend_from_slice(&[0u8; 24]);
        let frame = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                capabilities: Some(TxCapabilities {
                    supported: true,
                    spatial_streams: 1,
                    stbc: false,
                    ldpc: true,
                    short_gi: true,
                    max_bandwidth_mhz: 80,
                }),
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(test_bits(read_le32(&frame, 20), 8, 2), 0);
    }
    use openipc_core::ieee80211::build_wfb_header_with_frame_type;
    use openipc_core::radiotap::{
        build_radiotap_header, build_stream_radiotap, build_stream_radiotap_on_channel,
        ChannelBandwidth, TxRadioParams, FRAME_TYPE_RTS,
    };
    use openipc_core::ChannelId;

    fn read_le32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn test_bits(word: u32, offset: u8, len: u8) -> u32 {
        (word >> offset) & ((1u32 << len) - 1)
    }

    fn checksum_8822c_descriptor(desc: &[u8]) -> u16 {
        let mut copy = desc.to_vec();
        copy[28] = 0;
        copy[29] = 0;
        let pkt_offset = test_bits(read_le32(&copy, 4), 24, 5) as usize;
        let pairs = (pkt_offset + (TX_DESC_SIZE_8822C >> 3)) << 1;
        let mut checksum = 0u16;
        for idx in 0..pairs {
            checksum ^= le16(&copy, 2 * idx) ^ le16(&copy, 2 * idx + 1);
        }
        checksum
    }

    fn checksum_8822b_descriptor(desc: &[u8]) -> u16 {
        let mut copy = desc.to_vec();
        copy[28] = 0;
        copy[29] = 0;
        (0..16).fold(0u16, |checksum, word| checksum ^ le16(&copy, word))
    }

    fn checksum_jaguar1_descriptor(desc: &[u8]) -> u16 {
        let mut copy = desc.to_vec();
        copy[28] = 0;
        copy[29] = 0;
        let mut checksum = 0u16;
        for idx in 0..16 {
            checksum ^= le16(&copy, idx);
        }
        checksum
    }

    #[test]
    fn descriptor_selector_matches_chip_family() {
        assert_eq!(
            RealtekTxDescriptor::for_chip_family(ChipFamily::Rtl8812),
            RealtekTxDescriptor::Jaguar1
        );
        assert_eq!(
            RealtekTxDescriptor::for_chip_family(ChipFamily::Rtl8821),
            RealtekTxDescriptor::Jaguar1
        );
        assert_eq!(
            RealtekTxDescriptor::for_chip_family(ChipFamily::Rtl8814),
            RealtekTxDescriptor::Rtl8814
        );
        assert_eq!(
            RealtekTxDescriptor::for_chip_family(ChipFamily::Rtl8822c),
            RealtekTxDescriptor::Jaguar3
        );
        assert_eq!(
            RealtekTxDescriptor::for_chip_family(ChipFamily::Rtl8822e),
            RealtekTxDescriptor::Jaguar3
        );
    }

    #[test]
    fn rejects_payload_larger_than_descriptor_length_field() {
        let mut packet = vec![0x00, 0x00, 0x08, 0x00, 0, 0, 0, 0];
        packet.extend(std::iter::repeat_n(0u8, u16::MAX as usize + 1));

        let err = build_usb_tx_frame(&packet, RealtekTxOptions::default()).unwrap_err();

        assert_eq!(err, RealtekTxError::PayloadTooLarge);
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
            test_bits(read_le32(&usb, 28), 0, 16) as u16,
            checksum_jaguar1_descriptor(&usb[..TX_DESC_SIZE])
        );
        assert_eq!(
            &usb[TX_DESC_SIZE..TX_DESC_SIZE + 2],
            &[FRAME_TYPE_RTS, 0x01]
        );
    }

    #[test]
    fn channel_radiotap_is_stripped_without_changing_the_payload() {
        let mut packet =
            build_stream_radiotap_on_channel(TxMode::ht(4), 44).expect("valid WiFi channel");
        let frame = build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        );
        packet.extend_from_slice(&frame);

        let usb = build_usb_tx_frame(&packet, RealtekTxOptions::default()).unwrap();

        assert_eq!(&usb[TX_DESC_SIZE..], &frame);
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
    fn vht_descriptor_sets_rate_id_rate_and_phy_flags() {
        let mut mode = TxMode::vht(2, 3);
        mode.bandwidth = ChannelBandwidth::Mhz80;
        mode.short_gi = true;
        mode.ldpc = true;
        mode.stbc = true;
        let mut packet = build_stream_radiotap(mode);
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));

        let usb = build_usb_tx_frame(&packet, RealtekTxOptions::default()).unwrap();

        assert_eq!(test_bits(read_le32(&usb, 4), 16, 5), 9);
        assert_eq!(test_bits(read_le32(&usb, 16), 0, 7), 0x2c + 13);
        assert_eq!(test_bits(read_le32(&usb, 20), 4, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 20), 5, 2), 2);
        assert_eq!(test_bits(read_le32(&usb, 20), 7, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 20), 8, 2), 1);
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
        assert_eq!(test_bits(read_le32(&usb, 0), 24, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 0), 26, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 0), 31, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 4), 0, 7), 1);
        assert_eq!(test_bits(read_le32(&usb, 4), 8, 5), 0x12);
        assert_eq!(test_bits(read_le32(&usb, 4), 16, 5), 9);
        assert_eq!(test_bits(read_le32(&usb, 8), 24, 6), 0x3f);
        assert_eq!(test_bits(read_le32(&usb, 12), 8, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 12), 10, 1), 0);
        assert_eq!(test_bits(read_le32(&usb, 16), 0, 7), 0x0c);
        assert_eq!(test_bits(read_le32(&usb, 16), 17, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 16), 18, 6), 12);
        assert_eq!(test_bits(read_le32(&usb, 24), 0, 12), 1);
        assert_eq!(test_bits(read_le32(&usb, 32), 15, 1), 1);
        assert_ne!(test_bits(read_le32(&usb, 28), 0, 16), 0);
        assert_eq!(
            test_bits(read_le32(&usb, 28), 0, 16) as u16,
            checksum_8822c_descriptor(&usb[..TX_DESC_SIZE_8822C])
        );
    }

    #[test]
    fn jaguar3_descriptor_clamps_5ghz_cck_to_ofdm() {
        let mut packet = vec![
            0x00, 0x00, 0x0c, 0x00, 0x00, 0x80, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
        ];
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));

        let usb = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                current_channel: 161,
                descriptor: RealtekTxDescriptor::Jaguar3,
                tx_mode_default: Some(TxMode::legacy_1m()),
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(test_bits(read_le32(&usb, 16), 0, 7), 0x04);
    }

    #[test]
    fn jaguar2_descriptor_uses_first_32_byte_checksum() {
        let mut packet = build_radiotap_header(TxRadioParams::default());
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));
        let usb = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                descriptor: RealtekTxDescriptor::Jaguar2,
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(usb.len(), TX_DESC_SIZE_8822C + 24);
        assert_eq!(
            test_bits(read_le32(&usb, 28), 0, 16) as u16,
            checksum_8822b_descriptor(&usb[..TX_DESC_SIZE_8822C])
        );
    }

    #[test]
    fn jaguar3_places_40mhz_frame_in_lower_half_of_80mhz_rf_channel() {
        let mut mode = TxMode::ht(0);
        mode.bandwidth = ChannelBandwidth::Mhz40;
        let mut packet = build_stream_radiotap(mode);
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));
        let usb = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                descriptor: RealtekTxDescriptor::Jaguar3,
                configured_channel_width: ChannelWidth::Mhz80,
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(test_bits(read_le32(&usb, 20), 0, 4), 10);
    }

    #[test]
    fn ndpa_descriptor_disables_normal_sequence_and_fallback() {
        let mut packet = build_radiotap_header(TxRadioParams::default());
        packet.extend_from_slice(&build_wfb_header_with_frame_type(
            ChannelId::default_video(),
            [0x10, 0x00],
            FRAME_TYPE_RTS,
        ));
        let usb = build_usb_tx_frame(
            &packet,
            RealtekTxOptions {
                descriptor: RealtekTxDescriptor::Jaguar3,
                beamforming_ndpa: true,
                ..RealtekTxOptions::default()
            },
        )
        .unwrap();
        assert_eq!(test_bits(read_le32(&usb, 12), 22, 2), 1);
        assert_eq!(test_bits(read_le32(&usb, 32), 15, 1), 0);
        assert_eq!(test_bits(read_le32(&usb, 0), 24, 1), 0);
        assert_eq!(test_bits(read_le32(&usb, 12), 15, 1), 1);
        assert_eq!(test_bits(read_le32(&usb, 12), 10, 1), 1);
    }

    #[test]
    fn rtl8822b_firmware_page_adds_alignment_offset() {
        let chunk = vec![0x5a; 512 - TX_DESC_SIZE_8822C];
        let (page, offset) = build_firmware_page_8822b(&chunk);
        assert_eq!(offset, 8);
        assert_eq!(test_bits(read_le32(&page, 0), 16, 8), 56);
        assert_eq!(test_bits(read_le32(&page, 4), 24, 5), 1);
        assert_eq!(&page[56..], chunk.as_slice());
        assert_eq!(
            test_bits(read_le32(&page, 28), 0, 16) as u16,
            checksum_8822b_descriptor(&page[..TX_DESC_SIZE_8822C])
        );
    }
}
