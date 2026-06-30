//! Shared OpenIPC FPV receiver logic.
//!
//! Native and WebAssembly frontends feed bytes into these parsers and keep their
//! platform-specific device code at the edge.

/// Adaptive-link quality estimation and WFB feedback TX helpers.
pub mod adaptive;
/// OpenIPC/WFB link and channel identifiers.
pub mod channel;
/// Legacy WFB ChaCha20-Poly1305 compatibility helpers.
pub mod crypto;
/// Reed-Solomon forward-error-correction helpers.
pub mod fec;
/// Minimal 802.11 frame parsing and construction helpers.
pub mod ieee80211;
/// Single-channel WFB payload recovery pipeline.
pub mod pipeline;
/// Radiotap TX metadata builders and parsers.
pub mod radiotap;
/// Realtek USB RX aggregate parsing.
pub mod realtek;
/// Higher-level receive runtime for video and payload routes.
pub mod receiver;
/// Multi-route raw payload fanout manager.
pub mod routes;
/// RTP parsing and H.264/H.265 depacketization.
pub mod rtp;
/// WFB packet, session, crypto, and FEC assembly logic.
pub mod wfb;
/// WFB uplink packet transmitter.
pub mod wfb_tx;

pub use adaptive::{AdaptiveLink, AdaptiveLinkSender, LinkQuality};
pub use channel::{ChannelId, RadioPort};
pub use fec::{FecCode, FecError};
pub use ieee80211::{FrameLayout, WifiFrame};
pub use pipeline::{PayloadPipeline, PayloadPipelineEvent, RecoveredPayload};
pub use radiotap::{
    build_stream_radiotap, parse_tx_mode_str, ChannelBandwidth, TxMode, TxModeKind, TxRadioParams,
    FRAME_TYPE_DATA, FRAME_TYPE_RTS,
};
pub use realtek::{parse_rx_aggregate, RealtekRxPacket, RxPacketAttrib};
pub use receiver::{
    ReceiverBatch, ReceiverBatchCounters, ReceiverBatchOptions, ReceiverRuntime, RoutePayload,
    RtpPayloadTap,
};
pub use routes::{
    PayloadRouteError, PayloadRouteEvent, PayloadRouteId, PayloadRouteManager, PayloadRuntimeKey,
};
pub use rtp::{Codec, DepacketizedFrame, RtpDepacketizer, RtpHeader};
pub use wfb::{
    FecCounters, PlainAssembler, WfbKeypair, WfbOutput, WfbPacket, WfbReceiver, WfbSession,
};
pub use wfb_tx::{WfbTransmitter, WfbTxKeypair};
