use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::regs::*;
use crate::types::{ChannelWidth, ChipFamily, DriverError};

const DELTA_SWING_IDX_SIZE: usize = 30;
const TX_SCALE_TABLE_SIZE: usize = 37;
const PWR_TRACKING_LIMIT: i16 = 26;
const AVG_THERMAL_NUM: usize = 4;
const DEFAULT_OFDM_INDEX: u8 = 24;

const DELTA_SWING_TABLE_2GA_P: [u8; DELTA_SWING_IDX_SIZE] = [
    0, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
];
const DELTA_SWING_TABLE_2GA_N: [u8; DELTA_SWING_IDX_SIZE] = [
    0, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6, 7, 7, 7, 8, 8, 9, 10, 10, 10, 10, 10, 10,
];
const DELTA_SWING_TABLE_2GB_P: [u8; DELTA_SWING_IDX_SIZE] = [
    0, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
];
const DELTA_SWING_TABLE_2GB_N: [u8; DELTA_SWING_IDX_SIZE] = [
    0, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 5, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 11, 11, 11,
    11,
];
const DELTA_SWING_TABLE_5GA_P: [[u8; DELTA_SWING_IDX_SIZE]; 3] = [
    [
        0, 1, 1, 2, 2, 3, 4, 5, 6, 7, 7, 8, 8, 9, 10, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
        11, 11, 11, 11,
    ],
    [
        0, 1, 1, 2, 3, 3, 4, 5, 6, 7, 7, 8, 8, 9, 10, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
        11, 11, 11, 11,
    ],
    [
        0, 1, 1, 2, 3, 3, 4, 5, 6, 7, 7, 8, 8, 9, 10, 11, 11, 12, 12, 11, 11, 11, 11, 11, 11, 11,
        11, 11, 11, 11,
    ],
];
const DELTA_SWING_TABLE_5GA_N: [[u8; DELTA_SWING_IDX_SIZE]; 3] = [
    [
        0, 1, 1, 2, 2, 3, 4, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 15, 15,
        15, 15, 15,
    ],
    [
        0, 1, 1, 2, 2, 3, 4, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 15, 15,
        15, 15, 15,
    ],
    [
        0, 1, 1, 2, 2, 3, 4, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 15, 15,
        15, 15, 15,
    ],
];
const DELTA_SWING_TABLE_5GB_P: [[u8; DELTA_SWING_IDX_SIZE]; 3] = [
    [
        0, 1, 1, 2, 2, 3, 3, 4, 5, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 11, 11, 11, 11, 11, 11, 11,
        11, 11, 11,
    ],
    [
        0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 11, 11, 11, 11, 11, 11, 11,
        11, 11, 11,
    ],
    [
        0, 1, 1, 2, 3, 3, 4, 5, 6, 7, 7, 8, 8, 9, 9, 10, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
        11, 11, 11, 11,
    ],
];
const DELTA_SWING_TABLE_5GB_N: [[u8; DELTA_SWING_IDX_SIZE]; 3] = [
    [
        0, 1, 1, 2, 2, 3, 4, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 14,
        14, 14, 14,
    ],
    [
        0, 1, 1, 2, 2, 3, 4, 4, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 14,
        14, 14, 14,
    ],
    [
        0, 1, 1, 2, 2, 3, 4, 5, 6, 6, 7, 8, 8, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 16,
        16, 16, 16, 16,
    ],
];

const TX_SCALING_TABLE_JAGUAR: [u32; TX_SCALE_TABLE_SIZE] = [
    0x081, 0x088, 0x090, 0x099, 0x0a2, 0x0ac, 0x0b6, 0x0c0, 0x0cc, 0x0d8, 0x0e5, 0x0f2, 0x101,
    0x110, 0x120, 0x131, 0x143, 0x156, 0x16a, 0x180, 0x197, 0x1af, 0x1c8, 0x1e3, 0x200, 0x21e,
    0x23e, 0x261, 0x285, 0x2ab, 0x2d3, 0x2fe, 0x32b, 0x35c, 0x38e, 0x3c4, 0x3fe,
];

/// Mutable RTL8812 thermal power-tracking state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerTrackingState {
    /// True after initialization has populated the state.
    pub initialized: bool,
    /// True if EFUSE thermal calibration enables power tracking.
    pub enabled: bool,
    /// EFUSE thermal baseline.
    pub eeprom_thermal: u8,
    /// Default OFDM swing table index.
    pub default_ofdm_index: u8,
    /// Last raw thermal reading.
    pub thermal_value: u8,
    /// Moving-average thermal samples.
    pub thermal_value_avg: [u8; AVG_THERMAL_NUM],
    /// Moving-average write index.
    pub thermal_value_avg_index: u8,
    /// Absolute OFDM swing adjustment per RF path.
    pub absolute_ofdm_swing_idx: [i8; 2],
    /// Current power-index delta per RF path.
    pub delta_power_index: [i8; 2],
    /// Previous power-index delta per RF path.
    pub delta_power_index_last: [i8; 2],
}

impl Default for PowerTrackingState {
    fn default() -> Self {
        Self {
            initialized: false,
            enabled: false,
            eeprom_thermal: 0xff,
            default_ofdm_index: DEFAULT_OFDM_INDEX,
            thermal_value: 0xff,
            thermal_value_avg: [0; AVG_THERMAL_NUM],
            thermal_value_avg_index: 0,
            absolute_ofdm_swing_idx: [0; 2],
            delta_power_index: [0; 2],
            delta_power_index_last: [0; 2],
        }
    }
}

/// Report returned by one RTL8812 power-tracking tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerTrackingReport {
    /// Whether power tracking is enabled.
    pub enabled: bool,
    /// Raw thermal reading from hardware.
    pub thermal_raw: u8,
    /// Averaged thermal value used for decisions.
    pub thermal_average: u8,
    /// EFUSE thermal baseline.
    pub eeprom_thermal: u8,
    /// Absolute thermal delta from baseline.
    pub delta: u8,
    /// Default OFDM swing table index.
    pub default_ofdm_index: u8,
    /// Final OFDM swing index per RF path.
    pub final_ofdm_index: [u8; 2],
    /// Applied swing delta per RF path.
    pub swing_delta: [i8; 2],
    /// True when the tick wrote new swing values.
    pub applied: bool,
}

impl RealtekDevice {
    /// Initialize RTL8812 thermal TX power tracking state.
    pub async fn init_power_tracking_8812_async(
        &self,
        state: &mut PowerTrackingState,
    ) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family != ChipFamily::Rtl8812 {
            return Err(DriverError::UnsupportedPowerTrackingPath(chip.family));
        }
        let efuse = self.read_efuse_info_async(chip).await?;
        let swing_idx = self.lookup_tx_swing_index_from_bb_async().await?;
        state.default_ofdm_index = swing_idx.unwrap_or(DEFAULT_OFDM_INDEX);
        state.eeprom_thermal = efuse.thermal_meter;
        state.enabled = efuse.thermal_meter != 0xff;
        state.thermal_value = efuse.thermal_meter;
        state.thermal_value_avg = [0; AVG_THERMAL_NUM];
        state.thermal_value_avg_index = 0;
        state.absolute_ofdm_swing_idx = [0; 2];
        state.delta_power_index = [0; 2];
        state.delta_power_index_last = [0; 2];
        state.initialized = true;
        Ok(())
    }

    /// Reset RTL8812 thermal TX power tracking state to EFUSE/default values.
    pub async fn clear_power_tracking_8812_async(
        &self,
        state: &mut PowerTrackingState,
    ) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family != ChipFamily::Rtl8812 {
            return Err(DriverError::UnsupportedPowerTrackingPath(chip.family));
        }
        let efuse = self.read_efuse_info_async(chip).await?;
        state.thermal_value_avg = [0; AVG_THERMAL_NUM];
        state.thermal_value_avg_index = 0;
        state.absolute_ofdm_swing_idx = [0; 2];
        state.delta_power_index = [0; 2];
        state.delta_power_index_last = [0; 2];
        state.thermal_value = efuse.thermal_meter;
        state.eeprom_thermal = efuse.thermal_meter;
        state.enabled = efuse.thermal_meter != 0xff;
        state.default_ofdm_index = self
            .lookup_tx_swing_index_from_bb_async()
            .await?
            .unwrap_or(DEFAULT_OFDM_INDEX);
        state.initialized = true;
        Ok(())
    }

    /// Run one RTL8812 thermal TX power tracking tick.
    pub async fn tick_power_tracking_8812_async(
        &self,
        state: &mut PowerTrackingState,
        channel: u8,
        _width: ChannelWidth,
    ) -> Result<PowerTrackingReport, DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family != ChipFamily::Rtl8812 {
            return Err(DriverError::UnsupportedPowerTrackingPath(chip.family));
        }
        if !state.initialized {
            self.init_power_tracking_8812_async(state).await?;
        }

        let thermal_raw = self.read_rf_thermal_8812_async(chip).await?;
        if !state.enabled {
            return Ok(PowerTrackingReport {
                enabled: false,
                thermal_raw,
                thermal_average: thermal_raw,
                eeprom_thermal: state.eeprom_thermal,
                delta: 0,
                default_ofdm_index: state.default_ofdm_index,
                final_ofdm_index: [state.default_ofdm_index; 2],
                swing_delta: state.absolute_ofdm_swing_idx,
                applied: false,
            });
        }

        state.thermal_value_avg[state.thermal_value_avg_index as usize] = thermal_raw;
        state.thermal_value_avg_index = (state.thermal_value_avg_index + 1) % AVG_THERMAL_NUM as u8;
        let mut sum = 0u32;
        let mut count = 0u32;
        for sample in state.thermal_value_avg {
            if sample != 0 {
                sum += u32::from(sample);
                count += 1;
            }
        }
        let thermal_average = sum
            .checked_div(count)
            .map(|value| value as u8)
            .unwrap_or(thermal_raw);
        let delta_abs = thermal_average.abs_diff(state.thermal_value);
        let mut delta = thermal_average.abs_diff(state.eeprom_thermal);
        if delta as usize >= DELTA_SWING_IDX_SIZE {
            delta = (DELTA_SWING_IDX_SIZE - 1) as u8;
        }

        let mut applied = false;
        if delta_abs > 0 {
            update_delta_power_indices(state, channel, thermal_average, delta);
            if state.delta_power_index != state.delta_power_index_last {
                self.apply_power_tracking_swing_8812_async(state, chip)
                    .await?;
                applied = true;
            }
        }
        state.thermal_value = thermal_average;

        Ok(PowerTrackingReport {
            enabled: true,
            thermal_raw,
            thermal_average,
            eeprom_thermal: state.eeprom_thermal,
            delta,
            default_ofdm_index: state.default_ofdm_index,
            final_ofdm_index: final_indices(state),
            swing_delta: state.absolute_ofdm_swing_idx,
            applied,
        })
    }

    async fn lookup_tx_swing_index_from_bb_async(&self) -> Result<Option<u8>, DriverError> {
        let bb_swing = self
            .query_bb_reg_async(R_A_TX_SCALE_JAGUAR, 0xffe0_0000)
            .await?;
        Ok(TX_SCALING_TABLE_JAGUAR
            .iter()
            .position(|value| *value == bb_swing)
            .map(|idx| idx as u8))
    }

    async fn read_rf_thermal_8812_async(
        &self,
        chip: crate::types::ChipInfo,
    ) -> Result<u8, DriverError> {
        let rf = self.query_rf_reg_async(chip, RfPath::A, 0x42).await?;
        Ok(((rf & 0x0000_fc00) >> 10) as u8)
    }

    async fn apply_power_tracking_swing_8812_async(
        &self,
        state: &PowerTrackingState,
        chip: crate::types::ChipInfo,
    ) -> Result<(), DriverError> {
        for (path_index, register) in [R_A_TX_SCALE_JAGUAR, R_B_TX_SCALE_JAGUAR]
            .into_iter()
            .take(chip.total_rf_paths().min(2))
            .enumerate()
        {
            let idx = final_index(state, path_index);
            self.set_bb_reg_async(register, 0xffe0_0000, TX_SCALING_TABLE_JAGUAR[idx as usize])
                .await?;
        }
        Ok(())
    }
}

fn update_delta_power_indices(
    state: &mut PowerTrackingState,
    channel: u8,
    thermal_value: u8,
    delta: u8,
) {
    let warmer = thermal_value > state.eeprom_thermal;
    for path in 0..2 {
        state.delta_power_index_last[path] = state.delta_power_index[path];
        let table_value = tracking_table(path, warmer, channel)[delta as usize] as i8;
        let signed = if warmer { table_value } else { -table_value };
        state.delta_power_index[path] = signed;
        state.absolute_ofdm_swing_idx[path] = signed;
    }
}

fn tracking_table(path: usize, warmer: bool, channel: u8) -> &'static [u8; DELTA_SWING_IDX_SIZE] {
    if channel <= 14 {
        return match (path, warmer) {
            (0, true) => &DELTA_SWING_TABLE_2GA_P,
            (0, false) => &DELTA_SWING_TABLE_2GA_N,
            (_, true) => &DELTA_SWING_TABLE_2GB_P,
            (_, false) => &DELTA_SWING_TABLE_2GB_N,
        };
    }
    let bucket = if (36..=64).contains(&channel) {
        0
    } else if (100..=144).contains(&channel) {
        1
    } else {
        2
    };
    match (path, warmer) {
        (0, true) => &DELTA_SWING_TABLE_5GA_P[bucket],
        (0, false) => &DELTA_SWING_TABLE_5GA_N[bucket],
        (_, true) => &DELTA_SWING_TABLE_5GB_P[bucket],
        (_, false) => &DELTA_SWING_TABLE_5GB_N[bucket],
    }
}

fn final_indices(state: &PowerTrackingState) -> [u8; 2] {
    [final_index(state, 0), final_index(state, 1)]
}

fn final_index(state: &PowerTrackingState, path: usize) -> u8 {
    let idx = i16::from(state.default_ofdm_index) + i16::from(state.absolute_ofdm_swing_idx[path]);
    idx.clamp(0, PWR_TRACKING_LIMIT) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_tables_match_devourer_examples() {
        let mut state = PowerTrackingState {
            initialized: true,
            enabled: true,
            eeprom_thermal: 20,
            default_ofdm_index: 24,
            ..PowerTrackingState::default()
        };
        update_delta_power_indices(&mut state, 36, 25, 5);
        assert_eq!(state.absolute_ofdm_swing_idx, [3, 3]);
        assert_eq!(final_indices(&state), [26, 26]);

        update_delta_power_indices(&mut state, 6, 15, 5);
        assert_eq!(state.absolute_ofdm_swing_idx, [-2, -2]);
        assert_eq!(final_indices(&state), [22, 22]);
    }
}
