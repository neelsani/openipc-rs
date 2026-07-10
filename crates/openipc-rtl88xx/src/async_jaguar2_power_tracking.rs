//! Jaguar2 RTL8822B/RTL8821C thermal TX-power compensation.

use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::{ChipFamily, DriverError};

const N: usize = 30;
const SCALE: [u32; 37] = [
    0x081, 0x088, 0x090, 0x099, 0x0a2, 0x0ac, 0x0b6, 0x0c0, 0x0cc, 0x0d8, 0x0e5, 0x0f2, 0x101,
    0x110, 0x120, 0x131, 0x143, 0x156, 0x16a, 0x180, 0x197, 0x1af, 0x1c8, 0x1e3, 0x200, 0x21e,
    0x23e, 0x261, 0x285, 0x2ab, 0x2d3, 0x2fe, 0x32b, 0x35c, 0x38e, 0x3c4, 0x3fe,
];

const B2_A_UP: [u8; N] = [
    0, 1, 2, 3, 3, 4, 5, 6, 6, 7, 8, 9, 9, 10, 11, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21,
    22, 22, 22,
];
const B2_A_DOWN: [u8; N] = [
    0, 1, 2, 3, 3, 4, 5, 6, 6, 7, 8, 9, 10, 11, 12, 13, 13, 14, 15, 16, 17, 18, 18, 18, 18, 18, 18,
    18, 18, 18,
];
const B2_B_UP: [u8; N] = [
    0, 1, 1, 2, 3, 4, 4, 5, 6, 7, 7, 8, 9, 10, 11, 12, 12, 13, 14, 15, 16, 17, 17, 18, 19, 20, 21,
    22, 22, 22,
];
const B2_B_DOWN: [u8; N] = [
    0, 1, 2, 3, 3, 4, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 13, 14, 15, 16, 16, 17, 18, 18, 18, 18, 18,
    18, 18, 18,
];
const B5_A_UP: [[u8; N]; 3] = [
    [
        0, 1, 2, 2, 3, 4, 5, 5, 6, 7, 8, 9, 9, 10, 11, 12, 13, 14, 14, 15, 16, 17, 18, 19, 19, 20,
        20, 20, 20, 20,
    ],
    [
        0, 1, 2, 2, 3, 4, 4, 5, 6, 6, 7, 7, 8, 9, 9, 10, 11, 11, 12, 13, 14, 15, 16, 16, 17, 17,
        18, 18, 18, 18,
    ],
    [
        0, 1, 2, 3, 3, 4, 5, 5, 6, 6, 7, 8, 8, 9, 10, 11, 12, 12, 13, 14, 15, 15, 16, 17, 17, 18,
        18, 18, 18, 18,
    ],
];
const B5_A_DOWN: [[u8; N]; 3] = [
    [
        0, 1, 2, 2, 3, 3, 4, 5, 6, 7, 8, 8, 9, 9, 10, 11, 11, 12, 12, 12, 13, 13, 14, 14, 14, 15,
        15, 15, 15, 15,
    ],
    [
        0, 1, 2, 2, 3, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 14,
        14, 14, 14, 14,
    ],
    [
        0, 1, 2, 2, 3, 4, 4, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 12, 12, 13, 13, 14, 14, 14, 14,
        14, 14, 14,
    ],
];
const B5_B_UP: [[u8; N]; 3] = [
    [
        0, 1, 2, 2, 3, 4, 5, 6, 7, 8, 8, 9, 10, 11, 11, 12, 13, 14, 15, 15, 16, 17, 18, 18, 19, 19,
        19, 19, 19, 19,
    ],
    [
        0, 1, 2, 2, 3, 4, 5, 6, 6, 7, 8, 8, 9, 9, 10, 11, 12, 12, 13, 14, 15, 16, 17, 17, 18, 18,
        18, 18, 18, 18,
    ],
    [
        0, 1, 2, 2, 3, 4, 5, 5, 6, 6, 7, 8, 8, 9, 10, 10, 11, 12, 13, 14, 15, 15, 16, 16, 17, 17,
        17, 17, 17, 17,
    ],
];
const B5_B_DOWN: [[u8; N]; 3] = [
    [
        0, 1, 2, 3, 3, 4, 5, 6, 6, 7, 8, 9, 9, 10, 10, 11, 11, 12, 12, 12, 13, 13, 14, 14, 14, 15,
        15, 15, 15, 15,
    ],
    [
        0, 1, 1, 2, 2, 3, 3, 4, 5, 5, 6, 7, 7, 8, 8, 9, 10, 10, 11, 12, 12, 13, 13, 14, 14, 14, 14,
        14, 14, 14,
    ],
    [
        0, 1, 2, 2, 3, 3, 4, 4, 5, 6, 6, 7, 7, 8, 9, 9, 10, 10, 11, 12, 12, 13, 13, 14, 14, 14, 14,
        14, 14, 14,
    ],
];

const C2_UP: [u8; N] = [
    0, 1, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 4, 4, 5, 5, 5, 5, 6, 6, 6, 7, 7, 7, 8, 8, 9, 9, 9, 9,
];
const C2_DOWN: [u8; N] = [
    0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 6, 7, 7, 8, 8, 9,
];
const C5_UP: [[u8; N]; 3] = [
    [
        0, 1, 1, 2, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 11, 11, 12, 12, 12, 12, 12,
        12, 12,
    ],
    [
        0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 5, 6, 7, 7, 8, 8, 9, 10, 10, 11, 11, 12, 12, 12, 12, 12,
        12, 12, 12,
    ],
    [
        0, 1, 1, 1, 2, 3, 3, 3, 4, 4, 4, 5, 6, 6, 7, 7, 8, 8, 9, 10, 10, 11, 11, 12, 12, 12, 12,
        12, 12, 12,
    ],
];
const C5_DOWN: [[u8; N]; 3] = [
    [
        0, 1, 1, 2, 3, 3, 3, 4, 4, 5, 5, 6, 6, 6, 7, 8, 8, 8, 9, 9, 9, 10, 10, 11, 11, 12, 12, 12,
        12, 12,
    ],
    [
        0, 1, 1, 1, 2, 3, 3, 4, 4, 5, 5, 5, 6, 6, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 12, 12,
        12, 12, 12,
    ],
    [
        0, 1, 2, 2, 3, 4, 4, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 9, 10, 10, 11, 11, 12, 12, 12, 12,
        12, 12,
    ],
];

/// Mutable Jaguar2 thermal compensation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jaguar2PowerTrackingState {
    /// Whether the state has been initialized from EFUSE and current radio state.
    pub initialized: bool,
    /// Whether the EFUSE thermal baseline is programmed.
    pub enabled: bool,
    /// EFUSE thermal baseline.
    pub baseline: u8,
    /// Channel used to select the vendor delta table.
    pub channel: u8,
    /// Default BB swing index read from the current table-programmed state.
    pub default_ofdm_index: u8,
    samples: [u8; 4],
    sample_index: u8,
    last_swing: [i16; 2],
}

impl Default for Jaguar2PowerTrackingState {
    fn default() -> Self {
        Self {
            initialized: false,
            enabled: false,
            baseline: 0xff,
            channel: 0,
            default_ofdm_index: 24,
            samples: [0; 4],
            sample_index: 0,
            last_swing: [i16::MAX; 2],
        }
    }
}

/// Result of one Jaguar2 thermal compensation tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jaguar2PowerTrackingReport {
    /// Whether compensation is enabled by EFUSE.
    pub enabled: bool,
    /// Latest raw RF thermal sample.
    pub thermal_raw: u8,
    /// Four-sample moving average.
    pub thermal_average: u8,
    /// Absolute delta from the EFUSE baseline.
    pub delta: u8,
    /// Signed vendor swing selected for each path.
    pub swing: [i16; 2],
    /// Whether this tick changed hardware registers.
    pub applied: bool,
}

impl RealtekDevice {
    /// Initialize Jaguar2 thermal tracking from EFUSE and the active channel.
    pub async fn init_jaguar2_power_tracking_async(
        &self,
        state: &mut Jaguar2PowerTrackingState,
    ) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if !chip.family.is_jaguar2() {
            return Err(DriverError::UnsupportedPowerTrackingPath(chip.family));
        }
        let radio = self
            .current_radio_config()?
            .ok_or(DriverError::RadioNotInitialized)?;
        let efuse = self.read_efuse_info_async(chip).await?;
        let scale = self.query_bb_reg_async(0x0c1c, 0xffe0_0000).await?;
        state.initialized = true;
        state.enabled = efuse.thermal_meter != 0xff;
        state.baseline = efuse.thermal_meter;
        state.channel = radio.channel;
        state.default_ofdm_index = SCALE
            .iter()
            .position(|value| *value == scale)
            .map_or(24, |index| index as u8);
        state.samples = [0; 4];
        state.sample_index = 0;
        state.last_swing = [i16::MAX; 2];
        Ok(())
    }

    /// Run one Devourer-compatible Jaguar2 thermal compensation tick.
    pub async fn tick_jaguar2_power_tracking_async(
        &self,
        state: &mut Jaguar2PowerTrackingState,
    ) -> Result<Jaguar2PowerTrackingReport, DriverError> {
        if !state.initialized {
            self.init_jaguar2_power_tracking_async(state).await?;
        }
        let chip = self.probe_chip_async().await?;
        if !chip.family.is_jaguar2() {
            return Err(DriverError::UnsupportedPowerTrackingPath(chip.family));
        }
        let thermal = ((self.query_rf_reg_async(chip, RfPath::A, 0x42).await? >> 10) & 0x3f) as u8;
        if !state.enabled {
            return Ok(Jaguar2PowerTrackingReport {
                enabled: false,
                thermal_raw: thermal,
                thermal_average: thermal,
                delta: 0,
                swing: [0; 2],
                applied: false,
            });
        }
        state.samples[usize::from(state.sample_index)] = thermal;
        state.sample_index = (state.sample_index + 1) & 3;
        let (sum, count) = state
            .samples
            .into_iter()
            .filter(|sample| *sample != 0)
            .fold((0u16, 0u16), |(sum, count), sample| {
                (sum + u16::from(sample), count + 1)
            });
        let average = sum
            .checked_div(count)
            .map_or(thermal, |average| average as u8);
        let delta = average.abs_diff(state.baseline).min((N - 1) as u8);
        let warmer = average > state.baseline;
        let paths = if chip.family == ChipFamily::Rtl8821c {
            1
        } else {
            chip.total_rf_paths().min(2)
        };
        let mut swing = [0i16; 2];
        let mut applied = false;
        let current_ofdm = self
            .tx_power_control
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .ofdm_index
            .unwrap_or(63);
        for (path, tracked_swing) in swing.iter_mut().enumerate().take(paths) {
            let amount = jaguar2_swing(chip.family, path, state.channel, warmer, delta);
            *tracked_swing = if warmer {
                i16::from(amount)
            } else {
                -i16::from(amount)
            };
            if state.last_swing[path] == *tracked_swing {
                continue;
            }
            self.apply_jaguar2_power_tracking_swing_async(
                chip.family,
                path,
                *tracked_swing,
                current_ofdm,
                state.default_ofdm_index,
            )
            .await?;
            state.last_swing[path] = *tracked_swing;
            applied = true;
        }
        Ok(Jaguar2PowerTrackingReport {
            enabled: true,
            thermal_raw: thermal,
            thermal_average: average,
            delta,
            swing,
            applied,
        })
    }

    async fn apply_jaguar2_power_tracking_swing_async(
        &self,
        family: ChipFamily,
        path: usize,
        swing: i16,
        current_ofdm: u8,
        default_swing: u8,
    ) -> Result<(), DriverError> {
        let headroom = i16::from(63u8.saturating_sub(current_ofdm)).min(15);
        let upper = i16::from(default_swing) + 10;
        let (txagc, bb) = if (0..=headroom).contains(&swing) {
            (swing, i16::from(default_swing))
        } else if swing > headroom {
            (
                headroom,
                (i16::from(default_swing) + swing - headroom).min(upper),
            )
        } else {
            (0, (i16::from(default_swing) + swing).max(0))
        };
        let bb = bb.clamp(0, 36) as usize;
        if family == ChipFamily::Rtl8821c {
            self.set_bb_reg_async(0x0c94, 0x0000_007e, txagc as u32 & 0x3f)
                .await?;
            self.set_bb_reg_async(0x0c1c, 0xffe0_0000, SCALE[bb]).await
        } else {
            let coarse = if path == 0 { 0x0c94 } else { 0x0e94 };
            let scale = if path == 0 { 0x0c1c } else { 0x0e1c };
            self.set_bb_reg_async(coarse, 0x3e00_0000, txagc as u32 & 0x1f)
                .await?;
            self.set_bb_reg_async(scale, 0xffe0_0000, SCALE[bb]).await
        }
    }
}

fn jaguar2_swing(family: ChipFamily, path: usize, channel: u8, warmer: bool, delta: u8) -> u8 {
    let index = usize::from(delta.min((N - 1) as u8));
    let subband = if channel <= 64 {
        0
    } else if channel <= 144 {
        1
    } else {
        2
    };
    if family == ChipFamily::Rtl8821c {
        return if channel <= 14 {
            if warmer {
                C2_UP[index]
            } else {
                C2_DOWN[index]
            }
        } else if warmer {
            C5_UP[subband][index]
        } else {
            C5_DOWN[subband][index]
        };
    }
    if channel <= 14 {
        match (path, warmer) {
            (0, true) => B2_A_UP[index],
            (0, false) => B2_A_DOWN[index],
            (_, true) => B2_B_UP[index],
            (_, false) => B2_B_DOWN[index],
        }
    } else {
        match (path, warmer) {
            (0, true) => B5_A_UP[subband][index],
            (0, false) => B5_A_DOWN[subband][index],
            (_, true) => B5_B_UP[subband][index],
            (_, false) => B5_B_DOWN[subband][index],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_tables_select_by_variant_path_band_and_direction() {
        assert_eq!(jaguar2_swing(ChipFamily::Rtl8822b, 0, 36, true, 10), 8);
        assert_eq!(jaguar2_swing(ChipFamily::Rtl8822b, 1, 161, false, 10), 6);
        assert_eq!(jaguar2_swing(ChipFamily::Rtl8821c, 0, 6, false, 29), 9);
        assert_eq!(jaguar2_swing(ChipFamily::Rtl8821c, 0, 149, true, 29), 12);
    }
}
