//! RTL8822B dynamic spur detection and NBI/CSI suppression.

use crate::device::RealtekDevice;
use crate::{ChannelWidth, ChipFamily, ChipInfo, CsiMaskSpec, DriverError, RadioConfig};

const SPUR_THRESHOLD: u32 = 0x8d;
const FREQ_2G: [u32; 14] = [
    0xfc67, 0xfc27, 0xffe6, 0xffa6, 0xfc67, 0xfce7, 0xfca7, 0xfc67, 0xfc27, 0xffe6, 0xffa6, 0xff66,
    0xff26, 0xfce7,
];
const FREQ_5G: [u32; 10] = [
    0xffc0, 0xffc0, 0xfc81, 0xfc81, 0xfc41, 0xfc40, 0xff80, 0xff80, 0xff40, 0xfd42,
];

impl RealtekDevice {
    pub(crate) async fn spur_calibration_8822b_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
        center_channel: u8,
    ) -> Result<(), DriverError> {
        if chip.family != ChipFamily::Rtl8822b {
            return Ok(());
        }

        // phydm_dsde_init: always clear stale notch and CSI-mask state before
        // deciding whether this channel needs a fresh calibration.
        self.set_bb_reg_async(0x087c, 1 << 13, 0).await?;
        self.set_bb_reg_async(0x0c20, 1 << 28, 0).await?;
        self.set_bb_reg_async(0x0e20, 1 << 28, 0).await?;
        for register in (0x0880..=0x089c).step_by(4) {
            self.write_u32_async(register, 0).await?;
        }
        self.set_bb_reg_async(0x0874, 1, 0).await?;

        let width = match radio.channel_width {
            ChannelWidth::Mhz20 => 0,
            ChannelWidth::Mhz40 => 1,
            ChannelWidth::Mhz80 => 2,
            ChannelWidth::Mhz5 | ChannelWidth::Mhz10 => return Ok(()),
        };
        let is_2g = radio.channel <= 14;
        let Some(index) = spur_index(radio.channel, width, is_2g) else {
            return Ok(());
        };
        let frequencies = if is_2g { &FREQ_2G[..] } else { &FREQ_5G[..] };
        let Some(base) = frequencies.get(index).copied() else {
            return Ok(());
        };

        let mut max = [0u32; 2];
        for offset in 0..3u32 {
            let frequency_point = base - 1 + offset;
            for _ in 0..3 {
                self.set_bb_reg_async(0x0c00, 0xff, 4).await?;
                self.set_bb_reg_async(0x0e00, 0xff, 4).await?;
                let saved_910 = (self.read_u32_async(0x0910).await? >> 12) & 0x0f;

                self.set_bb_reg_async(0x0808, 0xff, 0x11).await?;
                self.write_u32_async(0x0910, (1 << 22) | frequency_point)
                    .await?;
                crate::time::sleep_micros(500).await;
                max[0] = max[0].max(self.read_u32_async(0x0f44).await? & 0xffff);
                self.set_bb_reg_async(0x0910, 1 << 22, 0).await?;

                if chip.total_rf_paths() >= 2 {
                    self.set_bb_reg_async(0x0808, 0xff, 0x22).await?;
                    self.write_u32_async(0x0910, (1 << 22) | (1 << 16) | frequency_point)
                        .await?;
                    crate::time::sleep_micros(500).await;
                    max[1] = max[1].max(self.read_u32_async(0x0f44).await? & 0xffff);
                    self.set_bb_reg_async(0x0910, 1 << 22, 0).await?;
                }

                self.set_bb_reg_async(0x0c00, 0xff, 7).await?;
                self.set_bb_reg_async(0x0e00, 0xff, 7).await?;
                self.set_bb_reg_async(0x0910, 0xf000, saved_910).await?;
                let paths = if chip.total_rf_paths() >= 2 { 3 } else { 1 };
                self.set_bb_reg_async(0x0808, 0xff, paths | (paths << 4))
                    .await?;
                self.toggle_igi_8822b_async().await?;
            }
        }

        if max[0] < SPUR_THRESHOLD && max[1] < SPUR_THRESHOLD {
            log::info!(
                target: "openipc_rtl88xx::spur",
                "RTL8822B spur scan ch={} center={} width={:?} psd_a={} psd_b={} below threshold",
                radio.channel,
                center_channel,
                radio.channel_width,
                max[0],
                max[1]
            );
            return Ok(());
        }

        self.set_bb_reg_async(0x082c, 0x000f_f000, 0x97).await?;
        self.set_bb_reg_async(0x082c, 0x0000_f000, 7).await?;
        let Some(notch_mhz) = spur_frequency_mhz(radio.channel, width) else {
            return Ok(());
        };
        let notch_khz = notch_mhz * 1000;
        let _ = self.apply_nbi_notch_async(radio, notch_khz).await?;
        let spec = CsiMaskSpec::new(notch_khz - 600, notch_khz + 600, 7)
            .expect("fixed spur mask is valid");
        let tones = self.apply_csi_mask_async(radio, spec).await?;
        log::info!(
            target: "openipc_rtl88xx::spur",
            "RTL8822B spur scan ch={} center={} width={:?} psd_a={} psd_b={} notch={}MHz csi_tones={tones}",
            radio.channel,
            center_channel,
            radio.channel_width,
            max[0],
            max[1],
            notch_mhz
        );
        Ok(())
    }
}

fn spur_index(channel: u8, width: u8, is_2g: bool) -> Option<usize> {
    if is_2g {
        return match width {
            0 if (5..=8).contains(&channel) => Some(usize::from(channel - 5)),
            0 if channel == 13 => Some(4),
            1 if (3..=11).contains(&channel) => Some(usize::from(channel + 2)),
            _ => None,
        };
    }
    match channel {
        153 => Some(0),
        161 => Some(1),
        54 => Some(2),
        118 => Some(3),
        151 => Some(4),
        159 => Some(5),
        58 => Some(6),
        122 => Some(7),
        155 => Some(8),
        _ => None,
    }
}

fn spur_frequency_mhz(channel: u8, width: u8) -> Option<u32> {
    match (width, channel) {
        (0, 153) => Some(5760),
        (0, 161) => Some(5800),
        (0, 5..=8) => Some(2440),
        (0, 13) => Some(2480),
        (1, 54) => Some(5280),
        (1, 118) => Some(5600),
        (1, 151) => Some(5760),
        (1, 159) => Some(5800),
        (1, 4..=6) => Some(2440),
        (1, 11) => Some(2480),
        (2, 58) => Some(5280),
        (2, 122) => Some(5600),
        (2, 155) => Some(5760),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_spur_channels_select_expected_scan_slots() {
        assert_eq!(spur_index(5, 0, true), Some(0));
        assert_eq!(spur_index(13, 0, true), Some(4));
        assert_eq!(spur_index(161, 0, false), Some(1));
        assert_eq!(spur_index(44, 0, false), None);
    }

    #[test]
    fn vendor_spur_notch_frequency_matches_channel_and_width() {
        assert_eq!(spur_frequency_mhz(161, 0), Some(5800));
        assert_eq!(spur_frequency_mhz(122, 2), Some(5600));
        assert_eq!(spur_frequency_mhz(44, 0), None);
    }
}
