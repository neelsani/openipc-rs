use crate::channel::ChannelId;
use crate::ieee80211::{FrameLayout, WifiFrame};
use crate::rtp::{DepacketizedFrame, RtpDepacketizer};
use crate::wfb::{
    parse_forwarder_packet, FecCounters, PlainAssembler, WfbError, WfbEvent, WfbKeypair, WfbPacket,
    WfbReceiver,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredPayload {
    pub packet_seq: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadPipelineEvent {
    IgnoredFrame,
    SessionEstablished {
        epoch: u64,
        fec_k: usize,
        fec_n: usize,
    },
    Payload(RecoveredPayload),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineEvent {
    IgnoredFrame,
    SessionEstablished {
        epoch: u64,
        fec_k: usize,
        fec_n: usize,
    },
    WfbPayload {
        packet_seq: u64,
        len: usize,
    },
    RtpPacket {
        packet_seq: u64,
        payload: Vec<u8>,
    },
    VideoFrame(DepacketizedFrame),
}

#[derive(Debug, Clone)]
pub struct ReceiverPipeline {
    payload_pipeline: PayloadPipeline,
    depacketizer: RtpDepacketizer,
}

#[derive(Debug, Clone)]
pub struct PayloadPipeline {
    channel_id: ChannelId,
    frame_layout: FrameLayout,
    assembler: PlainAssembler,
    wfb_receiver: Option<WfbReceiver>,
}

impl PayloadPipeline {
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

    pub const fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    pub fn fec_counters(&self) -> FecCounters {
        self.wfb_receiver
            .as_ref()
            .map(WfbReceiver::counters)
            .unwrap_or_else(|| self.assembler.counters())
    }

    pub fn accepts_80211_frame(&self, frame: &[u8]) -> bool {
        WifiFrame::parse(frame, self.frame_layout)
            .map(|frame| frame.matches_channel_id(self.channel_id))
            .unwrap_or(false)
    }

    /// Process a raw WFB 802.11 frame and stop at recovered payload bytes.
    ///
    /// This is the right boundary for non-video WFB channels.
    ///
    /// Callers receive decrypted/FEC-recovered payloads without RTP, codec, or
    /// telemetry-format assumptions. Use it for MAVLink, data, custom payload
    /// ports, or any other application protocol carried over WFB.
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

        if let Some(receiver) = self.wfb_receiver.as_mut() {
            let events = receiver.push_forwarder_packet(frame.payload())?;
            return Ok(Self::map_wfb_events(events));
        }

        match parse_forwarder_packet(frame.payload())? {
            WfbPacket::Data {
                data_nonce,
                encrypted_payload,
                ..
            } => self.push_decrypted_fragment(data_nonce, encrypted_payload),
            WfbPacket::SessionKey { .. } => Ok(Vec::new()),
        }
    }

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
                    packet_seq: payload.packet_seq,
                    data: payload.payload,
                })
            })
            .collect())
    }

    fn map_wfb_events(events: Vec<WfbEvent>) -> Vec<PayloadPipelineEvent> {
        events
            .into_iter()
            .map(|event| match event {
                WfbEvent::Session(session) => PayloadPipelineEvent::SessionEstablished {
                    epoch: session.epoch,
                    fec_k: session.fec_k,
                    fec_n: session.fec_n,
                },
                WfbEvent::Payload(payload) => PayloadPipelineEvent::Payload(RecoveredPayload {
                    packet_seq: payload.packet_seq,
                    data: payload.payload,
                }),
            })
            .collect()
    }
}

impl ReceiverPipeline {
    pub fn new(
        channel_id: ChannelId,
        frame_layout: FrameLayout,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<Self, WfbError> {
        Ok(Self {
            payload_pipeline: PayloadPipeline::new(channel_id, frame_layout, fec_k, fec_n)?,
            depacketizer: RtpDepacketizer::new(),
        })
    }

    pub fn with_keypair(
        channel_id: ChannelId,
        frame_layout: FrameLayout,
        keypair: WfbKeypair,
        minimum_epoch: u64,
    ) -> Result<Self, WfbError> {
        Ok(Self {
            payload_pipeline: PayloadPipeline::with_keypair(
                channel_id,
                frame_layout,
                keypair,
                minimum_epoch,
            )?,
            depacketizer: RtpDepacketizer::new(),
        })
    }

    pub const fn channel_id(&self) -> ChannelId {
        self.payload_pipeline.channel_id()
    }

    pub fn fec_counters(&self) -> FecCounters {
        self.payload_pipeline.fec_counters()
    }

    pub fn accepts_80211_frame(&self, frame: &[u8]) -> bool {
        self.payload_pipeline.accepts_80211_frame(frame)
    }

    /// Process a raw WFB 802.11 frame. This path handles session-key packets,
    /// encrypted WFB data packets, FEC recovery, RTP depacketization, and Annex-B
    /// video frame output.
    pub fn push_80211_frame(&mut self, frame: &[u8]) -> Result<Vec<PipelineEvent>, WfbError> {
        let events = self.payload_pipeline.push_80211_frame(frame)?;
        self.map_payload_events(events)
    }

    /// Process a WFB frame whose WFB data packet payload has already been
    /// decrypted by the platform adapter.
    pub fn push_decrypted_80211_frame(
        &mut self,
        frame: &[u8],
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PipelineEvent>, WfbError> {
        let events = self
            .payload_pipeline
            .push_decrypted_80211_frame(frame, decrypted_fragment)?;
        self.map_payload_events(events)
    }

    pub fn push_decrypted_fragment(
        &mut self,
        data_nonce: u64,
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PipelineEvent>, WfbError> {
        let events = self
            .payload_pipeline
            .push_decrypted_fragment(data_nonce, decrypted_fragment)?;
        self.map_payload_events(events)
    }

    pub fn push_rtp(&mut self, rtp_packet: &[u8]) -> Option<DepacketizedFrame> {
        self.depacketizer.push(rtp_packet).ok().flatten()
    }

    fn map_payload_events(
        &mut self,
        events: Vec<PayloadPipelineEvent>,
    ) -> Result<Vec<PipelineEvent>, WfbError> {
        let mut out = Vec::new();
        for event in events {
            match event {
                PayloadPipelineEvent::IgnoredFrame => out.push(PipelineEvent::IgnoredFrame),
                PayloadPipelineEvent::SessionEstablished {
                    epoch,
                    fec_k,
                    fec_n,
                } => out.push(PipelineEvent::SessionEstablished {
                    epoch,
                    fec_k,
                    fec_n,
                }),
                PayloadPipelineEvent::Payload(payload) => {
                    out.push(PipelineEvent::WfbPayload {
                        packet_seq: payload.packet_seq,
                        len: payload.data.len(),
                    });
                    out.push(PipelineEvent::RtpPacket {
                        packet_seq: payload.packet_seq,
                        payload: payload.data.clone(),
                    });
                    if let Ok(Some(frame)) = self.depacketizer.push(&payload.data) {
                        out.push(PipelineEvent::VideoFrame(frame));
                    }
                }
            }
        }
        Ok(out)
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
            .push_decrypted_fragment(0, &plain(b"mavlink bytes"))
            .unwrap();
        assert_eq!(
            events,
            vec![PayloadPipelineEvent::Payload(RecoveredPayload {
                packet_seq: 0,
                data: b"mavlink bytes".to_vec(),
            })]
        );
    }

    #[test]
    fn receiver_pipeline_still_emits_rtp_payload_event_from_raw_payload() {
        let mut pipeline =
            ReceiverPipeline::new(ChannelId::default_video(), FrameLayout::WithFcs, 1, 1).unwrap();
        let events = pipeline
            .push_decrypted_fragment(0, &plain(b"rtp bytes"))
            .unwrap();
        assert!(events.contains(&PipelineEvent::WfbPayload {
            packet_seq: 0,
            len: 9,
        }));
        assert!(events.contains(&PipelineEvent::RtpPacket {
            packet_seq: 0,
            payload: b"rtp bytes".to_vec(),
        }));
    }
}
