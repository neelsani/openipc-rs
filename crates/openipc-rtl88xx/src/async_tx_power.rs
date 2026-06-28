use crate::async_efuse::{tx_power_index_base, EfuseInfo};
use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::regs::*;
use crate::types::{ChannelWidth, ChipFamily, ChipInfo, DriverError};

const MGN_1M: u8 = 0x02;
const MGN_2M: u8 = 0x04;
const MGN_5_5M: u8 = 0x0b;
const MGN_6M: u8 = 0x0c;
const MGN_9M: u8 = 0x12;
const MGN_11M: u8 = 0x16;
const MGN_12M: u8 = 0x18;
const MGN_18M: u8 = 0x24;
const MGN_24M: u8 = 0x30;
const MGN_36M: u8 = 0x48;
const MGN_48M: u8 = 0x60;
const MGN_54M: u8 = 0x6c;
const MGN_MCS0: u8 = 0x80;
const MGN_VHT1SS_MCS0: u8 = 0xa0;

const RATES_CCK: &[u8] = &[MGN_1M, MGN_2M, MGN_5_5M, MGN_11M];
const RATES_OFDM: &[u8] = &[
    MGN_6M, MGN_9M, MGN_12M, MGN_18M, MGN_24M, MGN_36M, MGN_48M, MGN_54M,
];
const RATES_HT_1SS: &[u8] = &[
    MGN_MCS0,
    MGN_MCS0 + 1,
    MGN_MCS0 + 2,
    MGN_MCS0 + 3,
    MGN_MCS0 + 4,
    MGN_MCS0 + 5,
    MGN_MCS0 + 6,
    MGN_MCS0 + 7,
];
const RATES_HT_2SS: &[u8] = &[
    MGN_MCS0 + 8,
    MGN_MCS0 + 9,
    MGN_MCS0 + 10,
    MGN_MCS0 + 11,
    MGN_MCS0 + 12,
    MGN_MCS0 + 13,
    MGN_MCS0 + 14,
    MGN_MCS0 + 15,
];
const RATES_HT_3SS: &[u8] = &[
    MGN_MCS0 + 16,
    MGN_MCS0 + 17,
    MGN_MCS0 + 18,
    MGN_MCS0 + 19,
    MGN_MCS0 + 20,
    MGN_MCS0 + 21,
    MGN_MCS0 + 22,
    MGN_MCS0 + 23,
];
const RATES_VHT_1SS: &[u8] = &[
    MGN_VHT1SS_MCS0,
    MGN_VHT1SS_MCS0 + 1,
    MGN_VHT1SS_MCS0 + 2,
    MGN_VHT1SS_MCS0 + 3,
    MGN_VHT1SS_MCS0 + 4,
    MGN_VHT1SS_MCS0 + 5,
    MGN_VHT1SS_MCS0 + 6,
    MGN_VHT1SS_MCS0 + 7,
    MGN_VHT1SS_MCS0 + 8,
    MGN_VHT1SS_MCS0 + 9,
];
const RATES_VHT_2SS: &[u8] = &[
    MGN_VHT1SS_MCS0 + 10,
    MGN_VHT1SS_MCS0 + 11,
    MGN_VHT1SS_MCS0 + 12,
    MGN_VHT1SS_MCS0 + 13,
    MGN_VHT1SS_MCS0 + 14,
    MGN_VHT1SS_MCS0 + 15,
    MGN_VHT1SS_MCS0 + 16,
    MGN_VHT1SS_MCS0 + 17,
    MGN_VHT1SS_MCS0 + 18,
    MGN_VHT1SS_MCS0 + 19,
];
const RATES_VHT_3SS: &[u8] = &[
    MGN_VHT1SS_MCS0 + 20,
    MGN_VHT1SS_MCS0 + 21,
    MGN_VHT1SS_MCS0 + 22,
    MGN_VHT1SS_MCS0 + 23,
    MGN_VHT1SS_MCS0 + 24,
    MGN_VHT1SS_MCS0 + 25,
    MGN_VHT1SS_MCS0 + 26,
    MGN_VHT1SS_MCS0 + 27,
    MGN_VHT1SS_MCS0 + 28,
    MGN_VHT1SS_MCS0 + 29,
];

const TXAGC_A_REGS_8812: &[u16] = &[
    0x0c20, 0x0c24, 0x0c28, 0x0c2c, 0x0c30, 0x0c34, 0x0c38, 0x0c3c, 0x0c40, 0x0c44, 0x0c48, 0x0c4c,
];
const TXAGC_B_REGS_8812: &[u16] = &[
    0x0e20, 0x0e24, 0x0e28, 0x0e2c, 0x0e30, 0x0e34, 0x0e38, 0x0e3c, 0x0e40, 0x0e44, 0x0e48, 0x0e4c,
];
const TXAGC_RATES_8812: [[u8; 4]; 12] = [
    [MGN_1M, MGN_2M, MGN_5_5M, MGN_11M],
    [MGN_6M, MGN_9M, MGN_12M, MGN_18M],
    [MGN_24M, MGN_36M, MGN_48M, MGN_54M],
    [MGN_MCS0, MGN_MCS0 + 1, MGN_MCS0 + 2, MGN_MCS0 + 3],
    [MGN_MCS0 + 4, MGN_MCS0 + 5, MGN_MCS0 + 6, MGN_MCS0 + 7],
    [MGN_MCS0 + 8, MGN_MCS0 + 9, MGN_MCS0 + 10, MGN_MCS0 + 11],
    [MGN_MCS0 + 12, MGN_MCS0 + 13, MGN_MCS0 + 14, MGN_MCS0 + 15],
    [
        MGN_VHT1SS_MCS0,
        MGN_VHT1SS_MCS0 + 1,
        MGN_VHT1SS_MCS0 + 2,
        MGN_VHT1SS_MCS0 + 3,
    ],
    [
        MGN_VHT1SS_MCS0 + 4,
        MGN_VHT1SS_MCS0 + 5,
        MGN_VHT1SS_MCS0 + 6,
        MGN_VHT1SS_MCS0 + 7,
    ],
    [
        MGN_VHT1SS_MCS0 + 8,
        MGN_VHT1SS_MCS0 + 9,
        MGN_VHT1SS_MCS0 + 10,
        MGN_VHT1SS_MCS0 + 11,
    ],
    [
        MGN_VHT1SS_MCS0 + 12,
        MGN_VHT1SS_MCS0 + 13,
        MGN_VHT1SS_MCS0 + 14,
        MGN_VHT1SS_MCS0 + 15,
    ],
    [
        MGN_VHT1SS_MCS0 + 16,
        MGN_VHT1SS_MCS0 + 17,
        MGN_VHT1SS_MCS0 + 18,
        MGN_VHT1SS_MCS0 + 19,
    ],
];

impl RealtekDevice {
    pub async fn set_tx_power_override_async(
        &self,
        current_channel: u8,
        power: u8,
    ) -> Result<(), DriverError> {
        if power > 63 {
            return Err(DriverError::InvalidTxPower(power));
        }
        let chip = self.probe_chip_async().await?;
        match chip.family {
            ChipFamily::Rtl8814 => {
                self.set_tx_power_override_8814_async(chip, current_channel, power)
                    .await
            }
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                self.set_tx_power_override_8812_family_async(chip, current_channel, power)
                    .await
            }
        }
    }

    pub(crate) async fn set_tx_power_level_for_chip_async(
        &self,
        chip: ChipInfo,
        current_channel: u8,
        power: u8,
    ) -> Result<(), DriverError> {
        if power > 63 {
            return Err(DriverError::InvalidTxPower(power));
        }
        match chip.family {
            ChipFamily::Rtl8814 => {
                self.set_tx_power_override_8814_async(chip, current_channel, power)
                    .await
            }
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                self.set_tx_power_override_8812_family_async(chip, current_channel, power)
                    .await
            }
        }
    }

    pub(crate) async fn set_tx_power_level_from_efuse_for_chip_async(
        &self,
        chip: ChipInfo,
        current_channel: u8,
        width: ChannelWidth,
        efuse: EfuseInfo,
        fallback_power: u8,
    ) -> Result<(), DriverError> {
        if !efuse.tx_power.loaded {
            return self
                .set_tx_power_level_for_chip_async(chip, current_channel, fallback_power)
                .await;
        }

        match chip.family {
            ChipFamily::Rtl8814 => {
                self.set_tx_power_from_efuse_8814_async(chip, current_channel, width, efuse)
                    .await
            }
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                self.set_tx_power_from_efuse_8812_family_async(
                    chip,
                    current_channel,
                    width,
                    efuse,
                    fallback_power,
                )
                .await
            }
        }
    }

    async fn set_tx_power_from_efuse_8812_family_async(
        &self,
        chip: ChipInfo,
        current_channel: u8,
        width: ChannelWidth,
        efuse: EfuseInfo,
        fallback_power: u8,
    ) -> Result<(), DriverError> {
        let include_cck = is_2ghz_channel(current_channel);
        for path in RfPath::iter(chip.total_rf_paths().min(2)) {
            let path_idx = path_index(path) as usize;
            let regs = match path {
                RfPath::A => TXAGC_A_REGS_8812,
                RfPath::B => TXAGC_B_REGS_8812,
                _ => continue,
            };
            for (row, register) in regs.iter().copied().enumerate() {
                if row == 0 && !include_cck {
                    continue;
                }
                let mut word = 0u32;
                for (byte_idx, rate) in TXAGC_RATES_8812[row].iter().copied().enumerate() {
                    let power = tx_power_index_base(
                        &efuse.tx_power,
                        path_idx,
                        rate,
                        rate_ntx(rate),
                        width,
                        current_channel,
                    )
                    .unwrap_or(fallback_power);
                    word |= u32::from(power & !1) << (byte_idx * 8);
                }
                self.write_u32_async(register, word).await?;
            }
            let training_power = tx_power_index_base(
                &efuse.tx_power,
                path_idx,
                MGN_MCS0 + 7,
                0,
                width,
                current_channel,
            )
            .unwrap_or(fallback_power);
            self.write_tx_power_training_8812_async(path, training_power)
                .await?;
        }
        Ok(())
    }

    async fn set_tx_power_from_efuse_8814_async(
        &self,
        chip: ChipInfo,
        current_channel: u8,
        width: ChannelWidth,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let include_cck = is_2ghz_channel(current_channel);
        for path in RfPath::iter(chip.total_rf_paths()) {
            let path_idx = path_index(path) as usize;
            for rates in [
                if include_cck { Some(RATES_CCK) } else { None },
                Some(RATES_OFDM),
                Some(RATES_HT_1SS),
                Some(RATES_VHT_1SS),
                Some(RATES_HT_2SS),
                Some(RATES_VHT_2SS),
                Some(RATES_HT_3SS),
                Some(RATES_VHT_3SS),
            ]
            .into_iter()
            .flatten()
            {
                for rate in rates {
                    let power = tx_power_index_base(
                        &efuse.tx_power,
                        path_idx,
                        *rate,
                        rate_ntx(*rate),
                        width,
                        current_channel,
                    )
                    .unwrap_or(16);
                    self.write_txagc_table_8814_async(path, *rate, power)
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn set_tx_power_override_8812_family_async(
        &self,
        chip: ChipInfo,
        current_channel: u8,
        power: u8,
    ) -> Result<(), DriverError> {
        let word = txagc_word(power);
        let include_cck = is_2ghz_channel(current_channel);
        for path in RfPath::iter(chip.total_rf_paths().min(2)) {
            let regs = match path {
                RfPath::A => TXAGC_A_REGS_8812,
                RfPath::B => TXAGC_B_REGS_8812,
                _ => continue,
            };
            for (idx, register) in regs.iter().copied().enumerate() {
                if idx == 0 && !include_cck {
                    continue;
                }
                self.write_u32_async(register, word).await?;
            }
            self.write_tx_power_training_8812_async(path, power).await?;
        }
        Ok(())
    }

    async fn write_tx_power_training_8812_async(
        &self,
        path: RfPath,
        power: u8,
    ) -> Result<(), DriverError> {
        let register = match path {
            RfPath::A => 0x0c54,
            RfPath::B => 0x0e54,
            _ => return Ok(()),
        };
        let p0 = power.saturating_sub(10).max(2);
        let p1 = power.saturating_sub(8).max(2);
        let p2 = power.saturating_sub(6).max(2);
        let value = u32::from(p0) | (u32::from(p1) << 8) | (u32::from(p2) << 16);
        self.set_bb_reg_async(register, 0x00ff_ffff, value).await
    }

    async fn set_tx_power_override_8814_async(
        &self,
        chip: ChipInfo,
        current_channel: u8,
        power: u8,
    ) -> Result<(), DriverError> {
        let include_cck = is_2ghz_channel(current_channel);
        for path in RfPath::iter(chip.total_rf_paths()) {
            if include_cck {
                self.write_txagc_table_rates_8814_async(path, RATES_CCK, power)
                    .await?;
            }
            self.write_txagc_table_rates_8814_async(path, RATES_OFDM, power)
                .await?;
            self.write_txagc_table_rates_8814_async(path, RATES_HT_1SS, power)
                .await?;
            self.write_txagc_table_rates_8814_async(path, RATES_VHT_1SS, power)
                .await?;
            if chip.total_rf_paths() >= 2 {
                self.write_txagc_table_rates_8814_async(path, RATES_HT_2SS, power)
                    .await?;
                self.write_txagc_table_rates_8814_async(path, RATES_VHT_2SS, power)
                    .await?;
            }
            self.write_txagc_table_rates_8814_async(path, RATES_HT_3SS, power)
                .await?;
            self.write_txagc_table_rates_8814_async(path, RATES_VHT_3SS, power)
                .await?;
        }
        Ok(())
    }

    async fn write_txagc_table_rates_8814_async(
        &self,
        path: RfPath,
        rates: &[u8],
        power: u8,
    ) -> Result<(), DriverError> {
        for rate in rates {
            self.write_txagc_table_8814_async(path, *rate, power)
                .await?;
        }
        Ok(())
    }

    async fn write_txagc_table_8814_async(
        &self,
        path: RfPath,
        rate: u8,
        power: u8,
    ) -> Result<(), DriverError> {
        let value = 0x0080_1000
            | (u32::from(path_index(path)) << 8)
            | u32::from(mrate_to_hw_rate(rate))
            | (u32::from(power) << 24);
        self.set_bb_reg_async(0x1998, B_MASK_DWORD, value).await?;
        if rate == MGN_1M {
            self.set_bb_reg_async(0x1998, B_MASK_DWORD, value).await?;
        }
        Ok(())
    }
}

const fn txagc_word(power: u8) -> u32 {
    let byte = (power & !1) as u32;
    byte | (byte << 8) | (byte << 16) | (byte << 24)
}

const fn is_2ghz_channel(channel: u8) -> bool {
    channel <= 14
}

const fn path_index(path: RfPath) -> u8 {
    match path {
        RfPath::A => 0,
        RfPath::B => 1,
        RfPath::C => 2,
        RfPath::D => 3,
    }
}

const fn rate_ntx(rate: u8) -> usize {
    match rate {
        0x88..=0x8f | 0xaa..=0xb3 => 1,
        0x90..=0x97 | 0xb4..=0xbd => 2,
        0x98..=0x9f | 0xbe..=0xc7 => 3,
        _ => 0,
    }
}

const fn mrate_to_hw_rate(rate: u8) -> u8 {
    match rate {
        MGN_1M => 0x00,
        MGN_2M => 0x01,
        MGN_5_5M => 0x02,
        MGN_11M => 0x03,
        MGN_6M => 0x04,
        MGN_9M => 0x05,
        MGN_12M => 0x06,
        MGN_18M => 0x07,
        MGN_24M => 0x08,
        MGN_36M => 0x09,
        MGN_48M => 0x0a,
        MGN_54M => 0x0b,
        MGN_MCS0..=0x9f => 0x0c + (rate - MGN_MCS0),
        MGN_VHT1SS_MCS0..=0xc7 => 0x2c + (rate - MGN_VHT1SS_MCS0),
        _ => 0x00,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txagc_word_repeats_power_in_all_rate_bytes() {
        assert_eq!(txagc_word(40), 0x2828_2828);
        assert_eq!(txagc_word(41), 0x2828_2828);
    }

    #[test]
    fn rtl8814_txagc_command_matches_devourer_shape() {
        let value = 0x0080_1000
            | (u32::from(path_index(RfPath::C)) << 8)
            | u32::from(mrate_to_hw_rate(MGN_MCS0))
            | (u32::from(30u8) << 24);
        assert_eq!(value, 0x1e80_120c);
    }
}
