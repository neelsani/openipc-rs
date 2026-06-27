use std::collections::BTreeMap;

use crypto_box::aead::Aead;
use crypto_box::{Nonce as BoxNonce, PublicKey, SalsaBox, SecretKey};

use crate::channel::ChannelId;
use crate::crypto::decrypt_chacha20poly1305_legacy;
use crate::fec::FecCode;

pub const WIFI_MTU: usize = 4045;
pub const IEEE80211_HEADER_LEN: usize = 24;
pub const CRYPTO_BOX_SECRETKEY_LEN: usize = 32;
pub const CRYPTO_BOX_PUBLICKEY_LEN: usize = 32;
pub const CRYPTO_BOX_NONCE_LEN: usize = 24;
pub const CRYPTO_BOX_TAG_LEN: usize = 16;
pub const WSESSION_HDR_LEN: usize = 1 + CRYPTO_BOX_NONCE_LEN;
pub const WSESSION_DATA_LEN: usize = 8 + 4 + 1 + 1 + 1 + CHACHA20_POLY1305_KEY_LEN;
pub const WBLOCK_HDR_LEN: usize = 9;
pub const WPACKET_HDR_LEN: usize = 3;
pub const CHACHA20_POLY1305_KEY_LEN: usize = 32;
pub const CHACHA20_POLY1305_TAG_LEN: usize = 16;
pub const MAX_FEC_PAYLOAD: usize =
    WIFI_MTU - IEEE80211_HEADER_LEN - WBLOCK_HDR_LEN - CHACHA20_POLY1305_TAG_LEN;
pub const MAX_PAYLOAD_SIZE: usize = MAX_FEC_PAYLOAD - WPACKET_HDR_LEN;
pub const MAX_FORWARDER_PACKET_SIZE: usize = WIFI_MTU - IEEE80211_HEADER_LEN;
pub const MAX_BLOCK_IDX: u64 = (1u64 << 55) - 1;

pub const WFB_PACKET_DATA: u8 = 0x01;
pub const WFB_PACKET_KEY: u8 = 0x02;
pub const WFB_FEC_VDM_RS: u8 = 0x01;
pub const WFB_PACKET_FEC_ONLY: u8 = 0x01;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WfbError {
    Empty,
    TooLong,
    ShortDataPacket,
    ShortSessionPacket,
    InvalidKeypair,
    SessionEncryptFailed,
    SessionDecryptFailed,
    DataEncryptFailed,
    DataDecryptFailed,
    SessionEpochTooOld {
        session_epoch: u64,
        minimum_epoch: u64,
    },
    SessionChannelMismatch {
        expected: u32,
        actual: u32,
    },
    UnsupportedFecType(u8),
    UnknownPacketType(u8),
    InvalidFecParameters,
    InvalidFragmentIndex,
    BlockIndexOverflow,
    InvalidPlainPacket,
    PayloadTooLarge,
    MissingSession,
    FecRecoveryFailed,
}

impl std::fmt::Display for WfbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty WFB packet"),
            Self::TooLong => write!(f, "WFB packet exceeds maximum forwarder size"),
            Self::ShortDataPacket => write!(f, "short WFB data packet"),
            Self::ShortSessionPacket => write!(f, "invalid WFB session packet size"),
            Self::InvalidKeypair => write!(f, "WFB keypair must be 64 bytes"),
            Self::SessionEncryptFailed => write!(f, "unable to encrypt WFB session key"),
            Self::SessionDecryptFailed => write!(f, "unable to decrypt WFB session key"),
            Self::DataEncryptFailed => write!(f, "unable to encrypt WFB data packet"),
            Self::DataDecryptFailed => write!(f, "unable to decrypt WFB data packet"),
            Self::SessionEpochTooOld {
                session_epoch,
                minimum_epoch,
            } => write!(
                f,
                "WFB session epoch {session_epoch} is older than minimum {minimum_epoch}"
            ),
            Self::SessionChannelMismatch { expected, actual } => write!(
                f,
                "WFB session channel mismatch: expected 0x{expected:08x}, got 0x{actual:08x}"
            ),
            Self::UnsupportedFecType(fec_type) => {
                write!(f, "unsupported WFB FEC type {fec_type}")
            }
            Self::UnknownPacketType(packet_type) => {
                write!(f, "unknown WFB packet type 0x{packet_type:02x}")
            }
            Self::InvalidFecParameters => write!(f, "invalid WFB FEC parameters"),
            Self::InvalidFragmentIndex => write!(f, "invalid WFB fragment index"),
            Self::BlockIndexOverflow => write!(f, "WFB block index overflow"),
            Self::InvalidPlainPacket => write!(f, "invalid decrypted WFB packet"),
            Self::PayloadTooLarge => write!(f, "decrypted WFB payload is too large"),
            Self::MissingSession => write!(f, "WFB data packet arrived before session key"),
            Self::FecRecoveryFailed => write!(f, "WFB FEC recovery failed"),
        }
    }
}

impl std::error::Error for WfbError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WfbPacket<'a> {
    Data {
        data_nonce: u64,
        encrypted_payload: &'a [u8],
        associated_data: &'a [u8],
    },
    SessionKey {
        session_nonce: &'a [u8],
        encrypted_session: &'a [u8],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WfbKeypair {
    pub rx_secretkey: [u8; CRYPTO_BOX_SECRETKEY_LEN],
    pub tx_publickey: [u8; CRYPTO_BOX_PUBLICKEY_LEN],
}

impl WfbKeypair {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, WfbError> {
        if bytes.len() != CRYPTO_BOX_SECRETKEY_LEN + CRYPTO_BOX_PUBLICKEY_LEN {
            return Err(WfbError::InvalidKeypair);
        }
        let mut rx_secretkey = [0; CRYPTO_BOX_SECRETKEY_LEN];
        let mut tx_publickey = [0; CRYPTO_BOX_PUBLICKEY_LEN];
        rx_secretkey.copy_from_slice(&bytes[..CRYPTO_BOX_SECRETKEY_LEN]);
        tx_publickey.copy_from_slice(&bytes[CRYPTO_BOX_SECRETKEY_LEN..]);
        Ok(Self {
            rx_secretkey,
            tx_publickey,
        })
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FecCounters {
    pub total_packets: u64,
    pub recovered_packets: u64,
    pub lost_packets: u64,
    pub bad_packets: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WfbSession {
    pub epoch: u64,
    pub channel_id: ChannelId,
    pub fec_type: u8,
    pub fec_k: usize,
    pub fec_n: usize,
    pub session_key: [u8; CHACHA20_POLY1305_KEY_LEN],
}

impl WfbSession {
    fn parse(
        plaintext: &[u8],
        expected_channel_id: ChannelId,
        minimum_epoch: u64,
    ) -> Result<Self, WfbError> {
        if plaintext.len() != WSESSION_DATA_LEN {
            return Err(WfbError::SessionDecryptFailed);
        }
        let epoch = u64::from_be_bytes(plaintext[0..8].try_into().expect("checked length"));
        if epoch < minimum_epoch {
            return Err(WfbError::SessionEpochTooOld {
                session_epoch: epoch,
                minimum_epoch,
            });
        }

        let raw_channel = u32::from_be_bytes(plaintext[8..12].try_into().expect("checked length"));
        let channel_id = ChannelId::new(raw_channel);
        if channel_id != expected_channel_id {
            return Err(WfbError::SessionChannelMismatch {
                expected: expected_channel_id.raw(),
                actual: raw_channel,
            });
        }

        let fec_type = plaintext[12];
        if fec_type != WFB_FEC_VDM_RS {
            return Err(WfbError::UnsupportedFecType(fec_type));
        }
        let fec_k = plaintext[13] as usize;
        let fec_n = plaintext[14] as usize;
        if fec_k == 0 || fec_n == 0 || fec_k > fec_n {
            return Err(WfbError::InvalidFecParameters);
        }

        let mut session_key = [0; CHACHA20_POLY1305_KEY_LEN];
        session_key.copy_from_slice(&plaintext[15..47]);
        Ok(Self {
            epoch,
            channel_id,
            fec_type,
            fec_k,
            fec_n,
            session_key,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WfbEvent {
    Session(WfbSession),
    Payload(WfbOutput),
}

#[derive(Debug, Clone)]
pub struct WfbReceiver {
    channel_id: ChannelId,
    minimum_epoch: u64,
    keypair: WfbKeypair,
    session: Option<WfbSession>,
    assembler: Option<PlainAssembler>,
    incoming_packets: u64,
    session_packets: u64,
    data_packets: u64,
}

impl WfbReceiver {
    pub fn new(channel_id: ChannelId, keypair: WfbKeypair, minimum_epoch: u64) -> Self {
        Self {
            channel_id,
            minimum_epoch,
            keypair,
            session: None,
            assembler: None,
            incoming_packets: 0,
            session_packets: 0,
            data_packets: 0,
        }
    }

    pub fn session(&self) -> Option<&WfbSession> {
        self.session.as_ref()
    }

    pub fn counters(&self) -> FecCounters {
        let assembler = self
            .assembler
            .as_ref()
            .map(PlainAssembler::counters)
            .unwrap_or_default();
        FecCounters {
            total_packets: self.incoming_packets,
            recovered_packets: assembler.recovered_packets,
            lost_packets: assembler.lost_packets,
            bad_packets: assembler.bad_packets,
        }
    }

    pub fn push_forwarder_packet(&mut self, buf: &[u8]) -> Result<Vec<WfbEvent>, WfbError> {
        match parse_forwarder_packet(buf)? {
            WfbPacket::SessionKey {
                session_nonce,
                encrypted_session,
            } => {
                self.incoming_packets += 1;
                self.session_packets += 1;
                let session = self.decrypt_session(session_nonce, encrypted_session)?;
                let changed = self
                    .session
                    .as_ref()
                    .map(|current| current.session_key != session.session_key)
                    .unwrap_or(true);
                if changed {
                    self.assembler = Some(PlainAssembler::new(session.fec_k, session.fec_n)?);
                    self.session = Some(session.clone());
                    Ok(vec![WfbEvent::Session(session)])
                } else {
                    Ok(Vec::new())
                }
            }
            WfbPacket::Data {
                data_nonce,
                encrypted_payload,
                associated_data,
            } => {
                self.incoming_packets += 1;
                self.data_packets += 1;
                let session = self.session.as_ref().ok_or(WfbError::MissingSession)?;
                let nonce = &associated_data[1..WBLOCK_HDR_LEN];
                let decrypted = decrypt_chacha20poly1305_legacy(
                    &session.session_key,
                    nonce,
                    associated_data,
                    encrypted_payload,
                )
                .map_err(|_| WfbError::DataDecryptFailed)?;
                let assembler = self.assembler.as_mut().ok_or(WfbError::MissingSession)?;
                Ok(assembler
                    .push_decrypted_fragment(data_nonce, &decrypted)?
                    .into_iter()
                    .map(WfbEvent::Payload)
                    .collect())
            }
        }
    }

    fn decrypt_session(
        &self,
        session_nonce: &[u8],
        encrypted_session: &[u8],
    ) -> Result<WfbSession, WfbError> {
        let nonce: [u8; CRYPTO_BOX_NONCE_LEN] = session_nonce
            .try_into()
            .map_err(|_| WfbError::ShortSessionPacket)?;
        let rx_secret = SecretKey::from(self.keypair.rx_secretkey);
        let tx_public = PublicKey::from(self.keypair.tx_publickey);
        let cipher = SalsaBox::new(&tx_public, &rx_secret);
        let plaintext = cipher
            .decrypt(BoxNonce::from_slice(&nonce), encrypted_session)
            .map_err(|_| WfbError::SessionDecryptFailed)?;
        WfbSession::parse(&plaintext, self.channel_id, self.minimum_epoch)
    }
}

pub fn parse_forwarder_packet(buf: &[u8]) -> Result<WfbPacket<'_>, WfbError> {
    if buf.is_empty() {
        return Err(WfbError::Empty);
    }
    if buf.len() > MAX_FORWARDER_PACKET_SIZE {
        return Err(WfbError::TooLong);
    }

    match buf[0] {
        WFB_PACKET_DATA => {
            if buf.len() < WBLOCK_HDR_LEN + WPACKET_HDR_LEN {
                return Err(WfbError::ShortDataPacket);
            }
            let mut nonce = [0; 8];
            nonce.copy_from_slice(&buf[1..9]);
            Ok(WfbPacket::Data {
                data_nonce: u64::from_be_bytes(nonce),
                encrypted_payload: &buf[WBLOCK_HDR_LEN..],
                associated_data: &buf[..WBLOCK_HDR_LEN],
            })
        }
        WFB_PACKET_KEY => {
            if buf.len() != WSESSION_HDR_LEN + WSESSION_DATA_LEN + CRYPTO_BOX_TAG_LEN {
                return Err(WfbError::ShortSessionPacket);
            }
            Ok(WfbPacket::SessionKey {
                session_nonce: &buf[1..WSESSION_HDR_LEN],
                encrypted_session: &buf[WSESSION_HDR_LEN..],
            })
        }
        other => Err(WfbError::UnknownPacketType(other)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WfbOutput {
    pub packet_seq: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Block {
    fragments: Vec<Option<Vec<u8>>>,
    received: usize,
    next_fragment: usize,
}

impl Block {
    fn new(n: usize) -> Self {
        Self {
            fragments: vec![None; n],
            received: 0,
            next_fragment: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlainAssembler {
    fec_k: usize,
    fec_n: usize,
    fec: FecCode,
    blocks: BTreeMap<u64, Block>,
    next_block: Option<u64>,
    pub total_packets: u64,
    pub lost_packets: u64,
    pub recovered_packets: u64,
    pub bad_packets: u64,
}

impl PlainAssembler {
    pub fn new(fec_k: usize, fec_n: usize) -> Result<Self, WfbError> {
        if fec_k == 0 || fec_n == 0 || fec_k > fec_n || fec_n > 255 {
            return Err(WfbError::InvalidFecParameters);
        }
        let fec = FecCode::new(fec_k, fec_n).map_err(|_| WfbError::InvalidFecParameters)?;
        Ok(Self {
            fec_k,
            fec_n,
            fec,
            blocks: BTreeMap::new(),
            next_block: None,
            total_packets: 0,
            lost_packets: 0,
            recovered_packets: 0,
            bad_packets: 0,
        })
    }

    pub const fn fec_k(&self) -> usize {
        self.fec_k
    }

    pub const fn fec_n(&self) -> usize {
        self.fec_n
    }

    pub fn reset_fec(&mut self, fec_k: usize, fec_n: usize) -> Result<(), WfbError> {
        *self = Self::new(fec_k, fec_n)?;
        Ok(())
    }

    pub fn counters(&self) -> FecCounters {
        FecCounters {
            total_packets: self.total_packets,
            recovered_packets: self.recovered_packets,
            lost_packets: self.lost_packets,
            bad_packets: self.bad_packets,
        }
    }

    /// Push a decrypted WFB FEC fragment.
    pub fn push_decrypted_fragment(
        &mut self,
        data_nonce: u64,
        fragment: &[u8],
    ) -> Result<Vec<WfbOutput>, WfbError> {
        let block_idx = data_nonce >> 8;
        let fragment_idx = (data_nonce & 0xff) as usize;

        if block_idx > MAX_BLOCK_IDX {
            return Err(WfbError::BlockIndexOverflow);
        }
        if fragment_idx >= self.fec_n {
            return Err(WfbError::InvalidFragmentIndex);
        }
        self.total_packets += 1;

        if self.next_block.is_none() {
            self.next_block = Some(block_idx);
        }

        let block = self
            .blocks
            .entry(block_idx)
            .or_insert_with(|| Block::new(self.fec_n));
        if block.fragments[fragment_idx].is_none() {
            let mut padded = vec![0; MAX_FEC_PAYLOAD];
            let len = fragment.len().min(MAX_FEC_PAYLOAD);
            padded[..len].copy_from_slice(&fragment[..len]);
            block.fragments[fragment_idx] = Some(padded);
            block.received += 1;
        }

        Ok(self.drain_ready_blocks())
    }

    fn drain_ready_blocks(&mut self) -> Vec<WfbOutput> {
        let mut out = Vec::new();
        while let Some(block_idx) = self.next_block {
            if !self.blocks.contains_key(&block_idx) {
                break;
            }

            self.emit_contiguous_primary(block_idx, &mut out);
            let complete = self
                .blocks
                .get(&block_idx)
                .map(|block| block.next_fragment == self.fec_k)
                .unwrap_or(false);
            if complete {
                self.blocks.remove(&block_idx);
                self.next_block = Some(block_idx + 1);
                continue;
            }

            let can_recover = self
                .blocks
                .get(&block_idx)
                .map(|block| block.received >= self.fec_k)
                .unwrap_or(false);
            if can_recover {
                if let Some(block) = self.blocks.get_mut(&block_idx) {
                    match self
                        .fec
                        .recover_primary(&mut block.fragments, MAX_FEC_PAYLOAD)
                    {
                        Ok(recovered) => {
                            self.recovered_packets += recovered as u64;
                        }
                        Err(_) => {
                            self.bad_packets += 1;
                            self.force_flush_block(block_idx, &mut out);
                            continue;
                        }
                    }
                }
                self.emit_contiguous_primary(block_idx, &mut out);
                self.blocks.remove(&block_idx);
                self.next_block = Some(block_idx + 1);
                continue;
            }

            if self.should_force_flush(block_idx) {
                self.force_flush_block(block_idx, &mut out);
                continue;
            }

            break;
        }
        out
    }

    fn emit_contiguous_primary(&mut self, block_idx: u64, out: &mut Vec<WfbOutput>) {
        let Some(block) = self.blocks.get_mut(&block_idx) else {
            return;
        };
        while block.next_fragment < self.fec_k {
            let fragment_idx = block.next_fragment;
            let Some(fragment) = block.fragments[fragment_idx].as_deref() else {
                break;
            };
            let packet_seq = block_idx * self.fec_k as u64 + fragment_idx as u64;
            match parse_plain_packet(fragment) {
                Ok(Some(payload)) => out.push(WfbOutput {
                    packet_seq,
                    payload: payload.to_vec(),
                }),
                Ok(None) => {}
                Err(_) => {
                    self.bad_packets += 1;
                }
            }
            block.next_fragment += 1;
        }
    }

    fn should_force_flush(&self, block_idx: u64) -> bool {
        if self.blocks.len() > 40 {
            return true;
        }
        self.blocks
            .range((block_idx + 1)..)
            .any(|(_, block)| block.received >= self.fec_k)
    }

    fn force_flush_block(&mut self, block_idx: u64, out: &mut Vec<WfbOutput>) {
        if let Some(block) = self.blocks.remove(&block_idx) {
            for fragment_idx in block.next_fragment..self.fec_k {
                let packet_seq = block_idx * self.fec_k as u64 + fragment_idx as u64;
                match block.fragments[fragment_idx].as_deref() {
                    Some(fragment) => match parse_plain_packet(fragment) {
                        Ok(Some(payload)) => out.push(WfbOutput {
                            packet_seq,
                            payload: payload.to_vec(),
                        }),
                        Ok(None) => {}
                        Err(_) => {
                            self.bad_packets += 1;
                        }
                    },
                    None => {
                        self.lost_packets += 1;
                    }
                }
            }
            self.next_block = Some(block_idx + 1);
        }
    }
}

pub fn parse_plain_packet(fragment: &[u8]) -> Result<Option<&[u8]>, WfbError> {
    if fragment.len() < WPACKET_HDR_LEN {
        return Err(WfbError::InvalidPlainPacket);
    }
    let flags = fragment[0];
    let packet_size = u16::from_be_bytes([fragment[1], fragment[2]]) as usize;
    if packet_size > MAX_PAYLOAD_SIZE || WPACKET_HDR_LEN + packet_size > fragment.len() {
        return Err(WfbError::PayloadTooLarge);
    }
    if flags & WFB_PACKET_FEC_ONLY != 0 {
        return Ok(None);
    }
    Ok(Some(
        &fragment[WPACKET_HDR_LEN..WPACKET_HDR_LEN + packet_size],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::encrypt_chacha20poly1305_legacy;
    use crypto_box::aead::Aead;

    fn plain(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn padded(fragment: &[u8]) -> Vec<u8> {
        let mut out = vec![0; MAX_FEC_PAYLOAD];
        out[..fragment.len()].copy_from_slice(fragment);
        out
    }

    #[test]
    fn parses_forwarder_data_packet() {
        let mut packet = vec![WFB_PACKET_DATA];
        packet.extend_from_slice(&0x0102_0304_0506_0708u64.to_be_bytes());
        packet.extend_from_slice(&[9, 10, 11]);

        let parsed = parse_forwarder_packet(&packet).unwrap();
        match parsed {
            WfbPacket::Data {
                data_nonce,
                encrypted_payload,
                associated_data,
            } => {
                assert_eq!(data_nonce, 0x0102_0304_0506_0708);
                assert_eq!(encrypted_payload, &[9, 10, 11]);
                assert_eq!(associated_data.len(), WBLOCK_HDR_LEN);
            }
            WfbPacket::SessionKey { .. } => panic!("expected data"),
        }
    }

    #[test]
    fn emits_primary_fragments_in_order() {
        let mut assembler = PlainAssembler::new(2, 4).unwrap();
        let first = assembler
            .push_decrypted_fragment(0, &plain(b"first"))
            .unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].payload, b"first");
        let out = assembler
            .push_decrypted_fragment(1, &plain(b"second"))
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, b"second");
    }

    #[test]
    fn recovers_missing_primary_fragment_from_fec() {
        let fec = FecCode::new(3, 5).unwrap();
        let primary = vec![
            padded(&plain(b"first")),
            padded(&plain(b"second")),
            padded(&plain(b"third")),
        ];
        let parity = fec.encode(&primary, MAX_FEC_PAYLOAD).unwrap();

        let mut assembler = PlainAssembler::new(3, 5).unwrap();
        let first = assembler.push_decrypted_fragment(0, &primary[0]).unwrap();
        assert_eq!(first[0].payload, b"first");
        assert!(assembler
            .push_decrypted_fragment(2, &primary[2])
            .unwrap()
            .is_empty());
        let recovered = assembler.push_decrypted_fragment(3, &parity[0]).unwrap();
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].payload, b"second");
        assert_eq!(recovered[1].payload, b"third");
        assert_eq!(assembler.recovered_packets, 1);
    }

    #[test]
    fn skips_fec_only_plain_packets() {
        let mut fragment = vec![WFB_PACKET_FEC_ONLY];
        fragment.extend_from_slice(&4u16.to_be_bytes());
        fragment.extend_from_slice(b"skip");
        assert!(parse_plain_packet(&fragment).unwrap().is_none());
    }

    #[test]
    fn receiver_decrypts_session_and_data_packet() {
        let rx_secret = SecretKey::from([1; CRYPTO_BOX_SECRETKEY_LEN]);
        let tx_secret = SecretKey::from([2; CRYPTO_BOX_SECRETKEY_LEN]);
        let keypair = WfbKeypair {
            rx_secretkey: rx_secret.to_bytes(),
            tx_publickey: *tx_secret.public_key().as_bytes(),
        };
        let channel_id = ChannelId::default_video();
        let session_key = [7; CHACHA20_POLY1305_KEY_LEN];

        let mut session_plain = Vec::new();
        session_plain.extend_from_slice(&1u64.to_be_bytes());
        session_plain.extend_from_slice(&channel_id.raw().to_be_bytes());
        session_plain.push(WFB_FEC_VDM_RS);
        session_plain.push(1);
        session_plain.push(1);
        session_plain.extend_from_slice(&session_key);
        assert_eq!(session_plain.len(), WSESSION_DATA_LEN);

        let session_nonce = [3; CRYPTO_BOX_NONCE_LEN];
        let tx_box = SalsaBox::new(&rx_secret.public_key(), &tx_secret);
        let encrypted_session = tx_box
            .encrypt(
                BoxNonce::from_slice(&session_nonce),
                session_plain.as_slice(),
            )
            .unwrap();
        let mut session_packet = vec![WFB_PACKET_KEY];
        session_packet.extend_from_slice(&session_nonce);
        session_packet.extend_from_slice(&encrypted_session);

        let mut receiver = WfbReceiver::new(channel_id, keypair, 0);
        let session_events = receiver.push_forwarder_packet(&session_packet).unwrap();
        assert!(matches!(session_events.as_slice(), [WfbEvent::Session(_)]));

        let data_nonce = 0u64;
        let mut block_header = vec![WFB_PACKET_DATA];
        block_header.extend_from_slice(&data_nonce.to_be_bytes());
        let encrypted_data = encrypt_chacha20poly1305_legacy(
            &session_key,
            &block_header[1..WBLOCK_HDR_LEN],
            &block_header,
            &plain(b"rtp payload"),
        )
        .unwrap();
        let mut data_packet = block_header;
        data_packet.extend_from_slice(&encrypted_data);

        let payload_events = receiver.push_forwarder_packet(&data_packet).unwrap();
        match payload_events.as_slice() {
            [WfbEvent::Payload(payload)] => assert_eq!(payload.payload, b"rtp payload"),
            other => panic!("unexpected events: {other:?}"),
        }
    }
}
