use crate::device::RealtekDevice;
use crate::phy::{load_phy_table_async, load_plain_pairs_async, phy_context, RfPath};
use crate::regs::*;
use crate::rtl_data;
use crate::time::{sleep_micros, sleep_ms};
use crate::types::{ChipFamily, ChipInfo, DriverError};

impl RealtekDevice {
    pub(crate) async fn load_mac_tables_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        let table = match chip.family {
            ChipFamily::Rtl8812 => rtl_data::RTL8812_MAC_REG,
            ChipFamily::Rtl8814 => rtl_data::RTL8814_MAC_REG,
            ChipFamily::Rtl8821 => rtl_data::RTL8821_MAC_REG,
        };
        load_phy_table_async(table, phy_context(chip), |addr, value| async move {
            self.write_u8_async(addr as u16, value as u8).await
        })
        .await
    }

    pub(crate) async fn load_phy_tables_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        let (phy, agc) = match chip.family {
            ChipFamily::Rtl8812 => (rtl_data::RTL8812_PHY_REG, rtl_data::RTL8812_AGC_TAB),
            ChipFamily::Rtl8814 => (rtl_data::RTL8814_PHY_REG, rtl_data::RTL8814_AGC_TAB),
            ChipFamily::Rtl8821 => (rtl_data::RTL8821_PHY_REG, rtl_data::RTL8821_AGC_TAB),
        };
        load_phy_table_async(phy, phy_context(chip), |addr, value| async move {
            self.config_bb_phy_async(addr, value).await
        })
        .await?;
        load_phy_table_async(agc, phy_context(chip), |addr, value| async move {
            self.config_bb_agc_async(addr, value).await
        })
        .await
    }

    pub(crate) async fn load_rf_tables_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        match chip.family {
            ChipFamily::Rtl8812 => {
                load_plain_pairs_async(rtl_data::RTL8812_RADIO_A, |addr, value| async move {
                    self.set_rf_reg_async(chip, RfPath::A, addr as u16, B_LSSI_WRITE_DATA, value)
                        .await
                })
                .await?;
                if chip.total_rf_paths() >= 2 {
                    load_plain_pairs_async(rtl_data::RTL8812_RADIO_B, |addr, value| async move {
                        self.set_rf_reg_async(
                            chip,
                            RfPath::B,
                            addr as u16,
                            B_LSSI_WRITE_DATA,
                            value,
                        )
                        .await
                    })
                    .await?;
                }
            }
            ChipFamily::Rtl8821 => {
                load_plain_pairs_async(rtl_data::RTL8821_RADIO_A, |addr, value| async move {
                    self.set_rf_reg_async(chip, RfPath::A, addr as u16, B_LSSI_WRITE_DATA, value)
                        .await
                })
                .await?;
            }
            ChipFamily::Rtl8814 => {
                for (path, table) in [
                    (RfPath::A, rtl_data::RTL8814_RADIO_A),
                    (RfPath::B, rtl_data::RTL8814_RADIO_B),
                    (RfPath::C, rtl_data::RTL8814_RADIO_C),
                    (RfPath::D, rtl_data::RTL8814_RADIO_D),
                ] {
                    load_plain_pairs_async(table, |addr, value| async move {
                        self.set_rf_reg_async(chip, path, addr as u16, B_LSSI_WRITE_DATA, value)
                            .await
                    })
                    .await?;
                }
            }
        }
        Ok(())
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
