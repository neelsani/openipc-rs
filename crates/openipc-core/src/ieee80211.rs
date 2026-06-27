use crate::channel::ChannelId;

pub const IEEE80211_HEADER_LEN: usize = 24;
pub const IEEE80211_FCS_LEN: usize = 4;
pub const WFB_PREFIX: [u8; 2] = [0x57, 0x42];
pub const QOS_DATA_FROM_STA_TO_DS: [u8; 2] = [0x08, 0x01];
pub const SRC_MAC_CHANNEL_OFFSET: usize = 12;
pub const DST_MAC_CHANNEL_OFFSET: usize = 18;
pub const FRAME_SEQUENCE_OFFSET: usize = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameLayout {
    WithFcs,
    WithoutFcs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    TooShort,
    NotDataFrame,
    InvalidWfbAddressMirror,
    MissingPayload,
}

#[derive(Debug, Clone, Copy)]
pub struct WifiFrame<'a> {
    data: &'a [u8],
    layout: FrameLayout,
}

impl<'a> WifiFrame<'a> {
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

    pub const fn raw(&self) -> &'a [u8] {
        self.data
    }

    pub const fn layout(&self) -> FrameLayout {
        self.layout
    }

    pub fn payload(&self) -> &'a [u8] {
        match self.layout {
            FrameLayout::WithFcs => {
                &self.data[IEEE80211_HEADER_LEN..self.data.len() - IEEE80211_FCS_LEN]
            }
            FrameLayout::WithoutFcs => &self.data[IEEE80211_HEADER_LEN..],
        }
    }

    pub fn nonce(&self) -> [u8; 8] {
        let mut nonce = [0; 8];
        nonce[0..4].copy_from_slice(&self.data[11..15]);
        nonce[4..8].copy_from_slice(&self.data[17..21]);
        nonce
    }

    pub fn matches_channel_id(&self, channel_id: ChannelId) -> bool {
        let id = channel_id.to_be_bytes();
        self.data[10..12] == WFB_PREFIX
            && self.data[12..16] == id
            && self.data[16..18] == WFB_PREFIX
            && self.data[18..22] == id
    }

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

pub fn build_wfb_header(
    channel_id: ChannelId,
    sequence_control: [u8; 2],
) -> [u8; IEEE80211_HEADER_LEN] {
    build_wfb_header_with_frame_type(channel_id, sequence_control, QOS_DATA_FROM_STA_TO_DS[0])
}

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
