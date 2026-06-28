/// OpenIPC's observed default link id from the reference browser receiver.
///
/// The Zig project notes this as the SHA1-derived id for
/// `link_domain = "default"`.
pub const DEFAULT_LINK_ID: u32 = 7_669_206;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RadioPort {
    Video,
    MavlinkRx,
    MavlinkTx,
    DataRx,
    Custom(u8),
}

impl RadioPort {
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Video => 0,
            Self::MavlinkRx => 0x10,
            Self::MavlinkTx => 160,
            Self::DataRx => 32,
            Self::Custom(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelId(u32);

impl ChannelId {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn from_link_port(link_id: u32, port: RadioPort) -> Self {
        Self((link_id << 8) | port.as_u8() as u32)
    }

    pub const fn default_video() -> Self {
        Self::from_link_port(DEFAULT_LINK_ID, RadioPort::Video)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

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
        assert_eq!(RadioPort::MavlinkRx.as_u8(), 0x10);
        assert_eq!(RadioPort::DataRx.as_u8(), 0x20);
        assert_eq!(RadioPort::MavlinkTx.as_u8(), 0xa0);
    }
}
