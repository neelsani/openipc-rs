use crate::async_efuse::EfuseInfo;
use crate::device::RealtekDevice;
use crate::phy::{bit_shift, RfPath};
use crate::regs::*;
use crate::types::{ChipFamily, DriverError};

const RF_THERMAL_METER_REG: u16 = 0x42;
const RF_THERMAL_METER_MASK: u32 = 0x0000_fc00;
const BB_DBGPORT_SELECTOR_REG: u16 = 0x08fc;
const BB_DBGPORT_READBACK_REG: u16 = 0x0fa0;
const FIFO_PAGE_INFO_REGS_8814: [u16; 5] = [
    REG_FIFOPAGE_INFO_1_8814,
    REG_FIFOPAGE_INFO_2_8814,
    REG_FIFOPAGE_INFO_3_8814,
    REG_FIFOPAGE_INFO_4_8814,
    REG_FIFOPAGE_INFO_5_8814,
];

/// Thermal-meter readout and EFUSE baseline comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThermalStatus {
    /// Raw thermal-meter value.
    pub raw: u8,
    /// EFUSE thermal baseline.
    pub baseline: u8,
    /// Difference between raw and baseline.
    pub delta: i16,
    /// True when the baseline was programmed and the reading is meaningful.
    pub valid: bool,
}

impl ThermalStatus {
    /// Classify the thermal delta into a coarse UI/debug bucket.
    pub const fn bucket(self) -> ThermalBucket {
        if !self.valid {
            ThermalBucket::Unknown
        } else if self.delta < 8 {
            ThermalBucket::Cool
        } else if self.delta < 15 {
            ThermalBucket::Warm
        } else if self.delta < 25 {
            ThermalBucket::Hot
        } else {
            ThermalBucket::Critical
        }
    }
}

/// Coarse thermal status bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalBucket {
    /// Baseline or reading is not valid.
    Unknown,
    /// Thermal delta is low.
    Cool,
    /// Thermal delta is elevated but normal.
    Warm,
    /// Thermal delta is high.
    Hot,
    /// Thermal delta is critical.
    Critical,
}

/// Result of a BB debug-port read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BbDbgportRead {
    /// Debug selector requested by the caller.
    pub selector: u32,
    /// Value read back through the debug port.
    pub value: u32,
    /// Selector value that was present before the read.
    pub saved_selector: u32,
    /// True if the debug-port read indicates the chip is still responsive.
    pub chip_alive: bool,
}

impl RealtekDevice {
    /// Read the RF thermal meter and compare it to EFUSE baseline data.
    pub async fn read_thermal_status_async(&self) -> Result<ThermalStatus, DriverError> {
        let chip = self.probe_chip_async().await?;
        let efuse = if let Some(efuse) = self.efuse_info.get().copied() {
            efuse
        } else {
            let efuse = self.read_efuse_info_async(chip).await?;
            let _ = self.efuse_info.set(efuse);
            efuse
        };
        self.read_thermal_status_with_efuse_async(chip, efuse).await
    }

    /// Read RTL8814A FIFO page depth registers for queue diagnostics.
    pub async fn read_queue_depth_8814_async(&self) -> Result<[u32; 5], DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family != ChipFamily::Rtl8814 {
            return Ok([0; 5]);
        }

        let mut out = [0u32; 5];
        for (idx, register) in FIFO_PAGE_INFO_REGS_8814.iter().copied().enumerate() {
            out[idx] = self.read_u32_async(register).await?;
        }
        Ok(out)
    }

    /// Read a masked baseband register.
    pub async fn read_bb_reg_async(&self, register: u16, mask: u32) -> Result<u32, DriverError> {
        self.query_bb_reg_async(register, mask).await
    }

    /// Read a baseband debug-port selector and restore the previous selector.
    pub async fn read_bb_dbgport_async(&self, selector: u32) -> Result<BbDbgportRead, DriverError> {
        let saved_selector = self.read_u32_async(BB_DBGPORT_SELECTOR_REG).await?;
        self.write_u32_async(BB_DBGPORT_SELECTOR_REG, selector)
            .await?;
        let value = self.read_u32_async(BB_DBGPORT_READBACK_REG).await?;
        self.write_u32_async(BB_DBGPORT_SELECTOR_REG, saved_selector)
            .await?;

        let sys_cfg = self.read_u32_async(REG_SYS_CFG).await.unwrap_or(0);
        let chip_alive = sys_cfg != 0 && sys_cfg != u32::MAX;
        Ok(BbDbgportRead {
            selector,
            value,
            saved_selector,
            chip_alive,
        })
    }

    pub(crate) async fn read_thermal_status_with_efuse_async(
        &self,
        chip: crate::types::ChipInfo,
        efuse: EfuseInfo,
    ) -> Result<ThermalStatus, DriverError> {
        let rf = self
            .query_rf_reg_async(chip, RfPath::A, RF_THERMAL_METER_REG)
            .await?;
        let raw = ((rf & RF_THERMAL_METER_MASK) >> bit_shift(RF_THERMAL_METER_MASK)) as u8;
        let baseline = efuse.thermal_meter;
        Ok(ThermalStatus {
            raw,
            baseline,
            delta: if baseline == 0xff {
                0
            } else {
                i16::from(raw) - i16::from(baseline)
            },
            valid: baseline != 0xff,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thermal_buckets_match_devourer_thresholds() {
        assert_eq!(
            ThermalStatus {
                raw: 10,
                baseline: 0xff,
                delta: 0,
                valid: false,
            }
            .bucket(),
            ThermalBucket::Unknown
        );
        assert_eq!(
            ThermalStatus {
                raw: 31,
                baseline: 24,
                delta: 7,
                valid: true,
            }
            .bucket(),
            ThermalBucket::Cool
        );
        assert_eq!(
            ThermalStatus {
                raw: 39,
                baseline: 24,
                delta: 15,
                valid: true,
            }
            .bucket(),
            ThermalBucket::Hot
        );
    }
}
