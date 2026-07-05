//! Frame-free receive-energy and noise-histogram measurements.

use crate::device::RealtekDevice;
use crate::regs::BIT31;
use crate::time::sleep_ms;
use crate::types::DriverError;

/// One frame-free receive-energy measurement window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RxEnergy {
    /// OFDM false-alarm count since the previous read.
    pub fa_ofdm: u32,
    /// CCK false-alarm count since the previous read.
    pub fa_cck: u32,
    /// OFDM channel-busy count since the previous read.
    pub cca_ofdm: u32,
    /// CCK channel-busy count since the previous read.
    pub cca_cck: u32,
    /// Current seven-bit initial-gain index.
    pub igi: u8,
    /// Twelve IGI-relative NHM power buckets.
    pub nhm: [u8; 12],
    /// NHM measurement duration reported by hardware.
    pub nhm_duration: u16,
    /// Whether the NHM engine completed before the timeout.
    pub nhm_valid: bool,
}

#[derive(Clone, Copy)]
struct NhmRegisters {
    control: u16,
    period: u16,
    thresholds_0_3: u16,
    thresholds_4_7: u16,
    threshold_8: u16,
    threshold_8_shift: u8,
    ready: u16,
    results: [u16; 3],
}

const NHM_11AC: NhmRegisters = NhmRegisters {
    control: 0x0994,
    period: 0x0990,
    thresholds_0_3: 0x0998,
    thresholds_4_7: 0x099c,
    threshold_8: 0x09a0,
    threshold_8_shift: 0,
    ready: 0x0fb4,
    results: [0x0fa8, 0x0fac, 0x0fb0],
};

const NHM_JAGUAR3: NhmRegisters = NhmRegisters {
    control: 0x1e60,
    period: 0x1e40,
    thresholds_0_3: 0x1e44,
    thresholds_4_7: 0x1e48,
    threshold_8: 0x1e5c,
    threshold_8_shift: 16,
    ready: 0x2d4c,
    results: [0x2d40, 0x2d44, 0x2d48],
};

impl RealtekDevice {
    /// Measure channel activity without requiring a decodable 802.11 frame.
    ///
    /// Reading resets the hardware FA/CCA counters, so counts represent the
    /// interval since the previous call. NHM adds an approximately 2 ms sample.
    pub async fn read_rx_energy_async(&self) -> Result<RxEnergy, DriverError> {
        let chip = self.probe_chip_async().await?;
        let mut energy = if chip.family.is_jaguar3() {
            self.read_rx_energy_jaguar3_async().await?
        } else {
            self.read_rx_energy_11ac_async().await?
        };
        let registers = if chip.family.is_jaguar3() {
            NHM_JAGUAR3
        } else {
            NHM_11AC
        };
        self.read_nhm_async(registers, &mut energy).await?;
        Ok(energy)
    }

    async fn read_rx_energy_11ac_async(&self) -> Result<RxEnergy, DriverError> {
        let cca = self.read_u32_async(0x0f08).await?;
        let energy = RxEnergy {
            fa_ofdm: self.read_u32_async(0x0f48).await? & 0xffff,
            fa_cck: self.read_u32_async(0x0a5c).await? & 0xffff,
            cca_ofdm: cca >> 16,
            cca_cck: cca & 0xffff,
            igi: self.read_u8_async(0x0c50).await? & 0x7f,
            ..RxEnergy::default()
        };
        self.set_bb_reg_async(0x09a4, 1 << 17, 1).await?;
        self.set_bb_reg_async(0x09a4, 1 << 17, 0).await?;
        self.set_bb_reg_async(0x0a2c, 1 << 15, 0).await?;
        self.set_bb_reg_async(0x0a2c, 1 << 15, 1).await?;
        self.set_bb_reg_async(0x0b58, 1, 1).await?;
        self.set_bb_reg_async(0x0b58, 1, 0).await?;
        Ok(energy)
    }

    async fn read_rx_energy_jaguar3_async(&self) -> Result<RxEnergy, DriverError> {
        let cca = self.read_u32_async(0x2c08).await?;
        let d04 = self.read_u32_async(0x2d04).await?;
        let d08 = self.read_u32_async(0x2d08).await?;
        let d10 = self.read_u32_async(0x2d10).await?;
        let d20 = self.read_u32_async(0x2d20).await?;
        let d0c = self.read_u32_async(0x2d0c).await?;
        let energy = RxEnergy {
            fa_ofdm: (d04 >> 16)
                + (d08 & 0xffff)
                + (d08 >> 16)
                + (d10 & 0xffff)
                + (d20 & 0xffff)
                + (d20 >> 16)
                + (d10 >> 16)
                + (d0c & 0xffff),
            fa_cck: self.read_u32_async(0x1a5c).await? & 0xffff,
            cca_ofdm: cca >> 16,
            cca_cck: cca & 0xffff,
            igi: (self.read_u32_async(0x1d70).await? & 0x7f) as u8,
            ..RxEnergy::default()
        };
        self.set_bb_reg_async(0x1a2c, 0x3 << 14, 0).await?;
        self.set_bb_reg_async(0x1a2c, 0x3 << 14, 2).await?;
        self.set_bb_reg_async(0x1a2c, 0x3 << 12, 0).await?;
        self.set_bb_reg_async(0x1a2c, 0x3 << 12, 2).await?;
        self.set_bb_reg_async(0x1d2c, BIT31, 0).await?;
        self.set_bb_reg_async(0x1eb4, 1 << 25, 1).await?;
        self.set_bb_reg_async(0x1eb4, 1 << 25, 0).await?;
        self.set_bb_reg_async(0x1d2c, BIT31, 1).await?;
        Ok(energy)
    }

    async fn read_nhm_async(
        &self,
        registers: NhmRegisters,
        energy: &mut RxEnergy,
    ) -> Result<(), DriverError> {
        let base = (i32::from(energy.igi) - 14).max(0) * 2;
        let mut threshold = [0u8; 11];
        for (index, value) in threshold.iter_mut().enumerate() {
            *value = (base + 4 * index as i32).min(255) as u8;
        }
        self.set_bb_reg_async(registers.control, 0x0f00, 3).await?;
        self.set_bb_reg_async(registers.period, 0xffff_0000, 500)
            .await?;
        self.write_u32_async(
            registers.thresholds_0_3,
            u32::from_le_bytes(threshold[0..4].try_into().expect("fixed threshold slice")),
        )
        .await?;
        self.write_u32_async(
            registers.thresholds_4_7,
            u32::from_le_bytes(threshold[4..8].try_into().expect("fixed threshold slice")),
        )
        .await?;
        self.set_bb_reg_async(
            registers.threshold_8,
            0xff << registers.threshold_8_shift,
            u32::from(threshold[8]),
        )
        .await?;
        self.set_bb_reg_async(
            registers.control,
            0xffff_0000,
            u32::from(threshold[9]) | (u32::from(threshold[10]) << 8),
        )
        .await?;
        self.set_bb_reg_async(registers.control, 2, 0).await?;
        self.set_bb_reg_async(registers.control, 2, 1).await?;
        for _ in 0..15 {
            sleep_ms(1).await;
            let ready = self.read_u32_async(registers.ready).await?;
            if ready & (1 << 16) == 0 {
                continue;
            }
            for (word_index, address) in registers.results.into_iter().enumerate() {
                let bytes = self.read_u32_async(address).await?.to_le_bytes();
                energy.nhm[word_index * 4..word_index * 4 + 4].copy_from_slice(&bytes);
            }
            energy.nhm_duration = ready as u16;
            energy.nhm_valid = true;
            break;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_maps_match_devourer() {
        assert_eq!(NHM_11AC.results, [0x0fa8, 0x0fac, 0x0fb0]);
        assert_eq!(NHM_JAGUAR3.ready, 0x2d4c);
        assert!(crate::ChipFamily::Rtl8821c.is_jaguar2());
    }
}
