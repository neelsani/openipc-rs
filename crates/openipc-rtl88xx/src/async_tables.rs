use crate::async_efuse::EfuseInfo;
use crate::device::RealtekDevice;
use crate::phy::{load_phy_table_async, phy_context, RfPath};
use crate::regs::*;
use crate::rtl_data;
use crate::time::{sleep_micros, sleep_ms};
use crate::types::{ChipFamily, ChipInfo, DriverError, RfType};

impl RealtekDevice {
    pub(crate) async fn load_mac_tables_async(
        &self,
        chip: ChipInfo,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let table = match chip.family {
            ChipFamily::Rtl8812 => rtl_data::RTL8812_MAC_REG,
            ChipFamily::Rtl8814 => rtl_data::RTL8814_MAC_REG,
            ChipFamily::Rtl8821 => rtl_data::RTL8821_MAC_REG,
        };
        load_phy_table_async(table, phy_context(chip, efuse), |addr, value| async move {
            self.write_u8_async(addr as u16, value as u8).await
        })
        .await
    }

    pub(crate) async fn load_phy_tables_async(
        &self,
        chip: ChipInfo,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let (phy, agc) = match chip.family {
            ChipFamily::Rtl8812 => (rtl_data::RTL8812_PHY_REG, rtl_data::RTL8812_AGC_TAB),
            ChipFamily::Rtl8814 => (rtl_data::RTL8814_PHY_REG, rtl_data::RTL8814_AGC_TAB),
            ChipFamily::Rtl8821 => (rtl_data::RTL8821_PHY_REG, rtl_data::RTL8821_AGC_TAB),
        };
        load_phy_table_async(phy, phy_context(chip, efuse), |addr, value| async move {
            self.config_bb_phy_async(addr, value).await
        })
        .await?;
        load_phy_table_async(agc, phy_context(chip, efuse), |addr, value| async move {
            self.config_bb_agc_async(addr, value).await
        })
        .await?;
        self.configure_crystal_cap_async(chip, efuse).await
    }

    pub(crate) async fn load_rf_tables_async(
        &self,
        chip: ChipInfo,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        match chip.family {
            ChipFamily::Rtl8812 => {
                load_phy_table_async(
                    rtl_data::RTL8812_RADIO_A,
                    phy_context(chip, efuse),
                    |addr, value| async move {
                        self.set_rf_reg_async(
                            chip,
                            RfPath::A,
                            addr as u16,
                            B_LSSI_WRITE_DATA,
                            value,
                        )
                        .await
                    },
                )
                .await?;
                if chip.total_rf_paths() >= 2 {
                    load_phy_table_async(
                        rtl_data::RTL8812_RADIO_B,
                        phy_context(chip, efuse),
                        |addr, value| async move {
                            self.set_rf_reg_async(
                                chip,
                                RfPath::B,
                                addr as u16,
                                B_LSSI_WRITE_DATA,
                                value,
                            )
                            .await
                        },
                    )
                    .await?;
                }
            }
            ChipFamily::Rtl8821 => {
                load_phy_table_async(
                    rtl_data::RTL8821_RADIO_A,
                    phy_context(chip, efuse),
                    |addr, value| async move {
                        self.set_rf_reg_async(
                            chip,
                            RfPath::A,
                            addr as u16,
                            B_LSSI_WRITE_DATA,
                            value,
                        )
                        .await
                    },
                )
                .await?;
            }
            ChipFamily::Rtl8814 => {
                for (path, table) in [
                    (RfPath::A, rtl_data::RTL8814_RADIO_A),
                    (RfPath::B, rtl_data::RTL8814_RADIO_B),
                    (RfPath::C, rtl_data::RTL8814_RADIO_C),
                    (RfPath::D, rtl_data::RTL8814_RADIO_D),
                ] {
                    load_phy_table_async(
                        table,
                        phy_context(chip, efuse),
                        |addr, value| async move {
                            self.set_rf_reg_async(chip, path, addr as u16, B_LSSI_WRITE_DATA, value)
                                .await
                        },
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn configure_single_tx_path_async(
        &self,
        chip: ChipInfo,
    ) -> Result<(), DriverError> {
        if chip.family != ChipFamily::Rtl8812 || chip.rf_type != RfType::OneTOneR {
            return Ok(());
        }

        self.set_bb_reg_async(R_OFDMCCKEN_JAGUAR, B_MASK_BYTE0, 0x11)
            .await?;
        self.set_bb_reg_async(R_TX_PATH_JAGUAR, 0x0000_ffff, 0x1111)
            .await?;
        self.set_bb_reg_async(R_CCK_RX_JAGUAR, 0x0c00_0000, 0)
            .await?;
        self.set_bb_reg_async(0x08bc, 0xc000_0060, 0x4).await?;
        self.set_bb_reg_async(0x0e00, 0x0f, 0x4).await?;
        self.set_bb_reg_async(0x0e90, B_MASK_DWORD, 0).await?;
        self.set_bb_reg_async(0x0e60, B_MASK_DWORD, 0).await?;
        self.set_bb_reg_async(0x0e64, B_MASK_DWORD, 0).await
    }

    async fn configure_crystal_cap_async(
        &self,
        chip: ChipInfo,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        if chip.family != ChipFamily::Rtl8812 {
            return Ok(());
        }
        let crystal_cap = (efuse.crystal_cap & 0x3f) as u32;
        self.set_bb_reg_async(
            REG_MAC_PHY_CTRL,
            0x7ff8_0000,
            crystal_cap | (crystal_cap << 6),
        )
        .await
    }

    async fn config_bb_phy_async(&self, addr: u32, value: u32) -> Result<(), DriverError> {
        match addr {
            0xfe => sleep_ms(50).await,
            0xfd => sleep_ms(5).await,
            0xfc => sleep_ms(1).await,
            0xfb => sleep_micros(50).await,
            0xfa => sleep_micros(5).await,
            0xf9 => sleep_micros(1).await,
            _ => {
                self.set_bb_reg_async(addr as u16, B_MASK_DWORD, value)
                    .await?
            }
        }
        Ok(())
    }

    async fn config_bb_agc_async(&self, addr: u32, value: u32) -> Result<(), DriverError> {
        self.config_bb_phy_async(addr, value).await
    }
}
