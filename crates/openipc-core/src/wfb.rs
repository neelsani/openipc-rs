use std::collections::BTreeMap;

use crypto_box::aead::Aead;
use crypto_box::{Nonce as BoxNonce, PublicKey, SalsaBox, SecretKey};

use crate::channel::ChannelId;
use crate::crypto::decrypt_chacha20poly1305_legacy_into;
use crate::fec::FecCode;

/// WFB WiFi MTU used by OpenIPC forwarder packets.
pub const WIFI_MTU: usize = 4045;
/// 802.11 header length subtracted from WFB packet capacity.
pub const IEEE80211_HEADER_LEN: usize = 24;
/// crypto_box secret key length.
pub const CRYPTO_BOX_SECRETKEY_LEN: usize = 32;
/// crypto_box public key length.
pub const CRYPTO_BOX_PUBLICKEY_LEN: usize = 32;
/// crypto_box nonce length.
pub const CRYPTO_BOX_NONCE_LEN: usize = 24;
/// crypto_box authentication tag length.
pub const CRYPTO_BOX_TAG_LEN: usize = 16;
/// WFB session packet header length.
pub const WSESSION_HDR_LEN: usize = 1 + CRYPTO_BOX_NONCE_LEN;
/// Plain WFB session body length before crypto_box encryption.
pub const WSESSION_DATA_LEN: usize = 8 + 4 + 1 + 1 + 1 + CHACHA20_POLY1305_KEY_LEN;
/// WFB data-block header length.
pub const WBLOCK_HDR_LEN: usize = 9;
/// Plain WFB payload-fragment header length.
pub const WPACKET_HDR_LEN: usize = 3;
/// WFB session ChaCha20-Poly1305 key length.
pub const CHACHA20_POLY1305_KEY_LEN: usize = 32;
/// WFB session ChaCha20-Poly1305 authentication tag length.
pub const CHACHA20_POLY1305_TAG_LEN: usize = 16;
/// Maximum encrypted FEC fragment payload carried by one WFB data packet.
pub const MAX_FEC_PAYLOAD: usize =
    WIFI_MTU - IEEE80211_HEADER_LEN - WBLOCK_HDR_LEN - CHACHA20_POLY1305_TAG_LEN;
/// Maximum application payload before WFB fragment headers are added.
pub const MAX_PAYLOAD_SIZE: usize = MAX_FEC_PAYLOAD - WPACKET_HDR_LEN;
/// Maximum WFB forwarder packet payload after the 802.11 header.
pub const MAX_FORWARDER_PACKET_SIZE: usize = WIFI_MTU - IEEE80211_HEADER_LEN;
/// Largest WFB block index before a transmitter must rotate session keys.
pub const MAX_BLOCK_IDX: u64 = (1u64 << 55) - 1;

/// WFB packet type for encrypted data fragments.
pub const WFB_PACKET_DATA: u8 = 0x01;
/// WFB packet type for encrypted session-key packets.
pub const WFB_PACKET_KEY: u8 = 0x02;
/// FEC type used by WFB's Vandermonde Reed-Solomon blocks.
pub const WFB_FEC_VDM_RS: u8 = 0x01;
/// Flag marking a WFB packet as parity-only FEC data.
pub const WFB_PACKET_FEC_ONLY: u8 = 0x01;

/// Error returned while parsing, decrypting, or assembling WFB packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WfbError {
    /// Packet buffer is empty.
    Empty,
    /// Packet exceeds WFB forwarder size.
    TooLong,
    /// Data packet is too short.
    ShortDataPacket,
    /// Session packet is too short.
    ShortSessionPacket,
    /// WFB keypair is not the expected 64-byte file shape.
    InvalidKeypair,
    /// Session-key encryption failed.
    SessionEncryptFailed,
    /// Session-key decryption failed.
    SessionDecryptFailed,
    /// Data encryption failed.
    DataEncryptFailed,
    /// Data decryption failed.
    DataDecryptFailed,
    /// Session epoch was older than the configured minimum.
    SessionEpochTooOld {
        /// Epoch from the received session packet.
        session_epoch: u64,
        /// Minimum epoch accepted by the receiver.
        minimum_epoch: u64,
    },
    /// Session packet was for a different WFB channel.
    SessionChannelMismatch {
        /// Expected channel id.
        expected: u32,
        /// Actual channel id in the session packet.
        actual: u32,
    },
    /// FEC type is not the supported VDM Reed-Solomon mode.
    UnsupportedFecType(u8),
    /// Forwarder packet type is unknown.
    UnknownPacketType(u8),
    /// FEC parameters are invalid.
    InvalidFecParameters,
    /// Fragment index is outside the current FEC block.
    InvalidFragmentIndex,
    /// Data nonce encoded a block index beyond the supported range.
    BlockIndexOverflow,
    /// Decrypted plain packet is malformed.
    InvalidPlainPacket,
    /// Plain payload exceeds the WFB maximum.
    PayloadTooLarge,
    /// Encrypted data packet arrived before a session key.
    MissingSession,
    /// FEC recovery failed.
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

/// Borrowed WFB forwarder packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WfbPacket<'a> {
    /// Encrypted WFB data/FEC fragment packet.
    Data {
        /// Data nonce; high bits are block index and low byte is fragment index.
        data_nonce: u64,
        /// Encrypted fragment payload plus authentication tag.
        encrypted_payload: &'a [u8],
        /// Associated data used for WFB data authentication.
        associated_data: &'a [u8],
    },
    /// Encrypted WFB session-key packet.
    SessionKey {
        /// crypto_box session nonce.
        session_nonce: &'a [u8],
        /// Encrypted session data.
        encrypted_session: &'a [u8],
    },
}

/// Ground-station WFB keypair file contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WfbKeypair {
    /// Ground-station receive secret key.
    pub rx_secretkey: [u8; CRYPTO_BOX_SECRETKEY_LEN],
    /// Air-unit transmit public key.
    pub tx_publickey: [u8; CRYPTO_BOX_PUBLICKEY_LEN],
}

impl WfbKeypair {
    /// Parse the 64-byte `gs.key` style keypair.
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

/// Cumulative FEC counters for a WFB receiver or assembler.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FecCounters {
    /// Total data fragments observed.
    pub total_packets: u64,
    /// Primary fragments recovered by FEC.
    pub recovered_packets: u64,
    /// Primary fragments considered lost.
    pub lost_packets: u64,
    /// Malformed or unrecoverable fragments.
    pub bad_packets: u64,
}

/// Decrypted WFB session parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WfbSession {
    /// Session epoch.
    pub epoch: u64,
    /// Channel id this session applies to.
    pub channel_id: ChannelId,
    /// WFB FEC type.
    pub fec_type: u8,
    /// Primary fragment count.
    pub fec_k: usize,
    /// Total primary plus parity fragment count.
    pub fec_n: usize,
    /// Symmetric key used for WFB data packets.
    pub session_key: [u8; CHACHA20_POLY1305_KEY_LEN],
}

impl WfbSession {
    fn parse(
        plaintext: &[u8],
        expected_channel_id: ChannelId,
        minimum_epoch: u64,
    ) -> Result<Self, WfbError> {
        if plaintext.len() < WSESSION_DATA_LEN {
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
        if fec_k == 0 || fec_n == 0 || fec_k > fec_n || fec_n >= 256 {
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

/// Event emitted by an encrypted WFB receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WfbEvent {
    /// A session key was accepted.
    Session(WfbSession),
    /// One recovered payload was emitted.
    Payload(WfbOutput),
}

/// Encrypted WFB receiver for one channel.
#[derive(Debug, Clone)]
pub struct WfbReceiver {
    channel_id: ChannelId,
    minimum_epoch: u64,
    keypair: WfbKeypair,
    session: Option<WfbSession>,
    assembler: Option<PlainAssembler>,
    decrypt_scratch: Vec<u8>,
    incoming_packets: u64,
    session_packets: u64,
    data_packets: u64,
}

impl WfbReceiver {
    /// Create a receiver for one channel and keypair.
    pub fn new(channel_id: ChannelId, keypair: WfbKeypair, minimum_epoch: u64) -> Self {
        Self {
            channel_id,
            minimum_epoch,
            keypair,
            session: None,
            assembler: None,
            decrypt_scratch: Vec::with_capacity(MAX_FEC_PAYLOAD),
            incoming_packets: 0,
            session_packets: 0,
            data_packets: 0,
        }
    }

    /// Return the currently accepted WFB session, if any.
    pub fn session(&self) -> Option<&WfbSession> {
        self.session.as_ref()
    }

    /// Return cumulative receive/FEC counters.
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

    /// Push one WFB forwarder packet payload.
    pub fn push_forwarder_packet(&mut self, buf: &[u8]) -> Result<Vec<WfbEvent>, WfbError> {
        let mut events = Vec::new();
        self.push_forwarder_packet_with(buf, &mut |event| events.push(event))?;
        Ok(events)
    }

    pub(crate) fn push_forwarder_packet_with(
        &mut self,
        buf: &[u8],
        emit: &mut impl FnMut(WfbEvent),
    ) -> Result<(), WfbError> {
        log::trace!(target: "openipc_core::wfb", "received WFB forwarder packet bytes={}", buf.len());
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
                    log::info!(
                        target: "openipc_core::wfb",
                        "accepted WFB session epoch={} channel=0x{:08x} fec={}/{}",
                        session.epoch,
                        session.channel_id.raw(),
                        session.fec_k,
                        session.fec_n
                    );
                    self.assembler = Some(PlainAssembler::new(session.fec_k, session.fec_n)?);
                    self.session = Some(session.clone());
                    emit(WfbEvent::Session(session));
                }
                Ok(())
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
                decrypt_chacha20poly1305_legacy_into(
                    &session.session_key,
                    nonce,
                    associated_data,
                    encrypted_payload,
                    &mut self.decrypt_scratch,
                )
                .map_err(|_| WfbError::DataDecryptFailed)?;
                let assembler = self.assembler.as_mut().ok_or(WfbError::MissingSession)?;
                let mut payload_count = 0usize;
                assembler.push_decrypted_fragment_with(
                    data_nonce,
                    &self.decrypt_scratch,
                    &mut |payload| {
                        payload_count += 1;
                        emit(WfbEvent::Payload(payload));
                    },
                )?;
                log::trace!(
                    target: "openipc_core::wfb",
                    "processed encrypted WFB data fragment nonce={} payloads={}",
                    data_nonce,
                    payload_count
                );
                Ok(())
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
        let minimum_epoch = self
            .session
            .as_ref()
            .map(|session| session.epoch.max(self.minimum_epoch))
            .unwrap_or(self.minimum_epoch);
        WfbSession::parse(&plaintext, self.channel_id, minimum_epoch)
    }
}

/// Parse a WFB forwarder packet as data or session-key payload.
pub fn parse_forwarder_packet(buf: &[u8]) -> Result<WfbPacket<'_>, WfbError> {
    if buf.is_empty() {
        return Err(WfbError::Empty);
    }
    if buf.len() > MAX_FORWARDER_PACKET_SIZE {
        return Err(WfbError::TooLong);
    }

    match buf[0] {
        WFB_PACKET_DATA => {
            if buf.len() < WBLOCK_HDR_LEN + WPACKET_HDR_LEN + CHACHA20_POLY1305_TAG_LEN {
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
            if buf.len() < WSESSION_HDR_LEN + WSESSION_DATA_LEN + CRYPTO_BOX_TAG_LEN {
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

/// Recovered WFB application payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WfbOutput {
    /// Recovered packet sequence number.
    pub packet_seq: u64,
    /// Raw application payload bytes.
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Block {
    fragments: Vec<u8>,
    present: Vec<bool>,
    received: usize,
    next_fragment: usize,
}

impl Block {
    fn new(n: usize) -> Self {
        Self {
            fragments: vec![0; n * MAX_FEC_PAYLOAD],
            present: vec![false; n],
            received: 0,
            next_fragment: 0,
        }
    }

    fn reset(&mut self) {
        self.present.fill(false);
        self.received = 0;
        self.next_fragment = 0;
    }
}

/// Plain WFB FEC assembler.
///
/// This is used after data decryption, or directly for tests/pre-decrypted
/// captures. It accepts primary and parity fragments and emits recovered
/// application payloads in order.
#[derive(Debug, Clone)]
pub struct PlainAssembler {
    fec_k: usize,
    fec_n: usize,
    fec: FecCode,
    blocks: BTreeMap<u64, Block>,
    spare_blocks: Vec<Block>,
    next_block: Option<u64>,
    /// Total fragments observed.
    pub total_packets: u64,
    /// Primary fragments considered lost.
    pub lost_packets: u64,
    /// Primary fragments recovered by FEC.
    pub recovered_packets: u64,
    /// Malformed or unrecoverable fragments.
    pub bad_packets: u64,
}

impl PlainAssembler {
    /// Create a plain assembler for `fec_k` primary and `fec_n` total fragments.
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
            spare_blocks: Vec::with_capacity(2),
            next_block: None,
            total_packets: 0,
            lost_packets: 0,
            recovered_packets: 0,
            bad_packets: 0,
        })
    }

    /// Return the primary fragment count.
    pub const fn fec_k(&self) -> usize {
        self.fec_k
    }

    /// Return the total primary plus parity fragment count.
    pub const fn fec_n(&self) -> usize {
        self.fec_n
    }

    /// Reset assembler state and FEC parameters.
    pub fn reset_fec(&mut self, fec_k: usize, fec_n: usize) -> Result<(), WfbError> {
        *self = Self::new(fec_k, fec_n)?;
        Ok(())
    }

    /// Return cumulative FEC counters.
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
        let mut outputs = Vec::new();
        self.push_decrypted_fragment_with(data_nonce, fragment, &mut |output| {
            outputs.push(output);
        })?;
        Ok(outputs)
    }

    pub(crate) fn push_decrypted_fragment_with(
        &mut self,
        data_nonce: u64,
        fragment: &[u8],
        emit: &mut impl FnMut(WfbOutput),
    ) -> Result<(), WfbError> {
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
        if self
            .next_block
            .map(|next_block| block_idx < next_block)
            .unwrap_or(false)
        {
            return Ok(());
        }

        if !self.blocks.contains_key(&block_idx) {
            let mut block = self
                .spare_blocks
                .pop()
                .unwrap_or_else(|| Block::new(self.fec_n));
            block.reset();
            self.blocks.insert(block_idx, block);
        }
        let block = self
            .blocks
            .get_mut(&block_idx)
            .expect("block was inserted above");
        if !block.present[fragment_idx] {
            let len = fragment.len().min(MAX_FEC_PAYLOAD);
            let start = fragment_idx * MAX_FEC_PAYLOAD;
            let slot = &mut block.fragments[start..start + MAX_FEC_PAYLOAD];
            slot.fill(0);
            slot[..len].copy_from_slice(&fragment[..len]);
            block.present[fragment_idx] = true;
            block.received += 1;
        }

        self.drain_ready_blocks(emit);
        Ok(())
    }

    fn drain_ready_blocks(&mut self, emit: &mut impl FnMut(WfbOutput)) {
        while let Some(block_idx) = self.next_block {
            if !self.blocks.contains_key(&block_idx) {
                if self.should_skip_missing_block(block_idx) {
                    self.lost_packets += self.fec_k as u64;
                    self.next_block = Some(block_idx + 1);
                    continue;
                }
                break;
            }

            self.emit_contiguous_primary(block_idx, emit);
            let complete = self
                .blocks
                .get(&block_idx)
                .map(|block| block.next_fragment == self.fec_k)
                .unwrap_or(false);
            if complete {
                if let Some(block) = self.blocks.remove(&block_idx) {
                    self.recycle_block(block);
                }
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
                    match self.fec.recover_primary_into(
                        &mut block.fragments,
                        &mut block.present,
                        MAX_FEC_PAYLOAD,
                    ) {
                        Ok(recovered) => {
                            if recovered > 0 {
                                log::debug!(
                                    target: "openipc_core::fec",
                                    "recovered missing primary WFB fragments block={} recovered={}",
                                    block_idx,
                                    recovered
                                );
                            }
                            self.recovered_packets += recovered as u64;
                        }
                        Err(error) => {
                            log::warn!(
                                target: "openipc_core::fec",
                                "FEC recovery failed block={block_idx}: {error}"
                            );
                            self.bad_packets += 1;
                            self.force_flush_block(block_idx, emit);
                            continue;
                        }
                    }
                }
                self.emit_contiguous_primary(block_idx, emit);
                if let Some(block) = self.blocks.remove(&block_idx) {
                    self.recycle_block(block);
                }
                self.next_block = Some(block_idx + 1);
                continue;
            }

            if self.should_force_flush(block_idx) {
                self.force_flush_block(block_idx, emit);
                continue;
            }

            break;
        }
    }

    fn should_skip_missing_block(&self, block_idx: u64) -> bool {
        let Some((&next_present_block, block)) = self.blocks.range((block_idx + 1)..).next() else {
            return false;
        };

        block.received >= self.fec_k
            || self.blocks.len() > 40
            || next_present_block.saturating_sub(block_idx) >= 40
    }

    fn emit_contiguous_primary(&mut self, block_idx: u64, emit: &mut impl FnMut(WfbOutput)) {
        let Some(block) = self.blocks.get_mut(&block_idx) else {
            return;
        };
        while block.next_fragment < self.fec_k {
            let fragment_idx = block.next_fragment;
            if !block.present[fragment_idx] {
                break;
            }
            let start = fragment_idx * MAX_FEC_PAYLOAD;
            let fragment = &block.fragments[start..start + MAX_FEC_PAYLOAD];
            let packet_seq = block_idx * self.fec_k as u64 + fragment_idx as u64;
            match parse_plain_packet(fragment) {
                Ok(Some(payload)) => emit(WfbOutput {
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

    fn force_flush_block(&mut self, block_idx: u64, emit: &mut impl FnMut(WfbOutput)) {
        if let Some(block) = self.blocks.remove(&block_idx) {
            for fragment_idx in block.next_fragment..self.fec_k {
                let packet_seq = block_idx * self.fec_k as u64 + fragment_idx as u64;
                match block.present[fragment_idx] {
                    true => {
                        let start = fragment_idx * MAX_FEC_PAYLOAD;
                        match parse_plain_packet(&block.fragments[start..start + MAX_FEC_PAYLOAD]) {
                            Ok(Some(payload)) => emit(WfbOutput {
                                packet_seq,
                                payload: payload.to_vec(),
                            }),
                            Ok(None) => {}
                            Err(_) => {
                                self.bad_packets += 1;
                            }
                        }
                    }
                    false => {
                        self.lost_packets += 1;
                    }
                }
            }
            self.next_block = Some(block_idx + 1);
            self.recycle_block(block);
        }
    }

    fn recycle_block(&mut self, mut block: Block) {
        const MAX_SPARE_BLOCKS: usize = 4;
        if self.spare_blocks.len() < MAX_SPARE_BLOCKS {
            block.reset();
            self.spare_blocks.push(block);
        }
    }
}

/// Parse a decrypted WFB plain packet and return payload bytes when present.
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
        let encrypted = [9; WPACKET_HDR_LEN + CHACHA20_POLY1305_TAG_LEN];
        packet.extend_from_slice(&encrypted);

        let parsed = parse_forwarder_packet(&packet).unwrap();
        match parsed {
            WfbPacket::Data {
                data_nonce,
                encrypted_payload,
                associated_data,
            } => {
                assert_eq!(data_nonce, 0x0102_0304_0506_0708);
                assert_eq!(encrypted_payload, encrypted);
                assert_eq!(associated_data.len(), WBLOCK_HDR_LEN);
            }
            WfbPacket::SessionKey { .. } => panic!("expected data"),
        }
    }

    #[test]
    fn rejects_data_packets_without_encrypted_plain_header_and_tag() {
        let mut packet = vec![WFB_PACKET_DATA];
        packet.extend_from_slice(&0x0102_0304_0506_0708u64.to_be_bytes());
        packet.extend_from_slice(&[0; WPACKET_HDR_LEN + CHACHA20_POLY1305_TAG_LEN - 1]);

        assert_eq!(
            parse_forwarder_packet(&packet),
            Err(WfbError::ShortDataPacket)
        );
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
    fn reuses_completed_fec_block_storage() {
        let mut assembler = PlainAssembler::new(1, 1).unwrap();
        let first = padded(&plain(b"first"));
        assert_eq!(
            assembler.push_decrypted_fragment(0, &first).unwrap()[0].payload,
            b"first"
        );
        assert_eq!(assembler.spare_blocks.len(), 1);

        let allocation = assembler.spare_blocks[0].fragments.as_ptr();
        let second = plain(b"second");
        assert_eq!(
            assembler.push_decrypted_fragment(1 << 8, &second).unwrap()[0].payload,
            b"second"
        );
        assert_eq!(assembler.spare_blocks.len(), 1);
        assert_eq!(assembler.spare_blocks[0].fragments.as_ptr(), allocation);
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
    fn skips_fully_missing_blocks_when_later_block_is_ready() {
        let mut assembler = PlainAssembler::new(2, 2).unwrap();

        let first = assembler
            .push_decrypted_fragment(0, &plain(b"b0-f0"))
            .unwrap();
        assert_eq!(first[0].payload, b"b0-f0");

        assert!(assembler
            .push_decrypted_fragment(2 << 8, &plain(b"b2-f0"))
            .unwrap()
            .is_empty());
        let out = assembler
            .push_decrypted_fragment((2 << 8) | 1, &plain(b"b2-f1"))
            .unwrap();

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].payload, b"b2-f0");
        assert_eq!(out[1].payload, b"b2-f1");
        assert_eq!(assembler.lost_packets, 3);
    }

    #[test]
    fn ignores_late_fragments_from_already_flushed_blocks() {
        let mut assembler = PlainAssembler::new(2, 2).unwrap();

        assembler
            .push_decrypted_fragment(0, &plain(b"b0-f0"))
            .unwrap();
        assembler
            .push_decrypted_fragment(2 << 8, &plain(b"b2-f0"))
            .unwrap();
        assembler
            .push_decrypted_fragment((2 << 8) | 1, &plain(b"b2-f1"))
            .unwrap();

        let late = assembler
            .push_decrypted_fragment(1 << 8, &plain(b"late-b1-f0"))
            .unwrap();
        assert!(late.is_empty());
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
        // wfb-ng allows encrypted optional session TLVs after the fixed fields.
        session_plain.extend_from_slice(&[0x42, 0x00, 0x01, 0x99]);

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

        let mut older_session_plain = Vec::new();
        older_session_plain.extend_from_slice(&0u64.to_be_bytes());
        older_session_plain.extend_from_slice(&channel_id.raw().to_be_bytes());
        older_session_plain.push(WFB_FEC_VDM_RS);
        older_session_plain.push(1);
        older_session_plain.push(1);
        older_session_plain.extend_from_slice(&[8; CHACHA20_POLY1305_KEY_LEN]);
        let older_session_nonce = [4; CRYPTO_BOX_NONCE_LEN];
        let encrypted_older_session = tx_box
            .encrypt(
                BoxNonce::from_slice(&older_session_nonce),
                older_session_plain.as_slice(),
            )
            .unwrap();
        let mut older_session_packet = vec![WFB_PACKET_KEY];
        older_session_packet.extend_from_slice(&older_session_nonce);
        older_session_packet.extend_from_slice(&encrypted_older_session);

        assert_eq!(
            receiver.push_forwarder_packet(&older_session_packet),
            Err(WfbError::SessionEpochTooOld {
                session_epoch: 0,
                minimum_epoch: 1,
            })
        );
    }
}
