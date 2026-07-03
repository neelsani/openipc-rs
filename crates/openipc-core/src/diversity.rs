//! Packet-selection diversity for multiple independent receive radios.
//!
//! Radios tuned to the same channel often receive the same WFB fragment. The
//! combiner forwards the first valid copy immediately and rejects later copies
//! before session decryption and FEC assembly. It intentionally does not wait
//! for a stronger copy, so enabling diversity adds no comparison window to the
//! media path.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::{
    ieee80211::{FrameLayout, WifiFrame},
    wfb::{parse_forwarder_packet, WfbPacket, CRYPTO_BOX_NONCE_LEN},
    ChannelId,
};

/// Stable index assigned to one receive radio for the lifetime of a receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiversitySourceId(u16);

impl DiversitySourceId {
    /// Construct a source id from an application-assigned index.
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Return the application-assigned source index.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Result of examining one valid 802.11 receive frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiversityDecision {
    /// First observed copy of a WFB session or data packet.
    Accept,
    /// A previously accepted radio already delivered this WFB packet.
    Duplicate,
    /// The frame is not identifiable as WFB and should continue normally.
    Passthrough,
}

impl DiversityDecision {
    /// Return true when the frame should enter the shared receiver pipeline.
    pub const fn should_forward(self) -> bool {
        !matches!(self, Self::Duplicate)
    }
}

/// Cumulative packet-selection counters for one receive radio.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiversitySourceStats {
    /// Valid frames examined from this source.
    pub observed: u64,
    /// First WFB copies contributed by this source.
    pub accepted: u64,
    /// Copies discarded because another source arrived first.
    pub duplicates: u64,
    /// Frames that could not be classified as a WFB packet.
    pub passthrough: u64,
}

/// Cumulative state of the packet diversity combiner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiversityStats {
    /// First WFB copies forwarded to the protocol pipeline.
    pub accepted: u64,
    /// Duplicate WFB copies discarded before decryption.
    pub duplicates: u64,
    /// Unclassified frames forwarded without deduplication.
    pub passthrough: u64,
    /// Current number of packet identities in the bounded deduplication cache.
    pub cached_packets: usize,
    /// Per-radio contribution counters.
    pub sources: BTreeMap<DiversitySourceId, DiversitySourceStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PacketIdentity {
    Data {
        channel_id: ChannelId,
        session_generation: u64,
        data_nonce: u64,
    },
    Session {
        channel_id: ChannelId,
        nonce: [u8; CRYPTO_BOX_NONCE_LEN],
    },
}

/// Bounded first-valid-copy selector shared by all receive adapters.
#[derive(Debug, Clone)]
pub struct DiversityCombiner {
    capacity: usize,
    seen: HashSet<PacketIdentity>,
    insertion_order: VecDeque<PacketIdentity>,
    session_generations: HashMap<ChannelId, u64>,
    current_sessions: HashMap<ChannelId, [u8; CRYPTO_BOX_NONCE_LEN]>,
    session_order: VecDeque<ChannelId>,
    stats: DiversityStats,
}

impl Default for DiversityCombiner {
    fn default() -> Self {
        Self::new(8_192)
    }
}

impl DiversityCombiner {
    /// Create a combiner retaining at most `capacity` recent WFB identities.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            seen: HashSet::with_capacity(capacity.min(8_192)),
            insertion_order: VecDeque::with_capacity(capacity.min(8_192)),
            session_generations: HashMap::new(),
            current_sessions: HashMap::new(),
            session_order: VecDeque::new(),
            stats: DiversityStats::default(),
        }
    }

    /// Examine one CRC/ICV-valid frame and select the first WFB copy.
    ///
    /// Corrupt descriptor packets must be rejected by the caller before this
    /// method. Unrecognized frames are passed through so diversity does not
    /// change route-manager behavior for future payload formats.
    pub fn observe_frame(
        &mut self,
        source: DiversitySourceId,
        frame: &[u8],
        layout: FrameLayout,
    ) -> DiversityDecision {
        self.source_mut(source).observed += 1;
        let Some(identity) = self.packet_identity(frame, layout) else {
            self.stats.passthrough += 1;
            self.source_mut(source).passthrough += 1;
            return DiversityDecision::Passthrough;
        };

        if self.seen.contains(&identity) {
            self.stats.duplicates += 1;
            self.source_mut(source).duplicates += 1;
            return DiversityDecision::Duplicate;
        }

        if let PacketIdentity::Session { channel_id, nonce } = identity {
            let changed = self
                .current_sessions
                .get(&channel_id)
                .map(|current| current != &nonce)
                .unwrap_or(true);
            if changed {
                if !self.current_sessions.contains_key(&channel_id) {
                    self.remember_session_channel(channel_id);
                }
                let generation = self.session_generations.entry(channel_id).or_default();
                *generation = generation.wrapping_add(1);
                self.current_sessions.insert(channel_id, nonce);
            }
        }

        self.remember(identity);
        self.stats.accepted += 1;
        self.source_mut(source).accepted += 1;
        DiversityDecision::Accept
    }

    /// Return a snapshot of cumulative diversity counters.
    pub fn stats(&self) -> DiversityStats {
        let mut stats = self.stats.clone();
        stats.cached_packets = self.seen.len();
        stats
    }

    /// Clear packet identities and counters while keeping the configured size.
    pub fn reset(&mut self) {
        self.seen.clear();
        self.insertion_order.clear();
        self.session_generations.clear();
        self.current_sessions.clear();
        self.session_order.clear();
        self.stats = DiversityStats::default();
    }

    fn packet_identity(&self, frame: &[u8], layout: FrameLayout) -> Option<PacketIdentity> {
        let frame = WifiFrame::parse(frame, layout).ok()?;
        let channel_id = frame.channel_id()?;
        match parse_forwarder_packet(frame.payload()).ok()? {
            WfbPacket::Data { data_nonce, .. } => Some(PacketIdentity::Data {
                channel_id,
                session_generation: self
                    .session_generations
                    .get(&channel_id)
                    .copied()
                    .unwrap_or(0),
                data_nonce,
            }),
            WfbPacket::SessionKey { session_nonce, .. } => {
                let nonce = session_nonce.try_into().ok()?;
                Some(PacketIdentity::Session { channel_id, nonce })
            }
        }
    }

    fn remember(&mut self, identity: PacketIdentity) {
        if self.seen.insert(identity) {
            self.insertion_order.push_back(identity);
        }
        while self.insertion_order.len() > self.capacity {
            if let Some(expired) = self.insertion_order.pop_front() {
                self.seen.remove(&expired);
            }
        }
    }

    fn source_mut(&mut self, source: DiversitySourceId) -> &mut DiversitySourceStats {
        self.stats.sources.entry(source).or_default()
    }

    fn remember_session_channel(&mut self, channel_id: ChannelId) {
        while self.session_order.len() >= self.capacity {
            if let Some(expired) = self.session_order.pop_front() {
                self.current_sessions.remove(&expired);
                self.session_generations.remove(&expired);
            }
        }
        self.session_order.push_back(channel_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fec::FecCode,
        ieee80211::build_wfb_header,
        wfb::{PlainAssembler, CHACHA20_POLY1305_TAG_LEN, MAX_FEC_PAYLOAD, WPACKET_HDR_LEN},
        PayloadPipeline, PayloadPipelineEvent, WfbKeypair, WfbTransmitter, WfbTxKeypair,
    };
    use crypto_box::SecretKey;

    fn data_frame(channel: ChannelId, data_nonce: u64) -> Vec<u8> {
        let mut frame = Vec::from(build_wfb_header(channel, [0, 0]));
        frame.push(1);
        frame.extend_from_slice(&data_nonce.to_be_bytes());
        frame.resize(frame.len() + WPACKET_HDR_LEN + CHACHA20_POLY1305_TAG_LEN, 0);
        frame.extend_from_slice(&[0; 4]);
        frame
    }

    fn session_frame(channel: ChannelId, marker: u8) -> Vec<u8> {
        let mut frame = Vec::from(build_wfb_header(channel, [0, 0]));
        frame.push(2);
        frame.extend_from_slice(&[marker; CRYPTO_BOX_NONCE_LEN]);
        frame.resize(frame.len() + crate::wfb::WSESSION_DATA_LEN + 16, 0);
        frame.extend_from_slice(&[0; 4]);
        frame
    }

    fn plain(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0];
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out.resize(MAX_FEC_PAYLOAD, 0);
        out
    }

    fn linked_keypairs() -> (WfbTxKeypair, WfbKeypair) {
        let transmitter = SecretKey::from([3u8; 32]);
        let receiver = SecretKey::from([9u8; 32]);
        (
            WfbTxKeypair {
                tx_secretkey: transmitter.to_bytes(),
                rx_publickey: receiver.public_key().to_bytes(),
            },
            WfbKeypair {
                rx_secretkey: receiver.to_bytes(),
                tx_publickey: transmitter.public_key().to_bytes(),
            },
        )
    }

    fn wrap_forwarder_packet(channel: ChannelId, packet: &[u8]) -> Vec<u8> {
        let mut frame = Vec::from(build_wfb_header(channel, [0, 0]));
        frame.extend_from_slice(packet);
        frame.extend_from_slice(&[0; 4]);
        frame
    }

    #[test]
    fn first_valid_radio_wins_without_delaying_the_packet() {
        let mut combiner = DiversityCombiner::default();
        let frame = data_frame(ChannelId::default_video(), 42);

        assert_eq!(
            combiner.observe_frame(DiversitySourceId::new(1), &frame, FrameLayout::WithFcs),
            DiversityDecision::Accept
        );
        assert_eq!(
            combiner.observe_frame(DiversitySourceId::new(2), &frame, FrameLayout::WithFcs),
            DiversityDecision::Duplicate
        );
        let stats = combiner.stats();
        assert_eq!(stats.accepted, 1);
        assert_eq!(stats.duplicates, 1);
        assert_eq!(stats.sources[&DiversitySourceId::new(1)].accepted, 1);
        assert_eq!(stats.sources[&DiversitySourceId::new(2)].duplicates, 1);
    }

    #[test]
    fn a_new_session_can_reuse_data_nonces() {
        let mut combiner = DiversityCombiner::default();
        let channel = ChannelId::default_video();
        let data = data_frame(channel, 7);

        assert_eq!(
            combiner.observe_frame(
                DiversitySourceId::new(0),
                &session_frame(channel, 1),
                FrameLayout::WithFcs,
            ),
            DiversityDecision::Accept
        );
        assert_eq!(
            combiner.observe_frame(DiversitySourceId::new(0), &data, FrameLayout::WithFcs),
            DiversityDecision::Accept
        );
        assert_eq!(
            combiner.observe_frame(DiversitySourceId::new(1), &data, FrameLayout::WithFcs),
            DiversityDecision::Duplicate
        );
        assert_eq!(
            combiner.observe_frame(
                DiversitySourceId::new(1),
                &session_frame(channel, 2),
                FrameLayout::WithFcs,
            ),
            DiversityDecision::Accept
        );
        assert_eq!(
            combiner.observe_frame(DiversitySourceId::new(1), &data, FrameLayout::WithFcs),
            DiversityDecision::Accept
        );
    }

    #[test]
    fn session_tracking_is_bounded_with_the_packet_cache() {
        let mut combiner = DiversityCombiner::new(1);
        let first = ChannelId::new(1);
        let second = ChannelId::new(2);

        combiner.observe_frame(
            DiversitySourceId::new(0),
            &session_frame(first, 1),
            FrameLayout::WithFcs,
        );
        combiner.observe_frame(
            DiversitySourceId::new(0),
            &session_frame(second, 2),
            FrameLayout::WithFcs,
        );

        assert_eq!(combiner.current_sessions.len(), 1);
        assert!(!combiner.current_sessions.contains_key(&first));
        assert!(combiner.current_sessions.contains_key(&second));
    }

    #[test]
    fn fragments_from_two_radios_recover_one_shared_fec_block() {
        let channel = ChannelId::default_video();
        let primary = [plain(b"first"), plain(b"second"), plain(b"third")];
        let parity = FecCode::new(3, 5)
            .unwrap()
            .encode(&primary, MAX_FEC_PAYLOAD)
            .unwrap();
        let arrivals = [
            (DiversitySourceId::new(0), 0, primary[0].as_slice()),
            (DiversitySourceId::new(1), 0, primary[0].as_slice()),
            (DiversitySourceId::new(0), 2, primary[2].as_slice()),
            (DiversitySourceId::new(1), 3, parity[0].as_slice()),
        ];
        let mut combiner = DiversityCombiner::default();
        let mut assembler = PlainAssembler::new(3, 5).unwrap();
        let mut output = Vec::new();

        for (source, nonce, fragment) in arrivals {
            let frame = data_frame(channel, nonce);
            if combiner
                .observe_frame(source, &frame, FrameLayout::WithFcs)
                .should_forward()
            {
                output.extend(assembler.push_decrypted_fragment(nonce, fragment).unwrap());
            }
        }

        assert_eq!(
            output
                .into_iter()
                .map(|packet| packet.payload)
                .collect::<Vec<_>>(),
            vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()]
        );
        assert_eq!(combiner.stats().duplicates, 1);
        assert_eq!(assembler.recovered_packets, 1);
    }

    #[test]
    fn encrypted_fragments_from_two_radios_share_one_pipeline() {
        let channel = ChannelId::default_video();
        let (tx_keys, rx_keys) = linked_keypairs();
        let mut transmitter = WfbTransmitter::new(channel, tx_keys, 42, 2, 3).unwrap();
        let mut pipeline =
            PayloadPipeline::with_keypair(channel, FrameLayout::WithFcs, rx_keys, 0).unwrap();
        let mut combiner = DiversityCombiner::default();

        let session = wrap_forwarder_packet(channel, transmitter.session_forwarder_packet());
        assert!(combiner
            .observe_frame(DiversitySourceId::new(0), &session, FrameLayout::WithFcs)
            .should_forward());
        let events = pipeline.push_80211_frame(&session).unwrap();
        assert!(matches!(
            events.as_slice(),
            [PayloadPipelineEvent::SessionEstablished {
                epoch: 42,
                fec_k: 2,
                fec_n: 3
            }]
        ));

        let missing_primary = transmitter
            .forwarder_packets_for_payload(b"first", 0)
            .unwrap();
        assert_eq!(missing_primary.len(), 1);
        let second_and_parity = transmitter
            .forwarder_packets_for_payload(b"second", 0)
            .unwrap();
        assert_eq!(second_and_parity.len(), 2);
        let second = wrap_forwarder_packet(channel, &second_and_parity[0]);
        let parity = wrap_forwarder_packet(channel, &second_and_parity[1]);

        let arrivals = [
            (DiversitySourceId::new(0), second.as_slice()),
            (DiversitySourceId::new(1), second.as_slice()),
            (DiversitySourceId::new(1), parity.as_slice()),
        ];
        let mut payloads = Vec::new();
        for (source, frame) in arrivals {
            if combiner
                .observe_frame(source, frame, FrameLayout::WithFcs)
                .should_forward()
            {
                for event in pipeline.push_80211_frame(frame).unwrap() {
                    if let PayloadPipelineEvent::Payload(payload) = event {
                        payloads.push(payload.data);
                    }
                }
            }
        }

        assert_eq!(payloads, [b"first".to_vec(), b"second".to_vec()]);
        let stats = combiner.stats();
        assert_eq!(stats.duplicates, 1);
        assert_eq!(stats.sources[&DiversitySourceId::new(1)].accepted, 1);
    }
}
