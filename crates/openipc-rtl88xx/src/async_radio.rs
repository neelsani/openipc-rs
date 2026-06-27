use crate::device::RealtekDevice;
use crate::phy::{bit_shift, RfPath};
use crate::regs::*;
use crate::types::{ChannelWidth, ChipFamily, ChipInfo, DriverError, RadioConfig};

impl RealtekDevice {
    pub(crate) async fn set_monitor_mode_async(
        &self,
        accept_bad_fcs: bool,
    ) -> Result<(), DriverError> {
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
        self.write_u16_async(REG_RXFLTMAP2, 0xffff).await?;
        let rxfltmap1 = self.read_u16_async(REG_RXFLTMAP1).await.unwrap_or(0);
        self.write_u16_async(REG_RXFLTMAP1, rxfltmap1 | BIT8 as u16)
            .await
    }

    pub(crate) async fn set_channel_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
    ) -> Result<(), DriverError> {
        self.set_channel_number_async(chip, radio.channel).await?;
        self.set_bandwidth_async(chip, radio.channel_width, radio.channel_offset)
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

    async fn query_bb_reg_async(&self, register: u16, mask: u32) -> Result<u32, DriverError> {
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

    async fn query_rf_reg_async(
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
        self.query_bb_reg_async(readback, R_READ_DATA).await
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
