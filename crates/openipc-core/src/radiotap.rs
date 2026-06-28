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

const RADIOTAP_PRESENT_RATE: u32 = 1 << 2;
const RADIOTAP_PRESENT_TX_FLAGS: u32 = 1 << 15;
const RADIOTAP_PRESENT_MCS: u32 = 1 << 19;
const RADIOTAP_PRESENT_VHT: u32 = 1 << 21;
const RADIOTAP_TX_FLAGS_NO_ACK: u16 = 0x0008;

pub const RADIOTAP_LEGACY_LEN: usize = 13;
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
pub enum TxModeKind {
    Legacy,
    Ht,
    Vht,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxMode {
    pub kind: TxModeKind,
    pub legacy_rate_500kbps: u8,
    pub ht_mcs: u8,
    pub vht_mcs: u8,
    pub vht_nss: u8,
    pub bandwidth: ChannelBandwidth,
    pub short_gi: bool,
    pub ldpc: bool,
    pub stbc: bool,
}

impl TxMode {
    pub const fn legacy(rate_500kbps: u8) -> Self {
        Self {
            kind: TxModeKind::Legacy,
            legacy_rate_500kbps: rate_500kbps,
            ht_mcs: 0,
            vht_mcs: 0,
            vht_nss: 1,
            bandwidth: ChannelBandwidth::Mhz20,
            short_gi: false,
            ldpc: false,
            stbc: false,
        }
    }

    pub const fn legacy_6m() -> Self {
        Self::legacy(12)
    }

    pub const fn legacy_1m() -> Self {
        Self::legacy(2)
    }

    pub const fn ht(mcs: u8) -> Self {
        Self {
            kind: TxModeKind::Ht,
            legacy_rate_500kbps: 12,
            ht_mcs: mcs,
            vht_mcs: 0,
            vht_nss: 1,
            bandwidth: ChannelBandwidth::Mhz20,
            short_gi: false,
            ldpc: false,
            stbc: false,
        }
    }

    pub const fn vht(nss: u8, mcs: u8) -> Self {
        Self {
            kind: TxModeKind::Vht,
            legacy_rate_500kbps: 12,
            ht_mcs: 0,
            vht_mcs: mcs,
            vht_nss: nss,
            bandwidth: ChannelBandwidth::Mhz20,
            short_gi: false,
            ldpc: false,
            stbc: false,
        }
    }
}

impl Default for TxMode {
    fn default() -> Self {
        Self::legacy_6m()
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

pub fn build_stream_radiotap(mode: TxMode) -> Vec<u8> {
    match mode.kind {
        TxModeKind::Legacy => build_legacy_radiotap_header(mode),
        TxModeKind::Ht => build_ht_radiotap_header(TxRadioParams {
            mcs_index: mode.ht_mcs,
            bandwidth: mode.bandwidth,
            short_gi: mode.short_gi,
            stbc: u8::from(mode.stbc),
            ldpc: mode.ldpc,
            vht: false,
            ..TxRadioParams::default()
        }),
        TxModeKind::Vht => build_vht_radiotap_header(TxRadioParams {
            mcs_index: mode.vht_mcs,
            nss: mode.vht_nss,
            bandwidth: mode.bandwidth,
            short_gi: mode.short_gi,
            stbc: u8::from(mode.stbc),
            ldpc: mode.ldpc,
            vht: true,
            ..TxRadioParams::default()
        }),
    }
}

pub fn build_legacy_radiotap_header(mode: TxMode) -> Vec<u8> {
    vec![
        0x00,
        0x00,
        RADIOTAP_LEGACY_LEN as u8,
        0x00,
        (RADIOTAP_PRESENT_RATE | RADIOTAP_PRESENT_TX_FLAGS) as u8,
        ((RADIOTAP_PRESENT_RATE | RADIOTAP_PRESENT_TX_FLAGS) >> 8) as u8,
        0x00,
        0x00,
        mode.legacy_rate_500kbps,
        0x00,
        (RADIOTAP_TX_FLAGS_NO_ACK & 0xff) as u8,
        (RADIOTAP_TX_FLAGS_NO_ACK >> 8) as u8,
        0x00,
    ]
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

pub fn parse_tx_mode_str(spec: &str) -> TxMode {
    let trimmed = spec
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect::<String>();
    if trimmed.is_empty() {
        return TxMode::default();
    }

    let mut tokens = trimmed.split('/');
    let Some(rate_token) = tokens.next() else {
        return TxMode::default();
    };
    let Some(mut mode) = parse_tx_rate_token(rate_token) else {
        return TxMode::default();
    };

    for token in tokens {
        match token {
            "SGI" => mode.short_gi = true,
            "LDPC" => mode.ldpc = true,
            "STBC" => mode.stbc = true,
            "20" => mode.bandwidth = ChannelBandwidth::Mhz20,
            "40" => mode.bandwidth = ChannelBandwidth::Mhz40,
            "80" => mode.bandwidth = ChannelBandwidth::Mhz80,
            "160" => mode.bandwidth = ChannelBandwidth::Mhz160,
            _ => {}
        }
    }
    mode
}

pub fn parse_radiotap_tx_mode(packet: &[u8]) -> Result<Option<TxMode>, RadiotapError> {
    let len = radiotap_len(packet)?;
    if len < 8 || packet.len() < len {
        return Err(RadiotapError::InvalidLength);
    }
    let present = u32::from_le_bytes(packet[4..8].try_into().expect("present word is in range"));
    if present & (1 << 31) != 0 {
        return Err(RadiotapError::UnsupportedHeader);
    }

    let mut offset = 8usize;
    let mut mode = None;

    if present & RADIOTAP_PRESENT_RATE != 0 {
        require_field(packet, len, offset, 1)?;
        mode = Some(TxMode::legacy(packet[offset]));
        offset += 1;
    }

    if present & RADIOTAP_PRESENT_TX_FLAGS != 0 {
        offset = align(offset, 2);
        require_field(packet, len, offset, 2)?;
        offset += 2;
    }

    if present & RADIOTAP_PRESENT_MCS != 0 {
        require_field(packet, len, offset, 3)?;
        let known = packet[offset];
        let flags = packet[offset + 1];
        let mcs = packet[offset + 2];
        let mut ht = TxMode::ht(if known & IEEE80211_RADIOTAP_MCS_HAVE_MCS != 0 {
            mcs.min(31)
        } else {
            0
        });
        ht.bandwidth = if known & IEEE80211_RADIOTAP_MCS_HAVE_BW != 0 && flags & 0x03 == 1 {
            ChannelBandwidth::Mhz40
        } else {
            ChannelBandwidth::Mhz20
        };
        ht.short_gi =
            known & IEEE80211_RADIOTAP_MCS_HAVE_GI != 0 && flags & IEEE80211_RADIOTAP_MCS_SGI != 0;
        ht.ldpc = known & IEEE80211_RADIOTAP_MCS_HAVE_FEC != 0
            && flags & IEEE80211_RADIOTAP_MCS_FEC_LDPC != 0;
        ht.stbc = known & IEEE80211_RADIOTAP_MCS_HAVE_STBC != 0
            && ((flags >> IEEE80211_RADIOTAP_MCS_STBC_SHIFT) & 0x03) != 0;
        mode = Some(ht);
        offset += 3;
    }

    if present & RADIOTAP_PRESENT_VHT != 0 {
        offset = align(offset, 2);
        require_field(packet, len, offset, 12)?;
        let known = u16::from_le_bytes([packet[offset], packet[offset + 1]]);
        let flags = packet[offset + 2];
        let bandwidth = match packet[offset + 3] & 0x1f {
            1..=3 => ChannelBandwidth::Mhz40,
            4..=10 => ChannelBandwidth::Mhz80,
            11..=31 => ChannelBandwidth::Mhz160,
            _ => ChannelBandwidth::Mhz20,
        };
        let mcs_nss = packet[offset + 4];
        let mut vht = TxMode::vht((mcs_nss & 0x0f).clamp(1, 4), ((mcs_nss >> 4) & 0x0f).min(9));
        if known & (1 << 6) != 0 {
            vht.bandwidth = bandwidth;
        }
        vht.short_gi = known & (1 << 2) != 0 && flags & IEEE80211_RADIOTAP_VHT_FLAG_SGI != 0;
        vht.stbc = known & 1 != 0 && flags & IEEE80211_RADIOTAP_VHT_FLAG_STBC != 0;
        vht.ldpc = packet[offset + 8] & IEEE80211_RADIOTAP_VHT_CODING_LDPC_USER0 != 0;
        mode = Some(vht);
    }

    Ok(mode)
}

pub fn parse_radiotap_tx_info(packet: &[u8]) -> Result<RadiotapTxInfo, RadiotapError> {
    match parse_radiotap_tx_mode(packet)? {
        Some(mode) => Ok(RadiotapTxInfo {
            vht: mode.kind == TxModeKind::Vht,
            mcs_index: match mode.kind {
                TxModeKind::Legacy | TxModeKind::Ht => mode.ht_mcs,
                TxModeKind::Vht => mode.vht_mcs,
            },
            nss: mode.vht_nss,
            bandwidth: mode.bandwidth,
            short_gi: mode.short_gi,
            stbc: u8::from(mode.stbc),
            ldpc: mode.ldpc,
        }),
        None => Err(RadiotapError::UnsupportedHeader),
    }
}

fn parse_tx_rate_token(token: &str) -> Option<TxMode> {
    match token {
        "1M" => Some(TxMode::legacy(2)),
        "2M" => Some(TxMode::legacy(4)),
        "5.5M" | "5M" => Some(TxMode::legacy(11)),
        "6M" => Some(TxMode::legacy(12)),
        "9M" => Some(TxMode::legacy(18)),
        "11M" => Some(TxMode::legacy(22)),
        "12M" => Some(TxMode::legacy(24)),
        "18M" => Some(TxMode::legacy(36)),
        "24M" => Some(TxMode::legacy(48)),
        "36M" => Some(TxMode::legacy(72)),
        "48M" => Some(TxMode::legacy(96)),
        "54M" => Some(TxMode::legacy(108)),
        _ => {
            if let Some(raw) = token.strip_prefix("MCS") {
                return raw
                    .parse::<u8>()
                    .ok()
                    .filter(|mcs| *mcs <= 31)
                    .map(TxMode::ht);
            }
            if let Some(raw) = token.strip_prefix("VHT") {
                let (nss_raw, mcs_raw) = raw.split_once("SS_MCS")?;
                let nss = nss_raw.parse::<u8>().ok()?;
                let mcs = mcs_raw.parse::<u8>().ok()?;
                if (1..=4).contains(&nss) && mcs <= 9 {
                    return Some(TxMode::vht(nss, mcs));
                }
            }
            None
        }
    }
}

const fn align(offset: usize, alignment: usize) -> usize {
    (offset + alignment - 1) & !(alignment - 1)
}

fn require_field(
    packet: &[u8],
    radiotap_len: usize,
    offset: usize,
    len: usize,
) -> Result<(), RadiotapError> {
    if offset + len <= radiotap_len && offset + len <= packet.len() {
        Ok(())
    } else {
        Err(RadiotapError::InvalidLength)
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

    #[test]
    fn parses_devourer_tx_mode_strings() {
        let mode = parse_tx_mode_str("vht2ss_mcs5/80/sgi/ldpc/stbc");
        assert_eq!(mode.kind, TxModeKind::Vht);
        assert_eq!(mode.vht_nss, 2);
        assert_eq!(mode.vht_mcs, 5);
        assert_eq!(mode.bandwidth, ChannelBandwidth::Mhz80);
        assert!(mode.short_gi);
        assert!(mode.ldpc);
        assert!(mode.stbc);
    }

    #[test]
    fn legacy_stream_radiotap_carries_rate_and_tx_flags() {
        let mode = TxMode::legacy(12);
        let mut packet = build_stream_radiotap(mode);
        packet.extend_from_slice(&[0u8; 24]);
        let parsed = parse_radiotap_tx_mode(&packet).unwrap().unwrap();
        assert_eq!(parsed.kind, TxModeKind::Legacy);
        assert_eq!(parsed.legacy_rate_500kbps, 12);
    }
}
