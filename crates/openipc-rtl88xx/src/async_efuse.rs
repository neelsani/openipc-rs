use crate::device::RealtekDevice;
use crate::regs::*;
use crate::rtl_data::{
    TxPowerBand, TxPowerLimitBandwidth, TxPowerLimitRateSection, TxPowerRegulation,
    RTL8812A_PHY_REG_PG, RTL8812A_TX_POWER_LIMITS,
};
use crate::tx_power_defaults;
use crate::types::{ChannelWidth, ChipFamily, ChipInfo, DriverError};

const EFUSE_ACCESS_ON_JAGUAR: u8 = 0x69;
const EFUSE_ACCESS_OFF_JAGUAR: u8 = 0x00;
const EFUSE_MAP_LEN_JAGUAR: usize = 512;
const EFUSE_MAX_SECTION_JAGUAR: usize = 64;
const EFUSE_MAX_WORD_UNIT_JAGUAR: usize = 4;
const EFUSE_REAL_CONTENT_LEN_JAGUAR: u16 = 512;
const EFUSE_REAL_CONTENT_LEN_8822E: u16 = 1024;

const EEPROM_MAC_ADDR_8812AU: usize = 0x0d7;
const EEPROM_MAC_ADDR_8814AU: usize = 0x0d8;
const EEPROM_MAC_ADDR_8821AU: usize = 0x107;
const EEPROM_PA_TYPE_8812AU: usize = 0x0bc;
const EEPROM_LNA_TYPE_2G_8812AU: usize = 0x0bd;
const EEPROM_LNA_TYPE_5G_8812AU: usize = 0x0bf;
const EEPROM_RF_BOARD_OPTION_8812: usize = 0x0c1;
const EEPROM_TX_BBSWING_2G_8812: usize = 0x0c6;
const EEPROM_TX_BBSWING_5G_8812: usize = 0x0c7;
const EEPROM_RFE_OPTION_8812: usize = 0x0ca;
const EEPROM_THERMAL_METER_8812: usize = 0x0ba;
const EEPROM_THERMAL_METER_A_8822E: usize = 0x0d0;
const EEPROM_THERMAL_METER_B_8822E: usize = 0x0d1;
const EEPROM_XTAL_8812: usize = 0x0b9;
const EEPROM_DEFAULT_CRYSTAL_CAP_8812: u8 = 0x20;
const TX_POWER_PG_OFFSET: usize = 0x10;
const MAX_RF_PATHS: usize = 4;
const MAX_TX_COUNT: usize = 4;
const CENTER_CH_2G_NUM: usize = 14;
const CENTER_CH_5G_ALL_NUM: usize = 65;
const MAX_BANDS: usize = 2;
const NUM_RATE_IDX: usize = 84;
const NUM_RATE_SECTION: usize = 10;
const TX_POWER_BOOST: i16 = 2;
const CENTER_CH_5G_ALL: [u8; CENTER_CH_5G_ALL_NUM] = [
    15, 16, 17, 18, 20, 24, 28, 32, 36, 38, 40, 42, 44, 46, 48, 52, 54, 56, 58, 60, 62, 64, 68, 72,
    76, 80, 84, 88, 92, 96, 100, 102, 104, 106, 108, 110, 112, 116, 118, 120, 122, 124, 126, 128,
    132, 134, 136, 138, 140, 142, 144, 149, 151, 153, 155, 157, 159, 161, 165, 167, 169, 171, 173,
    175, 177,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TxPowerInfo {
    pub(crate) loaded: bool,
    pub(crate) tx_power_by_rate_loaded: bool,
    pub(crate) index24g_cck_base: [[u8; CENTER_CH_2G_NUM]; MAX_RF_PATHS],
    pub(crate) index24g_bw40_base: [[u8; CENTER_CH_2G_NUM]; MAX_RF_PATHS],
    pub(crate) index5g_bw40_base: [[u8; CENTER_CH_5G_ALL_NUM]; MAX_RF_PATHS],
    pub(crate) cck_24g_diff: [[i8; MAX_TX_COUNT]; MAX_RF_PATHS],
    pub(crate) ofdm_24g_diff: [[i8; MAX_TX_COUNT]; MAX_RF_PATHS],
    pub(crate) bw20_24g_diff: [[i8; MAX_TX_COUNT]; MAX_RF_PATHS],
    pub(crate) bw40_24g_diff: [[i8; MAX_TX_COUNT]; MAX_RF_PATHS],
    pub(crate) ofdm_5g_diff: [[i8; MAX_TX_COUNT]; MAX_RF_PATHS],
    pub(crate) bw20_5g_diff: [[i8; MAX_TX_COUNT]; MAX_RF_PATHS],
    pub(crate) bw40_5g_diff: [[i8; MAX_TX_COUNT]; MAX_RF_PATHS],
    pub(crate) bw80_5g_diff: [[i8; MAX_TX_COUNT]; MAX_RF_PATHS],
    pub(crate) tx_power_by_rate_offset: [[[i8; NUM_RATE_IDX]; MAX_RF_PATHS]; MAX_BANDS],
    pub(crate) tx_power_by_rate_base: [[[u8; NUM_RATE_SECTION]; MAX_RF_PATHS]; MAX_BANDS],
}

impl Default for TxPowerInfo {
    fn default() -> Self {
        Self {
            loaded: false,
            tx_power_by_rate_loaded: false,
            index24g_cck_base: [[0; CENTER_CH_2G_NUM]; MAX_RF_PATHS],
            index24g_bw40_base: [[0; CENTER_CH_2G_NUM]; MAX_RF_PATHS],
            index5g_bw40_base: [[0; CENTER_CH_5G_ALL_NUM]; MAX_RF_PATHS],
            cck_24g_diff: [[0; MAX_TX_COUNT]; MAX_RF_PATHS],
            ofdm_24g_diff: [[0; MAX_TX_COUNT]; MAX_RF_PATHS],
            bw20_24g_diff: [[0; MAX_TX_COUNT]; MAX_RF_PATHS],
            bw40_24g_diff: [[0; MAX_TX_COUNT]; MAX_RF_PATHS],
            ofdm_5g_diff: [[0; MAX_TX_COUNT]; MAX_RF_PATHS],
            bw20_5g_diff: [[0; MAX_TX_COUNT]; MAX_RF_PATHS],
            bw40_5g_diff: [[0; MAX_TX_COUNT]; MAX_RF_PATHS],
            bw80_5g_diff: [[0; MAX_TX_COUNT]; MAX_RF_PATHS],
            tx_power_by_rate_offset: [[[0; NUM_RATE_IDX]; MAX_RF_PATHS]; MAX_BANDS],
            tx_power_by_rate_base: [[[0; NUM_RATE_SECTION]; MAX_RF_PATHS]; MAX_BANDS],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EfuseInfo {
    pub(crate) mac: Option<[u8; 6]>,
    pub(crate) rfe_type: u8,
    pub(crate) external_pa_2g: bool,
    pub(crate) external_pa_5g: bool,
    pub(crate) external_lna_2g: bool,
    pub(crate) board_type: u8,
    pub(crate) type_gpa: u16,
    pub(crate) type_apa: u16,
    pub(crate) type_glna: u16,
    pub(crate) type_alna: u16,
    pub(crate) tx_bb_swing_2g: u8,
    pub(crate) tx_bb_swing_5g: u8,
    pub(crate) crystal_cap: u8,
    pub(crate) thermal_meter: u8,
    pub(crate) thermal_meter_paths: [u8; 2],
    pub(crate) tx_power: TxPowerInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AmplifierFlags {
    external_pa_2g: bool,
    external_pa_5g: bool,
    external_lna_2g: bool,
    external_lna_5g: bool,
    type_gpa: u16,
    type_apa: u16,
    type_glna: u16,
    type_alna: u16,
}

impl RealtekDevice {
    pub(crate) async fn read_efuse_info_async(
        &self,
        chip: ChipInfo,
    ) -> Result<EfuseInfo, DriverError> {
        let map = self.read_efuse_logical_map_async(chip).await?;
        let amplifiers = amplifier_flags_from_efuse_map(&map);
        let bluetooth_coexist = bluetooth_coexist_from_efuse_map(&map);
        Ok(EfuseInfo {
            mac: mac_from_efuse_map(chip, &map),
            rfe_type: rfe_type_from_efuse_map(chip, &map, amplifiers),
            external_pa_2g: amplifiers.external_pa_2g,
            external_pa_5g: amplifiers.external_pa_5g,
            external_lna_2g: amplifiers.external_lna_2g,
            board_type: board_type_from_amplifiers(amplifiers, bluetooth_coexist),
            type_gpa: amplifiers.type_gpa,
            type_apa: amplifiers.type_apa,
            type_glna: amplifiers.type_glna,
            type_alna: amplifiers.type_alna,
            tx_bb_swing_2g: efuse_or_zero(map[EEPROM_TX_BBSWING_2G_8812]),
            tx_bb_swing_5g: efuse_or_zero(map[EEPROM_TX_BBSWING_5G_8812]),
            crystal_cap: crystal_cap_from_efuse_map(&map),
            thermal_meter: thermal_meter_from_efuse_map(chip, &map),
            thermal_meter_paths: thermal_meter_paths_from_efuse_map(chip, &map),
            tx_power: tx_power_info_from_efuse_map(chip, &map),
        })
    }

    async fn read_efuse_logical_map_async(
        &self,
        chip: ChipInfo,
    ) -> Result<[u8; EFUSE_MAP_LEN_JAGUAR], DriverError> {
        if chip.family == ChipFamily::Rtl8822e {
            let map = self.read_efuse_logical_map_8822e_async().await?;
            let _ = self.jaguar3_efuse.set(map);
            return Ok(map);
        }
        self.read_efuse_autoload_probe_bytes_async().await?;
        self.efuse_power_switch_async(false, true).await?;
        let result = self.read_efuse_logical_map_powered_async().await;
        let power_off = self.efuse_power_switch_async(false, false).await;
        match (result, power_off) {
            (Ok(map), Ok(())) => Ok(map),
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err),
        }
    }

    pub(crate) async fn efuse_power_cut_8822e_async(
        &self,
        enabled: bool,
    ) -> Result<(), DriverError> {
        const REG_PMC_DBG_CTRL2: u16 = 0x00cc;
        const REG_EFUSE_CTRL_1: u16 = 0x00a4;
        const BIT_EF_BURST: u32 = 1 << 19;
        const EB2CORE: u16 = 1 << 8;
        const PWC_EV2EF_S: u16 = 1 << 14;
        const PWC_EV2EF_B: u16 = 1 << 15;
        const PMC_WRITE_MASK: u8 = 1 << 2;

        if enabled {
            let pmc = self.read_u8_async(REG_PMC_DBG_CTRL2).await.unwrap_or(0);
            self.write_u8_async(REG_PMC_DBG_CTRL2, pmc | PMC_WRITE_MASK)
                .await?;
            let iso = self.read_u16_async(REG_SYS_ISO_CTRL).await.unwrap_or(0);
            self.write_u16_async(REG_SYS_ISO_CTRL, iso | PWC_EV2EF_S)
                .await?;
            crate::time::sleep_ms(1).await;
            let iso = self.read_u16_async(REG_SYS_ISO_CTRL).await.unwrap_or(0);
            self.write_u16_async(REG_SYS_ISO_CTRL, iso | PWC_EV2EF_B)
                .await?;
            let iso = self.read_u16_async(REG_SYS_ISO_CTRL).await.unwrap_or(0);
            self.write_u16_async(REG_SYS_ISO_CTRL, iso & !EB2CORE)
                .await?;
            let burst = self.read_u32_async(REG_EFUSE_CTRL_1).await.unwrap_or(0);
            self.write_u32_async(REG_EFUSE_CTRL_1, burst | BIT_EF_BURST)
                .await?;
        } else {
            let burst = self.read_u32_async(REG_EFUSE_CTRL_1).await.unwrap_or(0);
            self.write_u32_async(REG_EFUSE_CTRL_1, burst & !BIT_EF_BURST)
                .await?;
            let iso = self.read_u16_async(REG_SYS_ISO_CTRL).await.unwrap_or(0);
            self.write_u16_async(REG_SYS_ISO_CTRL, iso | EB2CORE)
                .await?;
            let iso = self.read_u16_async(REG_SYS_ISO_CTRL).await.unwrap_or(0);
            self.write_u16_async(REG_SYS_ISO_CTRL, iso & !PWC_EV2EF_B)
                .await?;
            crate::time::sleep_ms(1).await;
            let iso = self.read_u16_async(REG_SYS_ISO_CTRL).await.unwrap_or(0);
            self.write_u16_async(REG_SYS_ISO_CTRL, iso & !PWC_EV2EF_S)
                .await?;
            let pmc = self.read_u8_async(REG_PMC_DBG_CTRL2).await.unwrap_or(0);
            self.write_u8_async(REG_PMC_DBG_CTRL2, pmc & !PMC_WRITE_MASK)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn read_efuse_byte_8822e_async(
        &self,
        address: u16,
    ) -> Result<u8, DriverError> {
        const BIT_EF_READY: u32 = 1 << 29;
        const EF_ADDR_MASK: u32 = 0x7ff << 16;
        const EF_DATA_MASK: u32 = 0xffff;

        let current = self.read_u32_async(REG_EFUSE_CTRL).await.unwrap_or(0);
        let trigger = (current & !(EF_ADDR_MASK | EF_DATA_MASK | BIT_EF_READY))
            | (u32::from(address & 0x7ff) << 16);
        self.write_u32_async(REG_EFUSE_CTRL, trigger).await?;
        for _ in 0..1000 {
            crate::time::sleep_micros(50).await;
            let value = self.read_u32_async(REG_EFUSE_CTRL).await.unwrap_or(0);
            if value & BIT_EF_READY != 0 {
                return Ok(value as u8);
            }
        }
        Ok(0xff)
    }

    async fn read_efuse_logical_map_8822e_async(
        &self,
    ) -> Result<[u8; EFUSE_MAP_LEN_JAGUAR], DriverError> {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        self.efuse_power_cut_8822e_async(true).await?;
        let result = async {
            let mut physical = 0u16;
            let mut ff_run = 0u8;
            while physical < EFUSE_REAL_CONTENT_LEN_8822E {
                let header0 = self.read_efuse_byte_8822e_async(physical).await?;
                physical += 1;
                if header0 == 0xff {
                    ff_run = ff_run.saturating_add(1);
                    if ff_run >= 64 {
                        break;
                    }
                    continue;
                }
                ff_run = 0;
                if header0 & 0xf0 != 0x30 || physical >= EFUSE_REAL_CONTENT_LEN_8822E {
                    break;
                }
                let header1 = self.read_efuse_byte_8822e_async(physical).await?;
                physical += 1;
                let word_enable = header1 & 0x0f;
                let data_len = (word_enable.count_zeros() - 4) as usize * 2;
                let mut data = [0xff; 8];
                let mut actual = 0usize;
                while actual < data_len && physical < EFUSE_REAL_CONTENT_LEN_8822E {
                    data[actual] = self.read_efuse_byte_8822e_async(physical).await?;
                    physical += 1;
                    actual += 1;
                }
                let _ = apply_efuse_block_8822e(&mut map, header0, header1, &data[..actual]);
            }
            Ok::<_, DriverError>(map)
        }
        .await;
        let power_off = self.efuse_power_cut_8822e_async(false).await;
        match (result, power_off) {
            (Ok(map), Ok(())) => Ok(map),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        }
    }

    async fn read_efuse_autoload_probe_bytes_async(&self) -> Result<(), DriverError> {
        for address in [0x0200, 0x0202, 0x0204, 0x0210] {
            let _ = self.read_efuse_probe_byte_async(address).await?;
        }
        Ok(())
    }

    async fn efuse_power_switch_async(
        &self,
        write: bool,
        power_on: bool,
    ) -> Result<(), DriverError> {
        if power_on {
            self.write_u8_async(REG_EFUSE_BURN_GNT_8812, EFUSE_ACCESS_ON_JAGUAR)
                .await?;

            let sys_iso = self.read_u16_async(REG_SYS_ISO_CTRL).await.unwrap_or(0);
            let _ev12v_missing = sys_iso & PWC_EV12V != PWC_EV12V;

            let mut sys_func = self.read_u16_async(REG_SYS_FUNC_EN).await.unwrap_or(0);
            if sys_func & FEN_ELDR != FEN_ELDR {
                sys_func |= FEN_ELDR;
                self.write_u16_async(REG_SYS_FUNC_EN, sys_func).await?;
            }

            let mut sys_clkr = self.read_u16_async(REG_SYS_CLKR).await.unwrap_or(0);
            if sys_clkr & LOADER_CLK_EN != LOADER_CLK_EN || sys_clkr & ANA8M != ANA8M {
                sys_clkr |= LOADER_CLK_EN | ANA8M;
                self.write_u16_async(REG_SYS_CLKR, sys_clkr).await?;
            }

            if write {
                let mut test = self.read_u8_async(REG_EFUSE_TEST + 3).await.unwrap_or(0);
                test &= 0x87;
                test |= 0x03 << 3;
                test |= BIT7 as u8;
                self.write_u8_async(REG_EFUSE_TEST + 3, test).await?;
            }
        } else {
            self.write_u8_async(REG_EFUSE_BURN_GNT_8812, EFUSE_ACCESS_OFF_JAGUAR)
                .await?;
            if write {
                let test = self.read_u8_async(REG_EFUSE_TEST + 3).await.unwrap_or(0);
                self.write_u8_async(REG_EFUSE_TEST + 3, test & !(BIT7 as u8))
                    .await?;
            }
        }
        Ok(())
    }

    async fn read_efuse_logical_map_powered_async(
        &self,
    ) -> Result<[u8; EFUSE_MAP_LEN_JAGUAR], DriverError> {
        let mut words = [[0xffffu16; EFUSE_MAX_WORD_UNIT_JAGUAR]; EFUSE_MAX_SECTION_JAGUAR];
        let mut physical_addr = 0u16;

        let mut header = self.read_efuse_byte_async(physical_addr).await?;
        if header == 0xff {
            return Ok([0xff; EFUSE_MAP_LEN_JAGUAR]);
        }
        physical_addr += 1;

        while header != 0xff && physical_addr < EFUSE_REAL_CONTENT_LEN_JAGUAR {
            let (offset, word_enable) = if is_extended_header(header) {
                let offset_low = (header & 0xe0) >> 5;
                let extended = self.read_efuse_byte_async(physical_addr).await?;
                physical_addr += 1;

                if all_words_disabled(extended) {
                    header = self.read_efuse_byte_async(physical_addr).await?;
                    if header != 0xff && physical_addr < EFUSE_REAL_CONTENT_LEN_JAGUAR {
                        physical_addr += 1;
                    }
                    continue;
                }

                (((extended & 0xf0) >> 1) | offset_low, extended & 0x0f)
            } else {
                ((header >> 4) & 0x0f, header & 0x0f)
            };

            if (offset as usize) < EFUSE_MAX_SECTION_JAGUAR {
                let section = &mut words[offset as usize];
                for (word_index, word) in section.iter_mut().enumerate() {
                    if word_enable & (1u8 << word_index) == 0 {
                        let lo = self.read_efuse_byte_async(physical_addr).await?;
                        physical_addr += 1;
                        *word = lo as u16;
                        if physical_addr >= EFUSE_REAL_CONTENT_LEN_JAGUAR {
                            break;
                        }

                        let hi = self.read_efuse_byte_async(physical_addr).await?;
                        physical_addr += 1;
                        *word |= (hi as u16) << 8;
                        if physical_addr >= EFUSE_REAL_CONTENT_LEN_JAGUAR {
                            break;
                        }
                    }
                }
            } else {
                for word_index in 0..EFUSE_MAX_WORD_UNIT_JAGUAR {
                    if word_enable & (1u8 << word_index) == 0 {
                        physical_addr += 1;
                        if physical_addr >= EFUSE_REAL_CONTENT_LEN_JAGUAR {
                            break;
                        }
                        physical_addr += 1;
                        if physical_addr >= EFUSE_REAL_CONTENT_LEN_JAGUAR {
                            break;
                        }
                    }
                }
            }

            header = self.read_efuse_byte_async(physical_addr).await?;
            if header != 0xff && physical_addr < EFUSE_REAL_CONTENT_LEN_JAGUAR {
                physical_addr += 1;
            }
        }

        Ok(flatten_efuse_words(words))
    }

    async fn read_efuse_probe_byte_async(&self, address: u16) -> Result<u8, DriverError> {
        self.write_u8_async(REG_EFUSE_CTRL + 1, (address & 0xff) as u8)
            .await?;
        let ctrl2 = self.read_u8_async(REG_EFUSE_CTRL + 2).await.unwrap_or(0);
        self.write_u8_async(
            REG_EFUSE_CTRL + 2,
            (((address >> 8) & 0x03) as u8) | (ctrl2 & 0xfc),
        )
        .await?;

        let ctrl3 = self.read_u8_async(REG_EFUSE_CTRL + 3).await.unwrap_or(0);
        self.write_u8_async(REG_EFUSE_CTRL + 3, ctrl3 & !(BIT7 as u8))
            .await?;

        let mut retries = 0u16;
        while self.read_u8_async(REG_EFUSE_CTRL + 3).await.unwrap_or(0) & BIT7 as u8 == 0
            && retries < 1000
        {
            crate::time::sleep_ms(1).await;
            retries += 1;
        }

        if retries < 100 {
            self.read_u8_async(REG_EFUSE_CTRL).await
        } else {
            Ok(0xff)
        }
    }

    async fn read_efuse_byte_async(&self, address: u16) -> Result<u8, DriverError> {
        let _ = self.read_u16_async(REG_EFUSE_TEST).await;
        self.write_u16_async(REG_EFUSE_TEST, 0).await?;

        self.write_u8_async(REG_EFUSE_CTRL + 1, (address & 0xff) as u8)
            .await?;
        let ctrl2 = self.read_u8_async(REG_EFUSE_CTRL + 2).await.unwrap_or(0);
        self.write_u8_async(
            REG_EFUSE_CTRL + 2,
            (((address >> 8) & 0x03) as u8) | (ctrl2 & 0xfc),
        )
        .await?;

        let ctrl3 = self.read_u8_async(REG_EFUSE_CTRL + 3).await.unwrap_or(0);
        self.write_u8_async(REG_EFUSE_CTRL + 3, ctrl3 & !(BIT7 as u8))
            .await?;

        let mut value = self.read_u32_async(REG_EFUSE_CTRL).await.unwrap_or(0);
        let mut retry = 0u16;
        while ((value >> 24) & BIT7) == 0 && retry < 10000 {
            value = self.read_u32_async(REG_EFUSE_CTRL).await.unwrap_or(0);
            retry += 1;
        }

        if retry < 10000 {
            Ok((value & 0xff) as u8)
        } else {
            Ok(0xff)
        }
    }
}

fn apply_efuse_block_8822e(
    map: &mut [u8; EFUSE_MAP_LEN_JAGUAR],
    header0: u8,
    header1: u8,
    data: &[u8],
) -> Option<usize> {
    if header0 & 0xf0 != 0x30 {
        return None;
    }
    let block = usize::from(((header0 & 0x0f) << 4) | (header1 >> 4));
    let word_enable = header1 & 0x0f;
    let expected = (word_enable.count_zeros() - 4) as usize * 2;
    if data.len() < expected {
        return None;
    }

    let mut data_index = 0usize;
    let base = block << 3;
    for word in 0..4usize {
        if word_enable & (1 << word) != 0 {
            continue;
        }
        for byte in 0..2usize {
            if let Some(slot) = map.get_mut(base + word * 2 + byte) {
                *slot = data[data_index];
            }
            data_index += 1;
        }
    }
    Some(expected)
}

fn mac_from_efuse_map(chip: ChipInfo, map: &[u8; EFUSE_MAP_LEN_JAGUAR]) -> Option<[u8; 6]> {
    let offset = match chip.family {
        ChipFamily::Rtl8812 => EEPROM_MAC_ADDR_8812AU,
        ChipFamily::Rtl8814 => EEPROM_MAC_ADDR_8814AU,
        ChipFamily::Rtl8821 => EEPROM_MAC_ADDR_8821AU,
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => return None,
    };
    let bytes = map.get(offset..offset + 6)?;

    let mut mac = [0u8; 6];
    mac.copy_from_slice(bytes);
    if is_valid_mac(mac) {
        Some(mac)
    } else {
        None
    }
}

fn rfe_type_from_efuse_map(
    chip: ChipInfo,
    map: &[u8; EFUSE_MAP_LEN_JAGUAR],
    amplifiers: AmplifierFlags,
) -> u8 {
    let rfe_option = map[EEPROM_RFE_OPTION_8812];
    match chip.family {
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
            if rfe_option == 0xff {
                0
            } else {
                rfe_option
            }
        }
        ChipFamily::Rtl8814 => {
            if rfe_option == 0xff || (rfe_option & BIT7 as u8) != 0 {
                1
            } else {
                rfe_option & 0x7f
            }
        }
        ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
            if rfe_option == 0xff {
                0
            } else if (rfe_option & BIT7 as u8) != 0 {
                if amplifiers.external_lna_2g && amplifiers.external_pa_2g {
                    3
                } else {
                    0
                }
            } else {
                let mut rfe_type = rfe_option & 0x3f;
                if rfe_type == 4
                    && (amplifiers.external_pa_5g
                        || amplifiers.external_pa_2g
                        || amplifiers.external_lna_5g
                        || amplifiers.external_lna_2g)
                {
                    rfe_type = 0;
                }
                rfe_type
            }
        }
    }
}

fn amplifier_flags_from_efuse_map(map: &[u8; EFUSE_MAP_LEN_JAGUAR]) -> AmplifierFlags {
    let pa_type = efuse_or_zero(map[EEPROM_PA_TYPE_8812AU]);
    let lna_2g = efuse_or_zero(map[EEPROM_LNA_TYPE_2G_8812AU]);
    let lna_5g = efuse_or_zero(map[EEPROM_LNA_TYPE_5G_8812AU]);

    let external_pa_2g = (pa_type & (BIT5 | BIT4) as u8) == (BIT5 | BIT4) as u8;
    let external_pa_5g = (pa_type & (BIT1 | BIT0) as u8) == (BIT1 | BIT0) as u8;
    let external_lna_2g = (lna_2g & (BIT7 | BIT3) as u8) == (BIT7 | BIT3) as u8;
    let external_lna_5g = (lna_5g & (BIT7 | BIT3) as u8) == (BIT7 | BIT3) as u8;

    AmplifierFlags {
        external_pa_2g,
        external_pa_5g,
        external_lna_2g,
        external_lna_5g,
        type_gpa: if external_pa_2g {
            (((lna_2g & BIT6 as u8) >> 6) << 2 | ((lna_2g & BIT2 as u8) >> 2)) as u16
        } else {
            0
        },
        type_apa: if external_pa_5g {
            (((lna_5g & BIT6 as u8) >> 6) << 2 | ((lna_5g & BIT2 as u8) >> 2)) as u16
        } else {
            0
        },
        type_glna: if external_lna_2g {
            (((lna_2g & (BIT5 | BIT4) as u8) >> 4) << 2 | (lna_2g & (BIT1 | BIT0) as u8)) as u16
        } else {
            0
        },
        type_alna: if external_lna_5g {
            (((lna_5g & (BIT5 | BIT4) as u8) >> 4) << 2 | (lna_5g & (BIT1 | BIT0) as u8)) as u16
        } else {
            0
        },
    }
}

fn bluetooth_coexist_from_efuse_map(map: &[u8; EFUSE_MAP_LEN_JAGUAR]) -> bool {
    ((map[EEPROM_RF_BOARD_OPTION_8812] & 0xe0) >> 5) == 0x01
}

fn board_type_from_amplifiers(amplifiers: AmplifierFlags, bluetooth_coexist: bool) -> u8 {
    let mut board_type = 0;
    if amplifiers.external_lna_2g {
        board_type |= BIT0 as u8;
    }
    if amplifiers.external_pa_2g {
        board_type |= BIT1 as u8;
    }
    if amplifiers.external_lna_5g {
        board_type |= BIT2 as u8;
    }
    if amplifiers.external_pa_5g {
        board_type |= BIT3 as u8;
    }
    if bluetooth_coexist {
        board_type |= BIT4 as u8;
    }
    board_type
}

fn flatten_efuse_words(
    words: [[u16; EFUSE_MAX_WORD_UNIT_JAGUAR]; EFUSE_MAX_SECTION_JAGUAR],
) -> [u8; EFUSE_MAP_LEN_JAGUAR] {
    let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
    for (section_index, section) in words.iter().enumerate() {
        for (word_index, word) in section.iter().enumerate() {
            let offset = (section_index * 8) + (word_index * 2);
            map[offset] = (*word & 0xff) as u8;
            map[offset + 1] = (*word >> 8) as u8;
        }
    }
    map
}

fn is_extended_header(header: u8) -> bool {
    (header & 0x1f) == 0x0f
}

fn all_words_disabled(word_enable: u8) -> bool {
    (word_enable & 0x0f) == 0x0f
}

fn is_valid_mac(mac: [u8; 6]) -> bool {
    !mac.iter().all(|byte| *byte == 0xff) && !mac.iter().all(|byte| *byte == 0x00)
}

fn efuse_or_zero(value: u8) -> u8 {
    if value == 0xff {
        0
    } else {
        value
    }
}

fn crystal_cap_from_efuse_map(map: &[u8; EFUSE_MAP_LEN_JAGUAR]) -> u8 {
    match map[EEPROM_XTAL_8812] {
        0xff => EEPROM_DEFAULT_CRYSTAL_CAP_8812,
        value => value,
    }
}

fn thermal_meter_from_efuse_map(chip: ChipInfo, map: &[u8; EFUSE_MAP_LEN_JAGUAR]) -> u8 {
    thermal_meter_paths_from_efuse_map(chip, map)[0]
}

fn thermal_meter_paths_from_efuse_map(chip: ChipInfo, map: &[u8; EFUSE_MAP_LEN_JAGUAR]) -> [u8; 2] {
    if chip.family == ChipFamily::Rtl8822e {
        [
            map[EEPROM_THERMAL_METER_A_8822E],
            map[EEPROM_THERMAL_METER_B_8822E],
        ]
    } else {
        let thermal = map[EEPROM_THERMAL_METER_8812];
        [thermal, thermal]
    }
}

fn tx_power_info_from_efuse_map(chip: ChipInfo, map: &[u8; EFUSE_MAP_LEN_JAGUAR]) -> TxPowerInfo {
    let paths = chip.total_rf_paths().min(MAX_RF_PATHS);
    let bytes_needed = TX_POWER_PG_OFFSET + paths * (18 + 24);
    if bytes_needed > map.len()
        || (chip.family.is_jaguar3()
            && map[TX_POWER_PG_OFFSET..bytes_needed]
                .iter()
                .all(|b| *b == 0xff))
    {
        return TxPowerInfo::default();
    }

    let mut info = TxPowerInfo {
        loaded: true,
        ..TxPowerInfo::default()
    };
    let mut cck_base_2g = [[0u8; 6]; MAX_RF_PATHS];
    let mut bw40_base_2g = [[0u8; 6]; MAX_RF_PATHS];
    let mut bw40_base_5g = [[0u8; 14]; MAX_RF_PATHS];
    let mut off = TX_POWER_PG_OFFSET;

    for path in 0..paths {
        for value in cck_base_2g[path].iter_mut() {
            *value = tx_power_base_byte(chip.family, map, off);
            off += 1;
        }
        for value in bw40_base_2g[path].iter_mut().take(5) {
            *value = tx_power_base_byte(chip.family, map, off);
            off += 1;
        }
        let v = map[off];
        off += 1;
        info.bw20_24g_diff[path][0] = pg_msb_diff(v);
        info.ofdm_24g_diff[path][0] = pg_lsb_diff(v);
        for tx in 1..4 {
            let v = map[off];
            off += 1;
            info.bw40_24g_diff[path][tx] = pg_msb_diff(v);
            info.bw20_24g_diff[path][tx] = pg_lsb_diff(v);
            let v = map[off];
            off += 1;
            info.ofdm_24g_diff[path][tx] = pg_msb_diff(v);
            info.cck_24g_diff[path][tx] = pg_lsb_diff(v);
        }

        for value in bw40_base_5g[path].iter_mut() {
            *value = tx_power_base_byte(chip.family, map, off);
            off += 1;
        }
        let v = map[off];
        off += 1;
        info.bw20_5g_diff[path][0] = pg_msb_diff(v);
        info.ofdm_5g_diff[path][0] = pg_lsb_diff(v);
        for tx in 1..4 {
            let v = map[off];
            off += 1;
            info.bw40_5g_diff[path][tx] = pg_msb_diff(v);
            info.bw20_5g_diff[path][tx] = pg_lsb_diff(v);
        }
        let v = map[off];
        off += 1;
        info.ofdm_5g_diff[path][1] = pg_msb_diff(v);
        info.ofdm_5g_diff[path][2] = pg_lsb_diff(v);
        let v = map[off];
        off += 1;
        info.ofdm_5g_diff[path][3] = pg_lsb_diff(v);
        for tx in 0..4 {
            let v = map[off];
            off += 1;
            info.bw80_5g_diff[path][tx] = pg_msb_diff(v);
        }
    }

    for path in 0..paths {
        for ch in 1..=14 {
            if let Some((group, cck_group)) = classify_2g_channel(ch as u8) {
                info.index24g_cck_base[path][ch - 1] = cck_base_2g[path][cck_group];
                info.index24g_bw40_base[path][ch - 1] = bw40_base_2g[path][group];
            }
        }
        for (idx, ch) in CENTER_CH_5G_ALL.into_iter().enumerate() {
            if let Some(group) = classify_5g_channel(ch) {
                info.index5g_bw40_base[path][idx] = bw40_base_5g[path][group];
            }
        }
    }
    load_tx_power_by_rate(chip, &mut info);
    info
}

pub(crate) fn tx_power_index_base(
    info: &TxPowerInfo,
    path: usize,
    rate: u8,
    ntx_idx: usize,
    bandwidth: ChannelWidth,
    channel: u8,
) -> Option<u8> {
    tx_power_index_base_with_policy(
        info,
        path,
        rate,
        ntx_idx,
        bandwidth,
        channel,
        TxPowerPolicy {
            enable_by_rate: tx_power_by_rate_enabled(),
            regulation: current_tx_power_regulation(),
        },
    )
}

fn tx_power_index_base_with_policy(
    info: &TxPowerInfo,
    path: usize,
    rate: u8,
    ntx_idx: usize,
    bandwidth: ChannelWidth,
    channel: u8,
    policy: TxPowerPolicy,
) -> Option<u8> {
    if !info.loaded || path >= MAX_RF_PATHS || ntx_idx >= MAX_TX_COUNT {
        return None;
    }
    let bandwidth_idx = match bandwidth {
        ChannelWidth::Mhz5 | ChannelWidth::Mhz10 => 0,
        ChannelWidth::Mhz20 => 0,
        ChannelWidth::Mhz40 => 1,
        ChannelWidth::Mhz80 => 2,
    };
    let (band, ch_idx, mut tx_power) = if channel <= 14 {
        let ch_idx = channel.checked_sub(1)? as usize;
        let power = if is_cck_rate(rate) {
            let mut power = i16::from(info.index24g_cck_base[path][ch_idx]);
            for idx in 0..=ntx_idx {
                power += i16::from(info.cck_24g_diff[path][idx]);
            }
            power
        } else {
            let mut power = i16::from(info.index24g_bw40_base[path][ch_idx]);
            if is_ofdm_rate(rate) {
                for idx in 0..=ntx_idx {
                    power += i16::from(info.ofdm_24g_diff[path][idx]);
                }
            } else {
                let diff = if bandwidth_idx == 0 {
                    &info.bw20_24g_diff[path]
                } else {
                    &info.bw40_24g_diff[path]
                };
                for value in diff.iter().take(ntx_idx + 1) {
                    power += i16::from(*value);
                }
            }
            power
        };
        (TxPowerBand::Ghz2, ch_idx, power)
    } else {
        let ch_idx = CENTER_CH_5G_ALL.iter().position(|ch| *ch == channel)?;
        if rate < 0x0c {
            return None;
        }
        let mut power = i16::from(info.index5g_bw40_base[path][ch_idx]);
        if is_ofdm_rate(rate) {
            for idx in 0..=ntx_idx {
                power += i16::from(info.ofdm_5g_diff[path][idx]);
            }
        } else {
            let diff = match bandwidth_idx {
                0 => &info.bw20_5g_diff[path],
                1 => &info.bw40_5g_diff[path],
                _ => &info.bw80_5g_diff[path],
            };
            for value in diff.iter().take(ntx_idx + 1) {
                power += i16::from(*value);
            }
        }
        (TxPowerBand::Ghz5, ch_idx, power)
    };

    let mut by_rate_diff = 0i16;
    if policy.enable_by_rate {
        by_rate_diff = tx_power_by_rate_offset(info, band, path, rate).unwrap_or(0);
        if let Some(limit) = tx_power_limit(
            policy.regulation,
            band,
            bandwidth,
            rate,
            ntx_idx,
            ch_idx,
            channel,
        ) {
            let headroom = (i16::from(limit) - tx_power - TX_POWER_BOOST).max(0);
            by_rate_diff = by_rate_diff.min(headroom);
        }
    }

    tx_power += by_rate_diff + TX_POWER_BOOST;
    Some(tx_power.clamp(0, 63) as u8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TxPowerPolicy {
    enable_by_rate: bool,
    regulation: TxPowerRegulation,
}

fn load_tx_power_by_rate(chip: ChipInfo, info: &mut TxPowerInfo) {
    let paths = chip.total_rf_paths().min(MAX_RF_PATHS);
    for row in RTL8812A_PHY_REG_PG {
        let band = row[0] as usize;
        let path = row[1] as usize;
        let register = row[3] as u16;
        let data = row[5];
        if band >= MAX_BANDS || path >= paths {
            continue;
        }
        let Some(rates) = rates_for_tx_power_register(register) else {
            continue;
        };
        for (idx, rate) in rates.iter().copied().enumerate() {
            let Some(rate_idx) = rate_to_idx(rate) else {
                continue;
            };
            info.tx_power_by_rate_offset[band][path][rate_idx] =
                ((data >> (idx * 8)) & 0xff) as u8 as i8;
        }
    }

    for band in 0..MAX_BANDS {
        for path in 0..paths {
            for (section, rate) in SECTION_BASE_RATES.iter().copied().enumerate() {
                let Some(rate_idx) = rate_to_idx(rate) else {
                    continue;
                };
                info.tx_power_by_rate_base[band][path][section] =
                    info.tx_power_by_rate_offset[band][path][rate_idx] as u8;
            }
        }
    }

    for band in 0..MAX_BANDS {
        for path in 0..paths {
            for rate_idx in 0..NUM_RATE_IDX {
                let Some(rate) = rate_from_idx(rate_idx) else {
                    continue;
                };
                let Some(section) = rate_to_section(rate) else {
                    continue;
                };
                let base = info.tx_power_by_rate_base[band][path][section];
                if base == 0 {
                    continue;
                }
                let raw = info.tx_power_by_rate_offset[band][path][rate_idx];
                info.tx_power_by_rate_offset[band][path][rate_idx] = raw.saturating_sub(base as i8);
            }
        }
    }

    info.tx_power_by_rate_loaded = true;
}

fn tx_power_by_rate_offset(
    info: &TxPowerInfo,
    band: TxPowerBand,
    path: usize,
    rate: u8,
) -> Option<i16> {
    if !info.tx_power_by_rate_loaded || path >= MAX_RF_PATHS {
        return None;
    }
    let section = rate_to_section(rate)?;
    if band == TxPowerBand::Ghz2 && section >= RateSection::Vht1ss as usize {
        return None;
    }
    let band_idx = match band {
        TxPowerBand::Ghz2 => 0,
        TxPowerBand::Ghz5 => 1,
    };
    let rate_idx = rate_to_idx(rate)?;
    Some(i16::from(
        info.tx_power_by_rate_offset[band_idx][path][rate_idx],
    ))
}

fn tx_power_limit(
    regulation: TxPowerRegulation,
    band: TxPowerBand,
    bandwidth: ChannelWidth,
    rate: u8,
    ntx_idx: usize,
    ch_idx: usize,
    channel: u8,
) -> Option<i8> {
    let bandwidth = match bandwidth {
        ChannelWidth::Mhz5 | ChannelWidth::Mhz10 => TxPowerLimitBandwidth::Mhz20,
        ChannelWidth::Mhz20 => TxPowerLimitBandwidth::Mhz20,
        ChannelWidth::Mhz40 => TxPowerLimitBandwidth::Mhz40,
        ChannelWidth::Mhz80 => TxPowerLimitBandwidth::Mhz80,
    };
    let rate_section = match rate_to_section(rate)? {
        section if section == RateSection::Cck as usize => TxPowerLimitRateSection::Cck,
        section if section == RateSection::Ofdm as usize => TxPowerLimitRateSection::Ofdm,
        section
            if (RateSection::Ht1ss as usize..=RateSection::Ht4ss as usize).contains(&section) =>
        {
            TxPowerLimitRateSection::Ht
        }
        section if section >= RateSection::Vht1ss as usize => TxPowerLimitRateSection::Vht,
        _ => return None,
    };

    let limit = RTL8812A_TX_POWER_LIMITS
        .iter()
        .find(|row| {
            row.regulation == regulation
                && row.band == band
                && row.bandwidth == bandwidth
                && row.rate_section == rate_section
                && row.ntx_idx == ntx_idx
                && row.channel == channel
        })
        .map(|row| row.limit)?;

    if limit < 63 && limit > -63 && ch_idx < CENTER_CH_5G_ALL_NUM {
        Some(limit)
    } else {
        None
    }
}

fn tx_power_by_rate_enabled() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var_os("OPENIPC_RS_ENABLE_TXPWR_BY_RATE").is_some()
            || std::env::var_os("DEVOURER_ENABLE_TXPWR_BY_RATE").is_some()
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

fn current_tx_power_regulation() -> TxPowerRegulation {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let value = std::env::var("OPENIPC_RS_REGULATION")
            .or_else(|_| std::env::var("DEVOURER_REGULATION"))
            .unwrap_or_default();
        if value.eq_ignore_ascii_case("ETSI") {
            TxPowerRegulation::Etsi
        } else if value.eq_ignore_ascii_case("MKK") {
            TxPowerRegulation::Mkk
        } else if value.eq_ignore_ascii_case("WW") || value.eq_ignore_ascii_case("WORLDWIDE") {
            TxPowerRegulation::Worldwide
        } else {
            TxPowerRegulation::Fcc
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        TxPowerRegulation::Fcc
    }
}

const SECTION_BASE_RATES: [u8; NUM_RATE_SECTION] =
    [0x16, 0x6c, 0x87, 0x8f, 0x97, 0x9f, 0xa7, 0xb1, 0xbb, 0xc5];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RateSection {
    Cck = 0,
    Ofdm = 1,
    Ht1ss = 2,
    Ht2ss = 3,
    Ht3ss = 4,
    Ht4ss = 5,
    Vht1ss = 6,
    Vht2ss = 7,
    Vht3ss = 8,
    Vht4ss = 9,
}

fn rate_to_section(rate: u8) -> Option<usize> {
    Some(match rate {
        0x02 | 0x04 | 0x0b | 0x16 => RateSection::Cck as usize,
        0x0c | 0x12 | 0x18 | 0x24 | 0x30 | 0x48 | 0x60 | 0x6c => RateSection::Ofdm as usize,
        0x80..=0x87 => RateSection::Ht1ss as usize,
        0x88..=0x8f => RateSection::Ht2ss as usize,
        0x90..=0x97 => RateSection::Ht3ss as usize,
        0x98..=0x9f => RateSection::Ht4ss as usize,
        0xa0..=0xa9 => RateSection::Vht1ss as usize,
        0xaa..=0xb3 => RateSection::Vht2ss as usize,
        0xb4..=0xbd => RateSection::Vht3ss as usize,
        0xbe..=0xc7 => RateSection::Vht4ss as usize,
        _ => return None,
    })
}

fn rate_to_idx(rate: u8) -> Option<usize> {
    Some(match rate {
        0x02 => 0,
        0x04 => 1,
        0x0b => 2,
        0x16 => 3,
        0x0c => 4,
        0x12 => 5,
        0x18 => 6,
        0x24 => 7,
        0x30 => 8,
        0x48 => 9,
        0x60 => 10,
        0x6c => 11,
        0x80..=0x9f => 12 + (rate - 0x80) as usize,
        0xa0..=0xc7 => 44 + (rate - 0xa0) as usize,
        _ => return None,
    })
}

fn rate_from_idx(idx: usize) -> Option<u8> {
    Some(match idx {
        0 => 0x02,
        1 => 0x04,
        2 => 0x0b,
        3 => 0x16,
        4 => 0x0c,
        5 => 0x12,
        6 => 0x18,
        7 => 0x24,
        8 => 0x30,
        9 => 0x48,
        10 => 0x60,
        11 => 0x6c,
        12..=43 => 0x80 + (idx as u8 - 12),
        44..=83 => 0xa0 + (idx as u8 - 44),
        _ => return None,
    })
}

fn rates_for_tx_power_register(register: u16) -> Option<[u8; 4]> {
    Some(match register {
        0x0c20 | 0x0e20 => [0x02, 0x04, 0x0b, 0x16],
        0x0c24 | 0x0e24 => [0x0c, 0x12, 0x18, 0x24],
        0x0c28 | 0x0e28 => [0x30, 0x48, 0x60, 0x6c],
        0x0c2c | 0x0e2c => [0x80, 0x81, 0x82, 0x83],
        0x0c30 | 0x0e30 => [0x84, 0x85, 0x86, 0x87],
        0x0c34 | 0x0e34 => [0x88, 0x89, 0x8a, 0x8b],
        0x0c38 | 0x0e38 => [0x8c, 0x8d, 0x8e, 0x8f],
        0x0c3c | 0x0e3c => [0xa0, 0xa1, 0xa2, 0xa3],
        0x0c40 | 0x0e40 => [0xa4, 0xa5, 0xa6, 0xa7],
        0x0c44 | 0x0e44 => [0xa8, 0xa9, 0xaa, 0xab],
        0x0c48 | 0x0e48 => [0xac, 0xad, 0xae, 0xaf],
        0x0c4c | 0x0e4c => [0xb0, 0xb1, 0xb2, 0xb3],
        _ => return None,
    })
}

fn classify_2g_channel(channel: u8) -> Option<(usize, usize)> {
    let group = match channel {
        1..=2 => 0,
        3..=5 => 1,
        6..=8 => 2,
        9..=11 => 3,
        12..=14 => 4,
        _ => return None,
    };
    let cck_group = if channel == 14 { 5 } else { group };
    Some((group, cck_group))
}

fn classify_5g_channel(channel: u8) -> Option<usize> {
    Some(match channel {
        16..=42 => 0,
        44..=48 => 1,
        50..=58 => 2,
        60..=98 => 3,
        100..=106 => 4,
        108..=114 => 5,
        116..=122 => 6,
        124..=130 => 7,
        132..=138 => 8,
        140..=144 => 9,
        149..=155 => 10,
        157..=161 => 11,
        165..=171 => 12,
        173..=253 => 13,
        _ => return None,
    })
}

fn tx_power_base_byte(family: ChipFamily, map: &[u8; EFUSE_MAP_LEN_JAGUAR], offset: usize) -> u8 {
    let programmed = map[offset];
    if programmed <= 63 {
        return programmed;
    }
    let Some(defaults) = (match family {
        ChipFamily::Rtl8812 => Some(tx_power_defaults::RTL8812A),
        ChipFamily::Rtl8814 => Some(tx_power_defaults::RTL8814A),
        ChipFamily::Rtl8821 => Some(tx_power_defaults::RTL8821A),
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => None,
    }) else {
        return programmed;
    };
    let index = offset.saturating_sub(TX_POWER_PG_OFFSET);
    defaults
        .get(index)
        .copied()
        .filter(|value| *value <= 63)
        .or_else(|| {
            tx_power_defaults::GENERIC
                .get(index)
                .copied()
                .filter(|value| *value <= 63)
        })
        .unwrap_or(programmed)
}

fn pg_msb_diff(value: u8) -> i8 {
    signed_nibble((value >> 4) & 0x0f)
}

fn pg_lsb_diff(value: u8) -> i8 {
    signed_nibble(value & 0x0f)
}

fn signed_nibble(nibble: u8) -> i8 {
    if nibble & 0x08 != 0 {
        (nibble | 0xf0) as i8
    } else {
        nibble as i8
    }
}

fn is_cck_rate(rate: u8) -> bool {
    matches!(rate, 0x02 | 0x04 | 0x0b | 0x16)
}

fn is_ofdm_rate(rate: u8) -> bool {
    matches!(rate, 0x0c | 0x12 | 0x18 | 0x24 | 0x30 | 0x48 | 0x60 | 0x6c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RfType;

    fn chip(family: ChipFamily) -> ChipInfo {
        ChipInfo {
            family,
            rf_type: match family {
                ChipFamily::Rtl8812 => RfType::TwoTTwoR,
                ChipFamily::Rtl8814 => RfType::FourTFourR,
                ChipFamily::Rtl8821 => RfType::OneTOneR,
                ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => RfType::TwoTTwoR,
            },
            cut_version: 0,
            sys_cfg: 0,
        }
    }

    #[test]
    fn rejects_empty_mac_addresses() {
        assert!(!is_valid_mac([0xff; 6]));
        assert!(!is_valid_mac([0x00; 6]));
        assert!(is_valid_mac([0x02, 0x0d, 0xb0, 0xc7, 0xe4, 0xb3]));
    }

    #[test]
    fn flattens_section_words_like_realtek_shadow_map() {
        let mut words = [[0xffffu16; EFUSE_MAX_WORD_UNIT_JAGUAR]; EFUSE_MAX_SECTION_JAGUAR];
        words[2][1] = 0x1234;
        let map = flatten_efuse_words(words);
        assert_eq!(map[18], 0x34);
        assert_eq!(map[19], 0x12);
    }

    #[test]
    fn derives_8812_rfe_type_from_devourer_bit7_case() {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        map[EEPROM_RFE_OPTION_8812] = BIT7 as u8;
        map[EEPROM_PA_TYPE_8812AU] = (BIT5 | BIT4) as u8;
        map[EEPROM_LNA_TYPE_2G_8812AU] = (BIT7 | BIT3) as u8;

        let amplifiers = amplifier_flags_from_efuse_map(&map);
        assert_eq!(
            rfe_type_from_efuse_map(chip(ChipFamily::Rtl8812), &map, amplifiers),
            3
        );
    }

    #[test]
    fn applies_8812_rfe4_external_amp_workaround() {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        map[EEPROM_RFE_OPTION_8812] = 4;
        map[EEPROM_PA_TYPE_8812AU] = (BIT1 | BIT0) as u8;

        let amplifiers = amplifier_flags_from_efuse_map(&map);
        assert_eq!(
            rfe_type_from_efuse_map(chip(ChipFamily::Rtl8812), &map, amplifiers),
            0
        );
    }

    #[test]
    fn derives_aviateur_board_type_and_amplifier_types() {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        map[EEPROM_RF_BOARD_OPTION_8812] = 0x20;
        map[EEPROM_PA_TYPE_8812AU] = (BIT5 | BIT4) as u8;
        map[EEPROM_LNA_TYPE_2G_8812AU] = (BIT7 | BIT6 | BIT5 | BIT3 | BIT2 | BIT1) as u8;

        let amplifiers = amplifier_flags_from_efuse_map(&map);
        assert_eq!(
            board_type_from_amplifiers(amplifiers, bluetooth_coexist_from_efuse_map(&map)),
            0x13
        );
        assert_eq!(amplifiers.type_gpa, 0x05);
        assert_eq!(amplifiers.type_glna, 0x0a);
    }

    #[test]
    fn reads_crystal_cap_with_aviateur_default() {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        assert_eq!(crystal_cap_from_efuse_map(&map), 0x20);
        map[EEPROM_XTAL_8812] = 0x2a;
        assert_eq!(crystal_cap_from_efuse_map(&map), 0x2a);
    }

    #[test]
    fn reads_thermal_meter_baseline_like_devourer() {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        assert_eq!(
            thermal_meter_from_efuse_map(chip(ChipFamily::Rtl8812), &map),
            0xff
        );
        map[EEPROM_THERMAL_METER_8812] = 0x1a;
        assert_eq!(
            thermal_meter_from_efuse_map(chip(ChipFamily::Rtl8812), &map),
            0x1a
        );
    }

    #[test]
    fn decodes_rtl8822e_two_byte_block_headers() {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];

        // Block 0x19 starts at logical 0xc8; word-enable 0xd carries word 1,
        // which contains the RFE option at logical offset 0xca.
        assert_eq!(
            apply_efuse_block_8822e(&mut map, 0x31, 0x9d, &[0x15, 0xaa]),
            Some(2)
        );
        assert_eq!(map[0xca], 0x15);
        assert_eq!(map[0xcb], 0xaa);
        assert_eq!(map[0xc8], 0xff);

        assert_eq!(apply_efuse_block_8822e(&mut map, 0x31, 0x9d, &[0x15]), None);
        assert_eq!(apply_efuse_block_8822e(&mut map, 0x21, 0x9d, &[]), None);
    }

    #[test]
    fn parses_tx_power_pg_block_and_computes_base_power() {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        let mut off = TX_POWER_PG_OFFSET;
        for value in [10, 11, 12, 13, 14, 15] {
            map[off] = value;
            off += 1;
        }
        for value in [20, 21, 22, 23, 24] {
            map[off] = value;
            off += 1;
        }
        map[off] = 0x21;
        off += 1;
        for _ in 0..6 {
            map[off] = 0;
            off += 1;
        }
        for _ in 0..24 {
            map[off] = 0;
            off += 1;
        }

        let info = tx_power_info_from_efuse_map(chip(ChipFamily::Rtl8812), &map);
        assert!(info.loaded);
        assert!(info.tx_power_by_rate_loaded);
        assert_eq!(
            info.tx_power_by_rate_base[0][0][RateSection::Ofdm as usize],
            0x30
        );
        assert_eq!(
            info.tx_power_by_rate_offset[0][0][rate_to_idx(0x0c).unwrap()],
            20
        );
        assert_eq!(
            tx_power_index_base(&info, 0, 0x0c, 0, crate::types::ChannelWidth::Mhz20, 6,),
            Some(25)
        );
        assert_eq!(
            tx_power_index_base_with_policy(
                &info,
                0,
                0x0c,
                0,
                crate::types::ChannelWidth::Mhz20,
                6,
                TxPowerPolicy {
                    enable_by_rate: true,
                    regulation: TxPowerRegulation::Fcc,
                },
            ),
            Some(36)
        );
        assert_eq!(
            tx_power_index_base(&info, 0, 0x02, 0, crate::types::ChannelWidth::Mhz20, 6,),
            Some(14)
        );
    }

    #[test]
    fn jaguar1_channel_groups_match_vendor_boundaries() {
        for (channel, expected) in [
            (16, Some(0)),
            (42, Some(0)),
            (60, Some(3)),
            (84, Some(3)),
            (98, Some(3)),
            (100, Some(4)),
            (106, Some(4)),
            (173, Some(13)),
            (253, Some(13)),
        ] {
            assert_eq!(classify_5g_channel(channel), expected, "channel {channel}");
        }
        assert_eq!(classify_5g_channel(15), None);
        assert_eq!(classify_5g_channel(99), None);
    }

    #[test]
    fn tx_power_bases_use_programmed_chip_and_generic_fallbacks() {
        let mut map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        assert_eq!(
            tx_power_base_byte(ChipFamily::Rtl8812, &map, TX_POWER_PG_OFFSET),
            0x2d
        );
        assert_eq!(
            tx_power_base_byte(ChipFamily::Rtl8821, &map, TX_POWER_PG_OFFSET + 42),
            0x2d
        );
        map[TX_POWER_PG_OFFSET] = 0x17;
        assert_eq!(
            tx_power_base_byte(ChipFamily::Rtl8814, &map, TX_POWER_PG_OFFSET),
            0x17
        );
        assert_eq!(
            tx_power_base_byte(ChipFamily::Rtl8822c, &map, TX_POWER_PG_OFFSET + 1),
            0xff
        );
    }

    #[test]
    fn unprogrammed_jaguar1_pg_map_still_loads_safe_vendor_bases() {
        let map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        let info = tx_power_info_from_efuse_map(chip(ChipFamily::Rtl8812), &map);
        assert!(info.loaded);
        assert_eq!(info.index24g_cck_base[0][0], 0x2d);
        assert_eq!(info.index24g_bw40_base[0][0], 0x2d);
        assert_eq!(info.index5g_bw40_base[0][8], 0x2a);
    }

    #[test]
    fn maps_8814_unprogrammed_rfe_to_devourer_default() {
        let map = [0xff; EFUSE_MAP_LEN_JAGUAR];
        let amplifiers = amplifier_flags_from_efuse_map(&map);
        assert_eq!(
            rfe_type_from_efuse_map(chip(ChipFamily::Rtl8814), &map, amplifiers),
            1
        );
    }
}
