/// OpenIPC's observed default link id from the reference browser receiver.
///
/// The Zig project notes this as the SHA1-derived id for
/// `link_domain = "default"`.
pub const DEFAULT_LINK_ID: u32 = 7_669_206;

/// Low-byte port selector inside an OpenIPC/WFB channel id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RadioPort {
    /// Air-unit to ground-station video RTP downlink.
    Video,
    /// Air-unit to ground-station telemetry downlink.
    ///
    /// OpenIPC commonly carries MAVLink or MSP/OSD-style telemetry here,
    /// depending on the transmitter-side telemetry router.
    TelemetryRx,
    /// Ground-station to air-unit telemetry uplink.
    TelemetryTx,
    /// Air-unit to ground-station tunnel/data downlink.
    TunnelRx,
    /// Ground-station to air-unit tunnel/data uplink.
    ///
    /// Adaptive-link feedback is sent over this path in aviateur, PixelPilot,
    /// and current OpenIPC firmware setups.
    TunnelTx,
    /// Air-unit to ground-station audio profile downlink.
    AudioRx,
    /// Ground-station to air-unit audio profile uplink.
    AudioTx,
    /// Legacy alias for [`RadioPort::TelemetryRx`].
    #[deprecated(note = "use RadioPort::TelemetryRx; port 0x10 is telemetry, not always MAVLink")]
    MavlinkRx,
    /// Legacy alias for [`RadioPort::TunnelTx`].
    ///
    /// Earlier openipc-rs builds used this for adaptive-link feedback. The
    /// wire value remains `0xa0`, but the accurate OpenIPC name is tunnel/data
    /// uplink. Use [`RadioPort::TelemetryTx`] when you mean telemetry port
    /// `0x90`.
    #[deprecated(
        note = "use RadioPort::TunnelTx for adaptive-link or RadioPort::TelemetryTx for telemetry uplink"
    )]
    MavlinkTx,
    /// Legacy alias for [`RadioPort::TunnelRx`].
    #[deprecated(note = "use RadioPort::TunnelRx")]
    DataRx,
    /// Caller-defined radio port for custom payload channels.
    Custom(u8),
}

impl RadioPort {
    /// Return the low byte used in an OpenIPC/WFB channel id.
    #[allow(deprecated)]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Video => 0,
            Self::TelemetryRx | Self::MavlinkRx => 0x10,
            Self::TelemetryTx => 0x90,
            Self::TunnelRx | Self::DataRx => 0x20,
            Self::TunnelTx | Self::MavlinkTx => 0xa0,
            Self::AudioRx => 0x30,
            Self::AudioTx => 0xb0,
            Self::Custom(value) => value,
        }
    }
}

/// OpenIPC/WFB logical channel id.
///
/// The high 24 bits are the link id and the low byte is the radio port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelId(u32);

impl ChannelId {
    /// Wrap a raw 32-bit channel id.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Build a channel id from a link id and radio port.
    pub const fn from_link_port(link_id: u32, port: RadioPort) -> Self {
        Self((link_id << 8) | port.as_u8() as u32)
    }

    /// Return the default OpenIPC video channel.
    pub const fn default_video() -> Self {
        Self::from_link_port(DEFAULT_LINK_ID, RadioPort::Video)
    }

    /// Return the raw big-endian channel id value.
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Return the raw channel id encoded for 802.11 address fields.
    pub const fn to_be_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_video_channel_matches_reference_value() {
        let id = ChannelId::default_video();
        assert_eq!(id.raw(), (DEFAULT_LINK_ID << 8));
        assert_eq!(id.to_be_bytes(), id.raw().to_be_bytes());
    }

    #[test]
    fn radio_ports_match_openipc_ground_station_conventions() {
        assert_eq!(RadioPort::Video.as_u8(), 0x00);
        assert_eq!(RadioPort::TelemetryRx.as_u8(), 0x10);
        assert_eq!(RadioPort::TunnelRx.as_u8(), 0x20);
        assert_eq!(RadioPort::AudioRx.as_u8(), 0x30);
        assert_eq!(RadioPort::TelemetryTx.as_u8(), 0x90);
        assert_eq!(RadioPort::TunnelTx.as_u8(), 0xa0);
        assert_eq!(RadioPort::AudioTx.as_u8(), 0xb0);
    }
}
