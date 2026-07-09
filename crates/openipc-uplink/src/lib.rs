//! Userspace networking and backward-compatible VTX control over WFB.

mod config;
mod control;
mod engine;
mod framing;
mod legacy_control;
mod network;
mod ssh;
mod stream;

pub use config::{ConfigBundle, ConfigParseError, VtxConfigSnapshot};
pub use control::{
    AdaptiveLinkSetting, CameraSetting, TelemetrySetting, VtxController, VtxSettingError,
    WfbSetting,
};
pub use engine::{
    TxBatch, TxFailureKind, TxFrame, TxOutcome, UplinkEngine, UplinkEngineConfig,
    UplinkEngineError, UplinkEngineMetrics, UplinkTrafficClass,
};
pub use framing::{
    frame_ip_packet, parse_tunnel_packets, parse_tunnel_payload, TunnelFramingError, TunnelPackets,
    MAX_TUNNEL_PACKET_LEN,
};
pub use legacy_control::LegacyControlClient;
pub use network::{NetworkConfig, NetworkError, NetworkMetrics, UserspaceNetwork};
pub use ssh::{
    CommandOutput, HostKeyPolicy, SshClient, SshCredentials, SshError, DEFAULT_SSH_PASSWORD,
    DEFAULT_SSH_USERNAME,
};
pub use stream::VirtualTcpStream;
