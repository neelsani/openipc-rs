use crate::channel::ChannelId;
use crate::ieee80211::{FrameLayout, WifiFrame};
use crate::rtp::{DepacketizedFrame, RtpDepacketizer};
use crate::wfb::{
    parse_forwarder_packet, FecCounters, PlainAssembler, WfbError, WfbEvent, WfbKeypair, WfbPacket,
    WfbReceiver,
};

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
    channel_id: ChannelId,
    frame_layout: FrameLayout,
    assembler: PlainAssembler,
    wfb_receiver: Option<WfbReceiver>,
    depacketizer: RtpDepacketizer,
}

impl ReceiverPipeline {
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
            channel_id,
            frame_layout,
            assembler: PlainAssembler::new(1, 1)?,
            wfb_receiver: Some(WfbReceiver::new(channel_id, keypair, minimum_epoch)),
            depacketizer: RtpDepacketizer::new(),
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

    /// Process a raw WFB 802.11 frame. This path handles session-key packets,
    /// encrypted WFB data packets, FEC recovery, RTP depacketization, and Annex-B
    /// video frame output.
    pub fn push_80211_frame(&mut self, frame: &[u8]) -> Result<Vec<PipelineEvent>, WfbError> {
        let Ok(frame) = WifiFrame::parse(frame, self.frame_layout) else {
            return Ok(vec![PipelineEvent::IgnoredFrame]);
        };
        if !frame.matches_channel_id(self.channel_id) {
            return Ok(vec![PipelineEvent::IgnoredFrame]);
        }

        let receiver = self.wfb_receiver.as_mut().ok_or(WfbError::MissingSession)?;
        let events = receiver.push_forwarder_packet(frame.payload())?;
        self.map_wfb_events(events)
    }

    /// Process a WFB frame whose WFB data packet payload has already been
    /// decrypted by the platform adapter.
    pub fn push_decrypted_80211_frame(
        &mut self,
        frame: &[u8],
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PipelineEvent>, WfbError> {
        let Ok(frame) = WifiFrame::parse(frame, self.frame_layout) else {
            return Ok(vec![PipelineEvent::IgnoredFrame]);
        };
        if !frame.matches_channel_id(self.channel_id) {
            return Ok(vec![PipelineEvent::IgnoredFrame]);
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
    ) -> Result<Vec<PipelineEvent>, WfbError> {
        let mut events = Vec::new();
        for payload in self
            .assembler
            .push_decrypted_fragment(data_nonce, decrypted_fragment)?
        {
            events.push(PipelineEvent::WfbPayload {
                packet_seq: payload.packet_seq,
                len: payload.payload.len(),
            });
            events.push(PipelineEvent::RtpPacket {
                packet_seq: payload.packet_seq,
                payload: payload.payload.clone(),
            });
            if let Ok(Some(frame)) = self.depacketizer.push(&payload.payload) {
                events.push(PipelineEvent::VideoFrame(frame));
            }
        }
        Ok(events)
    }

    pub fn push_rtp(&mut self, rtp_packet: &[u8]) -> Option<DepacketizedFrame> {
        self.depacketizer.push(rtp_packet).ok().flatten()
    }

    fn map_wfb_events(&mut self, events: Vec<WfbEvent>) -> Result<Vec<PipelineEvent>, WfbError> {
        let mut out = Vec::new();
        for event in events {
            match event {
                WfbEvent::Session(session) => out.push(PipelineEvent::SessionEstablished {
                    epoch: session.epoch,
                    fec_k: session.fec_k,
                    fec_n: session.fec_n,
                }),
                WfbEvent::Payload(payload) => {
                    out.push(PipelineEvent::WfbPayload {
                        packet_seq: payload.packet_seq,
                        len: payload.payload.len(),
                    });
                    out.push(PipelineEvent::RtpPacket {
                        packet_seq: payload.packet_seq,
                        payload: payload.payload.clone(),
                    });
                    if let Ok(Some(frame)) = self.depacketizer.push(&payload.payload) {
                        out.push(PipelineEvent::VideoFrame(frame));
                    }
                }
            }
        }
        Ok(out)
    }
}
