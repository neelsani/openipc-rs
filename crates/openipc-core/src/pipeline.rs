use crate::channel::ChannelId;
use crate::ieee80211::{FrameLayout, WifiFrame};
use crate::wfb::{
    parse_forwarder_packet, FecCounters, PlainAssembler, WfbError, WfbEvent, WfbKeypair, WfbPacket,
    WfbReceiver,
};

/// Payload recovered from one OpenIPC/WFB channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredPayload {
    /// WFB channel that produced this payload.
    pub channel_id: ChannelId,
    /// Recovered WFB packet sequence number.
    pub packet_seq: u64,
    /// Raw application payload bytes.
    pub data: Vec<u8>,
}

/// Event emitted by the lower-level single-channel payload pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadPipelineEvent {
    /// Frame was valid enough to inspect but did not match this pipeline.
    IgnoredFrame,
    /// A WFB session packet established or refreshed decryption/FEC state.
    SessionEstablished {
        /// Session epoch accepted from the transmitter.
        epoch: u64,
        /// Number of primary fragments in each FEC block.
        fec_k: usize,
        /// Total primary plus parity fragments in each FEC block.
        fec_n: usize,
    },
    /// A raw application payload was recovered.
    Payload(RecoveredPayload),
}

/// Single-channel OpenIPC/WFB recovery pipeline.
///
/// This type stops at recovered payload bytes. Use [`crate::ReceiverRuntime`]
/// when you also want route fanout and built-in RTP-to-video depacketization.
#[derive(Debug, Clone)]
pub struct PayloadPipeline {
    channel_id: ChannelId,
    frame_layout: FrameLayout,
    assembler: PlainAssembler,
    wfb_receiver: Option<WfbReceiver>,
}

impl PayloadPipeline {
    /// Create a pipeline for already-plain WFB fragments.
    pub fn new(
        channel_id: ChannelId,
        frame_layout: FrameLayout,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<Self, WfbError> {
        Ok(Self {
            channel_id,
            frame_layout,
            assembler: PlainAssembler::new(fec_k, fec_n)?,
            wfb_receiver: None,
        })
    }

    /// Create a pipeline that accepts encrypted WFB session and data packets.
    pub fn with_keypair(
        channel_id: ChannelId,
        frame_layout: FrameLayout,
        keypair: WfbKeypair,
        minimum_epoch: u64,
    ) -> Result<Self, WfbError> {
        Ok(Self {
            channel_id,
            frame_layout,
            assembler: PlainAssembler::new(1, 1)?,
            wfb_receiver: Some(WfbReceiver::new(channel_id, keypair, minimum_epoch)),
        })
    }

    /// Return this pipeline's channel id.
    pub const fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    /// Return cumulative FEC counters.
    pub fn fec_counters(&self) -> FecCounters {
        self.wfb_receiver
            .as_ref()
            .map(WfbReceiver::counters)
            .unwrap_or_else(|| self.assembler.counters())
    }

    /// Return true if an 802.11 frame belongs to this pipeline's channel.
    pub fn accepts_80211_frame(&self, frame: &[u8]) -> bool {
        WifiFrame::parse(frame, self.frame_layout)
            .map(|frame| frame.matches_channel_id(self.channel_id))
            .unwrap_or(false)
    }

    /// Process a raw WFB 802.11 frame and stop at recovered payload bytes.
    ///
    /// This is the protocol boundary shared by video, telemetry, and custom
    /// channels. It does not parse RTP, MAVLink, MSP, CRSF, IP, or any other
    /// application payload. Feed `RecoveredPayload::data` into the next-stage
    /// protocol handler chosen by the application.
    pub fn push_80211_frame(
        &mut self,
        frame: &[u8],
    ) -> Result<Vec<PayloadPipelineEvent>, WfbError> {
        let Ok(frame) = WifiFrame::parse(frame, self.frame_layout) else {
            return Ok(vec![PayloadPipelineEvent::IgnoredFrame]);
        };
        if !frame.matches_channel_id(self.channel_id) {
            return Ok(vec![PayloadPipelineEvent::IgnoredFrame]);
        }

        self.push_matched_payload(frame.payload())
    }

    /// Process the forwarder payload of an 802.11 frame already matched to this channel.
    pub(crate) fn push_matched_payload(
        &mut self,
        payload: &[u8],
    ) -> Result<Vec<PayloadPipelineEvent>, WfbError> {
        if let Some(receiver) = self.wfb_receiver.as_mut() {
            let events = receiver.push_forwarder_packet(payload)?;
            return Ok(self.map_wfb_events(events));
        }

        match parse_forwarder_packet(payload)? {
            WfbPacket::Data {
                data_nonce,
                encrypted_payload,
                ..
            } => self.push_decrypted_fragment(data_nonce, encrypted_payload),
            WfbPacket::SessionKey { .. } => Ok(Vec::new()),
        }
    }

    /// Process an 802.11 frame when the WFB data fragment is already decrypted.
    pub fn push_decrypted_80211_frame(
        &mut self,
        frame: &[u8],
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PayloadPipelineEvent>, WfbError> {
        let Ok(frame) = WifiFrame::parse(frame, self.frame_layout) else {
            return Ok(vec![PayloadPipelineEvent::IgnoredFrame]);
        };
        if !frame.matches_channel_id(self.channel_id) {
            return Ok(vec![PayloadPipelineEvent::IgnoredFrame]);
        }

        let packet = match parse_forwarder_packet(frame.payload())? {
            WfbPacket::Data { data_nonce, .. } => data_nonce,
            WfbPacket::SessionKey { .. } => return Ok(Vec::new()),
        };
        self.push_decrypted_fragment(packet, decrypted_fragment)
    }

    /// Push an already-decrypted WFB fragment into the plain assembler.
    pub fn push_decrypted_fragment(
        &mut self,
        data_nonce: u64,
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PayloadPipelineEvent>, WfbError> {
        let payloads = self
            .assembler
            .push_decrypted_fragment(data_nonce, decrypted_fragment)?;
        Ok(payloads
            .into_iter()
            .map(|payload| {
                PayloadPipelineEvent::Payload(RecoveredPayload {
                    channel_id: self.channel_id,
                    packet_seq: payload.packet_seq,
                    data: payload.payload,
                })
            })
            .collect())
    }

    fn map_wfb_events(&self, events: Vec<WfbEvent>) -> Vec<PayloadPipelineEvent> {
        events
            .into_iter()
            .map(|event| match event {
                WfbEvent::Session(session) => PayloadPipelineEvent::SessionEstablished {
                    epoch: session.epoch,
                    fec_k: session.fec_k,
                    fec_n: session.fec_n,
                },
                WfbEvent::Payload(payload) => PayloadPipelineEvent::Payload(RecoveredPayload {
                    channel_id: self.channel_id,
                    packet_seq: payload.packet_seq,
                    data: payload.payload,
                }),
            })
            .collect()
    }
}

/// Fully synthetic payload pipeline for tests and no-hardware development.
///
/// This type starts at the recovered-payload boundary. It does not parse
/// 802.11, decrypt WFB, or run FEC. Instead, callers inject payload bytes that
/// are emitted as [`PayloadPipelineEvent::Payload`] for the configured channel.
/// That lets higher layers exercise route fanout, RTP depacketization, audio
/// taps, metrics, and rendering without pretending to have a radio.
#[derive(Debug, Clone)]
pub struct MockPayloadPipeline {
    channel_id: ChannelId,
}

impl MockPayloadPipeline {
    /// Create a mock pipeline for one OpenIPC/WFB channel id.
    pub const fn new(channel_id: ChannelId) -> Self {
        Self { channel_id }
    }

    /// Return this mock pipeline's channel id.
    pub const fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    /// Mock channels do not run FEC, so counters are always zero.
    pub const fn fec_counters(&self) -> FecCounters {
        FecCounters {
            total_packets: 0,
            recovered_packets: 0,
            lost_packets: 0,
            bad_packets: 0,
        }
    }

    /// Emit one synthetic recovered payload.
    pub fn push_payload(&mut self, packet_seq: u64, data: &[u8]) -> Vec<PayloadPipelineEvent> {
        vec![PayloadPipelineEvent::Payload(RecoveredPayload {
            channel_id: self.channel_id,
            packet_seq,
            data: data.to_vec(),
        })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn payload_pipeline_emits_raw_recovered_payloads() {
        let mut pipeline =
            PayloadPipeline::new(ChannelId::default_video(), FrameLayout::WithFcs, 1, 1).unwrap();
        let events = pipeline
            .push_decrypted_fragment(0, &plain(b"telemetry bytes"))
            .unwrap();
        assert_eq!(
            events,
            vec![PayloadPipelineEvent::Payload(RecoveredPayload {
                channel_id: ChannelId::default_video(),
                packet_seq: 0,
                data: b"telemetry bytes".to_vec(),
            })]
        );
    }

    #[test]
    fn recovered_payloads_carry_the_pipeline_channel_id() {
        let channel_id = ChannelId::from_link_port(0x112233, crate::RadioPort::TunnelRx);
        let mut pipeline = PayloadPipeline::new(channel_id, FrameLayout::WithFcs, 1, 1).unwrap();
        let events = pipeline
            .push_decrypted_fragment(0, &plain(b"data bytes"))
            .unwrap();
        let PayloadPipelineEvent::Payload(payload) = &events[0] else {
            panic!("expected payload event");
        };
        assert_eq!(payload.channel_id, channel_id);
        assert_eq!(payload.data, b"data bytes");
    }

    #[test]
    fn mock_payload_pipeline_emits_recovered_payloads_without_wfb() {
        let channel_id = ChannelId::default_video();
        let mut pipeline = MockPayloadPipeline::new(channel_id);
        let events = pipeline.push_payload(42, b"mock rtp bytes");

        assert_eq!(
            events,
            vec![PayloadPipelineEvent::Payload(RecoveredPayload {
                channel_id,
                packet_seq: 42,
                data: b"mock rtp bytes".to_vec(),
            })]
        );
        assert_eq!(pipeline.fec_counters(), FecCounters::default());
    }
}
