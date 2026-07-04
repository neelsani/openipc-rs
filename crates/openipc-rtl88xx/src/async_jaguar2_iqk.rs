//! RTL8822B software IQK calibration.
//!
//! Register order and retry limits are a direct Rust transcription of
//! devourer's `Halrf8822b`, which follows Realtek's 8822B HALRF path.

use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::time::{sleep_micros, sleep_ms};
use crate::types::{ChipInfo, DriverError};

const RF_MASK: u32 = 0x000f_ffff;
const TX_IQK: usize = 0;
const RX_IQK1: usize = 1;
const RX_IQK2: usize = 2;

#[derive(Debug)]
struct Jaguar2IqkState {
    band_2g: bool,
    bandwidth: u8,
    iqk_step: u8,
    rx_iqk_step: u8,
    retries: [[u8; 3]; 2],
    gain_retries: [[u8; 2]; 2],
    lok_failed: [bool; 2],
    rx_fail_code: [u8; 2],
    lna_index: u8,
    boundary: bool,
    tmp_1bcc: u8,
    tmp_gntwl: u32,
}

impl Jaguar2IqkState {
    fn new(band_2g: bool) -> Self {
        Self {
            band_2g,
            bandwidth: 0,
            iqk_step: 1,
            rx_iqk_step: 1,
            retries: [[0; 3]; 2],
            gain_retries: [[0; 2]; 2],
            lok_failed: [true; 2],
            rx_fail_code: [0; 2],
            lna_index: 0,
            boundary: false,
            tmp_1bcc: 0x12,
            tmp_gntwl: 0,
        }
    }
}

impl RealtekDevice {
    pub(crate) async fn run_iqk_8822b_async(
        &self,
        chip: ChipInfo,
        band_2g: bool,
    ) -> Result<(), DriverError> {
        const MAC_REGISTERS: [u16; 2] = [0x0520, 0x0550];
        const BB_REGISTERS: [u16; 21] = [
            0x0808, 0x090c, 0x0c00, 0x0cb0, 0x0cb4, 0x0cbc, 0x0e00, 0x0eb0, 0x0eb4, 0x0ebc, 0x1990,
            0x09a4, 0x0a04, 0x0b00, 0x0838, 0x0c58, 0x0c5c, 0x0c6c, 0x0e58, 0x0e5c, 0x0e6c,
        ];
        const RF_REGISTERS: [u16; 5] = [0x0df, 0x08f, 0x065, 0x000, 0x001];

        self.write_u32_async(0x1b10, 0x8801_1c00).await?;
        let mut state = Jaguar2IqkState::new(band_2g);
        state.tmp_gntwl = self.iqk_lte_read_8822b_async(0x38).await?;

        let mut mac_backup = [0u32; 2];
        for (slot, register) in mac_backup.iter_mut().zip(MAC_REGISTERS) {
            *slot = self.read_u32_async(register).await?;
        }
        let mut bb_backup = [0u32; 21];
        for (slot, register) in bb_backup.iter_mut().zip(BB_REGISTERS) {
            *slot = self.read_u32_async(register).await?;
        }
        let mut rf_backup = [[0u32; 2]; 5];
        for (slot, register) in rf_backup.iter_mut().zip(RF_REGISTERS) {
            slot[0] = self.query_rf_reg_async(chip, RfPath::A, register).await?;
            slot[1] = self.query_rf_reg_async(chip, RfPath::B, register).await?;
        }

        loop {
            self.iqk_configure_macbb_8822b_async().await?;
            self.iqk_afe_setting_8822b_async(chip, true).await?;
            self.iqk_rfe_setting_8822b_async(false).await?;
            self.iqk_agc_boundary_8822b_async().await?;
            self.iqk_rf_setting_8822b_async(chip, state.band_2g).await?;
            self.iqk_start_8822b_async(chip, &mut state).await?;
            self.iqk_afe_setting_8822b_async(chip, false).await?;
            for (register, value) in MAC_REGISTERS.into_iter().zip(mac_backup) {
                self.write_u32_async(register, value).await?;
            }
            for (register, value) in BB_REGISTERS.into_iter().zip(bb_backup) {
                self.write_u32_async(register, value).await?;
            }
            self.set_rf_reg_async(chip, RfPath::A, 0xef, RF_MASK, 0)
                .await?;
            self.set_rf_reg_async(chip, RfPath::B, 0xef, RF_MASK, 0)
                .await?;
            self.iqk_rf_set_check_8822b_async(chip, RfPath::A, 0xdf, rf_backup[0][0] & !(1 << 4))
                .await?;
            self.iqk_rf_set_check_8822b_async(chip, RfPath::B, 0xdf, rf_backup[0][1] & !(1 << 4))
                .await?;
            for index in 1..RF_REGISTERS.len() {
                self.set_rf_reg_async(
                    chip,
                    RfPath::A,
                    RF_REGISTERS[index],
                    RF_MASK,
                    rf_backup[index][0],
                )
                .await?;
                self.set_rf_reg_async(
                    chip,
                    RfPath::B,
                    RF_REGISTERS[index],
                    RF_MASK,
                    rf_backup[index][1],
                )
                .await?;
            }
            if state.iqk_step == 7 {
                break;
            }
            sleep_ms(50).await;
        }
        self.set_rf_reg_async(chip, RfPath::A, 0xb8, RF_MASK, 0x00a00)
            .await?;
        self.set_rf_reg_async(chip, RfPath::A, 0xb8, RF_MASK, 0x80a00)
            .await?;
        log::debug!(
            target: "openipc_rtl88xx::iqk",
            "RTL8822B IQK complete lok={:?} tx_retry={:?} rx_gain_retry={:?} rx_retry={:?} rx_fail={:?}",
            state.lok_failed,
            [state.retries[0][TX_IQK], state.retries[1][TX_IQK]],
            state.gain_retries,
            [
                [state.retries[0][RX_IQK1], state.retries[0][RX_IQK2]],
                [state.retries[1][RX_IQK1], state.retries[1][RX_IQK2]],
            ],
            state.rx_fail_code
        );
        Ok(())
    }

    async fn iqk_rf_set_check_8822b_async(
        &self,
        chip: ChipInfo,
        path: RfPath,
        register: u16,
        value: u32,
    ) -> Result<(), DriverError> {
        self.set_rf_reg_async(chip, path, register, RF_MASK, value)
            .await?;
        for _ in 0..100 {
            if self.query_rf_reg_async(chip, path, register).await? == value {
                return Ok(());
            }
            sleep_micros(10).await;
            self.set_rf_reg_async(chip, path, register, RF_MASK, value)
                .await?;
        }
        Ok(())
    }

    async fn iqk_agc_boundary_8822b_async(&self) -> Result<(), DriverError> {
        for value in [0xf800_0008, 0xf80a_7008, 0xf801_5008, 0xf800_0008] {
            self.write_u32_async(0x1b00, value).await?;
        }
        Ok(())
    }

    async fn iqk_bb_reset_8822b_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        self.set_rf_reg_async(chip, RfPath::A, 0, RF_MASK, 0x10000)
            .await?;
        self.set_rf_reg_async(chip, RfPath::B, 0, RF_MASK, 0x10000)
            .await?;
        self.set_bb_reg_async(0x08f8, 0x0ff0_0000, 0).await?;
        for count in 0..=20000 {
            self.write_u32_async(0x08fc, 0).await?;
            self.set_bb_reg_async(0x198c, 0x7, 7).await?;
            let receiving = count < 20000 && self.query_bb_reg_async(0x0fa0, 1 << 3).await? != 0;
            if receiving {
                sleep_micros(10).await;
                continue;
            }
            self.write_u8_async(0x0808, 0).await?;
            self.set_bb_reg_async(0x0a04, 0x0f00_0000, 0).await?;
            self.set_bb_reg_async(0, 1 << 16, 0).await?;
            self.set_bb_reg_async(0, 1 << 16, 1).await?;
            if self.query_bb_reg_async(0x0660, 1 << 16).await? != 0 {
                self.write_u32_async(0x06b4, 0x8900_0006).await?;
            }
            break;
        }
        Ok(())
    }

    async fn iqk_afe_setting_8822b_async(
        &self,
        chip: ChipInfo,
        calibrating: bool,
    ) -> Result<(), DriverError> {
        if calibrating {
            for (register, values) in [
                (0x0c60, [0x5000_0000, 0x7007_0040]),
                (0x0e60, [0x5000_0000, 0x7007_0040]),
            ] {
                for value in values {
                    self.write_u32_async(register, value).await?;
                }
            }
            for (register, value) in [
                (0x0c58, 0xd800_0402),
                (0x0c5c, 0xd100_0120),
                (0x0c6c, 0x0000_0a15),
                (0x0e58, 0xd800_0402),
                (0x0e5c, 0xd100_0120),
                (0x0e6c, 0x0000_0a15),
            ] {
                self.write_u32_async(register, value).await?;
            }
            self.iqk_bb_reset_8822b_async(chip).await?;
        } else {
            for register in [0x0c60, 0x0e60] {
                self.write_u32_async(register, 0x5000_0000).await?;
                self.write_u32_async(register, 0x7003_8040).await?;
            }
        }
        self.set_bb_reg_async(0x09a4, 1 << 31, 0).await
    }

    async fn iqk_rfe_setting_8822b_async(&self, external_pa: bool) -> Result<(), DriverError> {
        let tail = if external_pa {
            0x0000_083b
        } else {
            0x0000_0100
        };
        for base in [0x0c00, 0x0e00] {
            self.write_u32_async(base + 0xb0, 0x7777_7777).await?;
            self.write_u32_async(base + 0xb4, 0x0000_7777).await?;
            self.write_u32_async(base + 0xbc, tail).await?;
        }
        Ok(())
    }

    async fn iqk_rf_setting_8822b_async(
        &self,
        chip: ChipInfo,
        band_2g: bool,
    ) -> Result<(), DriverError> {
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        self.write_u32_async(0x1bb8, 0).await?;
        for path in [RfPath::A, RfPath::B] {
            let df = (self.query_rf_reg_async(chip, path, 0xdf).await? & !(1 << 4))
                | (1 << 1)
                | (1 << 11);
            self.iqk_rf_set_check_8822b_async(chip, path, 0xdf, df)
                .await?;
            self.set_rf_reg_async(chip, path, 0x65, RF_MASK, 0x09000)
                .await?;
            self.set_rf_reg_async(chip, path, 0xef, 1 << 19, 1).await?;
            self.set_rf_reg_async(chip, path, 0x33, RF_MASK, 0x00026)
                .await?;
            self.set_rf_reg_async(chip, path, 0x3e, RF_MASK, 0x00037)
                .await?;
            self.set_rf_reg_async(
                chip,
                path,
                0x3f,
                RF_MASK,
                if band_2g { 0x5efce } else { 0xdefce },
            )
            .await?;
            self.set_rf_reg_async(chip, path, 0xef, 1 << 19, 0).await?;
        }
        Ok(())
    }

    async fn iqk_configure_macbb_8822b_async(&self) -> Result<(), DriverError> {
        self.write_u8_async(0x0522, 0x7f).await?;
        self.set_bb_reg_async(0x0550, (1 << 11) | (1 << 3), 0)
            .await?;
        self.set_bb_reg_async(0x090c, 1 << 15, 1).await?;
        for register in [0x0c94, 0x0e94] {
            self.set_bb_reg_async(register, 1, 1).await?;
            self.set_bb_reg_async(register, (1 << 11) | (1 << 10), 1)
                .await?;
        }
        self.write_u32_async(0x0c00, 4).await?;
        self.write_u32_async(0x0e00, 4).await?;
        self.set_bb_reg_async(0x0b00, 1 << 8, 0).await?;
        self.set_bb_reg_async(0x0808, 1 << 28, 0).await?;
        self.set_bb_reg_async(0x0838, 0x0e, 7).await
    }

    async fn iqk_lok_setting_8822b_async(
        &self,
        chip: ChipInfo,
        state: &Jaguar2IqkState,
        path: usize,
    ) -> Result<(), DriverError> {
        let rf_path = iqk_path(path);
        self.write_u32_async(0x1b00, 0xf800_0008 | ((path as u32) << 1))
            .await?;
        self.write_u32_async(0x1bcc, 9).await?;
        self.write_u8_async(0x1b23, 0).await?;
        self.write_u8_async(0x1b2b, if state.band_2g { 0 } else { 0x80 })
            .await?;
        self.set_rf_reg_async(
            chip,
            rf_path,
            0x56,
            RF_MASK,
            if state.band_2g { 0x50df2 } else { 0x5086c },
        )
        .await?;
        self.set_rf_reg_async(
            chip,
            rf_path,
            0x8f,
            RF_MASK,
            if state.band_2g { 0xadc00 } else { 0xa9c00 },
        )
        .await?;
        self.set_rf_reg_async(chip, rf_path, 0xef, 1 << 4, 1)
            .await?;
        self.set_rf_reg_async(chip, rf_path, 0x33, 0x03, u32::from(!state.band_2g))
            .await
    }

    async fn iqk_tx_setting_8822b_async(
        &self,
        chip: ChipInfo,
        state: &Jaguar2IqkState,
        path: usize,
    ) -> Result<(), DriverError> {
        let rf_path = iqk_path(path);
        self.write_u32_async(0x1b00, 0xf800_0008 | ((path as u32) << 1))
            .await?;
        self.write_u32_async(0x1bcc, 9).await?;
        self.write_u32_async(0x1b20, 0x0144_0008).await?;
        self.write_u32_async(0x1b00, if path == 0 { 0xf800_000a } else { 0xf800_0008 })
            .await?;
        self.write_u32_async(0x1bcc, 0x3f).await?;
        self.set_rf_reg_async(
            chip,
            rf_path,
            0x56,
            RF_MASK,
            if state.band_2g { 0x50df2 } else { 0x500ef },
        )
        .await?;
        self.set_rf_reg_async(
            chip,
            rf_path,
            0x8f,
            RF_MASK,
            if state.band_2g { 0xadc00 } else { 0xa9c00 },
        )
        .await?;
        self.write_u8_async(0x1b2b, if state.band_2g { 0 } else { 0x80 })
            .await
    }

    async fn iqk_rx1_setting_8822b_async(
        &self,
        chip: ChipInfo,
        state: &Jaguar2IqkState,
        path: usize,
    ) -> Result<(), DriverError> {
        let rf_path = iqk_path(path);
        self.write_u32_async(0x1b00, 0xf800_0008 | ((path as u32) << 1))
            .await?;
        self.write_u8_async(0x1bcc, 9).await?;
        self.write_u8_async(0x1b2b, if state.band_2g { 0 } else { 0x80 })
            .await?;
        self.write_u32_async(
            0x1b20,
            if state.band_2g {
                0x0145_0008
            } else {
                0x0085_0008
            },
        )
        .await?;
        self.write_u32_async(
            0x1b24,
            if state.band_2g {
                0x0146_0c88
            } else {
                0x0046_0048
            },
        )
        .await?;
        self.set_rf_reg_async(chip, rf_path, 0x56, RF_MASK, 0x510e0)
            .await?;
        self.set_rf_reg_async(
            chip,
            rf_path,
            0x8f,
            RF_MASK,
            if state.band_2g { 0xacc00 } else { 0xadc00 },
        )
        .await
    }

    async fn iqk_rx2_setting_8822b_async(
        &self,
        chip: ChipInfo,
        state: &mut Jaguar2IqkState,
        path: usize,
        gain_search: bool,
    ) -> Result<(), DriverError> {
        if gain_search {
            state.tmp_1bcc = if state.band_2g || path == 0 {
                0x12
            } else {
                0x09
            };
        }
        let rf_path = iqk_path(path);
        self.write_u32_async(0x1b00, 0xf800_0008 | ((path as u32) << 1))
            .await?;
        self.write_u8_async(0x1bcc, state.tmp_1bcc).await?;
        self.write_u8_async(0x1b2b, if state.band_2g { 0 } else { 0x80 })
            .await?;
        self.write_u32_async(
            0x1b20,
            if state.band_2g {
                0x0145_0008
            } else {
                0x0085_0008
            },
        )
        .await?;
        self.write_u32_async(
            0x1b24,
            if state.band_2g {
                0x0146_0848
            } else {
                0x0046_0848
            },
        )
        .await?;
        self.set_rf_reg_async(
            chip,
            rf_path,
            0x56,
            RF_MASK,
            if state.band_2g { 0x510e0 } else { 0x51060 },
        )
        .await?;
        self.set_rf_reg_async(chip, rf_path, 0x8f, RF_MASK, 0xa9c00)
            .await
    }

    async fn iqk_lte_read_8822b_async(&self, register: u16) -> Result<u32, DriverError> {
        self.write_u32_async(0x1700, 0x800f_0000 | u32::from(register))
            .await?;
        for _ in 0..30000 {
            if self.read_u8_async(0x1703).await? & (1 << 5) != 0 {
                break;
            }
        }
        self.read_u32_async(0x1708).await
    }

    async fn iqk_lte_write_8822b_async(
        &self,
        register: u16,
        mask: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        if mask == 0 {
            return Ok(());
        }
        let data = if mask == u32::MAX {
            value
        } else {
            (self.iqk_lte_read_8822b_async(register).await? & !mask)
                | (value << mask.trailing_zeros())
        };
        self.write_u32_async(0x1704, data).await?;
        for _ in 0..30000 {
            if self.read_u8_async(0x1703).await? & (1 << 5) != 0 {
                break;
            }
        }
        self.write_u32_async(0x1700, 0xc00f_0000 | u32::from(register))
            .await
    }

    async fn iqk_clear_rf8_8822b_async(
        &self,
        chip: ChipInfo,
        path: usize,
    ) -> Result<(), DriverError> {
        let path = iqk_path(path);
        for _ in 0..30000 {
            self.set_rf_reg_async(chip, path, 0xef, RF_MASK, 0).await?;
            self.set_rf_reg_async(chip, path, 0x08, RF_MASK, 0).await?;
            if self.query_rf_reg_async(chip, path, 0x08).await? == 0 {
                break;
            }
        }
        Ok(())
    }

    async fn iqk_check_cal_8822b_async(
        &self,
        chip: ChipInfo,
        path: usize,
        command: u8,
    ) -> Result<bool, DriverError> {
        let mut failed = true;
        for _ in 0..20000 {
            if self.query_rf_reg_async(chip, iqk_path(path), 0x08).await? == 0x12345 {
                failed = command != 0 && self.query_bb_reg_async(0x1b08, 1 << 26).await? != 0;
                break;
            }
            sleep_micros(10).await;
        }
        self.iqk_clear_rf8_8822b_async(chip, path).await?;
        Ok(failed)
    }

    async fn iqk_lok_one_shot_8822b_async(
        &self,
        chip: ChipInfo,
        state: &mut Jaguar2IqkState,
        path: usize,
    ) -> Result<bool, DriverError> {
        let command = 0xf800_0008 | (1 << (4 + path));
        self.iqk_lte_write_8822b_async(0x38, 0xffff, 0x7700).await?;
        self.write_u32_async(0x1b00, command).await?;
        self.write_u32_async(0x1b00, command + 1).await?;
        sleep_micros(10).await;
        let failed = self.iqk_check_cal_8822b_async(chip, path, 0).await?;
        self.iqk_lte_write_8822b_async(0x38, u32::MAX, state.tmp_gntwl)
            .await?;
        state.lok_failed[path] = failed;
        Ok(failed)
    }

    async fn iqk_one_shot_8822b_async(
        &self,
        chip: ChipInfo,
        state: &mut Jaguar2IqkState,
        path: usize,
        index: usize,
    ) -> Result<bool, DriverError> {
        let command = match index {
            TX_IQK => 0xf800_0008 | ((u32::from(state.bandwidth) + 4) << 8) | (1 << (path + 4)),
            RX_IQK1 if state.bandwidth == 2 => 0xf800_0808 | (1 << (path + 4)),
            RX_IQK1 => 0xf800_0708 | (1 << (path + 4)),
            _ => {
                self.write_u32_async(0x1b00, 0xf800_0008 | ((path as u32) << 1))
                    .await?;
                self.write_u32_async(
                    0x1b24,
                    (self.read_u32_async(0x1b24).await? & 0xffff_e3ff)
                        | (u32::from(state.lna_index & 7) << 10),
                )
                .await?;
                0xf800_0008 | ((u32::from(state.bandwidth) + 9) << 8) | (1 << (path + 4))
            }
        };
        self.iqk_lte_write_8822b_async(0x38, 0xffff, 0x7700).await?;
        self.write_u32_async(0x1b00, command).await?;
        self.write_u32_async(0x1b00, command + 1).await?;
        sleep_micros(10).await;
        let failed = self.iqk_check_cal_8822b_async(chip, path, 1).await?;
        self.iqk_lte_write_8822b_async(0x38, u32::MAX, state.tmp_gntwl)
            .await?;
        self.write_u32_async(0x1b00, 0xf800_0008 | ((path as u32) << 1))
            .await?;
        let apply = [0x0c94, 0x0e94][path];
        if index == TX_IQK && failed {
            self.set_bb_reg_async(apply, 1, 0).await?;
        } else if index == RX_IQK2 {
            self.write_u32_async(0x1b38, 0x2000_0000).await?;
            if failed {
                self.set_bb_reg_async(apply, (1 << 11) | (1 << 10), 0)
                    .await?;
            }
        }
        Ok(failed)
    }

    async fn iqk_gain_search_8822b_async(
        &self,
        chip: ChipInfo,
        state: &mut Jaguar2IqkState,
        path: usize,
        step: usize,
    ) -> Result<bool, DriverError> {
        const IQ_MUX: [u8; 4] = [0x09, 0x12, 0x1b, 0x24];
        if step == RX_IQK1 {
            let command = 0xf800_0208 | (1 << (path + 4));
            self.iqk_lte_write_8822b_async(0x38, 0xffff, 0x7700).await?;
            self.write_u32_async(0x1b00, command).await?;
            self.write_u32_async(0x1b00, command + 1).await?;
            sleep_micros(10).await;
            let failed = self.iqk_check_cal_8822b_async(chip, path, 1).await?;
            self.iqk_lte_write_8822b_async(0x38, u32::MAX, state.tmp_gntwl)
                .await?;
            return Ok(failed);
        }

        let Some(mut index) = IQ_MUX.iter().position(|value| *value == state.tmp_1bcc) else {
            return Ok(true);
        };
        self.write_u32_async(0x1b00, 0xf800_0008 | ((path as u32) << 1))
            .await?;
        self.write_u32_async(0x1bcc, u32::from(state.tmp_1bcc))
            .await?;
        let command = 0xf800_0308 | (1 << (path + 4));
        self.iqk_lte_write_8822b_async(0x38, 0xffff, 0x7700).await?;
        self.write_u32_async(0x1b00, command).await?;
        self.write_u32_async(0x1b00, command + 1).await?;
        sleep_micros(10).await;
        let _ = self.iqk_check_cal_8822b_async(chip, path, 1).await?;
        self.iqk_lte_write_8822b_async(0x38, u32::MAX, state.tmp_gntwl)
            .await?;
        let rf0 = self.query_rf_reg_async(chip, iqk_path(path), 0).await?;
        self.write_u32_async(0x1b00, 0xf800_0008 | ((path as u32) << 1))
            .await?;
        let gain = (rf0 & 0x1fe0) >> 5;
        state.lna_index = (gain >> 5) as u8;
        let bb_index = gain & 0x1f;
        let mut retry = true;
        if bb_index == 1 {
            if state.lna_index != 0 {
                state.lna_index -= 1;
            } else if index != 3 {
                index += 1;
            } else {
                state.boundary = true;
            }
        } else if bb_index == 0x0a {
            if index != 0 {
                index -= 1;
            } else if state.lna_index != 7 {
                state.lna_index += 1;
            } else {
                state.boundary = true;
            }
        } else {
            retry = false;
        }
        if state.boundary {
            retry = false;
        }
        state.tmp_1bcc = IQ_MUX[index];
        if retry {
            self.write_u32_async(
                0x1b24,
                (self.read_u32_async(0x1b24).await? & 0xffff_e3ff)
                    | (u32::from(state.lna_index) << 10),
            )
            .await?;
        }
        Ok(retry)
    }

    async fn iqk_rx_path_8822b_async(
        &self,
        chip: ChipInfo,
        state: &mut Jaguar2IqkState,
        path: usize,
    ) -> Result<(), DriverError> {
        loop {
            match state.rx_iqk_step {
                1 => {
                    self.iqk_rx1_setting_8822b_async(chip, state, path).await?;
                    let failed = self
                        .iqk_gain_search_8822b_async(chip, state, path, RX_IQK1)
                        .await?;
                    if failed && state.gain_retries[path][0] < 2 {
                        state.gain_retries[path][0] += 1;
                    } else if failed {
                        state.rx_fail_code[path] = 0;
                        state.rx_iqk_step = 5;
                    } else {
                        state.rx_iqk_step += 1;
                    }
                }
                2 => {
                    self.iqk_rx2_setting_8822b_async(chip, state, path, true)
                        .await?;
                    state.boundary = false;
                    let failed = self
                        .iqk_gain_search_8822b_async(chip, state, path, RX_IQK2)
                        .await?;
                    if failed && state.gain_retries[path][1] < 6 {
                        state.gain_retries[path][1] += 1;
                    } else {
                        state.rx_iqk_step += 1;
                    }
                }
                3 => {
                    self.iqk_rx1_setting_8822b_async(chip, state, path).await?;
                    let failed = self
                        .iqk_one_shot_8822b_async(chip, state, path, RX_IQK1)
                        .await?;
                    if failed && state.retries[path][RX_IQK1] < 2 {
                        state.retries[path][RX_IQK1] += 1;
                    } else if failed {
                        state.rx_fail_code[path] = 1;
                        state.rx_iqk_step = 5;
                    } else {
                        state.rx_iqk_step += 1;
                    }
                }
                4 => {
                    self.iqk_rx2_setting_8822b_async(chip, state, path, false)
                        .await?;
                    let failed = self
                        .iqk_one_shot_8822b_async(chip, state, path, RX_IQK2)
                        .await?;
                    if failed && state.retries[path][RX_IQK2] < 2 {
                        state.retries[path][RX_IQK2] += 1;
                    } else {
                        if failed {
                            state.rx_fail_code[path] = 2;
                        }
                        state.rx_iqk_step = 5;
                    }
                }
                _ => return Ok(()),
            }
            if state.rx_iqk_step == 5 {
                state.iqk_step += 1;
                state.rx_iqk_step = 1;
                return Ok(());
            }
        }
    }

    async fn iqk_start_8822b_async(
        &self,
        chip: ChipInfo,
        state: &mut Jaguar2IqkState,
    ) -> Result<(), DriverError> {
        for path in [RfPath::A, RfPath::B] {
            let grant = self.query_rf_reg_async(chip, path, 1).await? | (1 << 5) | 1;
            self.set_rf_reg_async(chip, path, 1, RF_MASK, grant).await?;
        }
        loop {
            match state.iqk_step {
                1 | 2 => {
                    let path = (state.iqk_step - 1) as usize;
                    self.iqk_lok_setting_8822b_async(chip, state, path).await?;
                    let _ = self.iqk_lok_one_shot_8822b_async(chip, state, path).await?;
                    state.iqk_step += 1;
                }
                3 | 4 => {
                    let path = (state.iqk_step - 3) as usize;
                    self.iqk_tx_setting_8822b_async(chip, state, path).await?;
                    let failed = self
                        .iqk_one_shot_8822b_async(chip, state, path, TX_IQK)
                        .await?;
                    if failed && state.retries[path][TX_IQK] < 3 {
                        state.retries[path][TX_IQK] += 1;
                    } else {
                        state.iqk_step += 1;
                    }
                }
                5 | 6 => {
                    let path = (state.iqk_step - 5) as usize;
                    self.iqk_rx_path_8822b_async(chip, state, path).await?;
                }
                _ => break,
            }
            if state.iqk_step == 7 {
                for path in 0..2u32 {
                    self.write_u32_async(0x1b00, 0xf800_0008 | (path << 1))
                        .await?;
                    self.write_u32_async(0x1b2c, 7).await?;
                    self.write_u32_async(0x1bcc, 0).await?;
                    self.write_u32_async(0x1b38, 0x2000_0000).await?;
                }
                break;
            }
        }
        Ok(())
    }
}

fn iqk_path(index: usize) -> RfPath {
    if index == 0 {
        RfPath::A
    } else {
        RfPath::B
    }
}
