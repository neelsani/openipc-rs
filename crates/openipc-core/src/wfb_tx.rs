use crypto_box::aead::Aead;
use crypto_box::{Nonce as BoxNonce, PublicKey, SalsaBox, SecretKey};
use rand_core::{OsRng, RngCore};

use crate::channel::ChannelId;
use crate::crypto::encrypt_chacha20poly1305_legacy;
use crate::fec::FecCode;
use crate::ieee80211::build_wfb_header_with_frame_type;
use crate::radiotap::{build_radiotap_header, TxRadioParams};
use crate::wfb::{
    WfbError, CHACHA20_POLY1305_KEY_LEN, CRYPTO_BOX_NONCE_LEN, CRYPTO_BOX_PUBLICKEY_LEN,
    CRYPTO_BOX_SECRETKEY_LEN, MAX_BLOCK_IDX, MAX_FEC_PAYLOAD, MAX_PAYLOAD_SIZE, WBLOCK_HDR_LEN,
    WFB_FEC_VDM_RS, WFB_PACKET_DATA, WFB_PACKET_KEY, WPACKET_HDR_LEN, WSESSION_DATA_LEN,
    WSESSION_HDR_LEN,
};

/// Key material used by the ground station when transmitting WFB uplink data.
///
/// This is the inverse of `WfbKeypair`: it contains the transmitter secret key
/// and the receiver public key needed to encrypt WFB session packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WfbTxKeypair {
    /// Secret key for the local transmitter.
    pub tx_secretkey: [u8; CRYPTO_BOX_SECRETKEY_LEN],
    /// Public key for the remote receiver.
    pub rx_publickey: [u8; CRYPTO_BOX_PUBLICKEY_LEN],
}

impl WfbTxKeypair {
    /// Parse a concatenated transmitter-secret + receiver-public keypair.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, WfbError> {
        if bytes.len() != CRYPTO_BOX_SECRETKEY_LEN + CRYPTO_BOX_PUBLICKEY_LEN {
            return Err(WfbError::InvalidKeypair);
        }
        let mut tx_secretkey = [0; CRYPTO_BOX_SECRETKEY_LEN];
        let mut rx_publickey = [0; CRYPTO_BOX_PUBLICKEY_LEN];
        tx_secretkey.copy_from_slice(&bytes[..CRYPTO_BOX_SECRETKEY_LEN]);
        rx_publickey.copy_from_slice(&bytes[CRYPTO_BOX_SECRETKEY_LEN..]);
        Ok(Self {
            tx_secretkey,
            rx_publickey,
        })
    }
}

/// Stateful WFB transmitter for adaptive-link and other uplink payloads.
///
/// The transmitter owns the current WFB session key, fragments payloads into
/// FEC blocks, emits parity fragments when configured, encrypts each block
/// fragment, and can optionally wrap packets in radiotap + 802.11 headers for
/// direct radio injection.
#[derive(Debug, Clone)]
pub struct WfbTransmitter {
    channel_id: ChannelId,
    keypair: WfbTxKeypair,
    epoch: u64,
    fec_k: usize,
    fec_n: usize,
    fec: FecCode,
    block: Vec<Vec<u8>>,
    block_index: u64,
    fragment_index: usize,
    max_packet_size: usize,
    session_key: [u8; CHACHA20_POLY1305_KEY_LEN],
    session_packet: Vec<u8>,
    sequence_control: u16,
}

impl WfbTransmitter {
    /// Create a transmitter for one WFB channel.
    ///
    /// `fec_k` is the number of source fragments per block and `fec_n` is the
    /// total number of source + parity fragments transmitted for that block.
    pub fn new(
        channel_id: ChannelId,
        keypair: WfbTxKeypair,
        epoch: u64,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<Self, WfbError> {
        if fec_k == 0 || fec_n == 0 || fec_k > fec_n || fec_n > 255 {
            return Err(WfbError::InvalidFecParameters);
        }
        let fec = FecCode::new(fec_k, fec_n).map_err(|_| WfbError::InvalidFecParameters)?;
        let mut tx = Self {
            channel_id,
            keypair,
            epoch,
            fec_k,
            fec_n,
            fec,
            block: vec![vec![0; MAX_FEC_PAYLOAD]; fec_n],
            block_index: 0,
            fragment_index: 0,
            max_packet_size: 0,
            session_key: [0; CHACHA20_POLY1305_KEY_LEN],
            session_packet: Vec::new(),
            sequence_control: 0,
        };
        tx.rotate_session_key()?;
        Ok(tx)
    }

    /// Return the WFB channel this transmitter writes to.
    pub const fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    /// Return the number of source fragments in each FEC block.
    pub const fn fec_k(&self) -> usize {
        self.fec_k
    }

    /// Return the total number of source + parity fragments in each FEC block.
    pub const fn fec_n(&self) -> usize {
        self.fec_n
    }

    /// Return the current encrypted WFB session packet without radio headers.
    ///
    /// Send this periodically before data packets so receivers can establish or
    /// refresh the session key for this channel.
    pub fn session_forwarder_packet(&self) -> &[u8] {
        &self.session_packet
    }

    /// Build the current session packet as a radiotap + 802.11 radio packet.
    pub fn session_radio_packet(&mut self, params: TxRadioParams) -> Vec<u8> {
        let packet = self.session_packet.clone();
        self.wrap_forwarder_packet(&packet, params)
    }

    /// Fragment, encrypt, FEC-encode, and wrap one payload for radio injection.
    pub fn radio_packets_for_payload(
        &mut self,
        payload: &[u8],
        params: TxRadioParams,
    ) -> Result<Vec<Vec<u8>>, WfbError> {
        let packets = self.forwarder_packets_for_payload(payload, 0)?;
        Ok(packets
            .into_iter()
            .map(|packet| self.wrap_forwarder_packet(&packet, params))
            .collect())
    }

    /// Fragment, encrypt, and FEC-encode one payload as WFB forwarder packets.
    ///
    /// The returned packets do not include radiotap or 802.11 headers, which
    /// makes this useful when another layer owns radio framing.
    pub fn forwarder_packets_for_payload(
        &mut self,
        payload: &[u8],
        flags: u8,
    ) -> Result<Vec<Vec<u8>>, WfbError> {
        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(WfbError::PayloadTooLarge);
        }

        let fragment_index = self.fragment_index;
        let fragment = &mut self.block[fragment_index];
        fragment.fill(0);
        fragment[0] = flags;
        fragment[1..3].copy_from_slice(&(payload.len() as u16).to_be_bytes());
        fragment[WPACKET_HDR_LEN..WPACKET_HDR_LEN + payload.len()].copy_from_slice(payload);
        let packet_size = WPACKET_HDR_LEN + payload.len();

        let mut out = vec![self.encrypt_block_fragment(fragment_index, packet_size)?];
        self.max_packet_size = self.max_packet_size.max(packet_size);
        self.fragment_index += 1;

        if self.fragment_index == self.fec_k {
            if self.fec_n > self.fec_k {
                let parity = self
                    .fec
                    .encode(&self.block[..self.fec_k], self.max_packet_size)
                    .map_err(|_| WfbError::FecRecoveryFailed)?;
                for (offset, parity_fragment) in parity.into_iter().enumerate() {
                    let idx = self.fec_k + offset;
                    self.block[idx].fill(0);
                    self.block[idx][..parity_fragment.len()].copy_from_slice(&parity_fragment);
                    out.push(self.encrypt_block_fragment(idx, self.max_packet_size)?);
                }
            }
            self.finish_block()?;
        }

        Ok(out)
    }

    fn finish_block(&mut self) -> Result<(), WfbError> {
        self.block_index += 1;
        self.fragment_index = 0;
        self.max_packet_size = 0;
        if self.block_index > MAX_BLOCK_IDX {
            self.block_index = 0;
            self.rotate_session_key()?;
        }
        Ok(())
    }

    fn encrypt_block_fragment(
        &self,
        fragment_index: usize,
        packet_size: usize,
    ) -> Result<Vec<u8>, WfbError> {
        let data_nonce = ((self.block_index & MAX_BLOCK_IDX) << 8) | fragment_index as u64;
        let mut block_header = [0u8; WBLOCK_HDR_LEN];
        block_header[0] = WFB_PACKET_DATA;
        block_header[1..].copy_from_slice(&data_nonce.to_be_bytes());
        let nonce = &block_header[1..WBLOCK_HDR_LEN];
        let encrypted = encrypt_chacha20poly1305_legacy(
            &self.session_key,
            nonce,
            &block_header,
            &self.block[fragment_index][..packet_size],
        )
        .map_err(|_| WfbError::DataEncryptFailed)?;

        let mut out = Vec::with_capacity(WBLOCK_HDR_LEN + encrypted.len());
        out.extend_from_slice(&block_header);
        out.extend_from_slice(&encrypted);
        Ok(out)
    }

    fn rotate_session_key(&mut self) -> Result<(), WfbError> {
        OsRng.fill_bytes(&mut self.session_key);
        self.session_packet = self.build_session_packet()?;
        Ok(())
    }

    fn build_session_packet(&self) -> Result<Vec<u8>, WfbError> {
        let mut nonce = [0u8; CRYPTO_BOX_NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);

        let mut session_data = [0u8; WSESSION_DATA_LEN];
        session_data[0..8].copy_from_slice(&self.epoch.to_be_bytes());
        session_data[8..12].copy_from_slice(&self.channel_id.raw().to_be_bytes());
        session_data[12] = WFB_FEC_VDM_RS;
        session_data[13] = self.fec_k as u8;
        session_data[14] = self.fec_n as u8;
        session_data[15..47].copy_from_slice(&self.session_key);

        let tx_secret = SecretKey::from(self.keypair.tx_secretkey);
        let rx_public = PublicKey::from(self.keypair.rx_publickey);
        let cipher = SalsaBox::new(&rx_public, &tx_secret);
        let encrypted = cipher
            .encrypt(BoxNonce::from_slice(&nonce), session_data.as_slice())
            .map_err(|_| WfbError::SessionEncryptFailed)?;

        let mut out = Vec::with_capacity(WSESSION_HDR_LEN + encrypted.len());
        out.push(WFB_PACKET_KEY);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&encrypted);
        Ok(out)
    }

    fn wrap_forwarder_packet(&mut self, forwarder_packet: &[u8], params: TxRadioParams) -> Vec<u8> {
        let mut out = build_radiotap_header(params);
        let seq = self.sequence_control.to_le_bytes();
        out.extend_from_slice(&build_wfb_header_with_frame_type(
            self.channel_id,
            seq,
            params.frame_type,
        ));
        out.extend_from_slice(forwarder_packet);
        self.sequence_control = self.sequence_control.wrapping_add(16);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfb::{WfbKeypair, WfbReceiver};
    use crypto_box::SecretKey;

    fn linked_keypairs() -> (WfbTxKeypair, WfbKeypair) {
        let ground_secret = SecretKey::from([3u8; 32]);
        let air_secret = SecretKey::from([9u8; 32]);
        let ground_public = ground_secret.public_key();
        let air_public = air_secret.public_key();
        (
            WfbTxKeypair {
                tx_secretkey: ground_secret.to_bytes(),
                rx_publickey: air_public.to_bytes(),
            },
            WfbKeypair {
                rx_secretkey: air_secret.to_bytes(),
                tx_publickey: ground_public.to_bytes(),
            },
        )
    }

    #[test]
    fn transmitted_session_and_payload_roundtrip() {
        let channel = ChannelId::from_link_port(0x112233, crate::RadioPort::TunnelTx);
        let (tx_keys, rx_keys) = linked_keypairs();
        let mut tx = WfbTransmitter::new(channel, tx_keys, 42, 1, 1).unwrap();
        let mut rx = WfbReceiver::new(channel, rx_keys, 0);

        let session_events = rx
            .push_forwarder_packet(tx.session_forwarder_packet())
            .unwrap();
        assert_eq!(session_events.len(), 1);

        let data_packets = tx.forwarder_packets_for_payload(b"hello", 0).unwrap();
        assert_eq!(data_packets.len(), 1);
        let events = rx.push_forwarder_packet(&data_packets[0]).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            crate::wfb::WfbEvent::Payload(payload) => assert_eq!(payload.payload, b"hello"),
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
