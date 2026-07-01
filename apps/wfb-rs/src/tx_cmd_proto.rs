#![allow(dead_code)]

pub const CMD_SET_FEC: u8 = 1;
pub const CMD_SET_RADIO: u8 = 2;
pub const CMD_GET_FEC: u8 = 3;
pub const CMD_GET_RADIO: u8 = 4;

pub const REQ_HEADER_LEN: usize = 5;
pub const RESP_HEADER_LEN: usize = 8;
pub const FEC_PAYLOAD_LEN: usize = 2;
pub const RADIO_PAYLOAD_LEN: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FecSettings {
    pub k: u8,
    pub n: u8,
}

impl FecSettings {
    pub const fn valid(self) -> bool {
        self.k >= 1 && self.n >= 1 && self.k <= self.n
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioSettings {
    pub stbc: u8,
    pub ldpc: bool,
    pub short_gi: bool,
    pub bandwidth: u8,
    pub mcs_index: u8,
    pub vht_mode: bool,
    pub vht_nss: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRequest {
    SetFec {
        req_id_be: u32,
        fec: FecSettings,
    },
    SetRadio {
        req_id_be: u32,
        radio: RadioSettings,
    },
    GetFec {
        req_id_be: u32,
    },
    GetRadio {
        req_id_be: u32,
    },
}

impl CommandRequest {
    pub const fn req_id_be(self) -> u32 {
        match self {
            Self::SetFec { req_id_be, .. }
            | Self::SetRadio { req_id_be, .. }
            | Self::GetFec { req_id_be }
            | Self::GetRadio { req_id_be } => req_id_be,
        }
    }

    pub fn encode(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(REQ_HEADER_LEN + RADIO_PAYLOAD_LEN);
        out.extend_from_slice(&self.req_id_be().to_ne_bytes());
        match self {
            Self::SetFec { fec, .. } => {
                out.push(CMD_SET_FEC);
                out.push(fec.k);
                out.push(fec.n);
            }
            Self::SetRadio { radio, .. } => {
                out.push(CMD_SET_RADIO);
                out.extend_from_slice(&encode_radio_payload(radio));
            }
            Self::GetFec { .. } => out.push(CMD_GET_FEC),
            Self::GetRadio { .. } => out.push(CMD_GET_RADIO),
        }
        out
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < REQ_HEADER_LEN {
            return None;
        }
        let req_id_be = u32::from_ne_bytes(bytes[0..4].try_into().ok()?);
        match bytes[4] {
            CMD_SET_FEC if bytes.len() == REQ_HEADER_LEN + FEC_PAYLOAD_LEN => Some(Self::SetFec {
                req_id_be,
                fec: FecSettings {
                    k: bytes[5],
                    n: bytes[6],
                },
            }),
            CMD_SET_RADIO if bytes.len() == REQ_HEADER_LEN + RADIO_PAYLOAD_LEN => {
                Some(Self::SetRadio {
                    req_id_be,
                    radio: decode_radio_payload(&bytes[5..12])?,
                })
            }
            CMD_GET_FEC if bytes.len() == REQ_HEADER_LEN => Some(Self::GetFec { req_id_be }),
            CMD_GET_RADIO if bytes.len() == REQ_HEADER_LEN => Some(Self::GetRadio { req_id_be }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandResponse {
    Ack {
        req_id_be: u32,
        errno: u32,
    },
    Fec {
        req_id_be: u32,
        errno: u32,
        fec: FecSettings,
    },
    Radio {
        req_id_be: u32,
        errno: u32,
        radio: RadioSettings,
    },
}

impl CommandResponse {
    pub const fn req_id_be(self) -> u32 {
        match self {
            Self::Ack { req_id_be, .. }
            | Self::Fec { req_id_be, .. }
            | Self::Radio { req_id_be, .. } => req_id_be,
        }
    }

    pub const fn errno(self) -> u32 {
        match self {
            Self::Ack { errno, .. } | Self::Fec { errno, .. } | Self::Radio { errno, .. } => errno,
        }
    }

    pub fn encode(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(RESP_HEADER_LEN + RADIO_PAYLOAD_LEN);
        out.extend_from_slice(&self.req_id_be().to_ne_bytes());
        out.extend_from_slice(&self.errno().to_be_bytes());
        match self {
            Self::Ack { .. } => {}
            Self::Fec { fec, .. } => {
                out.push(fec.k);
                out.push(fec.n);
            }
            Self::Radio { radio, .. } => out.extend_from_slice(&encode_radio_payload(radio)),
        }
        out
    }

    pub fn parse(bytes: &[u8], expected_payload_len: usize) -> Option<Self> {
        if bytes.len() != RESP_HEADER_LEN + expected_payload_len {
            return None;
        }
        let req_id_be = u32::from_ne_bytes(bytes[0..4].try_into().ok()?);
        let errno = u32::from_be_bytes(bytes[4..8].try_into().ok()?);
        match expected_payload_len {
            0 => Some(Self::Ack { req_id_be, errno }),
            FEC_PAYLOAD_LEN => Some(Self::Fec {
                req_id_be,
                errno,
                fec: FecSettings {
                    k: bytes[8],
                    n: bytes[9],
                },
            }),
            RADIO_PAYLOAD_LEN => Some(Self::Radio {
                req_id_be,
                errno,
                radio: decode_radio_payload(&bytes[8..15])?,
            }),
            _ => None,
        }
    }
}

pub const fn expected_response_payload_len(command: u8) -> usize {
    match command {
        CMD_GET_FEC => FEC_PAYLOAD_LEN,
        CMD_GET_RADIO => RADIO_PAYLOAD_LEN,
        _ => 0,
    }
}

pub fn encode_radio_payload(radio: RadioSettings) -> [u8; RADIO_PAYLOAD_LEN] {
    [
        radio.stbc,
        u8::from(radio.ldpc),
        u8::from(radio.short_gi),
        radio.bandwidth,
        radio.mcs_index,
        u8::from(radio.vht_mode),
        radio.vht_nss,
    ]
}

pub fn decode_radio_payload(bytes: &[u8]) -> Option<RadioSettings> {
    if bytes.len() != RADIO_PAYLOAD_LEN {
        return None;
    }
    Some(RadioSettings {
        stbc: bytes[0],
        ldpc: bytes[1] != 0,
        short_gi: bytes[2] != 0,
        bandwidth: bytes[3],
        mcs_index: bytes[4],
        vht_mode: bytes[5] != 0,
        vht_nss: bytes[6],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_radio_request_uses_packed_wfb_ng_layout() {
        let request = CommandRequest::SetRadio {
            req_id_be: 0x4433_2211,
            radio: RadioSettings {
                stbc: 1,
                ldpc: true,
                short_gi: false,
                bandwidth: 40,
                mcs_index: 3,
                vht_mode: true,
                vht_nss: 2,
            },
        };
        let bytes = request.encode();
        assert_eq!(
            bytes,
            vec![0x11, 0x22, 0x33, 0x44, CMD_SET_RADIO, 1, 1, 0, 40, 3, 1, 2]
        );
        assert_eq!(CommandRequest::parse(&bytes), Some(request));
    }

    #[test]
    fn get_fec_response_uses_network_order_errno() {
        let response = CommandResponse::Fec {
            req_id_be: 0x0403_0201,
            errno: 22,
            fec: FecSettings { k: 4, n: 8 },
        };
        let bytes = response.encode();
        assert_eq!(bytes, vec![1, 2, 3, 4, 0, 0, 0, 22, 4, 8]);
        assert_eq!(
            CommandResponse::parse(&bytes, FEC_PAYLOAD_LEN),
            Some(response)
        );
    }
}
