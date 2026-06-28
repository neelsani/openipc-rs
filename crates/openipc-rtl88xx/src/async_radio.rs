use crate::async_efuse::EfuseInfo;
use crate::device::RealtekDevice;
use crate::phy::{bit_shift, RfPath};
use crate::regs::*;
use crate::types::{ChannelWidth, ChipFamily, ChipInfo, DriverError, RadioConfig, RfType};

impl RealtekDevice {
    pub(crate) async fn set_monitor_mode_async(
        &self,
        accept_bad_fcs: bool,
    ) -> Result<(), DriverError> {
        let msr = self.read_u8_async(REG_CR + 2).await.unwrap_or(0) & 0x0c;
        self.write_u8_async(REG_CR + 2, msr).await?;
        let mut rcr = RCR_AAP
            | RCR_APM
            | RCR_AM
            | RCR_AB
            | RCR_APWRMGT
            | RCR_ADF
            | RCR_ACF
            | RCR_AMF
            | RCR_APP_PHYST_RXFF
            | RCR_APPFCS;
        if accept_bad_fcs {
            rcr |= RCR_ACRC32 | RCR_AICV;
        }
        self.write_u32_async(REG_RCR, rcr).await?;
        self.write_u16_async(REG_RXFLTMAP2, 0xffff).await
    }

    pub(crate) async fn enable_rx_bar_async(&self) -> Result<(), DriverError> {
        let rxfltmap1 = self.read_u16_async(REG_RXFLTMAP1).await.unwrap_or(0);
        self.write_u16_async(REG_RXFLTMAP1, rxfltmap1 | BIT8 as u16)
            .await
    }

    pub(crate) async fn set_channel_with_options_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
        efuse: EfuseInfo,
        skip_tx_power: bool,
    ) -> Result<(), DriverError> {
        let center_channel =
            center_channel(radio.channel, radio.channel_width, radio.channel_offset);
        self.switch_wireless_band_async(chip, radio.channel, radio.channel_width, efuse)
            .await?;
        self.set_channel_number_async(chip, center_channel).await?;
        self.set_bandwidth_async(chip, radio.channel_width, radio.channel_offset)
            .await?;
        if skip_tx_power {
            return Ok(());
        }
        self.set_tx_power_level_from_efuse_for_chip_async(
            chip,
            center_channel,
            radio.channel_width,
            efuse,
            16,
        )
        .await
    }

    async fn switch_wireless_band_async(
        &self,
        chip: ChipInfo,
        channel: u8,
        width: ChannelWidth,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        if chip.family == ChipFamily::Rtl8814 {
            self.switch_wireless_band_8814_async(channel, width, efuse)
                .await
        } else {
            self.switch_wireless_band_8812_async(chip, channel, width, efuse)
                .await
        }
    }

    async fn switch_wireless_band_8812_async(
        &self,
        chip: ChipInfo,
        channel: u8,
        width: ChannelWidth,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let is_2g = channel <= 14;
        if is_2g {
            self.set_bb_reg_async(R_OFDMCCKEN_JAGUAR, B_OFDM_EN_JAGUAR | B_CCK_EN_JAGUAR, 0x3)
                .await?;

            if chip.family == ChipFamily::Rtl8821 {
                self.set_rfe_reg_8821_async(true, efuse.external_lna_2g)
                    .await?;
            } else {
                self.set_bb_reg_async(R_BW_INDICATION_JAGUAR, 0x3, 0x1)
                    .await?;
                self.set_bb_reg_async(
                    R_PWED_TH_JAGUAR,
                    BIT13 | BIT14 | BIT15 | BIT16 | BIT17,
                    0x17,
                )
                .await?;

                let pwed_th = if width == ChannelWidth::Mhz20
                    && chip.rf_type == RfType::OneTOneR
                    && !efuse.external_lna_2g
                {
                    0x02
                } else {
                    0x04
                };
                self.set_bb_reg_async(R_PWED_TH_JAGUAR, BIT1 | BIT2 | BIT3, pwed_th)
                    .await?;
            }

            if chip.family == ChipFamily::Rtl8821 {
                self.set_bb_reg_async(R_A_TX_SCALE_JAGUAR, 0x0f00, 0)
                    .await?;
            } else {
                self.set_bb_reg_async(R_AGC_TABLE_JAGUAR, 0x3, 0).await?;
                self.set_rfe_reg_8812_async(true, efuse.rfe_type).await?;
            }

            self.set_bb_reg_async(R_TX_PATH_JAGUAR, 0xf0, 0x1).await?;
            self.set_bb_reg_async(R_CCK_RX_JAGUAR, 0x0f00_0000, 0x1)
                .await?;
            let cck_check = self.read_u8_async(REG_CCK_CHECK).await.unwrap_or(0);
            self.write_u8_async(REG_CCK_CHECK, cck_check & !(BIT7 as u8))
                .await?;
        } else {
            let cck_check = self.read_u8_async(REG_CCK_CHECK).await.unwrap_or(0);
            self.write_u8_async(REG_CCK_CHECK, cck_check | BIT7 as u8)
                .await?;

            let mut attempts = 0u8;
            while self.read_u16_async(REG_TXPKT_EMPTY).await.unwrap_or(0) & 0x30 != 0x30
                && attempts < 50
            {
                crate::time::sleep_ms(50).await;
                attempts += 1;
            }

            self.set_bb_reg_async(R_OFDMCCKEN_JAGUAR, B_OFDM_EN_JAGUAR | B_CCK_EN_JAGUAR, 0x3)
                .await?;

            if chip.family == ChipFamily::Rtl8821 {
                self.set_rfe_reg_8821_async(false, efuse.external_lna_2g)
                    .await?;
            } else {
                self.set_bb_reg_async(R_BW_INDICATION_JAGUAR, 0x3, 0x2)
                    .await?;
                self.set_bb_reg_async(
                    R_PWED_TH_JAGUAR,
                    BIT13 | BIT14 | BIT15 | BIT16 | BIT17,
                    0x15,
                )
                .await?;
                self.set_bb_reg_async(R_PWED_TH_JAGUAR, BIT1 | BIT2 | BIT3, 0x04)
                    .await?;
            }

            if chip.family == ChipFamily::Rtl8821 {
                self.set_bb_reg_async(R_A_TX_SCALE_JAGUAR, 0x0f00, 1)
                    .await?;
            } else {
                self.set_bb_reg_async(R_AGC_TABLE_JAGUAR, 0x3, 1).await?;
                self.set_rfe_reg_8812_async(false, efuse.rfe_type).await?;
            }

            self.set_bb_reg_async(R_TX_PATH_JAGUAR, 0xf0, 0x0).await?;
            self.set_bb_reg_async(R_CCK_RX_JAGUAR, 0x0f00_0000, 0x0f)
                .await?;
        }

        self.set_bb_swing_by_band_8812_async(is_2g, efuse).await
    }

    async fn switch_wireless_band_8814_async(
        &self,
        channel: u8,
        width: ChannelWidth,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let is_2g = channel <= 14;
        let sys_cfg3 = self.read_u8_async(0x1002).await.unwrap_or(0);
        self.write_u8_async(0x1002, sys_cfg3 & !(BIT0 as u8))
            .await?;

        if is_2g {
            self.set_bb_reg_async(R_AGC_TABLE_JAGUAR2, 0x1f, 0).await?;
            self.set_rfe_reg_8814_async(true, efuse.rfe_type).await?;
            self.set_bb_reg_async(R_TX_PATH_JAGUAR, 0xf0, 0x2).await?;
            self.set_bb_reg_async(R_CCK_RX_JAGUAR, 0x0f00_0000, 0x5)
                .await?;
            self.set_bb_reg_async(R_OFDMCCKEN_JAGUAR, B_OFDM_EN_JAGUAR | B_CCK_EN_JAGUAR, 0x3)
                .await?;
            self.write_u8_async(REG_CCK_CHECK, 0x00).await?;
            self.set_bb_reg_async(0x0a80, BIT18, 0).await?;
        } else {
            self.write_u8_async(REG_CCK_CHECK, 0x80).await?;
            self.set_bb_reg_async(0x0a80, BIT18, 1).await?;
            self.set_rfe_reg_8814_async(false, efuse.rfe_type).await?;
            self.set_bb_reg_async(R_TX_PATH_JAGUAR, 0xf0, 0).await?;
            self.set_bb_reg_async(R_CCK_RX_JAGUAR, 0x0f00_0000, 0x0f)
                .await?;
            self.set_bb_reg_async(R_OFDMCCKEN_JAGUAR, B_OFDM_EN_JAGUAR | B_CCK_EN_JAGUAR, 0x2)
                .await?;
        }

        self.set_bb_swing_by_band_8814_async(is_2g, efuse).await?;
        self.set_bw_reg_adc_8814_async(width).await?;
        self.set_bw_reg_agc_8814_async(is_2g, width).await?;

        let sys_cfg3 = self.read_u8_async(0x1002).await.unwrap_or(0);
        self.write_u8_async(0x1002, sys_cfg3 | BIT0 as u8).await?;
        self.init_rfe_gpio_8814_async(efuse.rfe_type).await
    }

    async fn set_rfe_reg_8821_async(
        &self,
        is_2g: bool,
        external_lna_2g: bool,
    ) -> Result<(), DriverError> {
        if is_2g {
            self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, 0xf000, 0x7)
                .await?;
            self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, 0xf0, 0x7)
                .await?;

            let lna_pinmux = if external_lna_2g { 0x2 } else { 0x7 };
            self.set_bb_reg_async(
                R_A_RFE_INV_JAGUAR,
                BIT20,
                if external_lna_2g { 1 } else { 0 },
            )
            .await?;
            self.set_bb_reg_async(R_A_RFE_INV_JAGUAR, BIT22, 0).await?;
            self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, BIT2 | BIT1 | BIT0, lna_pinmux)
                .await?;
            self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, BIT10 | BIT9 | BIT8, lna_pinmux)
                .await
        } else {
            self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, 0xf000, 0x5)
                .await?;
            self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, 0xf0, 0x4)
                .await?;
            self.set_bb_reg_async(R_A_RFE_INV_JAGUAR, BIT20, 0).await?;
            self.set_bb_reg_async(R_A_RFE_INV_JAGUAR, BIT22, 0).await?;
            self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, BIT2 | BIT1 | BIT0, 0x7)
                .await?;
            self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, BIT10 | BIT9 | BIT8, 0x7)
                .await
        }
    }

    async fn set_rfe_reg_8812_async(&self, is_2g: bool, rfe_type: u8) -> Result<(), DriverError> {
        match (is_2g, rfe_type) {
            (true, 0..=2) => {
                self.set_rfe_pair_8812_async(0x7777_7777, 0x7777_7777, B_MASK_RFE_INV_JAGUAR, 0)
                    .await
            }
            (true, 3) => {
                self.set_rfe_pair_8812_async(
                    0x5433_7770,
                    0x5433_7770,
                    B_MASK_RFE_INV_JAGUAR,
                    0x010,
                )
                .await?;
                self.set_bb_reg_async(R_ANTSEL_SW_JAGUAR, 0x0000_0303, 0x1)
                    .await
            }
            (true, 4) => {
                self.set_rfe_pair_8812_async(0x7777_7777, 0x7777_7777, B_MASK_RFE_INV_JAGUAR, 0x001)
                    .await
            }
            (true, 5) => {
                self.write_u8_async(R_A_RFE_PINMUX_JAGUAR + 2, 0x77).await?;
                self.set_bb_reg_async(R_B_RFE_PINMUX_JAGUAR, B_MASK_DWORD, 0x7777_7777)
                    .await?;
                let inv = self
                    .read_u8_async(R_A_RFE_INV_JAGUAR + 3)
                    .await
                    .unwrap_or(0);
                self.write_u8_async(R_A_RFE_INV_JAGUAR + 3, inv & !(BIT0 as u8))
                    .await?;
                self.set_bb_reg_async(R_B_RFE_INV_JAGUAR, B_MASK_RFE_INV_JAGUAR, 0)
                    .await
            }
            (true, 6) => {
                self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, B_MASK_DWORD, 0x0777_2770)
                    .await?;
                self.set_bb_reg_async(R_B_RFE_PINMUX_JAGUAR, B_MASK_DWORD, 0x0777_2770)
                    .await?;
                self.set_bb_reg_async(R_A_RFE_INV_JAGUAR, B_MASK_DWORD, 0x0000_0077)
                    .await?;
                self.set_bb_reg_async(R_B_RFE_INV_JAGUAR, B_MASK_DWORD, 0x0000_0077)
                    .await
            }
            (false, 0) => {
                self.set_rfe_pair_8812_async(0x7733_7717, 0x7733_7717, B_MASK_RFE_INV_JAGUAR, 0x010)
                    .await
            }
            (false, 1) => {
                self.set_rfe_pair_8812_async(0x7733_7717, 0x7733_7717, B_MASK_RFE_INV_JAGUAR, 0)
                    .await
            }
            (false, 2 | 4) => {
                self.set_rfe_pair_8812_async(0x7733_7777, 0x7733_7777, B_MASK_RFE_INV_JAGUAR, 0x010)
                    .await
            }
            (false, 3) => {
                self.set_rfe_pair_8812_async(
                    0x5433_7717,
                    0x5433_7717,
                    B_MASK_RFE_INV_JAGUAR,
                    0x010,
                )
                .await?;
                self.set_bb_reg_async(R_ANTSEL_SW_JAGUAR, 0x0000_0303, 0x1)
                    .await
            }
            (false, 5) => {
                self.write_u8_async(R_A_RFE_PINMUX_JAGUAR + 2, 0x33).await?;
                self.set_bb_reg_async(R_B_RFE_PINMUX_JAGUAR, B_MASK_DWORD, 0x7733_7777)
                    .await?;
                let inv = self
                    .read_u8_async(R_A_RFE_INV_JAGUAR + 3)
                    .await
                    .unwrap_or(0);
                self.write_u8_async(R_A_RFE_INV_JAGUAR + 3, inv | BIT0 as u8)
                    .await?;
                self.set_bb_reg_async(R_B_RFE_INV_JAGUAR, B_MASK_RFE_INV_JAGUAR, 0x010)
                    .await
            }
            (false, 6) => {
                self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, B_MASK_DWORD, 0x0773_7717)
                    .await?;
                self.set_bb_reg_async(R_B_RFE_PINMUX_JAGUAR, B_MASK_DWORD, 0x0773_7717)
                    .await?;
                self.set_bb_reg_async(R_A_RFE_INV_JAGUAR, B_MASK_DWORD, 0x0000_0077)
                    .await?;
                self.set_bb_reg_async(R_B_RFE_INV_JAGUAR, B_MASK_DWORD, 0x0000_0077)
                    .await
            }
            _ => Ok(()),
        }
    }

    async fn set_rfe_pair_8812_async(
        &self,
        pinmux_a: u32,
        pinmux_b: u32,
        inv_mask: u32,
        inv: u32,
    ) -> Result<(), DriverError> {
        self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, B_MASK_DWORD, pinmux_a)
            .await?;
        self.set_bb_reg_async(R_B_RFE_PINMUX_JAGUAR, B_MASK_DWORD, pinmux_b)
            .await?;
        self.set_bb_reg_async(R_A_RFE_INV_JAGUAR, inv_mask, inv)
            .await?;
        self.set_bb_reg_async(R_B_RFE_INV_JAGUAR, inv_mask, inv)
            .await
    }

    async fn set_rfe_reg_8814_async(&self, is_2g: bool, rfe_type: u8) -> Result<(), DriverError> {
        let rfe: (u32, u32, u32, Option<u32>, u32) = match (is_2g, rfe_type) {
            (true, 2) => (
                0x7270_7270,
                0x7270_7270,
                0x7270_7270,
                Some(0x7770_7770),
                0x72,
            ),
            (true, 1) => (
                0x7777_7777,
                0x7777_7777,
                0x7777_7777,
                Some(0x7777_7777),
                0x77,
            ),
            (true, _) => (0x7777_7777, 0x7777_7777, 0x7777_7777, None, 0x77),
            (false, 2) => (
                0x3717_3717,
                0x3717_3717,
                0x3717_3717,
                Some(0x7717_7717),
                0x37,
            ),
            (false, 1) => (
                0x3317_3317,
                0x3317_3317,
                0x3317_3317,
                Some(0x7717_7717),
                0x33,
            ),
            (false, _) => (
                0x5477_5477,
                0x5477_5477,
                0x5477_5477,
                Some(0x5477_5477),
                0x54,
            ),
        };

        self.set_bb_reg_async(R_A_RFE_PINMUX_JAGUAR, B_MASK_DWORD, rfe.0)
            .await?;
        self.set_bb_reg_async(R_B_RFE_PINMUX_JAGUAR, B_MASK_DWORD, rfe.1)
            .await?;
        self.set_bb_reg_async(R_C_RFE_PINMUX_8814, B_MASK_DWORD, rfe.2)
            .await?;
        if let Some(path_d) = rfe.3 {
            self.set_bb_reg_async(R_D_RFE_PINMUX_8814, B_MASK_DWORD, path_d)
                .await?;
        }
        self.set_bb_reg_async(R_D_RFE_INV_8814, 0x0ff0_0000, rfe.4)
            .await
    }

    async fn init_rfe_gpio_8814_async(&self, rfe_type: u8) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x1994, 0x0f, 0x0f).await?;
        let gpio = self.read_u8_async(0x0042).await.unwrap_or(0);
        let or_value: u8 = if rfe_type == 0 { 0xc0 } else { 0xf0 };
        self.write_u8_async(0x0042, gpio | or_value).await
    }

    async fn set_bb_swing_by_band_8812_async(
        &self,
        is_2g: bool,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let swing = if is_2g {
            efuse.tx_bb_swing_2g
        } else {
            efuse.tx_bb_swing_5g
        };
        self.set_bb_reg_async(R_A_TX_SCALE_JAGUAR, 0xffe0_0000, bb_swing_value(swing, 0))
            .await?;
        self.set_bb_reg_async(R_B_TX_SCALE_JAGUAR, 0xffe0_0000, bb_swing_value(swing, 1))
            .await
    }

    async fn set_bb_swing_by_band_8814_async(
        &self,
        is_2g: bool,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let swing = if is_2g {
            efuse.tx_bb_swing_2g
        } else {
            efuse.tx_bb_swing_5g
        };
        for (path_index, register) in [
            R_A_TX_SCALE_JAGUAR,
            R_B_TX_SCALE_JAGUAR,
            R_C_TX_SCALE_JAGUAR,
            R_D_TX_SCALE_JAGUAR,
        ]
        .into_iter()
        .enumerate()
        {
            self.set_bb_reg_async(register, 0xffe0_0000, bb_swing_value(swing, path_index))
                .await?;
        }
        Ok(())
    }

    async fn set_bw_reg_adc_8814_async(&self, width: ChannelWidth) -> Result<(), DriverError> {
        let value = match width {
            ChannelWidth::Mhz20 => 0,
            ChannelWidth::Mhz40 => 1,
            ChannelWidth::Mhz80 => 2,
        };
        self.set_bb_reg_async(R_RFMOD_JAGUAR, BIT1 | BIT0, value)
            .await
    }

    async fn set_bw_reg_agc_8814_async(
        &self,
        is_2g: bool,
        width: ChannelWidth,
    ) -> Result<(), DriverError> {
        let value = match width {
            ChannelWidth::Mhz20 => 6,
            ChannelWidth::Mhz40 if is_2g => 7,
            ChannelWidth::Mhz40 => 8,
            ChannelWidth::Mhz80 => 3,
        };
        self.set_bb_reg_async(R_AGC_TABLE_JAGUAR, 0xf000, value)
            .await
    }

    async fn set_channel_number_async(
        &self,
        chip: ChipInfo,
        channel: u8,
    ) -> Result<(), DriverError> {
        let fc_area = if chip.family == ChipFamily::Rtl8814 {
            if (36..=48).contains(&channel) {
                0x494
            } else if (50..=64).contains(&channel) {
                0x453
            } else if (100..=116).contains(&channel) {
                0x452
            } else if channel >= 118 {
                0x412
            } else {
                0x96a
            }
        } else if (36..=48).contains(&channel) || (15..=35).contains(&channel) {
            0x494
        } else if (50..=80).contains(&channel) {
            0x453
        } else if (82..=116).contains(&channel) {
            0x452
        } else if channel >= 118 {
            0x412
        } else {
            0x96a
        };
        self.set_bb_reg_async(R_FC_AREA_JAGUAR, 0x1ffe0000, fc_area)
            .await?;

        for path in RfPath::iter(chip.total_rf_paths()) {
            let rf_val = if chip.family == ChipFamily::Rtl8814 {
                if (36..=64).contains(&channel) {
                    0x101
                } else if (100..=140).contains(&channel) {
                    0x301
                } else if channel > 140 {
                    0x501
                } else {
                    0
                }
            } else if (36..=80).contains(&channel) || (15..=35).contains(&channel) {
                0x101
            } else if (82..=140).contains(&channel) {
                0x301
            } else if channel > 140 {
                0x501
            } else {
                0
            };
            if chip.family == ChipFamily::Rtl8814 {
                let combined = (rf_val << 8) | channel as u32;
                self.set_rf_reg_async(
                    chip,
                    path,
                    RF_CHNLBW_JAGUAR,
                    BIT18 | BIT17 | BIT16 | BIT9 | BIT8 | B_MASK_BYTE0,
                    combined,
                )
                .await?;
            } else {
                self.set_rf_reg_async(
                    chip,
                    path,
                    RF_CHNLBW_JAGUAR,
                    BIT18 | BIT17 | BIT16 | BIT9 | BIT8,
                    rf_val,
                )
                .await?;
                self.set_rf_reg_async(chip, path, RF_CHNLBW_JAGUAR, B_MASK_BYTE0, channel as u32)
                    .await?;
            }
        }

        if chip.family == ChipFamily::Rtl8814 {
            if (36..=64).contains(&channel) {
                self.set_bb_reg_async(0x0958, 0x1f, 1).await?;
            } else if (100..=144).contains(&channel) {
                self.set_bb_reg_async(0x0958, 0x1f, 2).await?;
            } else if channel >= 149 {
                self.set_bb_reg_async(0x0958, 0x1f, 3).await?;
            }
            if (1..=11).contains(&channel) {
                self.set_bb_reg_async(R_CCK0_TXFILTER1, B_MASK_DWORD, 0x1a1b0030)
                    .await?;
                self.set_bb_reg_async(R_CCK0_TXFILTER2, B_MASK_DWORD, 0x090e1317)
                    .await?;
                self.set_bb_reg_async(R_CCK0_DEBUGPORT, B_MASK_DWORD, 0x00000204)
                    .await?;
            } else if (12..=13).contains(&channel) {
                self.set_bb_reg_async(R_CCK0_TXFILTER1, B_MASK_DWORD, 0x1a1b0030)
                    .await?;
                self.set_bb_reg_async(R_CCK0_TXFILTER2, B_MASK_DWORD, 0x090e1217)
                    .await?;
                self.set_bb_reg_async(R_CCK0_DEBUGPORT, B_MASK_DWORD, 0x00000305)
                    .await?;
            } else if channel == 14 {
                self.set_bb_reg_async(R_CCK0_TXFILTER1, B_MASK_DWORD, 0x1a1b0030)
                    .await?;
                self.set_bb_reg_async(R_CCK0_TXFILTER2, B_MASK_DWORD, 0x00000e17)
                    .await?;
                self.set_bb_reg_async(R_CCK0_DEBUGPORT, B_MASK_DWORD, 0)
                    .await?;
            }
        }

        Ok(())
    }

    async fn set_bandwidth_async(
        &self,
        chip: ChipInfo,
        width: ChannelWidth,
        channel_offset: u8,
    ) -> Result<(), DriverError> {
        let mut trx = self.read_u16_async(REG_WMAC_TRXPTCL_CTL).await.unwrap_or(0);
        trx = match width {
            ChannelWidth::Mhz20 => trx & 0xfe7f,
            ChannelWidth::Mhz40 => (trx | BIT7 as u16) & 0xfeff,
            ChannelWidth::Mhz80 => (trx | BIT8 as u16) & 0xff7f,
        };
        self.write_u16_async(REG_WMAC_TRXPTCL_CTL, trx).await?;
        self.write_u8_async(REG_DATA_SC_8812, channel_offset)
            .await?;

        match width {
            ChannelWidth::Mhz20 => {
                if chip.family != ChipFamily::Rtl8814 {
                    self.set_bb_reg_async(R_RFMOD_JAGUAR, 0x003003c3, 0x00300200)
                        .await?;
                }
            }
            ChannelWidth::Mhz40 => {
                if chip.family != ChipFamily::Rtl8814 {
                    self.set_bb_reg_async(R_RFMOD_JAGUAR, 0x003003c3, 0x00300201)
                        .await?;
                }
                self.set_bb_reg_async(R_RFMOD_JAGUAR, 0x3c, channel_offset as u32)
                    .await?;
            }
            ChannelWidth::Mhz80 => {
                if chip.family != ChipFamily::Rtl8814 {
                    self.set_bb_reg_async(R_RFMOD_JAGUAR, 0x003003c3, 0x00300202)
                        .await?;
                }
                self.set_bb_reg_async(R_RFMOD_JAGUAR, 0x3c, channel_offset as u32)
                    .await?;
            }
        }

        for path in RfPath::iter(chip.total_rf_paths()) {
            self.set_rf_reg_async(
                chip,
                path,
                RF_CHNLBW_JAGUAR,
                BIT11 | BIT10,
                width.rf_bw_bits(),
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn set_bb_reg_async(
        &self,
        register: u16,
        mask: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        let data = if mask == B_MASK_DWORD {
            value
        } else {
            let original = self.read_u32_async(register).await?;
            let shift = bit_shift(mask);
            (original & !mask) | ((value << shift) & mask)
        };
        self.write_u32_async(register, data).await
    }

    pub(crate) async fn query_bb_reg_async(
        &self,
        register: u16,
        mask: u32,
    ) -> Result<u32, DriverError> {
        let value = self.read_u32_async(register).await?;
        Ok((value & mask) >> bit_shift(mask))
    }

    pub(crate) async fn set_rf_reg_async(
        &self,
        chip: ChipInfo,
        path: RfPath,
        register: u16,
        mask: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        let data = if mask == 0 {
            return Ok(());
        } else if mask == B_LSSI_WRITE_DATA {
            value
        } else {
            let original = self.query_rf_reg_async(chip, path, register).await?;
            let shift = bit_shift(mask);
            (original & !mask) | (value << shift)
        };
        self.rf_serial_write_async(path, register, data).await
    }

    pub(crate) async fn query_rf_reg_async(
        &self,
        chip: ChipInfo,
        path: RfPath,
        register: u16,
    ) -> Result<u32, DriverError> {
        if chip.family == ChipFamily::Rtl8814 {
            let base = match path {
                RfPath::A => 0x2800,
                RfPath::B => 0x2c00,
                RfPath::C => 0x3800,
                RfPath::D => 0x3c00,
            };
            let direct = base + ((register as u32 & 0xff) * 4);
            return self.query_bb_reg_async(direct as u16, 0x000f_ffff).await;
        }

        const R_HSSI_READ: u16 = 0x08b0;
        const B_HSSI_READ_ADDR: u32 = 0xff;
        const R_A_SI_READ: u16 = 0x0d08;
        const R_B_SI_READ: u16 = 0x0d48;
        const R_A_PI_READ: u16 = 0x0d04;
        const R_B_PI_READ: u16 = 0x0d44;
        const R_READ_DATA: u32 = 0x000f_ffff;
        let toggle_cca = register != 0 && chip.cut_version != 2;
        if toggle_cca {
            self.set_bb_reg_async(R_CCA_ON_SEC_JAGUAR, BIT3, 1).await?;
        }

        self.set_bb_reg_async(R_HSSI_READ, B_HSSI_READ_ADDR, register as u32 & 0xff)
            .await?;
        let pi_mode = match path {
            RfPath::A => self.query_bb_reg_async(0x0c00, BIT2).await? != 0,
            RfPath::B => self.query_bb_reg_async(0x0e00, BIT2).await? != 0,
            _ => false,
        };
        let readback = match (path, pi_mode) {
            (RfPath::A, true) => R_A_PI_READ,
            (RfPath::A, false) => R_A_SI_READ,
            (RfPath::B, true) => R_B_PI_READ,
            (RfPath::B, false) => R_B_SI_READ,
            _ => R_A_SI_READ,
        };
        let result = self.query_bb_reg_async(readback, R_READ_DATA).await;
        if toggle_cca {
            self.set_bb_reg_async(R_CCA_ON_SEC_JAGUAR, BIT3, 0).await?;
        }
        result
    }

    async fn rf_serial_write_async(
        &self,
        path: RfPath,
        register: u16,
        data: u32,
    ) -> Result<(), DriverError> {
        let lssi_write = match path {
            RfPath::A => 0x0c90,
            RfPath::B => 0x0e90,
            RfPath::C => 0x1890,
            RfPath::D => 0x1a90,
        };
        let data_and_addr = (((register as u32 & 0xff) << 20) | (data & 0x000f_ffff)) & 0x0fff_ffff;
        self.set_bb_reg_async(lssi_write, B_MASK_DWORD, data_and_addr)
            .await
    }
}

fn bb_swing_value(swing: u8, path_index: usize) -> u32 {
    match (swing >> (path_index * 2)) & 0x03 {
        0 => 0x200,
        1 => 0x16a,
        2 => 0x101,
        3 => 0x0b6,
        _ => 0x200,
    }
}

fn center_channel(channel: u8, width: ChannelWidth, offset: u8) -> u8 {
    match width {
        ChannelWidth::Mhz20 => channel,
        ChannelWidth::Mhz40 => center_channel_40(channel),
        ChannelWidth::Mhz80 => match channel {
            36 | 40 | 44 | 48 => 42,
            52 | 56 | 60 | 64 => 58,
            100 | 104 | 108 | 112 => 106,
            116 | 120 | 124 | 128 => 122,
            132 | 136 | 140 | 144 => 138,
            149 | 153 | 157 | 161 => 155,
            165 | 169 | 173 | 177 => 171,
            1..=14 => 7,
            _ => channel.saturating_add(offset),
        },
    }
}

fn center_channel_40(channel: u8) -> u8 {
    match channel {
        4 | 8 => 6,
        36 | 40 => 38,
        44 | 48 => 46,
        52 | 56 => 54,
        60 | 64 => 62,
        100 | 104 => 102,
        108 | 112 => 110,
        116 | 120 => 118,
        124 | 128 => 126,
        132 | 136 => 134,
        140 | 144 => 142,
        149 => 151,
        153 => 155,
        157 => 159,
        161 => 163,
        _ => channel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_40mhz_primary_channels_like_aviateur_devourer() {
        assert_eq!(center_channel(36, ChannelWidth::Mhz40, 0), 38);
        assert_eq!(center_channel(40, ChannelWidth::Mhz40, 0), 38);
        assert_eq!(center_channel(161, ChannelWidth::Mhz40, 0), 163);
        assert_eq!(center_channel(8, ChannelWidth::Mhz40, 0), 6);
    }

    #[test]
    fn maps_80mhz_primary_channels_like_aviateur_devourer() {
        assert_eq!(center_channel(36, ChannelWidth::Mhz80, 0), 42);
        assert_eq!(center_channel(120, ChannelWidth::Mhz80, 0), 122);
        assert_eq!(center_channel(161, ChannelWidth::Mhz80, 0), 155);
    }
}
