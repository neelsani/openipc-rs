pub const FRAME_TYPE_DATA: u8 = 0x08;
pub const FRAME_TYPE_RTS: u8 = 0xb4;

const IEEE80211_RADIOTAP_MCS_HAVE_BW: u8 = 0x01;
const IEEE80211_RADIOTAP_MCS_HAVE_MCS: u8 = 0x02;
const IEEE80211_RADIOTAP_MCS_HAVE_GI: u8 = 0x04;
const IEEE80211_RADIOTAP_MCS_HAVE_FEC: u8 = 0x10;
const IEEE80211_RADIOTAP_MCS_HAVE_STBC: u8 = 0x20;
const IEEE80211_RADIOTAP_MCS_SGI: u8 = 0x04;
const IEEE80211_RADIOTAP_MCS_FEC_LDPC: u8 = 0x10;
const IEEE80211_RADIOTAP_MCS_STBC_SHIFT: u8 = 5;

const IEEE80211_RADIOTAP_VHT_FLAG_STBC: u8 = 0x01;
const IEEE80211_RADIOTAP_VHT_FLAG_SGI: u8 = 0x04;
const IEEE80211_RADIOTAP_VHT_CODING_LDPC_USER0: u8 = 0x01;

pub const RADIOTAP_HT_LEN: usize = 13;
pub const RADIOTAP_VHT_LEN: usize = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelBandwidth {
    Mhz20,
    Mhz40,
    Mhz80,
    Mhz160,
}

impl ChannelBandwidth {
    pub const fn realtek_desc_bits(self) -> u8 {
        match self {
            Self::Mhz20 => 0,
            Self::Mhz40 => 1,
            Self::Mhz80 | Self::Mhz160 => 2,
        }
    }

    const fn ht_mcs_bits(self) -> u8 {
        match self {
            Self::Mhz20 => 0,
            Self::Mhz40 | Self::Mhz80 | Self::Mhz160 => 1,
        }
    }

    const fn vht_bits(self) -> u8 {
        match self {
            Self::Mhz20 => 0x00,
            Self::Mhz40 => 0x01,
            Self::Mhz80 => 0x04,
            Self::Mhz160 => 0x0b,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxRadioParams {
    pub mcs_index: u8,
    pub nss: u8,
    pub bandwidth: ChannelBandwidth,
    pub short_gi: bool,
    pub stbc: u8,
    pub ldpc: bool,
    pub vht: bool,
    pub frame_type: u8,
}

impl TxRadioParams {
    pub const fn openipc_uplink_default() -> Self {
        Self {
            mcs_index: 0,
            nss: 1,
            bandwidth: ChannelBandwidth::Mhz20,
            short_gi: false,
            stbc: 1,
            ldpc: true,
            vht: false,
            frame_type: FRAME_TYPE_RTS,
        }
    }
}

impl Default for TxRadioParams {
    fn default() -> Self {
        Self::openipc_uplink_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadiotapTxInfo {
    pub vht: bool,
    pub mcs_index: u8,
    pub nss: u8,
    pub bandwidth: ChannelBandwidth,
    pub short_gi: bool,
    pub stbc: u8,
    pub ldpc: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RadiotapError {
    TooShort,
    InvalidLength,
    UnsupportedHeader,
}

impl std::fmt::Display for RadiotapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => write!(f, "radiotap packet is too short"),
            Self::InvalidLength => write!(f, "radiotap length is invalid"),
            Self::UnsupportedHeader => write!(f, "unsupported radiotap TX header"),
        }
    }
}

impl std::error::Error for RadiotapError {}

pub fn build_radiotap_header(params: TxRadioParams) -> Vec<u8> {
    if params.vht {
        build_vht_radiotap_header(params)
    } else {
        build_ht_radiotap_header(params)
    }
}

pub fn build_ht_radiotap_header(params: TxRadioParams) -> Vec<u8> {
    let known = IEEE80211_RADIOTAP_MCS_HAVE_MCS
        | IEEE80211_RADIOTAP_MCS_HAVE_BW
        | IEEE80211_RADIOTAP_MCS_HAVE_GI
        | IEEE80211_RADIOTAP_MCS_HAVE_STBC
        | IEEE80211_RADIOTAP_MCS_HAVE_FEC;
    let mut flags = params.bandwidth.ht_mcs_bits();
    if params.short_gi {
        flags |= IEEE80211_RADIOTAP_MCS_SGI;
    }
    if params.ldpc {
        flags |= IEEE80211_RADIOTAP_MCS_FEC_LDPC;
    }
    flags |= (params.stbc & 0x03) << IEEE80211_RADIOTAP_MCS_STBC_SHIFT;

    vec![
        0x00,
        0x00,
        RADIOTAP_HT_LEN as u8,
        0x00,
        0x00,
        0x80,
        0x08,
        0x00,
        0x08,
        0x00,
        known,
        flags,
        params.mcs_index.min(31),
    ]
}

pub fn build_vht_radiotap_header(params: TxRadioParams) -> Vec<u8> {
    let mut flags = 0u8;
    if params.stbc != 0 {
        flags |= IEEE80211_RADIOTAP_VHT_FLAG_STBC;
    }
    if params.short_gi {
        flags |= IEEE80211_RADIOTAP_VHT_FLAG_SGI;
    }
    let nss = params.nss.clamp(1, 4);
    let mcs = params.mcs_index.min(9);
    let mcs_nss0 = (mcs << 4) | nss;
    let coding = if params.ldpc {
        IEEE80211_RADIOTAP_VHT_CODING_LDPC_USER0
    } else {
        0
    };

    vec![
        0x00,
        0x00,
        RADIOTAP_VHT_LEN as u8,
        0x00,
        0x00,
        0x80,
        0x20,
        0x00,
        0x08,
        0x00,
        0x45,
        0x00,
        flags,
        params.bandwidth.vht_bits(),
        mcs_nss0,
        0x00,
        0x00,
        0x00,
        coding,
        0x00,
        0x00,
        0x00,
    ]
}

pub fn radiotap_len(packet: &[u8]) -> Result<usize, RadiotapError> {
    if packet.len() < 4 {
        return Err(RadiotapError::TooShort);
    }
    let len = u16::from_le_bytes([packet[2], packet[3]]) as usize;
    if len == 0 || len >= packet.len() {
        return Err(RadiotapError::InvalidLength);
    }
    Ok(len)
}

pub fn parse_radiotap_tx_info(packet: &[u8]) -> Result<RadiotapTxInfo, RadiotapError> {
    let len = radiotap_len(packet)?;
    match len {
        RADIOTAP_HT_LEN if packet.len() >= RADIOTAP_HT_LEN => {
            let known = packet[10];
            let flags = packet[11];
            let bandwidth = if flags & 0x03 == 1 {
                ChannelBandwidth::Mhz40
            } else {
                ChannelBandwidth::Mhz20
            };
            Ok(RadiotapTxInfo {
                vht: false,
                mcs_index: if known & IEEE80211_RADIOTAP_MCS_HAVE_MCS != 0 {
                    packet[12].min(31)
                } else {
                    0
                },
                nss: 1,
                bandwidth,
                short_gi: known & IEEE80211_RADIOTAP_MCS_HAVE_GI != 0
                    && flags & IEEE80211_RADIOTAP_MCS_SGI != 0,
                stbc: if known & IEEE80211_RADIOTAP_MCS_HAVE_STBC != 0 {
                    (flags >> IEEE80211_RADIOTAP_MCS_STBC_SHIFT) & 0x03
                } else {
                    0
                },
                ldpc: known & IEEE80211_RADIOTAP_MCS_HAVE_FEC != 0
                    && flags & IEEE80211_RADIOTAP_MCS_FEC_LDPC != 0,
            })
        }
        RADIOTAP_VHT_LEN if packet.len() >= RADIOTAP_VHT_LEN => {
            let flags = packet[12];
            let bandwidth = match packet[13] & 0x1f {
                1..=3 => ChannelBandwidth::Mhz40,
                4..=10 => ChannelBandwidth::Mhz80,
                11..=31 => ChannelBandwidth::Mhz160,
                _ => ChannelBandwidth::Mhz20,
            };
            let mcs_nss = packet[14];
            Ok(RadiotapTxInfo {
                vht: true,
                mcs_index: ((mcs_nss >> 4) & 0x0f).min(9),
                nss: (mcs_nss & 0x0f).clamp(1, 4),
                bandwidth,
                short_gi: flags & IEEE80211_RADIOTAP_VHT_FLAG_SGI != 0,
                stbc: u8::from(flags & IEEE80211_RADIOTAP_VHT_FLAG_STBC != 0),
                ldpc: packet[18] & IEEE80211_RADIOTAP_VHT_CODING_LDPC_USER0 != 0,
            })
        }
        _ => Err(RadiotapError::UnsupportedHeader),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ht_header_roundtrips_tx_info() {
        let params = TxRadioParams {
            mcs_index: 3,
            bandwidth: ChannelBandwidth::Mhz40,
            short_gi: true,
            stbc: 1,
            ldpc: true,
            ..TxRadioParams::default()
        };
        let mut packet = build_radiotap_header(params);
        packet.extend_from_slice(&[0u8; 24]);
        let parsed = parse_radiotap_tx_info(&packet).unwrap();
        assert_eq!(parsed.mcs_index, 3);
        assert_eq!(parsed.bandwidth, ChannelBandwidth::Mhz40);
        assert!(parsed.short_gi);
        assert!(parsed.ldpc);
        assert_eq!(parsed.stbc, 1);
    }
}
