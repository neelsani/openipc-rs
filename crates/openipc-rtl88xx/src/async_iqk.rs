use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::types::{ChipFamily, ChipInfo, DriverError};

const RF_REG_MASK: u32 = 0x000f_ffff;

/// Report returned after an IQK calibration attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IqkReport {
    /// Chip information used for the IQK path.
    pub chip: ChipInfo,
    /// RF channel used for calibration.
    pub channel: u8,
    /// True when the selected chip path actually ran IQK.
    pub ran: bool,
}

impl RealtekDevice {
    /// Run IQK calibration for the selected chip/channel when supported.
    pub async fn run_iqk_async(
        &self,
        chip: ChipInfo,
        channel: u8,
    ) -> Result<IqkReport, DriverError> {
        match chip.family {
            ChipFamily::Rtl8812 => self.run_iqk_8812_async(chip, channel).await,
            ChipFamily::Rtl8814 => self.run_iqk_8814_async(chip, channel).await,
            ChipFamily::Rtl8821 => Err(DriverError::UnsupportedIqkPath(chip.family)),
        }
    }

    async fn run_iqk_8814_async(
        &self,
        chip: ChipInfo,
        channel: u8,
    ) -> Result<IqkReport, DriverError> {
        let mac_regs = [0x0520u16, 0x0550];
        let bb_regs = [
            0x0a14u16, 0x0808, 0x0838, 0x090c, 0x0810, 0x0cb0, 0x0eb0, 0x18b4, 0x1ab4, 0x1abc,
            0x09a4, 0x0764, 0x0cbc,
        ];
        let rf_regs = [0x00u16, 0x8f];

        let mut mac_backup = [0u32; 2];
        for (idx, register) in mac_regs.into_iter().enumerate() {
            mac_backup[idx] = self.read_u32_async(register).await?;
        }
        let mut bb_backup = [0u32; 13];
        for (idx, register) in bb_regs.into_iter().enumerate() {
            bb_backup[idx] = self.read_u32_async(register).await?;
        }

        self.afe_setting_8814_async(true).await?;
        let mut rf_backup = [[0u32; 4]; 2];
        for (reg_idx, register) in rf_regs.into_iter().enumerate() {
            for path in RfPath::iter(4) {
                rf_backup[reg_idx][path.index()] =
                    self.query_rf_reg_async(chip, path, register).await?;
            }
        }

        self.configure_mac_iqk_8814_async().await?;
        self.iqk_tx_8814_async(chip, channel).await?;
        self.reset_nctl_8814_async().await?;
        self.afe_setting_8814_async(false).await?;

        for (idx, register) in mac_regs.into_iter().enumerate() {
            self.write_u32_async(register, mac_backup[idx]).await?;
        }
        for (idx, register) in bb_regs.into_iter().enumerate() {
            self.write_u32_async(register, bb_backup[idx]).await?;
        }

        for path in RfPath::iter(4) {
            self.set_rf_reg_async(chip, path, 0xef, RF_REG_MASK, 0)
                .await?;
        }
        for (reg_idx, register) in rf_regs.into_iter().enumerate() {
            for path in RfPath::iter(4) {
                self.set_rf_reg_async(
                    chip,
                    path,
                    register,
                    RF_REG_MASK,
                    rf_backup[reg_idx][path.index()],
                )
                .await?;
            }
        }

        Ok(IqkReport {
            chip,
            channel,
            ran: true,
        })
    }

    async fn afe_setting_8814_async(&self, do_iqk: bool) -> Result<(), DriverError> {
        let afe = if do_iqk { 0x0e80_8003 } else { 0x0780_8003 };
        for register in [0x0c60u16, 0x0e60, 0x1860, 0x1a60] {
            self.write_u32_async(register, afe).await?;
        }
        self.set_bb_reg_async(0x090c, bit(13), 1).await?;
        self.set_bb_reg_async(0x0764, bit(10) | bit(9), 3).await?;
        self.set_bb_reg_async(0x0764, bit(10) | bit(9), 0).await?;
        self.set_bb_reg_async(0x0804, bit(2), 1).await?;
        self.set_bb_reg_async(0x0804, bit(2), 0).await
    }

    async fn configure_mac_iqk_8814_async(&self) -> Result<(), DriverError> {
        self.write_u8_async(0x0522, 0x3f).await?;
        self.set_bb_reg_async(0x0550, bit(11) | bit(3), 0).await?;
        self.write_u8_async(0x0808, 0).await?;
        self.set_bb_reg_async(0x0838, 0x0f, 0x0e).await?;
        self.set_bb_reg_async(0x0a14, bit(9) | bit(8), 3).await?;
        for register in [0x0cb0u16, 0x0eb0, 0x18b4, 0x1ab4] {
            self.write_u32_async(register, 0x7777_7777).await?;
        }
        self.set_bb_reg_async(0x1abc, 0x0ff0_0000, 0x77).await?;
        self.set_bb_reg_async(0x0cbc, 0x0f, 0).await
    }

    async fn reset_nctl_8814_async(&self) -> Result<(), DriverError> {
        self.write_u32_async(0x1b00, 0xf800_0000).await?;
        self.write_u32_async(0x1b80, 0x0000_0006).await?;
        self.write_u32_async(0x1b00, 0xf800_0000).await?;
        self.write_u32_async(0x1b80, 0x0000_0002).await
    }

    async fn iqk_tx_8814_async(&self, chip: ChipInfo, channel: u8) -> Result<(), DriverError> {
        for path in RfPath::iter(4) {
            self.set_rf_reg_async(chip, path, 0x58, bit(19), 1).await?;
        }
        for register in [0x0c94u16, 0x0e94, 0x1894, 0x1a94] {
            self.set_bb_reg_async(register, bit(11) | bit(10) | bit(0), 0x401)
                .await?;
        }

        let band_5g = channel > 14;
        self.write_u32_async(0x1b00, if band_5g { 0xf800_0ff1 } else { 0xf800_0ef1 })
            .await?;
        crate::time::sleep_ms(1).await;

        self.write_u32_async(0x0810, 0x2010_1063).await?;
        self.write_u32_async(0x090c, 0x0b00_c000).await?;
        self.lok_one_shot_8814_async(chip).await?;
        self.iqk_one_shot_8814_async(chip).await
    }

    async fn lok_one_shot_8814_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        for path_index in 0..4 {
            let path = RfPath::from_index(path_index).expect("path index is bounded");
            self.set_bb_reg_async(0x09a4, bit(21) | bit(20), path_index as u32)
                .await?;
            self.write_u32_async(0x1b00, 0xf800_0001 | (1u32 << (4 + path_index)))
                .await?;
            crate::time::sleep_ms(1).await;

            let mut not_ready = true;
            for _ in 0..10 {
                not_ready = self.query_bb_reg_async(0x1b00, bit(0)).await? != 0;
                if !not_ready {
                    break;
                }
                crate::time::sleep_ms(1).await;
            }

            if not_ready {
                self.reset_nctl_8814_async().await?;
                self.set_rf_reg_async(chip, path, 0x08, RF_REG_MASK, 0x08400)
                    .await?;
                continue;
            }

            self.write_u32_async(0x1b00, 0xf800_0000 | ((path_index as u32) << 1))
                .await?;
            self.write_u32_async(0x1bd4, 0x003f_0001).await?;
            let mut lok_temp2 = (self.query_bb_reg_async(0x1bfc, 0x003e_0000).await? + 0x10) & 0x1f;
            let mut lok_temp1 = (self.query_bb_reg_async(0x1bfc, 0x0000_003e).await? + 0x10) & 0x1f;
            for ii in 1..5 {
                lok_temp1 += (lok_temp1 & bit(4 - ii)) << (ii * 2);
                lok_temp2 += (lok_temp2 & bit(4 - ii)) << (ii * 2);
            }
            self.set_rf_reg_async(chip, path, 0x08, 0x07c00, lok_temp1 >> 4)
                .await?;
            self.set_rf_reg_async(chip, path, 0x08, 0xf8000, lok_temp2 >> 4)
                .await?;
        }
        Ok(())
    }

    async fn iqk_one_shot_8814_async(&self, _chip: ChipInfo) -> Result<(), DriverError> {
        const IQK_APPLY: [u16; 4] = [0x0c94, 0x0e94, 0x1894, 0x1a94];
        for idx in 0..=1 {
            for (path, apply_register) in IQK_APPLY.into_iter().enumerate() {
                let mut fail = true;
                let command_base = if idx == 0 { 3u32 } else { 9u32 };
                for _ in 0..=3 {
                    self.set_bb_reg_async(0x09a4, bit(21) | bit(20), path as u32)
                        .await?;
                    self.write_u32_async(
                        0x1b00,
                        0xf800_0001 | (command_base << 8) | (1u32 << (4 + path)),
                    )
                    .await?;
                    crate::time::sleep_ms(10).await;

                    for _ in 0..20 {
                        if self.query_bb_reg_async(0x1b00, bit(0)).await? == 0 {
                            fail = self.query_bb_reg_async(0x1b08, bit(26)).await? != 0;
                            break;
                        }
                        crate::time::sleep_ms(1).await;
                    }
                    if !fail {
                        break;
                    }
                    self.reset_nctl_8814_async().await?;
                }

                self.write_u32_async(0x1b00, 0xf800_0000 | ((path as u32) << 1))
                    .await?;
                if !fail {
                    if idx == 0 {
                        let _ = self.read_u32_async(0x1b38).await?;
                    } else {
                        self.write_u32_async(0x1b3c, 0x2000_0000).await?;
                        let _ = self.read_u32_async(0x1b3c).await?;
                    }
                } else if idx == 1 {
                    self.set_bb_reg_async(apply_register, bit(11) | bit(10), 0)
                        .await?;
                }
            }
        }
        Ok(())
    }
}

const fn bit(n: u32) -> u32 {
    1u32 << n
}
