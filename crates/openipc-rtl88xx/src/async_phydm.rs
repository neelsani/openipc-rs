use crate::device::RealtekDevice;
use crate::regs::*;
use crate::types::DriverError;

const REG_OFDM_FA_TYPE1: u16 = 0x0fcc;
const REG_OFDM_FA_TYPE2: u16 = 0x0fd0;
const REG_OFDM_FA_TYPE3: u16 = 0x0fbc;
const REG_OFDM_FA_TYPE4: u16 = 0x0fc0;
const REG_OFDM_FA_TYPE5: u16 = 0x0fc4;
const REG_OFDM_FA_TYPE6: u16 = 0x0fc8;
const REG_OFDM_FAIL: u16 = 0x0f48;
const REG_CCK_FA: u16 = 0x0a5c;
const REG_CCK_CCA_CNT: u16 = 0x0f08;
const REG_CCK_CRC32_CNT: u16 = 0x0f04;
const REG_VHT_CRC32_CNT: u16 = 0x0f0c;
const REG_HT_CRC32_CNT: u16 = 0x0f10;
const REG_OFDM_CRC32_CNT: u16 = 0x0f14;
const REG_BB_RX_PATH: u16 = 0x0808;

const DIG_MIN: u8 = 0x1c;
const DIG_MAX: u8 = 0x26;
const DIG_MAX_OF_MIN: u8 = 0x2a;

/// PHY false-alarm and CRC counters used by DIG/watchdog logic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FalseAlarmCounters {
    /// OFDM false-alarm fail count.
    pub cnt_ofdm_fail: u32,
    /// CCK false-alarm fail count.
    pub cnt_cck_fail: u32,
    /// OFDM clear-channel assessment count.
    pub cnt_ofdm_cca: u32,
    /// CCK clear-channel assessment count.
    pub cnt_cck_cca: u32,
    /// CCK CRC OK count.
    pub cnt_cck_crc32_ok: u32,
    /// CCK CRC error count.
    pub cnt_cck_crc32_error: u32,
    /// OFDM CRC OK count.
    pub cnt_ofdm_crc32_ok: u32,
    /// OFDM CRC error count.
    pub cnt_ofdm_crc32_error: u32,
    /// HT CRC OK count.
    pub cnt_ht_crc32_ok: u32,
    /// HT CRC error count.
    pub cnt_ht_crc32_error: u32,
    /// VHT CRC OK count.
    pub cnt_vht_crc32_ok: u32,
    /// VHT CRC error count.
    pub cnt_vht_crc32_error: u32,
    /// Combined false-alarm count.
    pub cnt_all: u32,
    /// Combined CCA count.
    pub cnt_cca_all: u32,
}

/// State carried between PHYDM DIG watchdog ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhydmDigState {
    /// True after the first initialization tick.
    pub initialized: bool,
    /// Current initial-gain index.
    pub cur_ig_value: u8,
    /// Configured minimum DIG value.
    pub dm_dig_min: u8,
    /// Configured maximum DIG value.
    pub dm_dig_max: u8,
    /// DIG max-of-min clamp value.
    pub dig_max_of_min: u8,
    /// Current RX gain range minimum.
    pub rx_gain_range_min: u8,
    /// Current RX gain range maximum.
    pub rx_gain_range_max: u8,
}

impl Default for PhydmDigState {
    fn default() -> Self {
        Self {
            initialized: false,
            cur_ig_value: 0,
            dm_dig_min: DIG_MIN,
            dm_dig_max: DIG_MAX,
            dig_max_of_min: DIG_MAX_OF_MIN,
            rx_gain_range_min: DIG_MIN,
            rx_gain_range_max: DIG_MAX_OF_MIN,
        }
    }
}

/// Report from one PHYDM watchdog/DIG tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhydmWatchdogReport {
    /// False-alarm counters sampled during the tick.
    pub counters: FalseAlarmCounters,
    /// Initial-gain value before the tick.
    pub previous_igi: u8,
    /// Initial-gain value after the tick.
    pub current_igi: u8,
}

impl RealtekDevice {
    /// Read PHY false-alarm and CRC counters used by DIG/watchdog logic.
    pub async fn read_false_alarm_counters_async(&self) -> Result<FalseAlarmCounters, DriverError> {
        let _ = self
            .query_bb_reg_async(REG_OFDM_FA_TYPE1, B_MASK_DWORD)
            .await?;
        let _ = self
            .query_bb_reg_async(REG_OFDM_FA_TYPE2, B_MASK_DWORD)
            .await?;
        let _ = self
            .query_bb_reg_async(REG_OFDM_FA_TYPE3, B_MASK_DWORD)
            .await?;
        let _ = self
            .query_bb_reg_async(REG_OFDM_FA_TYPE4, B_MASK_DWORD)
            .await?;
        let _ = self
            .query_bb_reg_async(REG_OFDM_FA_TYPE5, B_MASK_DWORD)
            .await?;
        let _ = self
            .query_bb_reg_async(REG_OFDM_FA_TYPE6, B_MASK_DWORD)
            .await?;

        let cnt_ofdm_fail = self.query_bb_reg_async(REG_OFDM_FAIL, 0x0000_ffff).await?;
        let cnt_cck_fail = self.query_bb_reg_async(REG_CCK_FA, 0x0000_ffff).await?;
        let cca = self
            .query_bb_reg_async(REG_CCK_CCA_CNT, B_MASK_DWORD)
            .await?;
        let cck_crc = self
            .query_bb_reg_async(REG_CCK_CRC32_CNT, B_MASK_DWORD)
            .await?;
        let ofdm_crc = self
            .query_bb_reg_async(REG_OFDM_CRC32_CNT, B_MASK_DWORD)
            .await?;
        let ht_crc = self
            .query_bb_reg_async(REG_HT_CRC32_CNT, B_MASK_DWORD)
            .await?;
        let vht_crc = self
            .query_bb_reg_async(REG_VHT_CRC32_CNT, B_MASK_DWORD)
            .await?;
        let cck_enable = self.query_bb_reg_async(REG_BB_RX_PATH, BIT28).await? != 0;

        let cnt_ofdm_cca = (cca >> 16) & 0xffff;
        let cnt_cck_cca = cca & 0xffff;
        let (cnt_all, cnt_cca_all) = if cck_enable {
            (
                cnt_ofdm_fail.saturating_add(cnt_cck_fail),
                cnt_ofdm_cca.saturating_add(cnt_cck_cca),
            )
        } else {
            (cnt_ofdm_fail, cnt_ofdm_cca)
        };

        Ok(FalseAlarmCounters {
            cnt_ofdm_fail,
            cnt_cck_fail,
            cnt_ofdm_cca,
            cnt_cck_cca,
            cnt_cck_crc32_error: (cck_crc >> 16) & 0xffff,
            cnt_cck_crc32_ok: cck_crc & 0xffff,
            cnt_ofdm_crc32_error: (ofdm_crc >> 16) & 0xffff,
            cnt_ofdm_crc32_ok: ofdm_crc & 0xffff,
            cnt_ht_crc32_error: (ht_crc >> 16) & 0xffff,
            cnt_ht_crc32_ok: ht_crc & 0xffff,
            cnt_vht_crc32_error: (vht_crc >> 16) & 0xffff,
            cnt_vht_crc32_ok: vht_crc & 0xffff,
            cnt_all,
            cnt_cca_all,
        })
    }

    /// Reset PHY false-alarm counters after a watchdog sample.
    pub async fn reset_false_alarm_counters_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x09a4, BIT17, 1).await?;
        self.set_bb_reg_async(0x09a4, BIT17, 0).await?;
        self.set_bb_reg_async(0x0a2c, BIT15, 0).await?;
        self.set_bb_reg_async(0x0a2c, BIT15, 1).await?;
        self.set_bb_reg_async(0x0b58, BIT0, 1).await?;
        self.set_bb_reg_async(0x0b58, BIT0, 0).await
    }

    /// Initialize dynamic initial-gain state from the current baseband register.
    pub async fn init_phydm_dig_async(&self, state: &mut PhydmDigState) -> Result<(), DriverError> {
        state.cur_ig_value = self.query_bb_reg_async(0x0c50, 0xff).await? as u8;
        state.dm_dig_min = DIG_MIN;
        state.dm_dig_max = DIG_MAX;
        state.dig_max_of_min = DIG_MAX_OF_MIN;
        state.rx_gain_range_min = state.dm_dig_min;
        state.rx_gain_range_max = state.dig_max_of_min;
        state.initialized = true;
        Ok(())
    }

    /// Run one PHYDM/DIG watchdog tick and apply the next initial-gain value.
    pub async fn run_phydm_watchdog_tick_async(
        &self,
        state: &mut PhydmDigState,
    ) -> Result<PhydmWatchdogReport, DriverError> {
        let counters = self.read_false_alarm_counters_async().await?;
        self.reset_false_alarm_counters_async().await?;
        if !state.initialized {
            self.init_phydm_dig_async(state).await?;
        }

        let previous_igi = state.cur_ig_value;
        let current_igi = dig_next_igi(state, counters.cnt_all);
        self.write_dig_igi_async(current_igi).await?;
        state.cur_ig_value = current_igi;

        Ok(PhydmWatchdogReport {
            counters,
            previous_igi,
            current_igi,
        })
    }

    async fn write_dig_igi_async(&self, igi: u8) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x0c50, 0xff, u32::from(igi)).await?;
        self.set_bb_reg_async(0x0e50, 0xff, u32::from(igi)).await?;
        self.set_bb_reg_async(0x1850, 0xff, u32::from(igi)).await?;
        self.set_bb_reg_async(0x1a50, 0xff, u32::from(igi)).await
    }
}

fn dig_next_igi(state: &mut PhydmDigState, fa_count: u32) -> u8 {
    state.dm_dig_max = DIG_MAX;
    state.dm_dig_min = DIG_MIN;
    state.rx_gain_range_max = state.dig_max_of_min;
    state.rx_gain_range_min = state.dm_dig_min;

    let mut new_igi = state.cur_ig_value;
    if fa_count > 750 {
        new_igi = new_igi.saturating_add(2);
    } else if fa_count > 500 {
        new_igi = new_igi.saturating_add(1);
    } else if fa_count < 250 {
        new_igi = new_igi.saturating_sub(2);
    }
    new_igi.clamp(state.rx_gain_range_min, state.rx_gain_range_max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dig_walks_like_devourer_monitor_mode() {
        let mut state = PhydmDigState {
            initialized: true,
            cur_ig_value: 0x20,
            ..PhydmDigState::default()
        };
        assert_eq!(dig_next_igi(&mut state, 800), 0x22);
        state.cur_ig_value = 0x22;
        assert_eq!(dig_next_igi(&mut state, 600), 0x23);
        state.cur_ig_value = 0x23;
        assert_eq!(dig_next_igi(&mut state, 100), 0x21);
    }

    #[test]
    fn dig_clamps_to_monitor_bounds() {
        let mut state = PhydmDigState {
            initialized: true,
            cur_ig_value: 0x2a,
            ..PhydmDigState::default()
        };
        assert_eq!(dig_next_igi(&mut state, 900), 0x2a);
        state.cur_ig_value = 0x1c;
        assert_eq!(dig_next_igi(&mut state, 0), 0x1c);
    }
}
