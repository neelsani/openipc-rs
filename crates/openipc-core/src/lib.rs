//! Shared OpenIPC FPV receiver logic.
//!
//! Native and WebAssembly frontends feed bytes into these parsers and keep their
//! platform-specific device code at the edge.

pub mod adaptive;
pub mod channel;
pub mod crypto;
pub mod fec;
pub mod ieee80211;
pub mod pipeline;
pub mod radiotap;
pub mod realtek;
pub mod realtek_tx;
pub mod rtp;
pub mod wfb;
pub mod wfb_tx;

pub use adaptive::{AdaptiveLink, AdaptiveLinkSender, LinkQuality};
pub use channel::{ChannelId, RadioPort};
pub use fec::{FecCode, FecError};
pub use ieee80211::{FrameLayout, WifiFrame};
pub use pipeline::{
    PayloadPipeline, PayloadPipelineEvent, PipelineEvent, ReceiverPipeline, RecoveredPayload,
};
pub use radiotap::{
    build_stream_radiotap, parse_tx_mode_str, ChannelBandwidth, TxMode, TxModeKind, TxRadioParams,
    FRAME_TYPE_DATA, FRAME_TYPE_RTS,
};
pub use realtek::{parse_rx_aggregate, RealtekRxPacket, RxPacketAttrib};
pub use realtek_tx::{build_usb_tx_frame, RealtekTxOptions};
pub use rtp::{Codec, DepacketizedFrame, RtpDepacketizer, RtpHeader};
pub use wfb::{
    FecCounters, PlainAssembler, WfbKeypair, WfbOutput, WfbPacket, WfbReceiver, WfbSession,
};
pub use wfb_tx::{WfbTransmitter, WfbTxKeypair};
