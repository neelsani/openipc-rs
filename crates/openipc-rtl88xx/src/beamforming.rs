//! Explicit-sounding controls shared by the Realtek AC chip generations.
//!
//! These APIs expose the same unassociated NDPA/NDP self-sounding path used by
//! devourer. They only configure hardware; callers still build an NDPA frame
//! and set [`crate::RealtekTxOptions::beamforming_ndpa`] on transmission.

use crate::device::RealtekDevice;
use crate::regs::{BIT11, BIT19, BIT28, BIT29, BIT30, BIT8};
use crate::types::{ChipFamily, DriverError};

const TXBF_CTRL: u16 = 0x042c;
const RX_FILTER_MAP0: u16 = 0x06a0;
const RX_FILTER_MAP1: u16 = 0x06a2;
const BEAMFORMER_INFO0: u16 = 0x06e4;
const CSI_REPORT_PARAM_20: u16 = 0x06f4;
const CSI_REPORT_PARAM_40: u16 = 0x06f8;
const CSI_REPORT_PARAM_80: u16 = 0x06fc;
const BEAMFORMEE_SELECT: u16 = 0x0714;
const SOUNDING_PROTOCOL_CONTROL: u16 = 0x0718;
const NDP_STANDBY: u16 = 0x071b;
const SELF_MAC: u16 = 0x0610;
const OUR_AID: u16 = 0x1680;

/// Feedback format emitted by an armed beamformee.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BeamformingFeedback {
    /// VHT SU compressed beamforming report.
    #[default]
    Su,
    /// MU report with the exclusive per-tone delta-SNR payload.
    Mu,
}

/// Parsed summary of an over-the-air compressed beamforming report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeamformingReport {
    /// True for VHT category `0x15`; false for HT category `0x07`.
    pub vht: bool,
    /// Transmitter/source address from the management frame.
    pub source: [u8; 6],
    /// Number of columns represented by the MIMO control field.
    pub columns: u8,
    /// Number of rows represented by the MIMO control field.
    pub rows: u8,
    /// Encoded channel-width field from the MIMO control header.
    pub channel_width: u8,
    /// Encoded subcarrier-grouping field from the MIMO control header.
    pub grouping: u8,
}

/// Identify and summarize an HT/VHT compressed beamforming report frame.
///
/// `frame` starts at the IEEE 802.11 frame-control field and may include an
/// FCS. The returned summary intentionally leaves the matrix payload opaque.
pub fn parse_beamforming_report(frame: &[u8]) -> Option<BeamformingReport> {
    if frame.len() < 29 {
        return None;
    }
    let subtype = frame[0] & 0xf0;
    if !matches!(subtype, 0xd0 | 0xe0) {
        return None;
    }
    let vht = frame[24] == 0x15 && frame[25] == 0;
    let ht = frame[24] == 0x07 && frame[25] == 0;
    if !vht && !ht {
        return None;
    }
    let control = &frame[26..29];
    Some(BeamformingReport {
        vht,
        source: frame[10..16].try_into().ok()?,
        columns: (control[0] & 0x07) + 1,
        rows: ((control[0] >> 3) & 0x07) + 1,
        channel_width: (control[0] >> 6) & 0x03,
        grouping: control[1] & 0x03,
    })
}

impl RealtekDevice {
    /// Arm the adapter as an explicit-sounding beamformer.
    ///
    /// `own_mac` must match the transmitter address in injected NDPA frames.
    /// Jaguar3 additionally receives the vendor RF/BB mode-table setup needed
    /// for its hardware-generated NDP.
    pub async fn arm_beamforming_sounder_async(
        &self,
        own_mac: Option<[u8; 6]>,
    ) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if let Some(mac) = own_mac {
            self.write_mac_address_async(SELF_MAC, mac).await?;
        }
        if chip.family.is_jaguar3() {
            self.configure_jaguar3_sounder_rf_async(chip.family).await?;
        }
        let protocol = if chip.family.is_jaguar1() { 0xcb } else { 0xdb };
        self.write_u8_async(SOUNDING_PROTOCOL_CONTROL, protocol)
            .await?;
        self.write_u8_async(NDP_STANDBY, 0x50).await?;
        self.write_u16_async(TXBF_CTRL, 0).await?;
        let enables = self.read_u8_async(TXBF_CTRL + 3).await? | 0xd0;
        self.write_u8_async(TXBF_CTRL + 3, enables).await?;
        let select = (self.read_u8_async(BEAMFORMEE_SELECT + 3).await? & 0x03) | 0x60;
        self.write_u8_async(BEAMFORMEE_SELECT + 3, select).await?;
        self.write_u16_async(BEAMFORMEE_SELECT, 0x0200).await
    }

    /// Arm the adapter to answer an unassociated NDPA/NDP exchange.
    ///
    /// `beamformer_mac` is matched against the NDPA transmitter address.
    /// Jaguar2/3 require an own address for the NDPA receiver-address match; if
    /// `own_mac` is omitted, the same deterministic lab addresses as devourer
    /// are used (`...:bb` for Jaguar2 and `...:ce` for Jaguar3).
    pub async fn arm_beamformee_async(
        &self,
        beamformer_mac: [u8; 6],
        own_mac: Option<[u8; 6]>,
        feedback: BeamformingFeedback,
    ) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if feedback == BeamformingFeedback::Mu && chip.family.is_jaguar1() {
            return Err(DriverError::UnsupportedBeamformingMode(chip.family));
        }
        let jaguar23 = !chip.family.is_jaguar1();
        let default_own_mac = if chip.family.is_jaguar2() {
            [0x00, 0xe0, 0x4c, 0x88, 0x22, 0xbb]
        } else {
            [0x00, 0xe0, 0x4c, 0x88, 0x22, 0xce]
        };
        if jaguar23 {
            self.write_mac_address_async(SELF_MAC, own_mac.unwrap_or(default_own_mac))
                .await?;
        } else if let Some(mac) = own_mac {
            self.write_mac_address_async(SELF_MAC, mac).await?;
        }

        self.write_u8_async(
            SOUNDING_PROTOCOL_CONTROL,
            if jaguar23 { 0xdb } else { 0xcb },
        )
        .await?;
        self.write_u8_async(NDP_STANDBY, 0x50).await?;
        self.write_mac_address_async(BEAMFORMER_INFO0, beamformer_mac)
            .await?;
        if jaguar23 {
            self.write_u16_async(OUR_AID, 0).await?;
            let filter0 = self.read_u8_async(RX_FILTER_MAP0 + 1).await? | 0x40;
            self.write_u8_async(RX_FILTER_MAP0 + 1, filter0).await?;
            let filter1 = self.read_u8_async(RX_FILTER_MAP1).await? | 0x30;
            self.write_u8_async(RX_FILTER_MAP1, filter1).await?;
            self.write_u16_async(CSI_REPORT_PARAM_20, 0x0109).await?;
        } else {
            for register in [
                CSI_REPORT_PARAM_20,
                CSI_REPORT_PARAM_40,
                CSI_REPORT_PARAM_80,
            ] {
                self.write_u32_async(register, 0x0108_0108).await?;
            }
            self.write_u32_async(0x09b4, 0x0108_1008).await?;
        }

        if feedback == BeamformingFeedback::Mu {
            self.arm_mu_beamformee_layer_async().await?;
        }
        Ok(())
    }

    async fn write_mac_address_async(&self, base: u16, mac: [u8; 6]) -> Result<(), DriverError> {
        for (offset, byte) in mac.into_iter().enumerate() {
            self.write_u8_async(base + offset as u16, byte).await?;
        }
        Ok(())
    }

    async fn arm_mu_beamformee_layer_async(&self) -> Result<(), DriverError> {
        let mut control = self.read_u32_async(0x14c0).await? & !0x0700;
        self.write_u32_async(0x14c0, control).await?;
        control = (control & !0x3f) | 1;
        self.write_u32_async(0x14c0, control).await?;
        self.write_u32_async(0x14c4, 0).await?;
        self.write_u32_async(0x14c8, 0x0011_1110).await?;
        self.write_u32_async(0x14cc, 0).await?;
        let entry = (self.read_u16_async(0x1684).await? & 0xfe00) | 0x0200;
        self.write_u16_async(0x1684, entry).await?;
        let txbf = self.read_u8_async(TXBF_CTRL + 3).await? | 0xd0;
        self.write_u8_async(TXBF_CTRL + 3, txbf).await?;
        self.write_u8_async(0x045d, 0x04).await?;
        let ndpa_option = self.read_u8_async(0x045f).await? & 0xfc;
        self.write_u8_async(0x045f, ndpa_option).await?;
        let sounding = self.read_u32_async(SOUNDING_PROTOCOL_CONTROL).await?;
        self.write_u32_async(
            SOUNDING_PROTOCOL_CONTROL,
            (sounding & 0xff00_00ff) | 0x0002_0200,
        )
        .await
    }

    async fn configure_jaguar3_sounder_rf_async(
        &self,
        family: ChipFamily,
    ) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x1c90, BIT8, 0).await?;
        let writes: &[(u16, u16, u32, u32)] = if family == ChipFamily::Rtl8822c {
            &[
                (0x3c00, 0xef, BIT19, 1),
                (0x3c00, 0x33, 0x0f, 3),
                (0x3c00, 0x3e, 0x03, 2),
                (0x3c00, 0x3f, 0x000f_ffff, 0x65aff),
                (0x3c00, 0xef, BIT19, 0),
                (0x4c00, 0xef, BIT19, 1),
                (0x4c00, 0x33, 0x0f, 3),
                (0x4c00, 0x3f, 0x000f_ffff, 0x996bf),
                (0x4c00, 0x33, 0x0f, 1),
                (0x4c00, 0x3f, 0x000f_ffff, 0x99230),
                (0x4c00, 0xef, BIT19, 0),
            ]
        } else {
            &[
                (0x3c00, 0xef, BIT19, 1),
                (0x3c00, 0x33, 0x0f, 3),
                (0x3c00, 0x3e, 0x0f, 4),
                (0x3c00, 0x3f, 0x000f_ffff, 0xc1aff),
                (0x3c00, 0xef, BIT19, 0),
                (0x4c00, 0xef, BIT19, 1),
                (0x4c00, 0x33, 0x0f, 3),
                (0x4c00, 0x3e, 0x0f, 1),
                (0x4c00, 0x3f, 0x000f_ffff, 0x306bf),
                (0x4c00, 0xef, BIT19, 0),
            ]
        };
        for &(base, register, mask, value) in writes {
            self.set_bb_reg_async(base + (register << 2), mask, value)
                .await?;
        }
        self.set_bb_reg_async(0x1c90, BIT8, 1).await?;
        self.set_bb_reg_async(0x1830, BIT29, 1).await?;
        self.set_bb_reg_async(0x4130, BIT29, 1).await?;
        self.set_bb_reg_async(0x1e24, BIT11, 1).await?;
        self.set_bb_reg_async(0x1e24, BIT28 | BIT29, 2).await?;
        self.set_bb_reg_async(0x1e24, BIT30, 1).await?;
        self.set_bb_reg_async(0x0820, 0x00ff, 0x33).await?;
        self.set_bb_reg_async(0x1e2c, 0xffff, 0x0404).await?;
        self.set_bb_reg_async(0x0820, 0xffff_0000, 0x33).await?;
        self.set_bb_reg_async(0x1e30, 0xffff, 0x0404).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_vht_action_no_ack_report() {
        let mut frame = [0u8; 40];
        frame[0] = 0xe0;
        frame[10..16].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        frame[24] = 0x15;
        frame[26] = 0b01_001_010;
        frame[27] = 3;
        let report = parse_beamforming_report(&frame).unwrap();
        assert!(report.vht);
        assert_eq!(report.source, [1, 2, 3, 4, 5, 6]);
        assert_eq!(report.columns, 3);
        assert_eq!(report.rows, 2);
        assert_eq!(report.channel_width, 1);
        assert_eq!(report.grouping, 3);
    }

    #[test]
    fn rejects_unrelated_action_frames() {
        let mut frame = [0u8; 40];
        frame[0] = 0xd0;
        frame[24] = 0x7f;
        assert_eq!(parse_beamforming_report(&frame), None);
    }
}
