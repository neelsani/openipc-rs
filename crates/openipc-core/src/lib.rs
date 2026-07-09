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
/// First-valid-copy packet diversity for multiple receive adapters.
pub mod diversity;
/// Reed-Solomon forward-error-correction helpers.
pub mod fec;
mod fec_simd;
/// Minimal 802.11 frame parsing and construction helpers.
pub mod ieee80211;
/// Synthetic RTP source for no-hardware development.
pub mod mock;
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
/// WiFi channel/frequency helpers and sweep-list parsing.
pub mod wifi;

pub use adaptive::{
    AdaptiveLink, AdaptiveLinkSender, LinkQuality, ADAPTIVE_LINK_GS_PORT, ADAPTIVE_LINK_VTX_PORT,
};
pub use channel::{ChannelId, RadioPort};
pub use diversity::{
    DiversityCombiner, DiversityDecision, DiversitySourceId, DiversitySourceStats, DiversityStats,
};
pub use fec::{FecCode, FecError};
pub use ieee80211::{FrameLayout, WifiFrame};
pub use mock::{MockRtpFrame, MockRtpPipeline};
pub use pipeline::{MockPayloadPipeline, PayloadPipeline, PayloadPipelineEvent, RecoveredPayload};
pub use radiotap::{
    build_stream_radiotap, build_stream_radiotap_on_channel, parse_radiotap_tx_channel,
    parse_radiotap_tx_metadata, parse_tx_mode_str, try_parse_tx_mode_str, ChannelBandwidth,
    RadiotapTxMetadata, TxMode, TxModeKind, TxRadioParams, FRAME_TYPE_DATA, FRAME_TYPE_RTS,
};
pub use realtek::{
    parse_rx_aggregate, parse_rx_aggregate_with_kind, parse_rx_aggregate_with_kind_diagnostics,
    RealtekRxPacket, RxAggregateDiagnostics, RxDescriptorKind, RxPacketAttrib,
};
pub use receiver::{
    ReceiverBatch, ReceiverBatchCounters, ReceiverBatchOptions, ReceiverRuntime, RoutePayload,
    RtpPayloadTap,
};
pub use routes::{
    PayloadChannelRuntime, PayloadRouteError, PayloadRouteEvent, PayloadRouteId,
    PayloadRouteManager, PayloadRuntimeKey,
};
pub use rtp::{
    Codec, CodecConfigState, DamagedFramePolicy, DepacketizedFrame, FrameDamage, RtpDepacketizer,
    RtpDepacketizerStatus, RtpHeader, RtpReorderBuffer, RtpReorderStatus,
};
pub use wfb::{
    FecCounters, PlainAssembler, WfbKeypair, WfbOutput, WfbPacket, WfbReceiver, WfbSession,
};
pub use wfb_tx::{WfbTransmitter, WfbTxKeypair};
pub use wifi::{channel_to_frequency, frequency_to_channel, parse_channel_sweep};
