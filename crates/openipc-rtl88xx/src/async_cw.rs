//! Devourer-compatible MP single-tone (CW carrier) control.

use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::regs::{BIT1, BIT28, BIT29, B_LSSI_WRITE_DATA, B_MASK_DWORD};
use crate::types::{ChipFamily, ChipInfo, DriverError};

const RF_AC: u16 = 0x00;
const RF_LO_ENABLE: u16 = 0x58;
const RF_MASK: u32 = 0x000f_ffff;

#[derive(Debug, Clone, Copy)]
struct CwToneSnapshot {
    chip: ChipInfo,
    channel: u8,
    rf0: u32,
    bb: [u32; 4],
}

#[derive(Debug, Default)]
pub(crate) struct CwToneState {
    snapshot: Option<CwToneSnapshot>,
}

impl CwToneState {
    pub(crate) const fn is_active(&self) -> bool {
        self.snapshot.is_some()
    }
}

impl RealtekDevice {
    /// Radiate a bare RF carrier at the center of the currently tuned channel.
    ///
    /// This is the same SDR-validated MP-mode sequence used by devourer for
    /// Jaguar1, Jaguar2, and Jaguar3. It is intended for RF testing, not normal
    /// OpenIPC reception. Always call [`Self::stop_cw_tone_async`] afterwards.
    pub async fn start_cw_tone_async(&self, channel: u8, gain: u8) -> Result<(), DriverError> {
        if self
            .cw_tone
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .snapshot
            .is_some()
        {
            return Ok(());
        }

        let chip = self.probe_chip_async().await?;
        let gain = gain & 0x1f;
        let attempts = if chip.family.is_jaguar3() { 4 } else { 1 };
        let mut last_error = None;
        for attempt in 1..=attempts {
            match self.start_cw_tone_once(chip, channel, gain).await {
                Ok(snapshot) => {
                    self.cw_tone
                        .lock()
                        .map_err(|_| DriverError::DriverStatePoisoned)?
                        .snapshot = Some(snapshot);
                    log::info!(target: "openipc_rtl88xx::cw", "CW tone armed chip={} channel={} gain={}", chip.family.name(), channel, gain);
                    return Ok(());
                }
                Err(err) => {
                    last_error = Some(err);
                    if attempt < attempts {
                        log::warn!(target: "openipc_rtl88xx::cw", "CW tone arm hit a USB/register error; retry {attempt}/{attempts}");
                        crate::time::sleep_ms(150).await;
                    }
                }
            }
        }
        Err(last_error.expect("CW tone attempt count is non-zero"))
    }

    /// Stop a CW carrier and restore the RF/baseband state captured at start.
    pub async fn stop_cw_tone_async(&self) -> Result<(), DriverError> {
        let snapshot = self
            .cw_tone
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .snapshot;
        let Some(snapshot) = snapshot else {
            return Ok(());
        };

        self.stop_cw_tone_once(snapshot).await?;
        self.cw_tone
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .snapshot = None;
        log::info!(target: "openipc_rtl88xx::cw", "CW tone stopped; RF/BB state restored");
        Ok(())
    }

    async fn start_cw_tone_once(
        &self,
        chip: ChipInfo,
        channel: u8,
        gain: u8,
    ) -> Result<CwToneSnapshot, DriverError> {
        match chip.family {
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                let registers = [0x0cb0, 0x0eb0, 0x0cb4, 0x0eb4];
                let bb = self.read_bb_snapshot(registers).await?;
                let rf0 = self.query_rf_reg_async(chip, RfPath::A, RF_AC).await?;
                self.set_bb_reg_async(0x0808, BIT28 | BIT29, 0).await?;
                self.key_jaguar12_rf(chip, gain).await?;
                self.set_bb_reg_async(0x0cb0, 0x00ff_00f0, 0x0007_7007)
                    .await?;
                self.set_bb_reg_async(0x0eb0, 0x00ff_00f0, 0x0007_7007)
                    .await?;
                let efuse = self.read_efuse_info_async(chip).await?;
                let pa = if efuse.external_pa_5g {
                    Some(0x12)
                } else if efuse.external_pa_2g {
                    Some(0x11)
                } else {
                    None
                };
                if let Some(pa) = pa {
                    self.set_bb_reg_async(0x0cb4, 0x0ff0_0000, pa).await?;
                    self.set_bb_reg_async(0x0eb4, 0x0ff0_0000, pa).await?;
                }
                Ok(CwToneSnapshot {
                    chip,
                    channel,
                    rf0,
                    bb,
                })
            }
            ChipFamily::Rtl8814 => {
                let registers = [0x0c1c, 0x0e1c, 0x181c, 0x1a1c];
                let bb = self.read_bb_snapshot(registers).await?;
                let rf0 = self.query_rf_reg_async(chip, RfPath::A, RF_AC).await?;
                self.set_bb_reg_async(0x0838, BIT1, 1).await?;
                self.key_jaguar12_rf(chip, gain).await?;
                for register in registers {
                    self.set_bb_reg_async(register, 0xffe0_0000, 0).await?;
                }
                Ok(CwToneSnapshot {
                    chip,
                    channel,
                    rf0,
                    bb,
                })
            }
            ChipFamily::Rtl8822b => {
                let registers = [0x0cb0, 0x0eb0, 0x0cb4, 0x0eb4];
                let bb = self.read_bb_snapshot(registers).await?;
                let rf0 = self.query_rf_reg_async(chip, RfPath::A, RF_AC).await?;
                self.set_bb_reg_async(0x0808, BIT28 | BIT29, 0).await?;
                self.key_jaguar12_rf(chip, gain).await?;
                self.set_bb_reg_async(0x0cb0, B_MASK_DWORD, 0x7777_7777)
                    .await?;
                self.set_bb_reg_async(0x0eb0, B_MASK_DWORD, 0x7777_7777)
                    .await?;
                self.set_bb_reg_async(0x0cb4, 0x0000_ffff, 0x7777).await?;
                self.set_bb_reg_async(0x0eb4, 0x0000_ffff, 0x7777).await?;
                self.set_bb_reg_async(0x0cbc, 0x0000_0fff, 0x00b).await?;
                self.set_bb_reg_async(0x0ebc, 0x0000_0fff, 0x830).await?;
                Ok(CwToneSnapshot {
                    chip,
                    channel,
                    rf0,
                    bb,
                })
            }
            ChipFamily::Rtl8821c => {
                let registers = [0x0cb0, 0x0eb0, 0x0cb4, 0x0eb4];
                let bb = self.read_bb_snapshot(registers).await?;
                let rf0 = self.query_rf_reg_async(chip, RfPath::A, RF_AC).await?;
                self.set_bb_reg_async(0x0808, BIT28 | BIT29, 0).await?;
                self.set_rf_reg_async(chip, RfPath::A, RF_AC, 0xf0000, 2)
                    .await?;
                self.set_rf_reg_async(chip, RfPath::A, RF_AC, 0x1f, u32::from(gain))
                    .await?;
                if channel <= 14 {
                    self.set_rf_reg_async(chip, RfPath::A, 0x75, 1 << 16, 1)
                        .await?;
                } else {
                    self.set_rf_reg_async(chip, RfPath::A, RF_LO_ENABLE, BIT1, 1)
                        .await?;
                }
                self.set_bb_reg_async(0x0cb0, 0x0000_f0f0, 0x0707).await?;
                Ok(CwToneSnapshot {
                    chip,
                    channel,
                    rf0,
                    bb,
                })
            }
            ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                let is_2g = channel <= 14;
                let rfe = if chip.family == ChipFamily::Rtl8822e {
                    Some((
                        self.read_u32_async(0x0040).await?,
                        self.read_u32_async(0x0064).await?,
                    ))
                } else {
                    None
                };
                if is_2g {
                    self.set_bb_reg_async(0x1a9c, 1 << 20, 0).await?;
                    self.set_bb_reg_async(0x1a14, 0x300, 3).await?;
                }
                self.set_bb_reg_async(0x1d58, 0xff8, 0x1ff).await?;
                self.set_bb_reg_async(0x1d08, 1, 1).await?;
                self.set_bb_reg_async(0x1e70, 0xf, 0).await?;
                self.set_bb_reg_async(0x1e70, 0xf, 4).await?;
                self.set_bb_reg_async(0x1c3c, 1, 1).await?;
                self.set_bb_reg_async(0x1a00, 0x3, 0).await?;
                self.set_bb_reg_async(0x1a00, 1 << 3, 1).await?;
                self.set_bb_reg_async(0x1ca4, 0x7, 1).await?;
                let rf0 = self.query_bb_reg_async(0x3c00, RF_MASK).await?;
                let keyed = (rf0 & !0xf_001f) | (2 << 16) | u32::from(gain);
                self.write_u32_async(0x1808, keyed & RF_MASK).await?;
                self.set_bb_reg_async(0x3d60, BIT1, 1).await?;
                if let Some((v40, v64)) = rfe {
                    self.write_u32_async(0x0040, v40 | 0x1403_0008).await?;
                    self.write_u32_async(0x0064, v64 & !0x0204_0000).await?;
                }
                Ok(CwToneSnapshot {
                    chip,
                    channel,
                    rf0,
                    bb: [0; 4],
                })
            }
        }
    }

    async fn stop_cw_tone_once(&self, state: CwToneSnapshot) -> Result<(), DriverError> {
        match state.chip.family {
            ChipFamily::Rtl8814 => {
                self.set_rf_reg_async(state.chip, RfPath::A, RF_LO_ENABLE, BIT1, 0)
                    .await?;
                self.set_rf_reg_async(state.chip, RfPath::A, RF_AC, B_LSSI_WRITE_DATA, state.rf0)
                    .await?;
                self.set_bb_reg_async(0x0838, BIT1, 0).await?;
                self.restore_bb([0x0c1c, 0x0e1c, 0x181c, 0x1a1c], state.bb)
                    .await
            }
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                self.set_bb_reg_async(0x0808, BIT28 | BIT29, 3).await?;
                self.set_rf_reg_async(state.chip, RfPath::A, RF_AC, B_LSSI_WRITE_DATA, state.rf0)
                    .await?;
                self.set_rf_reg_async(state.chip, RfPath::A, RF_LO_ENABLE, BIT1, 0)
                    .await?;
                self.restore_bb([0x0cb0, 0x0eb0, 0x0cb4, 0x0eb4], state.bb)
                    .await
            }
            ChipFamily::Rtl8822b => {
                self.set_bb_reg_async(0x0808, BIT28 | BIT29, 3).await?;
                self.restore_bb([0x0cb0, 0x0eb0, 0x0cb4, 0x0eb4], state.bb)
                    .await?;
                self.set_rf_reg_async(state.chip, RfPath::A, RF_AC, RF_MASK, state.rf0)
                    .await?;
                self.set_rf_reg_async(state.chip, RfPath::A, RF_LO_ENABLE, BIT1, 0)
                    .await?;
                self.set_bb_reg_async(0x0cbc, 0xfff, 0).await?;
                self.set_bb_reg_async(0x0ebc, 0xfff, 0).await
            }
            ChipFamily::Rtl8821c => {
                self.set_bb_reg_async(0x0808, BIT28 | BIT29, 3).await?;
                self.restore_bb([0x0cb0, 0x0eb0, 0x0cb4, 0x0eb4], state.bb)
                    .await?;
                self.set_rf_reg_async(state.chip, RfPath::A, RF_AC, RF_MASK, state.rf0)
                    .await?;
                self.set_rf_reg_async(state.chip, RfPath::A, 0x75, 1 << 16, 0)
                    .await?;
                self.set_rf_reg_async(state.chip, RfPath::A, RF_LO_ENABLE, BIT1, 0)
                    .await
            }
            ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                self.set_bb_reg_async(0x3d60, BIT1, 0).await?;
                self.write_u32_async(0x1808, state.rf0 & RF_MASK).await?;
                self.set_bb_reg_async(0x1ca4, 0x7, 0).await?;
                crate::time::sleep_ms(10).await;
                self.set_bb_reg_async(0x1d0c, 1 << 16, 0).await?;
                self.set_bb_reg_async(0x1d0c, 1 << 16, 1).await?;
                self.set_bb_reg_async(0x1e70, 0xf, 0).await?;
                self.set_bb_reg_async(0x1d08, 1, 0).await?;
                self.set_bb_reg_async(0x1d58, 0xff8, 0).await?;
                if state.channel <= 14 {
                    self.set_bb_reg_async(0x1a9c, 1 << 20, 1).await?;
                    self.set_bb_reg_async(0x1a14, 0x300, 0).await?;
                }
                Ok(())
            }
        }
    }

    async fn key_jaguar12_rf(&self, chip: ChipInfo, gain: u8) -> Result<(), DriverError> {
        self.set_rf_reg_async(chip, RfPath::A, RF_AC, 0xf0000, 2)
            .await?;
        self.set_rf_reg_async(chip, RfPath::A, RF_AC, 0x1f, u32::from(gain))
            .await?;
        self.set_rf_reg_async(chip, RfPath::A, RF_LO_ENABLE, BIT1, 1)
            .await
    }

    async fn read_bb_snapshot(&self, registers: [u16; 4]) -> Result<[u32; 4], DriverError> {
        let mut values = [0; 4];
        for (index, register) in registers.into_iter().enumerate() {
            values[index] = self.read_u32_async(register).await?;
        }
        Ok(values)
    }

    async fn restore_bb(&self, registers: [u16; 4], values: [u32; 4]) -> Result<(), DriverError> {
        for (register, value) in registers.into_iter().zip(values) {
            self.write_u32_async(register, value).await?;
        }
        Ok(())
    }
}
