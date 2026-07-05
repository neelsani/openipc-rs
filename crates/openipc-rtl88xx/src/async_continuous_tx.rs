//! Hardware modulated-continuous-transmit control.

use crate::device::RealtekDevice;
use crate::regs::{BIT16, BIT17, BIT18, BIT25, B_MASK_DWORD};
use crate::time::sleep_ms;
use crate::types::DriverError;

#[derive(Debug, Clone, Copy)]
enum ContinuousTxSnapshot {
    Jaguar12 { cca: Option<u32> },
    Jaguar3 { tx_pause: u8, cck_tx: u32 },
}

#[derive(Debug, Default)]
pub(crate) struct ContinuousTxState {
    snapshot: Option<ContinuousTxSnapshot>,
}

impl RealtekDevice {
    /// Start a hardware-generated modulated continuous carrier.
    ///
    /// Jaguar1/2 use the OFDM continuous-TX path. Jaguar3 uses the vendor PMAC
    /// 6 Mbps generator. This is an RF diagnostic and must be explicitly stopped.
    pub async fn start_continuous_tx_async(&self) -> Result<(), DriverError> {
        if self
            .continuous_tx
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .snapshot
            .is_some()
        {
            return Ok(());
        }
        let chip = self.probe_chip_async().await?;
        let snapshot = if chip.family.is_jaguar3() {
            let tx_pause = self.read_u8_async(0x0522).await?;
            let cck_tx = self.read_u32_async(0x1a04).await? & 0xf000_0000;
            self.write_u8_async(0x0522, 0xff).await?;
            self.set_bb_reg_async(0x1d58, 0x0ff8, 0x1ff).await?;
            self.set_bb_reg_async(0x1a14, 0x0300, 3).await?;
            self.set_bb_reg_async(0x1a04, 0xf000_0000, 0).await?;
            self.set_bb_reg_async(0x1c3c, 1, 1).await?;
            self.set_bb_reg_async(0x1a00, 3, 0).await?;
            self.set_bb_reg_async(0x1a00, 1 << 3, 1).await?;
            self.set_bb_reg_async(0x1ca4, 7, 1).await?;
            self.set_bb_reg_async(0x1eb4, 0x000f_ffff, 1).await?;
            self.set_bb_reg_async(0x0908, 0x00ff_ffff, 0x027d0b).await?;
            self.set_bb_reg_async(0x0a58, 0x003f_8000, 4).await?;
            self.set_bb_reg_async(0x0900, 1 << 1, 0).await?;
            self.set_bb_reg_async(0x0900, 0xff00_0000, 0).await?;
            self.set_bb_reg_async(0x1ae0, 0x7000, 0).await?;
            self.set_bb_reg_async(0x0900, 1 | (1 << 2), 0).await?;
            self.set_bb_reg_async(0x09b8, 0xffff_0000, 2000).await?;
            self.set_bb_reg_async(0x1d08, 1, 1).await?;
            self.set_bb_reg_async(0x1e70, 0x0f, 0).await?;
            self.set_bb_reg_async(0x1e70, 0x0f, 4).await?;
            ContinuousTxSnapshot::Jaguar3 { tx_pause, cck_tx }
        } else {
            let cca = if chip.family.is_jaguar2() {
                let value = self.read_u32_async(0x0838).await?;
                self.set_bb_reg_async(0x0838, 0xff, 0x6d).await?;
                Some(value)
            } else {
                None
            };
            self.set_bb_reg_async(0x0800, BIT25, 1).await?;
            self.set_bb_reg_async(0x0a00, 3, 0).await?;
            self.set_bb_reg_async(0x0a00, 1 << 3, 1).await?;
            self.set_bb_reg_async(0x0914, BIT18 | BIT17 | BIT16, 1)
                .await?;
            if chip.family.is_jaguar1() {
                self.set_bb_reg_async(0x0c90, B_MASK_DWORD, 0x0100_0500)
                    .await?;
                self.set_bb_reg_async(0x0e90, B_MASK_DWORD, 0x0100_0500)
                    .await?;
            }
            ContinuousTxSnapshot::Jaguar12 { cca }
        };
        self.continuous_tx
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .snapshot = Some(snapshot);
        log::info!(target: "openipc_rtl88xx::continuous_tx", "modulated continuous TX started on {}", chip.family.name());
        Ok(())
    }

    /// Stop modulated continuous TX and restore normal receive/transmit state.
    pub async fn stop_continuous_tx_async(&self) -> Result<(), DriverError> {
        let snapshot = self
            .continuous_tx
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .snapshot;
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        match snapshot {
            ContinuousTxSnapshot::Jaguar12 { cca } => {
                self.set_bb_reg_async(0x0914, BIT18 | BIT17 | BIT16, 0)
                    .await?;
                sleep_ms(10).await;
                self.set_bb_reg_async(0x0100, 1 << 8, 0).await?;
                self.set_bb_reg_async(0x0100, 1 << 8, 1).await?;
                if let Some(cca) = cca {
                    self.set_bb_reg_async(0x0838, 0xff, cca & 0xff).await?;
                } else {
                    self.write_u32_async(0x0c90, 0x0100_0100).await?;
                    self.write_u32_async(0x0e90, 0x0100_0100).await?;
                }
            }
            ContinuousTxSnapshot::Jaguar3 { tx_pause, cck_tx } => {
                self.set_bb_reg_async(0x1ca4, 7, 0).await?;
                sleep_ms(10).await;
                self.set_bb_reg_async(0x1d0c, 1 << 16, 0).await?;
                self.set_bb_reg_async(0x1d0c, 1 << 16, 1).await?;
                self.set_bb_reg_async(0x1e70, 0x0f, 0).await?;
                self.set_bb_reg_async(0x1d08, 1, 0).await?;
                self.write_u8_async(0x0522, tx_pause).await?;
                self.set_bb_reg_async(0x1d58, 0x0ff8, 0).await?;
                self.set_bb_reg_async(0x1a14, 0x0300, 0).await?;
                self.set_bb_reg_async(0x1a04, 0xf000_0000, cck_tx >> 28)
                    .await?;
            }
        }
        self.continuous_tx
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .snapshot = None;
        log::info!(target: "openipc_rtl88xx::continuous_tx", "modulated continuous TX stopped");
        Ok(())
    }
}
