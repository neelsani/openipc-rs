use crate::channel::ChannelId;

/// Length of the 802.11 data header used by OpenIPC/WFB frames.
pub const IEEE80211_HEADER_LEN: usize = 24;
/// Length of the optional 802.11 frame check sequence.
pub const IEEE80211_FCS_LEN: usize = 4;
/// Marker bytes mirrored in the OpenIPC/WFB source and destination addresses.
pub const WFB_PREFIX: [u8; 2] = [0x57, 0x42];
/// Frame-control bytes for QoS data from station to distribution system.
pub const QOS_DATA_FROM_STA_TO_DS: [u8; 2] = [0x08, 0x01];
/// Offset of the channel-id bytes in the source-address mirror.
pub const SRC_MAC_CHANNEL_OFFSET: usize = 12;
/// Offset of the channel-id bytes in the destination-address mirror.
pub const DST_MAC_CHANNEL_OFFSET: usize = 18;
/// Offset of the 802.11 sequence-control bytes.
pub const FRAME_SEQUENCE_OFFSET: usize = 22;

/// Whether received 802.11 frames include their trailing FCS bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameLayout {
    /// Frames still include the 4-byte FCS.
    WithFcs,
    /// Frames have already had the FCS stripped.
    WithoutFcs,
}

/// Reason an 802.11 frame could not be interpreted as an OpenIPC/WFB frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// The buffer is too short for the selected [`FrameLayout`].
    TooShort,
    /// The frame-control bytes are not the expected QoS data shape.
    NotDataFrame,
    /// The mirrored OpenIPC address/channel fields are malformed.
    InvalidWfbAddressMirror,
    /// No payload remains after the 802.11 header and optional FCS.
    MissingPayload,
}

/// Borrowed view of an OpenIPC/WFB 802.11 data frame.
#[derive(Debug, Clone, Copy)]
pub struct WifiFrame<'a> {
    data: &'a [u8],
    layout: FrameLayout,
}

impl<'a> WifiFrame<'a> {
    /// Parse and validate a borrowed 802.11 frame.
    pub fn parse(data: &'a [u8], layout: FrameLayout) -> Result<Self, FrameError> {
        let min_len = match layout {
            FrameLayout::WithFcs => IEEE80211_HEADER_LEN + IEEE80211_FCS_LEN + 1,
            FrameLayout::WithoutFcs => IEEE80211_HEADER_LEN + 1,
        };
        if data.len() < min_len {
            return Err(FrameError::TooShort);
        }

        let frame = Self { data, layout };
        if !frame.is_qos_data_from_sta_to_ds() {
            return Err(FrameError::NotDataFrame);
        }
        if !frame.has_mirrored_air_id_and_radio_port() {
            return Err(FrameError::InvalidWfbAddressMirror);
        }
        if frame.payload().is_empty() {
            return Err(FrameError::MissingPayload);
        }
        Ok(frame)
    }

    /// Return the original frame bytes.
    pub const fn raw(&self) -> &'a [u8] {
        self.data
    }

    /// Return the frame layout used during parsing.
    pub const fn layout(&self) -> FrameLayout {
        self.layout
    }

    /// Return the WFB forwarder payload without the 802.11 header or FCS.
    pub fn payload(&self) -> &'a [u8] {
        match self.layout {
            FrameLayout::WithFcs => {
                &self.data[IEEE80211_HEADER_LEN..self.data.len() - IEEE80211_FCS_LEN]
            }
            FrameLayout::WithoutFcs => &self.data[IEEE80211_HEADER_LEN..],
        }
    }

    /// Build the 8-byte nonce input mirrored from the WFB address fields.
    pub fn nonce(&self) -> [u8; 8] {
        let mut nonce = [0; 8];
        nonce[0..4].copy_from_slice(&self.data[11..15]);
        nonce[4..8].copy_from_slice(&self.data[17..21]);
        nonce
    }

    /// Return true when both mirrored address fields match `channel_id`.
    pub fn matches_channel_id(&self, channel_id: ChannelId) -> bool {
        let id = channel_id.to_be_bytes();
        self.data[10..12] == WFB_PREFIX
            && self.data[12..16] == id
            && self.data[16..18] == WFB_PREFIX
            && self.data[18..22] == id
    }

    /// Extract the mirrored channel id when the WFB address fields agree.
    pub fn channel_id(&self) -> Option<ChannelId> {
        if self.data[10..12] != WFB_PREFIX || self.data[16..18] != WFB_PREFIX {
            return None;
        }
        if self.data[12..16] != self.data[18..22] {
            return None;
        }
        let mut bytes = [0; 4];
        bytes.copy_from_slice(&self.data[12..16]);
        Some(ChannelId::new(u32::from_be_bytes(bytes)))
    }

    fn is_qos_data_from_sta_to_ds(&self) -> bool {
        self.data[0..2] == QOS_DATA_FROM_STA_TO_DS
    }

    fn has_mirrored_air_id_and_radio_port(&self) -> bool {
        self.data[10] == self.data[16] && self.data[15] == self.data[21]
    }
}

/// Build the standard OpenIPC/WFB QoS data 802.11 header.
pub fn build_wfb_header(
    channel_id: ChannelId,
    sequence_control: [u8; 2],
) -> [u8; IEEE80211_HEADER_LEN] {
    build_wfb_header_with_frame_type(channel_id, sequence_control, QOS_DATA_FROM_STA_TO_DS[0])
}

/// Build an OpenIPC/WFB 802.11 header with an explicit first frame-control byte.
pub fn build_wfb_header_with_frame_type(
    channel_id: ChannelId,
    sequence_control: [u8; 2],
    frame_type: u8,
) -> [u8; IEEE80211_HEADER_LEN] {
    let id = channel_id.to_be_bytes();
    [
        frame_type,
        0x01,
        0x00,
        0x00,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0x57,
        0x42,
        id[0],
        id[1],
        id[2],
        id[3],
        0x57,
        0x42,
        id[0],
        id[1],
        id[2],
        id[3],
        sequence_control[0],
        sequence_control[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reference_wfb_frame_with_fcs() {
        let channel = ChannelId::default_video();
        let mut bytes = Vec::from(build_wfb_header(channel, [0x10, 0x00]));
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);

        let frame = WifiFrame::parse(&bytes, FrameLayout::WithFcs).unwrap();
        assert!(frame.matches_channel_id(channel));
        assert_eq!(frame.channel_id(), Some(channel));
        assert_eq!(frame.payload(), &[1, 2, 3, 4]);
        assert_eq!(
            frame.nonce(),
            [
                0x42,
                channel.to_be_bytes()[0],
                channel.to_be_bytes()[1],
                channel.to_be_bytes()[2],
                0x42,
                channel.to_be_bytes()[0],
                channel.to_be_bytes()[1],
                channel.to_be_bytes()[2]
            ]
        );
    }

    #[test]
    fn rejects_short_frames() {
        assert_eq!(
            WifiFrame::parse(&[0x08, 0x01], FrameLayout::WithFcs).unwrap_err(),
            FrameError::TooShort
        );
    }
}
