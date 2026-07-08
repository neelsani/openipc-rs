use crate::async_efuse::EfuseInfo;
use crate::device::RealtekDevice;
use crate::phy::{load_phy_table_async, phy_context, RfPath};
use crate::regs::*;
use crate::rtl_data;
use crate::time::{sleep_micros, sleep_ms};
use crate::types::{ChipFamily, ChipInfo, DriverError, RfType};

type ReferenceTable = &'static [u32];
type ReferenceRfTables = [Option<ReferenceTable>; 4];

fn mac_table_for(family: ChipFamily) -> Option<ReferenceTable> {
    match family {
        ChipFamily::Rtl8812 => Some(rtl_data::RTL8812_MAC_REG),
        ChipFamily::Rtl8814 => Some(rtl_data::RTL8814_MAC_REG),
        ChipFamily::Rtl8821 => Some(rtl_data::RTL8821_MAC_REG),
        ChipFamily::Rtl8822b => Some(rtl_data::RTL8822B_MAC_REG),
        ChipFamily::Rtl8821c => Some(rtl_data::RTL8821C_MAC_REG),
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => None,
    }
}

fn phy_tables_for(family: ChipFamily) -> Option<(ReferenceTable, ReferenceTable)> {
    match family {
        ChipFamily::Rtl8812 => Some((rtl_data::RTL8812_PHY_REG, rtl_data::RTL8812_AGC_TAB)),
        ChipFamily::Rtl8814 => Some((rtl_data::RTL8814_PHY_REG, rtl_data::RTL8814_AGC_TAB)),
        ChipFamily::Rtl8821 => Some((rtl_data::RTL8821_PHY_REG, rtl_data::RTL8821_AGC_TAB)),
        ChipFamily::Rtl8822b => Some((rtl_data::RTL8822B_PHY_REG, rtl_data::RTL8822B_AGC_TAB)),
        ChipFamily::Rtl8821c => Some((rtl_data::RTL8821C_PHY_REG, rtl_data::RTL8821C_AGC_TAB)),
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => None,
    }
}

fn rf_tables_for(family: ChipFamily) -> Option<ReferenceRfTables> {
    match family {
        ChipFamily::Rtl8812 => Some([
            Some(rtl_data::RTL8812_RADIO_A),
            Some(rtl_data::RTL8812_RADIO_B),
            None,
            None,
        ]),
        ChipFamily::Rtl8821 => Some([Some(rtl_data::RTL8821_RADIO_A), None, None, None]),
        ChipFamily::Rtl8814 => Some([
            Some(rtl_data::RTL8814_RADIO_A),
            Some(rtl_data::RTL8814_RADIO_B),
            Some(rtl_data::RTL8814_RADIO_C),
            Some(rtl_data::RTL8814_RADIO_D),
        ]),
        ChipFamily::Rtl8822b => Some([
            Some(rtl_data::RTL8822B_RADIO_A),
            Some(rtl_data::RTL8822B_RADIO_B),
            None,
            None,
        ]),
        ChipFamily::Rtl8821c => Some([Some(rtl_data::RTL8821C_RADIO_A), None, None, None]),
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => None,
    }
}

const fn crystal_cap_mask(family: ChipFamily) -> Option<u32> {
    match family {
        ChipFamily::Rtl8812 => Some(0x7ff8_0000),
        ChipFamily::Rtl8821 => Some(0x00ff_f000),
        ChipFamily::Rtl8814 => Some(0x07ff_8000),
        ChipFamily::Rtl8822b
        | ChipFamily::Rtl8821c
        | ChipFamily::Rtl8822c
        | ChipFamily::Rtl8822e => None,
    }
}

impl RealtekDevice {
    pub(crate) async fn load_mac_tables_async(
        &self,
        chip: ChipInfo,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let table =
            mac_table_for(chip.family).ok_or(DriverError::UnsupportedFirmwarePath(chip.family))?;
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
        let (phy, agc) =
            phy_tables_for(chip.family).ok_or(DriverError::UnsupportedFirmwarePath(chip.family))?;
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
        let tables =
            rf_tables_for(chip.family).ok_or(DriverError::UnsupportedFirmwarePath(chip.family))?;
        for (path, table) in [RfPath::A, RfPath::B, RfPath::C, RfPath::D]
            .into_iter()
            .zip(tables)
        {
            if path.index() >= chip.total_rf_paths() {
                break;
            }
            let Some(table) = table else { continue };
            load_phy_table_async(table, phy_context(chip, efuse), |addr, value| async move {
                self.config_rf_table_entry_async(chip, path, addr, value)
                    .await
            })
            .await?;
        }

        if chip.family == ChipFamily::Rtl8814 {
            // PHY_RFConfig8814A copies path A's RC-calibration word to
            // every other RF chain after all four radio tables load.
            let rck1 = self.query_rf_reg_async(chip, RfPath::A, 0x1c).await?;
            for path in [RfPath::B, RfPath::C, RfPath::D] {
                self.set_rf_reg_async(chip, path, 0x1c, B_LSSI_WRITE_DATA, rck1)
                    .await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn configure_single_tx_path_async(
        &self,
        chip: ChipInfo,
    ) -> Result<(), DriverError> {
        if chip.rf_type != RfType::OneTOneR {
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
        let crystal_cap = (efuse.crystal_cap & 0x3f) as u32;
        let Some(mask) = crystal_cap_mask(chip.family) else {
            return Ok(());
        };
        self.set_bb_reg_async(REG_MAC_PHY_CTRL, mask, crystal_cap | (crystal_cap << 6))
            .await
    }

    pub(crate) async fn config_bb_phy_async(
        &self,
        addr: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        match addr {
            0xfe => sleep_ms(50).await,
            0xfd => sleep_ms(5).await,
            0xfc => sleep_ms(1).await,
            0xfb => sleep_micros(50).await,
            0xfa => sleep_micros(5).await,
            0xf9 => sleep_micros(1).await,
            _ => {
                self.set_bb_reg_async(addr as u16, B_MASK_DWORD, value)
                    .await?;
                sleep_micros(1).await;
            }
        }
        Ok(())
    }

    async fn config_rf_table_entry_async(
        &self,
        chip: ChipInfo,
        path: RfPath,
        addr: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        if matches!(addr, 0xfe | 0xffe) {
            sleep_ms(50).await;
            return Ok(());
        }

        self.set_rf_reg_async(chip, path, addr as u16, B_LSSI_WRITE_DATA, value)
            .await?;
        sleep_micros(1).await;
        Ok(())
    }

    async fn config_bb_agc_async(&self, addr: u32, value: u32) -> Result<(), DriverError> {
        self.config_bb_phy_async(addr, value).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_dispatch_uses_each_familys_normal_payloads() {
        for (family, mac, phy, agc, radios) in [
            (ChipFamily::Rtl8812, 224, 470, 668, [864, 848, 0, 0]),
            (ChipFamily::Rtl8821, 196, 344, 504, [1_734, 0, 0, 0]),
            (
                ChipFamily::Rtl8814,
                286,
                4_622,
                6_280,
                [4_634, 4_396, 4_524, 4_600],
            ),
            (
                ChipFamily::Rtl8822b,
                250,
                2_984,
                21_368,
                [10_638, 9_234, 0, 0],
            ),
            (ChipFamily::Rtl8821c, 276, 3_356, 3_200, [5_424, 0, 0, 0]),
        ] {
            assert_eq!(mac_table_for(family).unwrap().len(), mac);
            let selected_phy = phy_tables_for(family).unwrap();
            assert_eq!(selected_phy.0.len(), phy);
            assert_eq!(selected_phy.1.len(), agc);
            assert_eq!(
                rf_tables_for(family)
                    .unwrap()
                    .map(|table| table.map_or(0, <[u32]>::len)),
                radios
            );
        }
        for family in [ChipFamily::Rtl8822c, ChipFamily::Rtl8822e] {
            assert!(mac_table_for(family).is_none());
            assert!(phy_tables_for(family).is_none());
            assert!(rf_tables_for(family).is_none());
        }
    }

    #[test]
    fn crystal_cap_masks_match_jaguar1_register_layouts() {
        assert_eq!(crystal_cap_mask(ChipFamily::Rtl8812), Some(0x7ff8_0000));
        assert_eq!(crystal_cap_mask(ChipFamily::Rtl8821), Some(0x00ff_f000));
        assert_eq!(crystal_cap_mask(ChipFamily::Rtl8814), Some(0x07ff_8000));
        assert_eq!(crystal_cap_mask(ChipFamily::Rtl8822b), None);
    }
}
