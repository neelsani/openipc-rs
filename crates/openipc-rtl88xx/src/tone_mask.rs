//! RX-side CSI tone masking and narrow-band interference filtering.
//!
//! Realtek AC silicon cannot transmit a punctured VHT preamble. It can,
//! however, de-weight selected receive-equalizer tones or place one NBI notch
//! so a dirty slice contributes less to channel estimation.

use crate::device::RealtekDevice;
use crate::types::{ChannelWidth, ChipFamily, ChipInfo, DriverError, MonitorOptions, RadioConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToneMaskFamily {
    Ac1,
    Ac2_8814,
    Ac2_8822b,
    Jaguar3,
}

/// Frequency range to suppress in the receive CSI equalizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsiMaskSpec {
    /// Inclusive lower frequency in kHz.
    pub low_khz: u32,
    /// Inclusive upper frequency in kHz.
    pub high_khz: u32,
    /// Jaguar3 suppression weight (`0..=7`, where 7 is strongest).
    pub weight: u8,
}

impl CsiMaskSpec {
    /// Construct a validated kHz-range specification.
    pub const fn new(low_khz: u32, high_khz: u32, weight: u8) -> Option<Self> {
        if high_khz < low_khz || weight > 7 {
            None
        } else {
            Some(Self {
                low_khz,
                high_khz,
                weight,
            })
        }
    }

    /// Parse devourer's `<start>[-<end>][/weight]` MHz syntax.
    pub fn parse_mhz(value: &str) -> Option<Self> {
        let (range, weight) = match value.split_once('/') {
            Some((range, weight)) => (range, weight.parse::<u8>().ok()?),
            None => (value, 7),
        };
        if weight > 7 || range.is_empty() {
            return None;
        }
        let (low, high) = match range.split_once('-') {
            Some((low, high)) => (low.parse::<u32>().ok()?, high.parse::<u32>().ok()?),
            None => {
                let frequency = range.parse::<u32>().ok()?;
                (frequency, frequency)
            }
        };
        Self::new(low.checked_mul(1000)?, high.checked_mul(1000)?, weight)
    }
}

/// Derive the center frequency of the configured RF channel in MHz.
pub fn center_frequency_mhz(radio: RadioConfig) -> Option<u32> {
    let bandwidth = match radio.channel_width {
        ChannelWidth::Mhz40 => 40,
        ChannelWidth::Mhz80 => 80,
        ChannelWidth::Mhz5 | ChannelWidth::Mhz10 | ChannelWidth::Mhz20 => 20,
    };
    let secondary = match radio.channel_offset {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    find_center_frequency_mhz(u32::from(radio.channel), bandwidth, secondary)
}

/// Enumerate signed 312.5 kHz subcarrier indices covered by a mask range.
pub fn enumerate_mask_tones(
    center_mhz: u32,
    bandwidth_mhz: u32,
    low_khz: u32,
    high_khz: u32,
) -> Vec<i32> {
    if center_mhz == 0 || high_khz < low_khz {
        return Vec::new();
    }
    let center_khz = i64::from(center_mhz) * 1000;
    let edge = (bandwidth_mhz * 8 / 5) as i32;
    let low_twice = (i64::from(low_khz) - center_khz) * 2;
    let high_twice = (i64::from(high_khz) - center_khz) * 2;
    let div_ceil = |value: i64, divisor: i64| {
        if value >= 0 {
            (value + divisor - 1) / divisor
        } else {
            -((-value) / divisor)
        }
    };
    let div_floor = |value: i64, divisor: i64| {
        if value >= 0 {
            value / divisor
        } else {
            -((-value + divisor - 1) / divisor)
        }
    };
    let mut first = div_ceil(low_twice, 625) as i32;
    let mut last = div_floor(high_twice, 625) as i32;
    if first > last && low_khz == high_khz {
        let ten_units = (i64::from(low_khz) - center_khz) * 32 / 1000;
        let nearest = ((ten_units + if ten_units >= 0 { 5 } else { -5 }) / 10) as i32;
        first = nearest;
        last = nearest;
    }
    (first..=last)
        .filter(|tone| (-edge..=edge).contains(tone))
        .collect()
}

impl RealtekDevice {
    /// Apply a per-subcarrier receive CSI mask for the selected channel.
    pub async fn apply_csi_mask_async(
        &self,
        radio: RadioConfig,
        spec: CsiMaskSpec,
    ) -> Result<usize, DriverError> {
        let chip = self.probe_chip_async().await?;
        self.apply_csi_mask_for_chip_async(chip, radio, spec).await
    }

    /// Clear every receive CSI-mask entry and disable the mask.
    pub async fn clear_csi_mask_async(&self) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        match tone_mask_family(chip.family) {
            ToneMaskFamily::Jaguar3 => self.clear_csi_mask_jaguar3_async().await,
            _ => self.clear_csi_mask_11ac_async().await,
        }
    }

    /// Enable one NBI notch at an absolute frequency in kHz.
    ///
    /// Returns `false` when the requested frequency is outside the configured
    /// RF channel and leaves the filter disabled.
    pub async fn apply_nbi_notch_async(
        &self,
        radio: RadioConfig,
        frequency_khz: u32,
    ) -> Result<bool, DriverError> {
        let chip = self.probe_chip_async().await?;
        self.apply_nbi_for_chip_async(chip, radio, frequency_khz)
            .await
    }

    /// Disable the generation-specific NBI filter.
    pub async fn disable_nbi_notch_async(&self) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        match tone_mask_family(chip.family) {
            ToneMaskFamily::Jaguar3 => self.disable_nbi_jaguar3_async().await,
            family => {
                self.disable_nbi_11ac_async(family, chip.total_rf_paths() >= 2)
                    .await
            }
        }
    }

    pub(crate) async fn apply_interference_mitigation_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
        options: MonitorOptions,
    ) -> Result<(), DriverError> {
        if let Some(spec) = options.csi_mask {
            let count = self
                .apply_csi_mask_for_chip_async(chip, radio, spec)
                .await?;
            log::info!(target: "openipc_rtl88xx::tone_mask", "CSI mask applied center={:?} MHz tones={count} weight={}", center_frequency_mhz(radio), spec.weight);
        }
        if let Some(frequency_khz) = options.nbi_frequency_khz {
            let applied = self
                .apply_nbi_for_chip_async(chip, radio, frequency_khz)
                .await?;
            log::info!(target: "openipc_rtl88xx::tone_mask", "NBI notch frequency_khz={frequency_khz} applied={applied}");
        }
        Ok(())
    }

    async fn apply_csi_mask_for_chip_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
        spec: CsiMaskSpec,
    ) -> Result<usize, DriverError> {
        let Some(center) = center_frequency_mhz(radio) else {
            return Ok(0);
        };
        let bandwidth = effective_bandwidth_mhz(radio.channel_width);
        match tone_mask_family(chip.family) {
            ToneMaskFamily::Jaguar3 => {
                self.apply_csi_mask_jaguar3_async(center, bandwidth, spec)
                    .await
            }
            _ => {
                self.apply_csi_mask_11ac_async(center, bandwidth, spec)
                    .await
            }
        }
    }

    async fn apply_csi_mask_11ac_async(
        &self,
        center: u32,
        bandwidth: u32,
        spec: CsiMaskSpec,
    ) -> Result<usize, DriverError> {
        let tones = enumerate_mask_tones(center, bandwidth, spec.low_khz, spec.high_khz);
        let mut shadow = [0u8; 32];
        for tone in &tones {
            let index = if *tone >= 0 {
                (*tone as u32).min(127)
            } else {
                let magnitude = (-*tone as u32).min(128);
                (128 + (128 - magnitude)).min(255)
            };
            shadow[index as usize >> 3] |= 1 << (index & 7);
        }
        for (index, word) in shadow.chunks_exact(4).enumerate() {
            self.write_u32_async(
                0x0880 + index as u16 * 4,
                u32::from_le_bytes(word.try_into().expect("four-byte chunk")),
            )
            .await?;
        }
        self.set_bb_reg_async(0x0874, 1, u32::from(!tones.is_empty()))
            .await?;
        Ok(tones.len())
    }

    async fn clear_csi_mask_11ac_async(&self) -> Result<(), DriverError> {
        for register in (0x0880..=0x089c).step_by(4) {
            self.write_u32_async(register, 0).await?;
        }
        self.set_bb_reg_async(0x0874, 1, 0).await
    }

    async fn apply_csi_mask_jaguar3_async(
        &self,
        center: u32,
        bandwidth: u32,
        spec: CsiMaskSpec,
    ) -> Result<usize, DriverError> {
        let rf_bandwidth = self.read_u8_async(0x09b0).await?;
        let tone_count = if ((rf_bandwidth & 0x0c) >> 2) == 2 {
            128u32
        } else {
            64u32
        };
        let tones = enumerate_mask_tones(center, bandwidth, spec.low_khz, spec.high_khz);
        let mut table = [0u8; 128];
        let nibble = 0x08 | (spec.weight & 0x07);
        for tone in &tones {
            let index = if *tone >= 0 {
                (*tone as u32).min(tone_count - 1)
            } else {
                (tone_count * 2) - (-*tone as u32).min(tone_count)
            };
            if index >= 256 {
                continue;
            }
            table[index as usize >> 1] |= if index & 1 == 0 { nibble } else { nibble << 4 };
        }
        self.set_bb_reg_async(0x1ee8, 0x03, 3).await?;
        self.set_bb_reg_async(0x1d94, 0xc000_0000, 1).await?;
        for address in 0..tone_count {
            self.set_bb_reg_async(0x1d94, 0x00ff_0000, address).await?;
            self.set_bb_reg_async(0x1d94, 0x0000_00ff, u32::from(table[address as usize]))
                .await?;
        }
        self.set_bb_reg_async(0x1ee8, 0x03, 0).await?;
        self.set_bb_reg_async(0x0c0c, 1 << 3, u32::from(!tones.is_empty()))
            .await?;
        Ok(tones.len())
    }

    async fn clear_csi_mask_jaguar3_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x1ee8, 0x03, 3).await?;
        self.set_bb_reg_async(0x1d94, 0xc000_0000, 1).await?;
        for address in 0..128 {
            self.set_bb_reg_async(0x1d94, 0x00ff_0000, address).await?;
            self.set_bb_reg_async(0x1d94, 0x0000_00ff, 0).await?;
        }
        self.set_bb_reg_async(0x1ee8, 0x03, 0).await?;
        self.set_bb_reg_async(0x0c0c, 1 << 3, 0).await
    }

    async fn apply_nbi_for_chip_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
        frequency_khz: u32,
    ) -> Result<bool, DriverError> {
        let Some(center) = center_frequency_mhz(radio) else {
            return Ok(false);
        };
        let bandwidth = effective_bandwidth_mhz(radio.channel_width);
        match tone_mask_family(chip.family) {
            ToneMaskFamily::Jaguar3 => {
                self.apply_nbi_jaguar3_async(
                    center,
                    bandwidth,
                    frequency_khz,
                    chip.total_rf_paths(),
                )
                .await
            }
            family => {
                self.apply_nbi_11ac_async(
                    family,
                    center,
                    bandwidth,
                    frequency_khz / 1000,
                    chip.total_rf_paths() >= 2,
                )
                .await
            }
        }
    }

    async fn apply_nbi_11ac_async(
        &self,
        family: ToneMaskFamily,
        center: u32,
        bandwidth: u32,
        frequency_mhz: u32,
        at_least_two_paths: bool,
    ) -> Result<bool, DriverError> {
        const LUT_128: [u32; 27] = [
            25, 55, 85, 115, 135, 155, 185, 205, 225, 245, 265, 285, 305, 335, 355, 375, 395, 415,
            435, 455, 485, 505, 525, 555, 585, 615, 635,
        ];
        const LUT_256: [u32; 59] = [
            25, 55, 85, 115, 135, 155, 175, 195, 225, 245, 265, 285, 305, 325, 345, 365, 385, 405,
            425, 445, 465, 485, 505, 525, 545, 565, 585, 605, 625, 645, 665, 695, 715, 735, 755,
            775, 795, 815, 835, 855, 875, 895, 915, 935, 955, 975, 995, 1015, 1035, 1055, 1085,
            1105, 1125, 1145, 1175, 1195, 1225, 1255, 1275,
        ];
        let lower = center - bandwidth / 2;
        let upper = center + bandwidth / 2;
        if !(lower..=upper).contains(&frequency_mhz) {
            return Ok(false);
        }
        let distance = frequency_mhz.abs_diff(center);
        let index_tenths = distance << 5;
        let table: &[u32] = if family == ToneMaskFamily::Ac1 || bandwidth == 80 {
            &LUT_256
        } else {
            &LUT_128
        };
        let register_index = table
            .iter()
            .position(|threshold| index_tenths < *threshold)
            .map_or(0, |index| index as u32 + 1);
        self.set_bb_reg_async(0x087c, 0x000f_c000, register_index)
            .await?;
        self.set_bb_reg_async(0x087c, 1 << 13, 1).await?;
        if family == ToneMaskFamily::Ac2_8822b {
            self.set_bb_reg_async(0x0c20, 1 << 28, 1).await?;
            if at_least_two_paths {
                self.set_bb_reg_async(0x0e20, 1 << 28, 1).await?;
            }
        }
        Ok(true)
    }

    async fn disable_nbi_11ac_async(
        &self,
        family: ToneMaskFamily,
        at_least_two_paths: bool,
    ) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x087c, 1 << 13, 0).await?;
        if family == ToneMaskFamily::Ac2_8822b {
            self.set_bb_reg_async(0x0c20, 1 << 28, 0).await?;
            if at_least_two_paths {
                self.set_bb_reg_async(0x0e20, 1 << 28, 0).await?;
            }
        }
        Ok(())
    }

    async fn apply_nbi_jaguar3_async(
        &self,
        center: u32,
        bandwidth: u32,
        frequency_khz: u32,
        paths: usize,
    ) -> Result<bool, DriverError> {
        let center_khz = center * 1000;
        let lower = center_khz - bandwidth * 500;
        let upper = center_khz + bandwidth * 500;
        if !(lower..=upper).contains(&frequency_khz) {
            return Ok(false);
        }
        let mut index = frequency_khz.abs_diff(center_khz) / 312;
        let rf_bandwidth = self.read_u8_async(0x09b0).await?;
        let tone_count = if ((rf_bandwidth & 0x0c) >> 2) == 2 {
            128
        } else {
            64
        };
        if frequency_khz >= center_khz {
            index = index.min(tone_count - 1);
        } else {
            index = tone_count * 2 - index.min(tone_count);
        }
        for register in [0x1944, 0x4044].into_iter().take(paths.min(2)) {
            self.set_bb_reg_async(register, 0x001f_f000, index).await?;
        }
        self.set_bb_reg_async(0x0818, 1 << 3, 0).await?;
        self.set_bb_reg_async(0x0818, 1 << 11, 1).await?;
        self.set_bb_reg_async(0x1d3c, 0x7800_0000, 0x0f).await?;
        for register in [0x1940, 0x4040].into_iter().take(paths.min(2)) {
            self.set_bb_reg_async(register, 1 << 31, 1).await?;
        }
        Ok(true)
    }

    async fn disable_nbi_jaguar3_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x0818, 1 << 3, 1).await?;
        self.set_bb_reg_async(0x1d3c, 0x7800_0000, 0).await?;
        self.set_bb_reg_async(0x0818, 1 << 3, 0).await?;
        self.set_bb_reg_async(0x0818, 1 << 11, 0).await?;
        self.set_bb_reg_async(0x1940, 1 << 31, 0).await?;
        self.set_bb_reg_async(0x4040, 1 << 31, 0).await
    }
}

fn tone_mask_family(family: ChipFamily) -> ToneMaskFamily {
    match family {
        ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => ToneMaskFamily::Ac1,
        ChipFamily::Rtl8814 => ToneMaskFamily::Ac2_8814,
        ChipFamily::Rtl8822b => ToneMaskFamily::Ac2_8822b,
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => ToneMaskFamily::Jaguar3,
    }
}

fn effective_bandwidth_mhz(width: ChannelWidth) -> u32 {
    match width {
        ChannelWidth::Mhz40 => 40,
        ChannelWidth::Mhz80 => 80,
        ChannelWidth::Mhz5 | ChannelWidth::Mhz10 | ChannelWidth::Mhz20 => 20,
    }
}

fn find_center_frequency_mhz(channel: u32, bandwidth: u32, secondary: u8) -> Option<u32> {
    if (1..=14).contains(&channel) {
        if bandwidth == 80 {
            return None;
        }
        let mut center = 2412 + (channel - 1) * 5;
        if bandwidth == 40 && secondary == 1 {
            if channel >= 10 {
                return None;
            }
            center += 10;
        } else if bandwidth == 40 && secondary == 2 {
            if channel <= 2 {
                return None;
            }
            center -= 10;
        }
        return Some(center);
    }
    if !(16..=253).contains(&channel) {
        return None;
    }
    let mut central_channel = channel;
    if matches!(bandwidth, 40 | 80) {
        const START_40: [u32; 29] = [
            20, 28, 36, 44, 52, 60, 68, 76, 84, 92, 100, 108, 116, 124, 132, 140, 149, 157, 165,
            173, 181, 189, 197, 205, 213, 221, 229, 237, 245,
        ];
        const START_80: [u32; 14] = [
            20, 36, 52, 68, 84, 100, 116, 132, 149, 165, 181, 197, 213, 229,
        ];
        let starts: &[u32] = if bandwidth == 40 {
            &START_40
        } else {
            &START_80
        };
        let offset = if bandwidth == 40 { 2 } else { 6 };
        for pair in starts.windows(2) {
            if channel < pair[1] {
                central_channel = pair[0] + offset;
                break;
            }
        }
    }
    Some(5080 + (central_channel - 16) * 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_frequency_matches_phydm_blocks() {
        assert_eq!(find_center_frequency_mhz(36, 80, 0), Some(5210));
        assert_eq!(find_center_frequency_mhz(48, 80, 0), Some(5210));
        assert_eq!(find_center_frequency_mhz(36, 40, 0), Some(5190));
        assert_eq!(find_center_frequency_mhz(100, 80, 0), Some(5530));
        assert_eq!(find_center_frequency_mhz(149, 80, 0), Some(5775));
        assert_eq!(find_center_frequency_mhz(6, 40, 1), Some(2447));
        assert_eq!(find_center_frequency_mhz(6, 40, 2), Some(2427));
        assert_eq!(find_center_frequency_mhz(11, 40, 1), None);
        assert_eq!(find_center_frequency_mhz(1, 40, 2), None);
        assert_eq!(find_center_frequency_mhz(6, 80, 0), None);
    }

    #[test]
    fn tone_enumeration_matches_devourer_vectors() {
        let top = enumerate_mask_tones(5210, 80, 5_230_000, 5_250_000);
        assert_eq!(
            (top.len(), top.first(), top.last()),
            (65, Some(&64), Some(&128))
        );
        let bottom = enumerate_mask_tones(5210, 80, 5_170_000, 5_190_000);
        assert_eq!(
            (bottom.len(), bottom.first(), bottom.last()),
            (65, Some(&-128), Some(&-64))
        );
        assert_eq!(
            enumerate_mask_tones(5210, 80, 5_209_000, 5_211_000),
            (-3..=3).collect::<Vec<_>>()
        );
        assert_eq!(
            enumerate_mask_tones(5210, 80, 5_100_000, 5_300_000).len(),
            257
        );
        assert_eq!(
            enumerate_mask_tones(5210, 80, 5_220_000, 5_220_000),
            vec![32]
        );
        assert_eq!(
            enumerate_mask_tones(5210, 80, 5_211_000, 5_211_000),
            vec![3]
        );
        assert!(enumerate_mask_tones(5210, 80, 5_300_000, 5_300_000).is_empty());
    }

    #[test]
    fn parses_devourer_csi_mask_syntax() {
        assert_eq!(
            CsiMaskSpec::parse_mhz("5230-5250/5"),
            CsiMaskSpec::new(5_230_000, 5_250_000, 5)
        );
        assert_eq!(
            CsiMaskSpec::parse_mhz("2447"),
            CsiMaskSpec::new(2_447_000, 2_447_000, 7)
        );
        assert_eq!(CsiMaskSpec::parse_mhz("junk"), None);
        assert_eq!(CsiMaskSpec::parse_mhz("5250-5230"), None);
    }
}
