use crate::async_iqk::IqkReport;
use crate::device::RealtekDevice;
use crate::rtl_data;
use crate::time::{sleep_micros, sleep_ms};
use crate::types::{ChannelWidth, ChipFamily, ChipInfo, DriverError};

const MASKDWORD: u32 = 0xffff_ffff;
const RFREG_MASK: u32 = 0x000f_ffff;
const SS: usize = 2;
const PATH_A: u8 = 0;
const PATH_B: u8 = 1;
const TXIQK: u8 = 0;
const RXIQK: u8 = 1;
const TX_IQK: u8 = 0;
const RX_IQK: u8 = 1;
const RXIQK1: u8 = 1;
const RXIQK2: u8 = 2;
const RXK_STEP: u8 = 6;
const IQK_STEP: u8 = 7;
const RXIQK_GS_LIMIT: u8 = 6;
const KCOUNT_LIMIT_80M: u8 = 2;
const KCOUNT_LIMIT_OTHERS: u8 = 4;
const IQMUX: [u8; 5] = [0x09, 0x12, 0x1b, 0x24, 0x24];
const RF_WIN: [u16; 2] = [0x3c00, 0x4c00];
const MAC_REG_NUM_8822C: usize = 3;
const BB_REG_NUM_8822C: usize = 21;
const RF_REG_NUM_8822C: usize = 3;
const DACK_SN: usize = 100;

const MAC_BACKUP_REGS: [u16; MAC_REG_NUM_8822C] = [0x0520, 0x001c, 0x0070];
const BB_BACKUP_REGS: [u16; BB_REG_NUM_8822C] = [
    0x0820, 0x0824, 0x1c38, 0x1c68, 0x1d60, 0x180c, 0x410c, 0x1c3c, 0x1a14, 0x1d58, 0x1d70, 0x1864,
    0x4164, 0x186c, 0x416c, 0x1a14, 0x1e70, 0x080c, 0x1e7c, 0x18a4, 0x41a4,
];
const RF_BACKUP_REGS: [u16; RF_REG_NUM_8822C] = [0x19, 0xdf, 0x9e];

/// Mutable Jaguar3 thermal TX power tracking state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jaguar3PowerTrackingState {
    /// Chip family this state was initialized for.
    pub chip_family: Option<ChipFamily>,
    /// Per-path cold-boot thermal references. `-1` means not sampled yet.
    pub thermal_ref: [i16; 2],
    /// Thermal reference used for LC-tank recalibration decisions.
    pub thermal_lck_ref: i16,
    /// Current Jaguar3 channel used to select EU thermal compensation tables.
    pub channel: u8,
    /// Per-path rolling thermal average used by RTL8822E.
    pub thermal_average: [[u8; 4]; 2],
    /// Next rolling-average slot for each RF path.
    pub thermal_average_index: [u8; 2],
    /// Number of valid rolling-average samples for each RF path.
    pub thermal_average_count: [u8; 2],
    /// True after the RTL8822E thermal meter has been triggered.
    pub thermal_meter_triggered: bool,
}

impl Default for Jaguar3PowerTrackingState {
    fn default() -> Self {
        Self {
            chip_family: None,
            thermal_ref: [-1, -1],
            thermal_lck_ref: -1,
            channel: 0,
            thermal_average: [[0; 4]; 2],
            thermal_average_index: [0; 2],
            thermal_average_count: [0; 2],
            thermal_meter_triggered: false,
        }
    }
}

/// Report returned by one Jaguar3 thermal power tracking tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jaguar3PowerTrackingReport {
    /// Raw thermal readings from RF paths A and B.
    pub thermal_raw: [u8; 2],
    /// Cold references used for the compensation calculation.
    pub thermal_ref: [i16; 2],
    /// Signed compensation index written to the per-path PWR_TRACK field.
    pub compensation_index: [i8; 2],
    /// True when this tick re-ran LCK because thermal drift crossed the threshold.
    pub lck_ran: bool,
}

impl RealtekDevice {
    pub(crate) async fn dac_calibrate_8822c_async(&self) -> Result<(), DriverError> {
        let mut cal = Jaguar3Cal::new(self);
        cal.dac_calibrate().await
    }

    pub(crate) async fn run_iqk_8822c_async(
        &self,
        chip: ChipInfo,
        width: ChannelWidth,
        channel: u8,
    ) -> Result<IqkReport, DriverError> {
        let mut cal = Jaguar3Cal::new(self);
        cal.phy_iq_calibrate(width, channel).await?;
        self.write_u16_async(0x0522, 0x0000).await?;
        Ok(IqkReport {
            chip,
            channel,
            ran: true,
        })
    }

    /// Run one Jaguar3 thermal TX power tracking tick.
    pub async fn tick_jaguar3_power_tracking_async(
        &self,
        state: &mut Jaguar3PowerTrackingState,
    ) -> Result<Jaguar3PowerTrackingReport, DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family == ChipFamily::Rtl8822e {
            return self
                .tick_power_tracking_8822e_inner_async(chip, state)
                .await;
        }
        if state.chip_family != Some(chip.family) {
            *state = Jaguar3PowerTrackingState {
                chip_family: Some(chip.family),
                ..Jaguar3PowerTrackingState::default()
            };
        }
        let mut cal = Jaguar3Cal::new(self);
        cal.pwr_track(state).await
    }

    /// Compatibility alias for [`Self::tick_jaguar3_power_tracking_async`].
    pub async fn tick_power_tracking_8822c_async(
        &self,
        state: &mut Jaguar3PowerTrackingState,
    ) -> Result<Jaguar3PowerTrackingReport, DriverError> {
        self.tick_jaguar3_power_tracking_async(state).await
    }
}

struct Jaguar3Cal<'a> {
    dev: &'a RealtekDevice,
    iqk: IqkInfo,
}

impl<'a> Jaguar3Cal<'a> {
    fn new(dev: &'a RealtekDevice) -> Self {
        Self {
            dev,
            iqk: IqkInfo::default(),
        }
    }

    async fn bb_read(&self, addr: u16) -> Result<u32, DriverError> {
        self.dev.read_u32_async(addr).await
    }

    async fn bb_write(&self, addr: u16, val: u32) -> Result<(), DriverError> {
        self.dev.write_u32_async(addr, val).await
    }

    async fn bb_get(&self, addr: u16, mask: u32) -> Result<u32, DriverError> {
        self.dev.query_bb_reg_async(addr, mask).await
    }

    async fn bb_set(&self, addr: u16, mask: u32, val: u32) -> Result<(), DriverError> {
        self.dev.set_bb_reg_async(addr, mask, val).await
    }

    async fn mac_read8(&self, addr: u16) -> Result<u8, DriverError> {
        self.dev.read_u8_async(addr).await
    }

    async fn mac_write8(&self, addr: u16, val: u8) -> Result<(), DriverError> {
        self.dev.write_u8_async(addr, val).await
    }

    async fn rf_read(&self, path: u8, addr: u16, mask: u32) -> Result<u32, DriverError> {
        let direct = RF_WIN[pidx(path)] + ((addr & 0xff) << 2);
        self.bb_get(direct, mask & RFREG_MASK).await
    }

    async fn rf_write(&self, path: u8, addr: u16, mask: u32, val: u32) -> Result<(), DriverError> {
        let direct = RF_WIN[pidx(path)] + ((addr & 0xff) << 2);
        self.bb_set(direct, mask & RFREG_MASK, val).await
    }

    async fn nctl(&self) -> Result<(), DriverError> {
        for &(addr, mask, val) in rtl_data::RTL8822C_IQK_NCTL {
            if mask == MASKDWORD {
                self.bb_write(addr, val).await?;
            } else {
                self.bb_set(addr, mask, val).await?;
            }
        }
        Ok(())
    }

    async fn backup_mac_bb(
        &self,
        mac: &mut [u32; MAC_REG_NUM_8822C],
        bb: &mut [u32; BB_REG_NUM_8822C],
    ) -> Result<(), DriverError> {
        for (idx, register) in MAC_BACKUP_REGS.into_iter().enumerate() {
            mac[idx] = self.bb_read(register).await?;
        }
        for (idx, register) in BB_BACKUP_REGS.into_iter().enumerate() {
            bb[idx] = self.bb_read(register).await?;
        }
        Ok(())
    }

    async fn backup_rf(&self, rf: &mut [[u32; 2]; RF_REG_NUM_8822C]) -> Result<(), DriverError> {
        for (idx, register) in RF_BACKUP_REGS.into_iter().enumerate() {
            rf[idx][pidx(PATH_A)] = self.rf_read(PATH_A, register, RFREG_MASK).await?;
            rf[idx][pidx(PATH_B)] = self.rf_read(PATH_B, register, RFREG_MASK).await?;
        }
        Ok(())
    }

    async fn restore_mac_bb(
        &self,
        mac: &[u32; MAC_REG_NUM_8822C],
        bb: &[u32; BB_REG_NUM_8822C],
    ) -> Result<(), DriverError> {
        self.bb_write(0x1d70, 0x5050_5050).await?;
        for (idx, register) in MAC_BACKUP_REGS.into_iter().enumerate() {
            self.bb_write(register, mac[idx]).await?;
        }
        for (idx, register) in BB_BACKUP_REGS.into_iter().enumerate() {
            self.bb_write(register, bb[idx]).await?;
        }
        self.bb_set(
            0x180c,
            1 << 31,
            if self.iqk.iqk_fail_report[0][pidx(PATH_A)][pidx(RXIQK)] {
                0
            } else {
                1
            },
        )
        .await?;
        self.bb_set(
            0x410c,
            1 << 31,
            if self.iqk.iqk_fail_report[0][pidx(PATH_B)][pidx(RXIQK)] {
                0
            } else {
                1
            },
        )
        .await
    }

    async fn restore_rf(&self, rf: &[[u32; 2]; RF_REG_NUM_8822C]) -> Result<(), DriverError> {
        self.rf_write(PATH_A, 0xef, RFREG_MASK, 0).await?;
        self.rf_write(PATH_B, 0xef, RFREG_MASK, 0).await?;
        for (idx, register) in RF_BACKUP_REGS.into_iter().enumerate() {
            self.rf_write(PATH_A, register, RFREG_MASK, rf[idx][pidx(PATH_A)])
                .await?;
            self.rf_write(PATH_B, register, RFREG_MASK, rf[idx][pidx(PATH_B)])
                .await?;
        }
        self.rf_write(PATH_A, 0xde, 1 << 16, 0).await?;
        self.rf_write(PATH_B, 0xde, 1 << 16, 0).await
    }

    async fn btc_wait_ready(&self) -> Result<(), DriverError> {
        for _ in 0..10 {
            if self.mac_read8(0x1703).await.unwrap_or(0) & (1 << 5) != 0 {
                break;
            }
            sleep_ms(10).await;
        }
        Ok(())
    }

    async fn btc_read_indirect(&self, register: u16) -> Result<u32, DriverError> {
        self.btc_wait_ready().await?;
        self.bb_write(0x1700, 0x800f_0000 | register as u32).await?;
        self.bb_read(0x1708).await
    }

    async fn btc_write_indirect(
        &self,
        register: u16,
        mask: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        if mask == 0 {
            return Ok(());
        }
        let value = if mask == MASKDWORD {
            value
        } else {
            let bitpos = mask_shift(mask);
            let current = self.btc_read_indirect(register).await?;
            (current & !mask) | ((value << bitpos) & mask)
        };
        self.btc_wait_ready().await?;
        self.bb_write(0x1704, value).await?;
        self.bb_write(0x1700, 0xc00f_0000 | register as u32).await
    }

    async fn set_gnt_wl_high(&self) -> Result<(), DriverError> {
        self.btc_write_indirect(0x38, 0xff00, 0x77).await
    }

    async fn set_gnt_wl_gnt_bt(&self, before_k: bool) -> Result<(), DriverError> {
        if before_k {
            self.set_gnt_wl_high().await
        } else {
            self.btc_write_indirect(0x38, MASKDWORD, self.iqk.tmp_gntwl)
                .await
        }
    }

    async fn check_cal(&self, path: u8, cmd: u8) -> Result<bool, DriverError> {
        let mut fail = true;
        for _ in 0..30000 {
            if self.mac_read8(0x2d9c).await.unwrap_or(0) == 0x55 {
                fail = if cmd == 0 {
                    false
                } else {
                    self.bb_get(0x1b08, 1 << 26).await.unwrap_or(1) != 0
                };
                break;
            }
            sleep_micros(10).await;
        }
        self.mac_write8(0x1b10, 0).await?;
        sleep_micros(10).await;
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        self.bb_set(0x1b20, (1 << 26) | (1 << 25), if fail { 0 } else { 2 })
            .await?;
        Ok(false)
    }

    async fn get_cfir(&mut self, idx: u8, path: u8) -> Result<(), DriverError> {
        let bit20_16 = (1 << 20) | (1 << 19) | (1 << 18) | (1 << 17) | (1 << 16);
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        if idx == TX_IQK {
            self.bb_set(0x1b20, (1 << 31) | (1 << 30), 0x3).await?;
        } else {
            self.bb_set(0x1b20, (1 << 31) | (1 << 30), 0x1).await?;
        }
        self.bb_set(0x1bd4, 1 << 21, 1).await?;
        self.bb_set(0x1bd4, bit20_16, 0x10).await?;
        for i in 0..=16 {
            self.bb_set(0x1bd8, MASKDWORD, 0xe000_0001 | (i << 2))
                .await?;
            let tmp = self.bb_get(0x1bfc, MASKDWORD).await?;
            self.iqk.iqk_cfir_real[0][pidx(path)][pidx(idx)][i as usize] =
                ((tmp & 0x0fff_0000) >> 16) as u16;
            self.iqk.iqk_cfir_imag[0][pidx(path)][pidx(idx)][i as usize] = (tmp & 0x0fff) as u16;
        }
        Ok(())
    }

    async fn backup_iqk(&mut self, step: u8, path: u8) -> Result<(), DriverError> {
        match step {
            0 => {
                self.iqk.iqk_channel[1] = self.iqk.iqk_channel[0];
                for i in 0..SS {
                    self.iqk.lok_idac[1][i] = self.iqk.lok_idac[0][i];
                    self.iqk.rxiqk_agc[1][i] = self.iqk.rxiqk_agc[0][i];
                    self.iqk.bypass_iqk[1][i] = self.iqk.bypass_iqk[0][i];
                    self.iqk.rxiqk_fail_code[1][i] = self.iqk.rxiqk_fail_code[0][i];
                    for j in 0..2 {
                        self.iqk.iqk_fail_report[1][i][j] = self.iqk.iqk_fail_report[0][i][j];
                        for k in 0..=16 {
                            self.iqk.iqk_cfir_real[1][i][j][k] = self.iqk.iqk_cfir_real[0][i][j][k];
                            self.iqk.iqk_cfir_imag[1][i][j][k] = self.iqk.iqk_cfir_imag[0][i][j][k];
                        }
                    }
                }
                for i in 0..SS {
                    self.iqk.rxiqk_fail_code[0][i] = 0;
                    self.iqk.rxiqk_agc[0][i] = 0;
                    for j in 0..2 {
                        self.iqk.iqk_fail_report[0][i][j] = true;
                        self.iqk.gs_retry_count[0][i][j] = 0;
                    }
                    for j in 0..3 {
                        self.iqk.retry_count[0][i][j] = 0;
                    }
                }
                self.iqk.iqk_channel[0] = self.iqk.rf_reg18;
            }
            1 => self.iqk.lok_idac[0][pidx(path)] = self.rf_read(path, 0x58, RFREG_MASK).await?,
            2 => self.get_cfir(TX_IQK, path).await?,
            3 => self.get_cfir(RX_IQK, path).await?,
            _ => {}
        }
        Ok(())
    }

    async fn one_shot(&mut self, path: u8, idx: u8) -> Result<bool, DriverError> {
        let is_nb = self.iqk.is_nb_iqk;
        self.set_gnt_wl_gnt_bt(true).await?;

        let temp = if idx == TXIQK {
            if is_nb {
                (0x1 << 8) | (1 << (path + 4)) | ((path as u32) << 1)
            } else {
                ((self.iqk.bw_val as u32 + 4) << 8) | (1 << (path + 4)) | ((path as u32) << 1)
            }
        } else if idx == RXIQK1 {
            if is_nb {
                (0x2 << 8) | (1 << (path + 4)) | ((path as u32) << 1)
            } else {
                ((self.iqk.bw_val as u32 + 7) << 8) | (1 << (path + 4)) | ((path as u32) << 1)
            }
        } else if is_nb {
            (0x3 << 8) | (1 << (path + 4)) | ((path as u32) << 1)
        } else {
            ((self.iqk.bw_val as u32 + 0x0a) << 8) | (1 << (path + 4)) | ((path as u32) << 1)
        };
        let iqk_cmd = 0x08 | temp;
        sleep_micros(10).await;
        self.bb_write(0x1b00, iqk_cmd).await?;
        self.bb_write(0x1b00, iqk_cmd + 1).await?;
        let fail = self.check_cal(path, 1).await?;

        if path == PATH_B {
            self.rf_write(PATH_B, 0x00, 0xf0000, 1).await?;
        }
        self.set_gnt_wl_gnt_bt(false).await?;

        if idx == TXIQK {
            self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
            self.iqk.iqk_fail_report[0][pidx(path)][pidx(TXIQK)] = fail;
            if !fail {
                if is_nb {
                    self.iqk.nbtxk_1b38[pidx(path)] = self.bb_read(0x1b38).await?;
                } else {
                    self.backup_iqk(2, path).await?;
                }
            }
        }
        if idx == RXIQK2 {
            self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
            let mut t = self.rf_read(path, 0x00, RFREG_MASK).await? >> 5;
            t = (t & 0xff) | ((self.iqk.tmp1bcc as u32) << 8);
            self.iqk.rxiqk_agc[0][pidx(path)] = t as u16;
            self.iqk.iqk_fail_report[0][pidx(path)][pidx(RXIQK)] = fail;
            if !fail {
                if is_nb {
                    self.iqk.nbrxk_1b3c[pidx(path)] = self.bb_read(0x1b3c).await?;
                } else {
                    self.backup_iqk(3, path).await?;
                }
            }
        }
        Ok(fail)
    }

    async fn cal_path_off(&self) -> Result<(), DriverError> {
        self.bb_set(0x1bb8, 1 << 20, 0).await?;
        for path in 0..SS {
            self.bb_set(0x1b00, (1 << 2) | (1 << 1), path as u32)
                .await?;
            self.bb_set(0x1bcc, 0x3f, 0x3f).await?;
        }
        Ok(())
    }

    async fn rf_direct_access(&self, path: u8, direct: bool) -> Result<(), DriverError> {
        let register = if path == PATH_A { 0x1c } else { 0xec };
        self.bb_set(register, (1 << 31) | (1 << 30), if direct { 2 } else { 0 })
            .await
    }

    async fn lok_setting(&self, path: u8, idac_bs: u8) -> Result<(), DriverError> {
        self.cal_path_off().await?;
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        self.bb_set(0x1b20, (1 << 31) | (1 << 30), 0).await?;
        self.bb_set(0x1b20, 0x3e0, 0x12).await?;
        self.rf_write(path, 0xdf, 1 << 4, 0).await?;
        self.rf_write(path, 0x9e, 1 << 5, 0).await?;
        self.rf_write(path, 0x9e, 1 << 10, 0).await?;
        self.rf_write(path, 0xde, 1 << 16, 1).await?;
        self.rf_write(
            path,
            0x56,
            0xfff,
            if self.iqk.is_5g { 0x868 } else { 0x887 },
        )
        .await?;
        self.rf_write(path, 0xef, 1 << 2, 1).await?;
        if self.iqk.is_5g {
            self.rf_write(path, 0x33, 1, 0).await?;
        }
        self.rf_write(path, 0x08, 0x70, idac_bs as u32).await?;
        self.rf_write(path, 0xef, 1 << 2, 0).await?;
        self.rf_write(path, 0x57, 1, 0).await?;
        self.rf_write(path, 0xef, 1 << 4, 1).await?;
        self.rf_write(path, 0x33, 0x7f, if self.iqk.is_5g { 0x20 } else { 0 })
            .await?;
        self.mac_write8(0x1bcc, 0x09).await?;
        self.mac_write8(0x1b10, 0).await?;
        self.bb_set(0x1b2c, 0xfff, if self.iqk.is_nb_iqk { 0x08 } else { 0x38 })
            .await
    }

    async fn lok_one_shot(&mut self, path: u8, for_rxk: bool) -> Result<bool, DriverError> {
        self.set_gnt_wl_gnt_bt(true).await?;
        let cmd = 0x08 | (1 << (path + 4)) | ((path as u32) << 1);
        sleep_micros(10).await;
        self.rf_direct_access(path, false).await?;
        self.bb_write(0x1b00, cmd).await?;
        self.bb_write(0x1b00, cmd + 1).await?;
        sleep_micros(2000).await;
        self.rf_direct_access(path, true).await?;
        self.rf_write(path, 0xef, 1 << 4, 0).await?;
        let notready = self.check_cal(path, 0).await?;
        if path == PATH_B {
            self.rf_write(PATH_B, 0x00, 0xf0000, 1).await?;
        }
        self.set_gnt_wl_gnt_bt(false).await?;
        if !for_rxk {
            self.iqk.rf_reg58 = self.rf_read(path, 0x58, RFREG_MASK).await?;
        }
        if !notready {
            self.backup_iqk(1, path).await?;
        }
        self.iqk.lok_fail[pidx(path)] = notready;
        Ok(notready)
    }

    async fn lok_check(&self, path: u8) -> Result<bool, DriverError> {
        self.cal_path_off().await?;
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        let t = self.rf_read(path, 0x58, RFREG_MASK).await?;
        let idac_i = ((t & 0xfc000) >> 14) as u8;
        let idac_q = ((t & 0x03f00) >> 8) as u8;
        Ok(!(idac_i <= 0x03 || idac_i >= 0x3c || idac_q <= 0x03 || idac_q >= 0x3c))
    }

    async fn lok_tune(&mut self, path: u8) -> Result<(), DriverError> {
        let mut idac_bs = 0x04;
        loop {
            self.lok_setting(path, idac_bs).await?;
            self.lok_one_shot(path, false).await?;
            if !self.lok_check(path).await? {
                if idac_bs == 0x06 {
                    break;
                }
                idac_bs += 1;
            } else {
                break;
            }
        }
        Ok(())
    }

    async fn txk_setting(&self, path: u8) -> Result<(), DriverError> {
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        self.bb_set(0x1bb8, 1 << 20, 0).await?;
        self.bb_write(0x1b20, 0x0004_0008).await?;
        if self.iqk.is_5g {
            let mut rf64 = self.rf_read(path, 0x64, RFREG_MASK).await?;
            rf64 = (rf64 & 0xfff0f) | 0x010;
            self.rf_write(path, 0xdf, 1 << 6, 1).await?;
            self.rf_write(path, 0x64, RFREG_MASK, rf64).await?;
            self.rf_write(path, 0x56, 0xfff, 0x8c6).await?;
        } else {
            self.rf_write(path, 0x56, 0xfff, 0x887).await?;
        }
        self.mac_write8(0x1bcc, 0x09).await?;
        self.bb_set(0x1b2c, 0xfff, if self.iqk.is_nb_iqk { 0x08 } else { 0x38 })
            .await
    }

    async fn lok_for_rxk_setting(&self, path: u8) -> Result<(), DriverError> {
        self.cal_path_off().await?;
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        self.bb_set(0x1bb8, 1 << 20, 0).await?;
        self.bb_set(0x1b20, (1 << 31) | (1 << 30), 0).await?;
        self.rf_write(path, 0x53, 1, 1).await?;
        self.rf_write(path, 0x00, 0xf0000, 0x7).await?;
        self.rf_write(path, 0x9e, 1 << 5, 1).await?;
        self.rf_write(path, 0x9e, 1 << 10, 1).await?;
        if !self.iqk.is_5g {
            self.bb_set(0x1b20, 0x3e0, 0x12).await?;
        }
        self.rf_write(path, 0xde, 1 << 16, 1).await?;
        self.rf_write(path, 0x56, 0xfff, if self.iqk.is_5g { 0 } else { 0x10 })
            .await?;
        self.rf_write(path, 0xef, 1 << 2, 1).await?;
        self.rf_write(path, 0x33, 1, if self.iqk.is_5g { 1 } else { 0 })
            .await?;
        self.rf_write(path, 0x08, 0x70, 0x4).await?;
        self.rf_write(path, 0xef, 1 << 2, 0).await?;
        self.rf_write(path, 0x57, 1, 0).await?;
        self.rf_write(path, 0xef, 1 << 4, 1).await?;
        self.rf_write(path, 0x33, 0x7f, if self.iqk.is_5g { 0x20 } else { 0 })
            .await?;
        self.mac_write8(0x1bcc, 0x09).await?;
        self.rf_write(path, 0xef, 1 << 4, 1).await?;
        self.mac_write8(0x1b10, 0).await?;
        self.mac_write8(0x1bcc, 0x12).await?;
        self.bb_set(0x1b2c, 0xfff, if self.iqk.is_nb_iqk { 0x08 } else { 0x38 })
            .await
    }

    async fn rxk1_setting(&self, path: u8) -> Result<(), DriverError> {
        self.cal_path_off().await?;
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        self.bb_set(0x1bb8, 1 << 20, 0).await?;
        self.bb_set(0x1b20, (1 << 31) | (1 << 30), 0).await?;
        self.bb_set(0x1b20, 0x3e0, 0x12).await?;
        self.rf_write(path, 0xde, 1 << 16, 1).await?;
        self.rf_write(path, 0x56, 0xfff, if self.iqk.is_5g { 0 } else { 0x20 })
            .await?;
        self.mac_write8(0x1bcc, 0x12).await?;
        self.bb_set(0x1b2c, 0xfff, if self.iqk.is_nb_iqk { 0x08 } else { 0x38 })
            .await
    }

    async fn rxk2_setting(&mut self, path: u8, is_gs: bool) -> Result<(), DriverError> {
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        self.bb_set(0x1b20, (1 << 31) | (1 << 30), 0).await?;
        if is_gs {
            self.iqk.tmp1bcc = 0x12;
        }
        self.rf_write(path, 0xde, 1 << 16, 1).await?;
        self.rf_write(path, 0x56, 0xfff, if self.iqk.is_5g { 0 } else { 0x20 })
            .await?;
        self.mac_write8(0x1bcc, self.iqk.tmp1bcc as u8).await?;
        self.bb_set(0x1b18, 1 << 1, 1).await?;
        self.bb_write(
            0x1b24,
            if self.iqk.is_5g {
                0x0007_0c08
            } else {
                0x0007_1808
            },
        )
        .await?;
        self.mac_write8(0x1b10, 0).await?;
        self.bb_set(0x1b2c, 0xfff, if self.iqk.is_nb_iqk { 0x08 } else { 0x38 })
            .await
    }

    async fn gain_search_fail(&mut self, path: u8, step: u8) -> Result<bool, DriverError> {
        if step == RXIQK1 {
            let cmd = 0x208 | (1 << (path + 4)) | ((path as u32) << 1);
            sleep_micros(10).await;
            self.bb_write(0x1b00, cmd).await?;
            self.bb_write(0x1b00, cmd + 1).await?;
            return self.check_cal(path, 1).await;
        }

        let mut idx = IQMUX
            .iter()
            .position(|value| *value == self.iqk.tmp1bcc as u8)
            .unwrap_or(4);
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        self.bb_write(0x1bcc, self.iqk.tmp1bcc as u32).await?;
        let cmd = 0x308 | (1 << (path + 4)) | ((path as u32) << 1);
        sleep_micros(10).await;
        self.bb_write(0x1b00, cmd).await?;
        self.bb_write(0x1b00, cmd + 1).await?;
        sleep_micros(20).await;
        let rf_reg0 = self.rf_read(path, 0x00, RFREG_MASK).await?;
        let k2fail = self.check_cal(path, 1).await?;
        if k2fail {
            let next = if idx > 3 { 4 } else { idx };
            self.iqk.tmp1bcc = IQMUX[next] as u16;
            return Ok(true);
        }
        self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
        let tmp = (rf_reg0 & 0x1fe0) >> 5;
        self.iqk.lna_idx = (tmp >> 5) as u8;
        let bb_idx = tmp & 0x1f;
        let mut fail = true;
        if bb_idx <= 0x01 {
            if idx != 3 {
                idx += 1;
            } else {
                self.iqk.isbnd = true;
            }
        } else if bb_idx >= 0x0a {
            if idx != 0 {
                idx -= 1;
            } else {
                self.iqk.isbnd = true;
            }
        } else {
            fail = false;
            self.iqk.isbnd = false;
        }
        if self.iqk.isbnd {
            fail = false;
        }
        self.iqk.tmp1bcc = IQMUX[idx] as u16;
        if !fail {
            self.bb_write(0x1be8, ((self.iqk.tmp1bcc as u32) << 8) | bb_idx)
                .await?;
        }
        Ok(fail)
    }

    async fn rx_iqk_by_path(&mut self, path: u8) -> Result<bool, DriverError> {
        let mut kfail = false;
        match self.iqk.rxiqk_step {
            0 => {
                self.lok_for_rxk_setting(path).await?;
                self.lok_one_shot(path, true).await?;
                self.iqk.rxiqk_step += 1;
            }
            1 => self.iqk.rxiqk_step += 1,
            2 => loop {
                self.rxk1_setting(path).await?;
                kfail = self.one_shot(path, RXIQK1).await?;
                if kfail && self.iqk.retry_count[0][pidx(path)][pidx(RXIQK1)] < 2 {
                    self.iqk.retry_count[0][pidx(path)][pidx(RXIQK1)] += 1;
                } else if kfail {
                    self.iqk.rxiqk_fail_code[0][pidx(path)] = 1;
                    self.iqk.rxiqk_step = RXK_STEP;
                    break;
                } else {
                    self.iqk.rxiqk_step += 1;
                    break;
                }
            },
            3 => {
                self.rxk2_setting(path, true).await?;
                self.iqk.isbnd = false;
                loop {
                    kfail = self.gain_search_fail(path, RXIQK2).await?;
                    if kfail && self.iqk.gs_retry_count[0][pidx(path)][1] < RXIQK_GS_LIMIT {
                        self.iqk.gs_retry_count[0][pidx(path)][1] += 1;
                    } else {
                        self.iqk.rxiqk_step += 1;
                        break;
                    }
                }
            }
            4 => loop {
                self.rxk2_setting(path, false).await?;
                kfail = self.one_shot(path, RXIQK2).await?;
                if kfail && self.iqk.retry_count[0][pidx(path)][pidx(RXIQK2)] < 2 {
                    self.iqk.retry_count[0][pidx(path)][pidx(RXIQK2)] += 1;
                } else if kfail {
                    self.iqk.rxiqk_fail_code[0][pidx(path)] = 2;
                    self.iqk.rxiqk_step = RXK_STEP;
                    break;
                } else {
                    self.iqk.rxiqk_step += 1;
                    break;
                }
            },
            5 => self.iqk.rxiqk_step += 1,
            _ => {}
        }
        Ok(kfail)
    }

    async fn iqk_by_path(&mut self, _segment: bool) -> Result<(), DriverError> {
        match self.iqk.iqk_step {
            0 => {
                let mut counter = 0;
                loop {
                    counter += 1;
                    let kfail = self.rx_iqk_by_path(PATH_A).await?;
                    if !kfail && self.iqk.rxiqk_step == RXK_STEP {
                        self.iqk.iqk_step += 1;
                        self.iqk.rxiqk_step = 0;
                        break;
                    }
                    if counter > 60 && self.iqk.rxiqk_step == 0 {
                        self.iqk.iqk_step += 1;
                        break;
                    }
                }
                self.iqk.kcount += 1;
            }
            1 => {
                self.lok_tune(PATH_A).await?;
                self.iqk.iqk_step += 1;
            }
            2 => {
                self.txk_setting(PATH_A).await?;
                self.one_shot(PATH_A, TXIQK).await?;
                self.rf_write(PATH_A, 0xef, 1 << 4, 0).await?;
                self.iqk.kcount += 1;
                self.iqk.iqk_step += 1;
            }
            3 => {
                let mut counter = 0;
                loop {
                    counter += 1;
                    let kfail = self.rx_iqk_by_path(PATH_B).await?;
                    if !kfail && self.iqk.rxiqk_step == RXK_STEP {
                        self.iqk.iqk_step += 1;
                        self.iqk.rxiqk_step = 0;
                        break;
                    }
                    if counter > 60 && self.iqk.rxiqk_step == 0 {
                        self.iqk.iqk_step += 1;
                        break;
                    }
                }
                self.iqk.kcount += 1;
            }
            4 => {
                self.lok_tune(PATH_B).await?;
                self.iqk.iqk_step += 1;
            }
            5 => {
                self.txk_setting(PATH_B).await?;
                let kfail = self.one_shot(PATH_B, TXIQK).await?;
                self.rf_write(PATH_B, 0xef, 1 << 4, 0).await?;
                self.iqk.kcount += 1;
                if kfail && self.iqk.retry_count[0][pidx(PATH_B)][pidx(TXIQK)] < 3 {
                    self.iqk.retry_count[0][pidx(PATH_B)][pidx(TXIQK)] += 1;
                } else {
                    self.iqk.iqk_step += 1;
                }
            }
            6 => self.iqk.iqk_step += 1,
            _ => {}
        }

        if self.iqk.iqk_step == IQK_STEP {
            for path in 0..SS {
                self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
                self.bb_set(0x1bb8, 1 << 20, 0).await?;
                self.bb_set(0x1bcc, 0xff, 0).await?;
                if self.iqk.is_nb_iqk {
                    self.bb_set(0x1b20, 1 << 26, 0).await?;
                    self.bb_write(0x1b38, self.iqk.nbtxk_1b38[path]).await?;
                    self.bb_write(0x1b3c, self.iqk.nbrxk_1b3c[path]).await?;
                } else {
                    self.bb_set(0x1b20, 1 << 26, 1).await?;
                    self.bb_write(0x1b38, 0x4000_0000).await?;
                    self.bb_write(0x1b3c, 0x4000_0000).await?;
                }
                self.rf_write(path as u8, 0x00, 0xf0000, 0x3).await?;
            }
        }
        Ok(())
    }

    async fn start_iqk(&mut self, segment: bool) -> Result<(), DriverError> {
        let kcount_limit = if self.iqk.bw_val == 2 {
            KCOUNT_LIMIT_80M
        } else {
            KCOUNT_LIMIT_OTHERS
        };
        for _ in 0..100 {
            self.iqk_by_path(segment).await?;
            if self.iqk.iqk_step == IQK_STEP {
                break;
            }
            if segment && self.iqk.kcount == kcount_limit {
                break;
            }
        }
        Ok(())
    }

    async fn information(&mut self) -> Result<(), DriverError> {
        self.iqk.is_tssi_mode = self.bb_get(0x1e7c, 1 << 30).await? != 0;
        let r18 = self.rf_read(PATH_A, 0x18, RFREG_MASK).await?;
        self.iqk.iqk_band = ((r18 & (1 << 16)) >> 16) as u8;
        self.iqk.iqk_ch = (r18 & 0xff) as u8;
        self.iqk.iqk_bw = ((r18 & 0x3000) >> 12) as u8;
        Ok(())
    }

    async fn macbb(&self) -> Result<(), DriverError> {
        if self.iqk.is_tssi_mode {
            self.bb_set(0x1e7c, 1 << 30, 0).await?;
            self.bb_set(0x18a4, 1 << 28, 0).await?;
            self.bb_set(0x41a4, 1 << 28, 0).await?;
        }
        self.mac_write8(0x0522, 0xff).await?;
        self.bb_set(0x0070, 0xff00_0000, 0x06).await?;
        self.bb_set(0x1e24, 1 << 17, 1).await?;
        self.bb_set(0x1cd0, (1 << 30) | (1 << 29) | (1 << 28), 0x7)
            .await?;
        self.bb_set(0x1d60, 1 << 31, 1).await?;
        self.bb_write(0x1c38, MASKDWORD).await?;
        self.bb_set(0x1a14, 0x300, 0x3).await?;
        self.bb_set(0x0824, 0x30000, 0x3).await
    }

    async fn bb_for_dpk_setting(&self) -> Result<(), DriverError> {
        self.bb_set(0x1e24, 1 << 17, 1).await?;
        self.bb_set(0x1cd0, 1 << 28, 1).await?;
        self.bb_set(0x1cd0, 1 << 29, 1).await?;
        self.bb_set(0x1cd0, 1 << 30, 1).await?;
        self.bb_set(0x1cd0, 1 << 31, 0).await?;
        self.bb_set(0x1d58, 0xff8, 0x1ff).await?;
        self.bb_set(0x1864, 1 << 31, 1).await?;
        self.bb_set(0x4164, 1 << 31, 1).await?;
        self.bb_set(0x180c, 1 << 27, 1).await?;
        self.bb_set(0x410c, 1 << 27, 1).await?;
        self.bb_set(0x186c, 1 << 7, 1).await?;
        self.bb_set(0x416c, 1 << 7, 1).await?;
        self.bb_set(0x180c, 0x3, 0).await?;
        self.bb_set(0x410c, 0x3, 0).await?;
        self.bb_set(0x1a00, (1 << 1) | 1, 0x2).await
    }

    async fn afe_setting(&self, do_iqk: bool) -> Result<(), DriverError> {
        if do_iqk {
            self.afe_ramp(0x1830).await?;
            self.afe_ramp(0x4130).await?;
            self.bb_write(0x1c38, 0).await?;
            sleep_micros(10).await;
            return self.bb_write(0x1c38, MASKDWORD).await;
        }
        if self.iqk.is_tssi_mode {
            let b = 0x4 >> self.iqk.iqk_band;
            self.bb_write(0x1c38, 0xf7d5_005e).await?;
            self.bb_set(0x1860, 0x0000_7000, b).await?;
            let restore = [
                0x700b_8041,
                0x701f_0040 | b,
                0x702f_0040 | b,
                0x703f_0040 | b,
                0x704f_0040 | b,
                0x705b_8041,
                0x706f_0040 | b,
            ];
            for value in restore {
                self.bb_write(0x1830, value).await?;
            }
            for value in restore {
                self.bb_write(0x4130, value).await?;
            }
        } else {
            let restore = [
                0x700b_8041,
                0x7014_4041,
                0x7024_4041,
                0x7034_4041,
                0x7044_4041,
                0x705b_8041,
                0x7064_4041,
                0x707b_8041,
                0x708b_8041,
                0x709b_8041,
                0x70ab_8041,
                0x70bb_8041,
                0x70cb_8041,
                0x70db_8041,
                0x70eb_8041,
                0x70fb_8041,
            ];
            for value in restore {
                self.bb_write(0x1830, value).await?;
            }
            for value in restore {
                self.bb_write(0x4130, value).await?;
            }
        }
        self.bb_set(0x1bb8, 1 << 20, 0).await?;
        self.bb_set(0x1bcc, 0xff, 0).await?;
        self.bb_set(0x1d58, 0xff8, 0).await?;
        self.bb_set(0x1864, 1 << 31, 0).await?;
        self.bb_set(0x4164, 1 << 31, 0).await?;
        self.bb_set(0x180c, 1 << 27, 0).await?;
        self.bb_set(0x410c, 1 << 27, 0).await?;
        self.bb_set(0x186c, 1 << 7, 0).await?;
        self.bb_set(0x416c, 1 << 7, 0).await?;
        self.bb_set(0x180c, (1 << 1) | 1, 0x3).await?;
        self.bb_set(0x410c, (1 << 1) | 1, 0x3).await?;
        self.bb_set(0x1a00, (1 << 1) | 1, 0).await
    }

    async fn afe_ramp(&self, register: u16) -> Result<(), DriverError> {
        self.bb_write(register, 0x700f_0001).await?;
        for n in 0..16 {
            self.bb_write(register, 0x700f_0001 | (n << 20)).await?;
        }
        self.bb_write(register, 0x70ff_0001).await
    }

    async fn fill_report(&self, channel_slot: usize) -> Result<(), DriverError> {
        for path in 0..SS {
            let t1 = (self.iqk.iqk_fail_report[channel_slot][path][pidx(TX_IQK)] as u32) << path;
            let t2 =
                (self.iqk.iqk_fail_report[channel_slot][path][pidx(RX_IQK)] as u32) << (path + 4);
            let t3 = (self.iqk.rxiqk_fail_code[channel_slot][path] as u32 & 0x3) << (path * 2 + 8);
            self.bb_write(0x1b00, 0x08 | ((path as u32) << 1)).await?;
            self.bb_set(0x1bf0, 0x0000_ffff, t1 | t2 | t3).await?;
            self.bb_write(0x1be8, self.iqk.rxiqk_agc[channel_slot][path] as u32)
                .await?;
        }
        Ok(())
    }

    async fn phy_iq_calibrate(
        &mut self,
        width: ChannelWidth,
        channel: u8,
    ) -> Result<(), DriverError> {
        self.iqk.is_nb_iqk = matches!(width, ChannelWidth::Mhz5 | ChannelWidth::Mhz10);
        self.iqk.bw_val = if self.iqk.is_nb_iqk {
            0
        } else {
            match width {
                ChannelWidth::Mhz40 => 1,
                ChannelWidth::Mhz80 => 2,
                _ => 0,
            }
        };
        self.iqk.is_5g = channel > 14;
        let mut mac = [0u32; MAC_REG_NUM_8822C];
        let mut bbk = [0u32; BB_REG_NUM_8822C];
        let mut rf = [[0u32; 2]; RF_REG_NUM_8822C];

        self.iqk.rf_reg18 = self.rf_read(PATH_A, 0x18, RFREG_MASK).await?;
        self.iqk.iqk_times += 1;
        self.iqk.kcount = 0;
        self.iqk.iqk_step = 0;
        self.iqk.rxiqk_step = 0;
        self.iqk.tmp_gntwl = self.btc_read_indirect(0x38).await.unwrap_or(0);

        self.information().await?;
        self.backup_iqk(0, 0).await?;
        self.backup_mac_bb(&mut mac, &mut bbk).await?;
        self.backup_rf(&mut rf).await?;

        for _ in 0..3 {
            self.macbb().await?;
            self.bb_for_dpk_setting().await?;
            self.afe_setting(true).await?;
            self.nctl().await?;
            self.start_iqk(false).await?;
            self.afe_setting(false).await?;
            self.restore_rf(&rf).await?;
            self.restore_mac_bb(&mac, &bbk).await?;
            if self.iqk.iqk_step == IQK_STEP {
                break;
            }
            self.iqk.kcount = 0;
            sleep_ms(5).await;
        }
        self.fill_report(0).await
    }

    async fn dac_calibrate(&mut self) -> Result<(), DriverError> {
        let bp_reg = [
            0x180c, 0x1810, 0x410c, 0x4110, 0x1c3c, 0x1c24, 0x1d70, 0x09b4, 0x1a00, 0x1a14, 0x1d58,
            0x1c38, 0x1e24, 0x1e28, 0x1860, 0x4160,
        ];
        let mut bp = [0u32; 16];
        for (idx, register) in bp_reg.into_iter().enumerate() {
            bp[idx] = self.bb_read(register).await?;
        }
        let bprf = [
            self.rf_read(PATH_A, 0x8f, RFREG_MASK).await?,
            self.rf_read(PATH_B, 0x8f, RFREG_MASK).await?,
        ];

        let mut ic = 0;
        let mut qc = 0;
        let mut adc_ic_a = 0;
        let mut adc_qc_a = 0;
        let mut adc_ic_b = 0;
        let mut adc_qc_b = 0;

        self.bb_set(0x1d58, 0xff8, 0x1ff).await?;
        self.bb_set(0x1a00, 0x3, 0x2).await?;
        self.bb_set(0x1a14, 0x300, 0x3).await?;
        self.bb_write(0x1d70, 0x7e7e_7e7e).await?;
        self.bb_set(0x180c, 0x3, 0).await?;
        self.bb_set(0x410c, 0x3, 0).await?;
        self.bb_write(0x1b00, 0x0000_0008).await?;
        self.mac_write8(0x1bcc, 0x3f).await?;
        self.bb_write(0x1b00, 0x0000_000a).await?;
        self.mac_write8(0x1bcc, 0x3f).await?;
        self.bb_set(0x1e24, 1 << 31, 0).await?;
        self.bb_set(0x1e28, 0xf, 0x3).await?;

        let temp_a = self
            .dack_path_a(&mut ic, &mut qc, &mut adc_ic_a, &mut adc_qc_a)
            .await?;
        self.dack_path_b(&mut ic, &mut qc, &mut adc_ic_b, &mut adc_qc_b)
            .await?;

        let _ = temp_a;
        self.bb_write(0x1b00, 0x0000_0008).await?;
        self.mac_write8(0x1bcc, 0).await?;
        self.bb_write(0x1b00, 0x0000_000a).await?;
        self.mac_write8(0x1bcc, 0).await?;
        for (idx, register) in bp_reg.into_iter().enumerate() {
            self.bb_write(register, bp[idx]).await?;
        }
        self.rf_write(PATH_A, 0x8f, RFREG_MASK, bprf[0]).await?;
        self.rf_write(PATH_B, 0x8f, RFREG_MASK, bprf[1]).await
    }

    async fn dack_path_a(
        &self,
        ic: &mut u32,
        qc: &mut u32,
        adc_ic_a: &mut u32,
        adc_qc_a: &mut u32,
    ) -> Result<u32, DriverError> {
        self.bb_set(0x1830, 1 << 30, 0).await?;
        self.bb_write(0x1860, 0xf004_0ff0).await?;
        self.bb_write(0x180c, 0xdff0_0220).await?;
        self.bb_write(0x1810, 0x02dd_08c4).await?;
        self.bb_write(0x180c, 0x1000_0260).await?;
        self.rf_write(PATH_A, 0x00, RFREG_MASK, 0x10000).await?;
        self.rf_write(PATH_B, 0x00, RFREG_MASK, 0x10000).await?;
        let mut temp = 0;
        for _ in 0..10 {
            self.bb_write(0x1c3c, 0x0008_8003).await?;
            self.bb_write(0x1c24, 0x0001_0002).await?;
            (*ic, *qc) = self.dack_mode().await?;
            if *ic != 0 {
                *ic = 0x400 - *ic;
                *adc_ic_a = *ic;
            }
            if *qc != 0 {
                *qc = 0x400 - *qc;
                *adc_qc_a = *qc;
            }
            temp = (*ic & 0x3ff) | ((*qc & 0x3ff) << 10);
            self.bb_write(0x1868, temp).await?;
            self.bb_write(0x1c3c, 0x0008_8103).await?;
            (*ic, *qc) = self.dack_mode().await?;
            if *ic >= 0x200 {
                *ic = 0x400 - *ic;
            }
            if *qc >= 0x200 {
                *qc = 0x400 - *qc;
            }
            if *ic < 5 && *qc < 5 {
                break;
            }
        }
        self.bb_write(0x1c3c, 0x0000_0003).await?;
        self.bb_write(0x180c, 0x1000_0260).await?;
        self.bb_write(0x1810, 0x02d5_08c4).await?;
        self.rf_write(PATH_A, 0x8f, 1 << 13, 1).await?;
        for _ in 0..10 {
            self.bb_write(0x1868, temp).await?;
            self.bb_write(0x180c, 0xdff0_0220).await?;
            self.bb_write(0x1860, 0xf004_0ff0).await?;
            self.bb_write(0x1c38, MASKDWORD).await?;
            self.bb_write(0x1810, 0x02d5_08c5).await?;
            self.bb_write(0x09b4, 0xdb66_db00).await?;
            self.bb_write(0x18b0, 0x0a11_fb88).await?;
            self.bb_write(0x18bc, 0x0008_ff81).await?;
            self.bb_write(0x18c0, 0x0003_d208).await?;
            self.bb_write(0x18cc, 0x0a11_fb88).await?;
            self.bb_write(0x18d8, 0x0008_ff81).await?;
            self.bb_write(0x18dc, 0x0003_d208).await?;
            self.bb_write(0x18b8, 0x6000_0000).await?;
            sleep_ms(2).await;
            self.bb_write(0x18bc, 0x000a_ff8d).await?;
            sleep_ms(2).await;
            self.bb_write(0x18b0, 0x0a11_fb89).await?;
            self.bb_write(0x18cc, 0x0a11_fb89).await?;
            sleep_ms(1).await;
            self.bb_write(0x18b8, 0x6200_0000).await?;
            self.bb_write(0x18d4, 0x6200_0000).await?;
            sleep_ms(1).await;
            self.dack_poll(0x2808, 0x7fff80, 0xffff).await?;
            self.dack_poll(0x2834, 0x7fff80, 0xffff).await?;
            self.bb_write(0x18b8, 0x0200_0000).await?;
            sleep_ms(1).await;
            self.bb_write(0x18bc, 0x0008_ff87).await?;
            self.bb_write(0x09b4, 0xdb6d_b600).await?;
            self.bb_write(0x1810, 0x02d5_08c5).await?;
            self.bb_write(0x18bc, 0x0008_ff87).await?;
            self.bb_write(0x1860, 0xf000_0000).await?;
            self.bb_set(0x18bc, 0xf000_0000, 0).await?;
            self.bb_set(0x18c0, 0xf, 0x8).await?;
            self.bb_set(0x18d8, 0xf000_0000, 0).await?;
            self.bb_set(0x18dc, 0xf, 0x8).await?;
            self.bb_write(0x1b00, 0x0000_0008).await?;
            self.mac_write8(0x1bcc, 0x3f).await?;
            self.bb_write(0x180c, 0xdff0_0220).await?;
            self.bb_write(0x1810, 0x02d5_08c5).await?;
            self.bb_write(0x1c3c, 0x0008_8103).await?;
            (*ic, *qc) = self.dack_mode().await?;
            if *ic != 0 {
                *ic = 0x400 - *ic;
            }
            if *qc != 0 {
                *qc = 0x400 - *qc;
            }
            *ic = dack_adjust(*ic);
            *qc = dack_adjust(*qc);
            self.bb_write(0x180c, 0xdff0_0220).await?;
            self.bb_write(0x1810, 0x02d5_08c5).await?;
            self.bb_write(0x09b4, 0xdb66_db00).await?;
            self.bb_write(0x18b0, 0x0a11_fb88).await?;
            self.bb_write(0x18bc, 0xc008_ff81).await?;
            self.bb_write(0x18c0, 0x0003_d208).await?;
            self.bb_set(0x18bc, 0xf000_0000, *ic & 0xf).await?;
            self.bb_set(0x18c0, 0xf, (*ic & 0xf0) >> 4).await?;
            self.bb_write(0x18cc, 0x0a11_fb88).await?;
            self.bb_write(0x18d8, 0xe008_ff81).await?;
            self.bb_write(0x18dc, 0x0003_d208).await?;
            self.bb_set(0x18d8, 0xf000_0000, *qc & 0xf).await?;
            self.bb_set(0x18dc, 0xf, (*qc & 0xf0) >> 4).await?;
            self.bb_write(0x18b8, 0x6000_0000).await?;
            sleep_ms(2).await;
            self.bb_set(0x18bc, 0xe, 0x6).await?;
            sleep_ms(2).await;
            self.bb_write(0x18b0, 0x0a11_fb89).await?;
            self.bb_write(0x18cc, 0x0a11_fb89).await?;
            sleep_ms(1).await;
            self.bb_write(0x18b8, 0x6200_0000).await?;
            self.bb_write(0x18d4, 0x6200_0000).await?;
            sleep_ms(1).await;
            self.dack_poll(0x2824, 0x07f8_0000, *ic).await?;
            self.dack_poll(0x2850, 0x07f8_0000, *qc).await?;
            self.bb_write(0x18b8, 0x0200_0000).await?;
            sleep_ms(1).await;
            self.bb_set(0x18bc, 0xe, 0x3).await?;
            self.bb_write(0x09b4, 0xdb6d_b600).await?;
            let temp1 = ((*adc_ic_a + 0x10) & 0x3ff) | (((*adc_qc_a + 0x10) & 0x3ff) << 10);
            self.bb_write(0x1868, temp1).await?;
            self.bb_write(0x1810, 0x02d5_08c5).await?;
            self.bb_write(0x1860, 0xf000_0000).await?;
            (*ic, *qc) = self.dack_mode().await?;
            *ic = dack_sub_0x10(*ic);
            *qc = dack_sub_0x10(*qc);
            if *ic >= 0x200 {
                *ic = 0x400 - *ic;
            }
            if *qc >= 0x200 {
                *qc = 0x400 - *qc;
            }
            if *ic < 5 && *qc < 5 {
                break;
            }
        }
        self.bb_write(0x1868, 0).await?;
        self.bb_write(0x1810, 0x02d5_08c4).await?;
        self.bb_set(0x18bc, 1, 0).await?;
        self.bb_set(0x1830, 1 << 30, 1).await?;
        Ok(temp)
    }

    async fn dack_path_b(
        &self,
        ic: &mut u32,
        qc: &mut u32,
        adc_ic_b: &mut u32,
        adc_qc_b: &mut u32,
    ) -> Result<(), DriverError> {
        self.bb_set(0x4130, 1 << 30, 0).await?;
        self.bb_write(0x4130, 0x30db_8041).await?;
        self.bb_write(0x4160, 0xf004_0ff0).await?;
        self.bb_write(0x410c, 0xdff0_0220).await?;
        self.bb_write(0x4110, 0x02dd_08c4).await?;
        self.bb_write(0x410c, 0x1000_0260).await?;
        self.rf_write(PATH_A, 0x00, RFREG_MASK, 0x10000).await?;
        self.rf_write(PATH_B, 0x00, RFREG_MASK, 0x10000).await?;
        let mut temp = 0;
        for _ in 0..10 {
            self.bb_write(0x1c3c, 0x000a_8003).await?;
            self.bb_write(0x1c24, 0x0001_0002).await?;
            (*ic, *qc) = self.dack_mode().await?;
            if *ic != 0 {
                *ic = 0x400 - *ic;
                *adc_ic_b = *ic;
            }
            if *qc != 0 {
                *qc = 0x400 - *qc;
                *adc_qc_b = *qc;
            }
            temp = (*ic & 0x3ff) | ((*qc & 0x3ff) << 10);
            self.bb_write(0x4168, temp).await?;
            self.bb_write(0x1c3c, 0x000a_8103).await?;
            (*ic, *qc) = self.dack_mode().await?;
            if *ic >= 0x200 {
                *ic = 0x400 - *ic;
            }
            if *qc >= 0x200 {
                *qc = 0x400 - *qc;
            }
            if *ic < 5 && *qc < 5 {
                break;
            }
        }
        self.bb_write(0x1c3c, 0x0000_0003).await?;
        self.bb_write(0x410c, 0x1000_0260).await?;
        self.bb_write(0x4110, 0x02d5_08c4).await?;
        self.rf_write(PATH_B, 0x8f, 1 << 13, 1).await?;
        for _ in 0..10 {
            self.bb_write(0x4168, temp).await?;
            self.bb_write(0x410c, 0xdff0_0220).await?;
            self.bb_write(0x4110, 0x02d5_08c5).await?;
            self.bb_write(0x09b4, 0xdb66_db00).await?;
            self.bb_write(0x41b0, 0x0a11_fb88).await?;
            self.bb_write(0x41bc, 0x0008_ff81).await?;
            self.bb_write(0x41c0, 0x0003_d208).await?;
            self.bb_write(0x41cc, 0x0a11_fb88).await?;
            self.bb_write(0x41d8, 0x0008_ff81).await?;
            self.bb_write(0x41dc, 0x0003_d208).await?;
            self.bb_write(0x41b8, 0x6000_0000).await?;
            sleep_ms(2).await;
            self.bb_write(0x41bc, 0x000a_ff8d).await?;
            sleep_ms(2).await;
            self.bb_write(0x41b0, 0x0a11_fb89).await?;
            self.bb_write(0x41cc, 0x0a11_fb89).await?;
            sleep_ms(1).await;
            self.bb_write(0x41b8, 0x6200_0000).await?;
            self.bb_write(0x41d4, 0x6200_0000).await?;
            sleep_ms(1).await;
            self.dack_poll(0x4508, 0x7fff80, 0xffff).await?;
            self.dack_poll(0x4534, 0x7fff80, 0xffff).await?;
            self.bb_write(0x41b8, 0x0200_0000).await?;
            sleep_ms(1).await;
            self.bb_write(0x41bc, 0x0008_ff87).await?;
            self.bb_write(0x09b4, 0xdb6d_b600).await?;
            self.bb_write(0x4110, 0x02d5_08c5).await?;
            self.bb_write(0x41bc, 0x0008_ff87).await?;
            self.bb_write(0x4160, 0xf000_0000).await?;
            self.bb_set(0x41bc, 0xf000_0000, 0).await?;
            self.bb_set(0x41c0, 0xf, 0x8).await?;
            self.bb_set(0x41d8, 0xf000_0000, 0).await?;
            self.bb_set(0x41dc, 0xf, 0x8).await?;
            self.bb_write(0x1b00, 0x0000_000a).await?;
            self.mac_write8(0x1bcc, 0x3f).await?;
            self.bb_write(0x410c, 0xdff0_0220).await?;
            self.bb_write(0x4110, 0x02d5_08c5).await?;
            self.bb_write(0x1c3c, 0x000a_8103).await?;
            (*ic, *qc) = self.dack_mode().await?;
            if *ic != 0 {
                *ic = 0x400 - *ic;
            }
            if *qc != 0 {
                *qc = 0x400 - *qc;
            }
            *ic = dack_adjust(*ic);
            *qc = dack_adjust(*qc);
            self.bb_write(0x410c, 0xdff0_0220).await?;
            self.bb_write(0x4110, 0x02d5_08c5).await?;
            self.bb_write(0x09b4, 0xdb66_db00).await?;
            self.bb_write(0x41b0, 0x0a11_fb88).await?;
            self.bb_write(0x41bc, 0xc008_ff81).await?;
            self.bb_write(0x41c0, 0x0003_d208).await?;
            self.bb_set(0x41bc, 0xf000_0000, *ic & 0xf).await?;
            self.bb_set(0x41c0, 0xf, (*ic & 0xf0) >> 4).await?;
            self.bb_write(0x41cc, 0x0a11_fb88).await?;
            self.bb_write(0x41d8, 0xe008_ff81).await?;
            self.bb_write(0x41dc, 0x0003_d208).await?;
            self.bb_set(0x41d8, 0xf000_0000, *qc & 0xf).await?;
            self.bb_set(0x41dc, 0xf, (*qc & 0xf0) >> 4).await?;
            self.bb_write(0x41b8, 0x6000_0000).await?;
            sleep_ms(2).await;
            self.bb_set(0x41bc, 0xe, 0x6).await?;
            sleep_ms(2).await;
            self.bb_write(0x41b0, 0x0a11_fb89).await?;
            self.bb_write(0x41cc, 0x0a11_fb89).await?;
            sleep_ms(1).await;
            self.bb_write(0x41b8, 0x6200_0000).await?;
            self.bb_write(0x41d4, 0x6200_0000).await?;
            sleep_ms(1).await;
            self.dack_poll(0x4524, 0x07f8_0000, *ic).await?;
            self.dack_poll(0x4550, 0x07f8_0000, *qc).await?;
            self.bb_write(0x41b8, 0x0200_0000).await?;
            sleep_ms(1).await;
            self.bb_set(0x41bc, 0xe, 0x3).await?;
            self.bb_write(0x09b4, 0xdb6d_b600).await?;
            let temp1 = ((*adc_ic_b + 0x10) & 0x3ff) | (((*adc_qc_b + 0x10) & 0x3ff) << 10);
            self.bb_write(0x4168, temp1).await?;
            self.bb_write(0x4110, 0x02d5_08c5).await?;
            self.bb_write(0x4160, 0xf000_0000).await?;
            (*ic, *qc) = self.dack_mode().await?;
            *ic = dack_sub_0x10(*ic);
            *qc = dack_sub_0x10(*qc);
            if *ic >= 0x200 {
                *ic = 0x400 - *ic;
            }
            if *qc >= 0x200 {
                *qc = 0x400 - *qc;
            }
            if *ic < 5 && *qc < 5 {
                break;
            }
        }
        self.bb_write(0x4168, 0).await?;
        self.bb_write(0x4110, 0x02d5_08c4).await?;
        self.bb_set(0x41bc, 1, 0).await?;
        self.bb_set(0x4130, 1 << 30, 1).await
    }

    async fn dack_poll(&self, addr: u16, mask: u32, data: u32) -> Result<(), DriverError> {
        for _ in 0..100000 {
            if self.bb_get(addr, mask).await? == data {
                break;
            }
        }
        Ok(())
    }

    async fn dack_mode(&self) -> Result<(u32, u32), DriverError> {
        let mut iv = [0u32; DACK_SN];
        let mut qv = [0u32; DACK_SN];
        let mut sample_count = 0usize;
        for _ in 0..10000 {
            if sample_count >= DACK_SN {
                break;
            }
            let temp = self.bb_get(0x2dbc, 0x3f_ffff).await?;
            iv[sample_count] = (temp & 0x3ff000) >> 12;
            qv[sample_count] = temp & 0x3ff;
            if !(dack_compare(iv[sample_count]) || dack_compare(qv[sample_count])) {
                sample_count += 1;
            }
        }
        for _ in 0..100 {
            let (mut i_min, mut i_max, mut q_min, mut q_max) = (iv[0], iv[0], qv[0], qv[0]);
            for idx in 0..DACK_SN {
                dack_minmax(iv[idx], &mut i_min, &mut i_max);
                dack_minmax(qv[idx], &mut q_min, &mut q_max);
            }
            let i_d = if (i_max < 0x200 && i_min < 0x200) || (i_max >= 0x200 && i_min >= 0x200) {
                i_max.saturating_sub(i_min)
            } else {
                i_max + (0x400 - i_min)
            };
            let q_d = if (q_max < 0x200 && q_min < 0x200) || (q_max >= 0x200 && q_min >= 0x200) {
                q_max.saturating_sub(q_min)
            } else {
                q_max + (0x400 - q_min)
            };
            dack_bsort(&mut iv);
            dack_bsort(&mut qv);
            if i_d > 5 || q_d > 5 {
                let temp = self.bb_get(0x2dbc, 0x3f_ffff).await?;
                iv[0] = (temp & 0x3ff000) >> 12;
                qv[0] = temp & 0x3ff;
                let temp = self.bb_get(0x2dbc, 0x3f_ffff).await?;
                iv[DACK_SN - 1] = (temp & 0x3ff000) >> 12;
                qv[DACK_SN - 1] = temp & 0x3ff;
            } else {
                break;
            }
        }
        Ok((dack_average(&iv), dack_average(&qv)))
    }

    async fn pwr_track(
        &mut self,
        state: &mut Jaguar3PowerTrackingState,
    ) -> Result<Jaguar3PowerTrackingReport, DriverError> {
        const P5A: [u8; 30] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30,
        ];
        const N5A: [u8; 30] = [
            0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 23, 24, 25, 26,
            27, 28, 29, 30, 31, 33,
        ];
        const P5B: [u8; 30] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
            22, 23, 24, 25, 26, 27,
        ];
        const N5B: [u8; 30] = [
            0, 1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 22, 23, 24, 25,
            26, 27, 28, 29, 30, 32,
        ];
        let pos = [&P5A, &P5B];
        let neg = [&N5A, &N5B];
        let ref_reg = [0x18a0u16, 0x41a0u16];
        for path in [PATH_A, PATH_B] {
            self.rf_write(path, 0x42, 1 << 19, 1).await?;
            self.rf_write(path, 0x42, 1 << 19, 0).await?;
            self.rf_write(path, 0x42, 1 << 19, 1).await?;
        }
        sleep_micros(15).await;
        let mut thermal = [0u8; 2];
        for path in [PATH_A, PATH_B] {
            thermal[pidx(path)] = self.rf_read(path, 0x42, 0x7e).await? as u8;
            if state.thermal_ref[pidx(path)] < 0 {
                state.thermal_ref[pidx(path)] = thermal[pidx(path)] as i16;
            }
        }
        if state.thermal_lck_ref < 0 {
            state.thermal_lck_ref = thermal[pidx(PATH_A)] as i16;
        }
        let mut lck_ran = false;
        if (thermal[pidx(PATH_A)] as i16 - state.thermal_lck_ref).abs() >= 8 {
            self.do_lck().await?;
            state.thermal_lck_ref = thermal[pidx(PATH_A)] as i16;
            lck_ran = true;
        }
        let mut compensation = [0i8; 2];
        for path in [PATH_A, PATH_B] {
            let idx = pidx(path);
            let delta = (thermal[idx] as i16 - state.thermal_ref[idx])
                .unsigned_abs()
                .min(29) as usize;
            let value = if thermal[idx] as i16 > state.thermal_ref[idx] {
                pos[idx][delta] as i8
            } else {
                -(neg[idx][delta] as i8)
            };
            compensation[idx] = value;
            self.bb_set(ref_reg[idx], 0x7f, value as u32 & 0x7f).await?;
        }
        Ok(Jaguar3PowerTrackingReport {
            thermal_raw: thermal,
            thermal_ref: state.thermal_ref,
            compensation_index: compensation,
            lck_ran,
        })
    }

    async fn do_lck(&self) -> Result<(), DriverError> {
        self.rf_write(PATH_A, 0xbb, RFREG_MASK, 0x80010).await?;
        self.rf_write(PATH_A, 0xb0, RFREG_MASK, 0x1f0fa).await?;
        sleep_micros(1).await;
        self.rf_write(PATH_A, 0xca, RFREG_MASK, 0x80000).await?;
        self.rf_write(PATH_A, 0xc9, RFREG_MASK, 0x80001).await?;
        for _ in 0..100 {
            if self.rf_read(PATH_A, 0xca, 0x1000).await? != 1 {
                break;
            }
            sleep_ms(1).await;
        }
        self.rf_write(PATH_A, 0xb0, RFREG_MASK, 0x1f0f8).await?;
        self.rf_write(PATH_B, 0xbb, RFREG_MASK, 0x80010).await?;
        self.rf_write(PATH_A, 0xcc, RFREG_MASK, 0x0f000).await?;
        self.rf_write(PATH_A, 0xcc, RFREG_MASK, 0x4f000).await?;
        sleep_micros(1).await;
        self.rf_write(PATH_A, 0xcc, RFREG_MASK, 0x0f000).await
    }
}

#[derive(Debug, Clone)]
struct IqkInfo {
    iqk_step: u8,
    rxiqk_step: u8,
    kcount: u8,
    iqk_times: u32,
    rf_reg18: u32,
    tmp_gntwl: u32,
    tmp1bcc: u16,
    is_nb_iqk: bool,
    is_tssi_mode: bool,
    is_5g: bool,
    bw_val: u8,
    rf_reg58: u32,
    iqk_channel: [u32; 2],
    lok_idac: [[u32; SS]; 2],
    rxiqk_agc: [[u16; SS]; 2],
    bypass_iqk: [[u8; SS]; 2],
    rxiqk_fail_code: [[u8; SS]; 2],
    lok_fail: [bool; SS],
    iqk_fail_report: [[[bool; 2]; SS]; 2],
    iqk_cfir_real: [[[[u16; 17]; 2]; SS]; 2],
    iqk_cfir_imag: [[[[u16; 17]; 2]; SS]; 2],
    gs_retry_count: [[[u8; 2]; SS]; 2],
    retry_count: [[[u8; 3]; SS]; 2],
    nbtxk_1b38: [u32; SS],
    nbrxk_1b3c: [u32; SS],
    lna_idx: u8,
    isbnd: bool,
    iqk_band: u8,
    iqk_ch: u8,
    iqk_bw: u8,
}

impl Default for IqkInfo {
    fn default() -> Self {
        Self {
            iqk_step: 0,
            rxiqk_step: 0,
            kcount: 0,
            iqk_times: 0,
            rf_reg18: 0,
            tmp_gntwl: 0,
            tmp1bcc: 0,
            is_nb_iqk: false,
            is_tssi_mode: false,
            is_5g: true,
            bw_val: 0,
            rf_reg58: 0,
            iqk_channel: [0; 2],
            lok_idac: [[0; SS]; 2],
            rxiqk_agc: [[0; SS]; 2],
            bypass_iqk: [[0; SS]; 2],
            rxiqk_fail_code: [[0; SS]; 2],
            lok_fail: [true; SS],
            iqk_fail_report: [[[false; 2]; SS]; 2],
            iqk_cfir_real: [[[[0; 17]; 2]; SS]; 2],
            iqk_cfir_imag: [[[[0; 17]; 2]; SS]; 2],
            gs_retry_count: [[[0; 2]; SS]; 2],
            retry_count: [[[0; 3]; SS]; 2],
            nbtxk_1b38: [0; SS],
            nbrxk_1b3c: [0; SS],
            lna_idx: 0,
            isbnd: false,
            iqk_band: 0,
            iqk_ch: 0,
            iqk_bw: 0,
        }
    }
}

fn pidx(path: u8) -> usize {
    path as usize
}

fn mask_shift(mask: u32) -> u32 {
    mask.trailing_zeros()
}

fn dack_compare(value: u32) -> bool {
    if value >= 0x200 && (0x400 - value) > 0x64 {
        return true;
    }
    value < 0x200 && value > 0x64
}

fn dack_minmax(value: u32, min: &mut u32, max: &mut u32) {
    if value >= 0x200 {
        if *min >= 0x200 {
            if *min > value {
                *min = value;
            }
        } else {
            *min = value;
        }
        if *max >= 0x200 && *max < value {
            *max = value;
        }
    } else {
        if *min < 0x200 && *min > value {
            *min = value;
        }
        if *max >= 0x200 || *max < value {
            *max = value;
        }
    }
}

fn dack_bsort(values: &mut [u32; DACK_SN]) {
    for i in 0..DACK_SN - 1 {
        for j in 0..DACK_SN - 1 - i {
            let same_sign = (values[j] >= 0x200 && values[j + 1] >= 0x200)
                || (values[j] < 0x200 && values[j + 1] < 0x200);
            let swap = if same_sign {
                values[j] > values[j + 1]
            } else {
                values[j] < 0x200 && values[j + 1] >= 0x200
            };
            if swap {
                values.swap(j, j + 1);
            }
        }
    }
}

fn dack_average(values: &[u32; DACK_SN]) -> u32 {
    let mut minus = 0;
    let mut plus = 0;
    for value in &values[10..DACK_SN - 10] {
        if *value > 0x200 {
            minus += 0x400 - *value;
        } else {
            plus += *value;
        }
    }
    if plus > minus {
        (plus - minus) / (DACK_SN as u32 - 20)
    } else {
        let value = (minus - plus) / (DACK_SN as u32 - 20);
        if value == 0 {
            0
        } else {
            0x400 - value
        }
    }
}

fn dack_adjust(value: u32) -> u32 {
    if value < 0x300 {
        value * 2 * 6 / 5 + 0x80
    } else {
        0x7f - ((0x400 - value) * 2 * 6 / 5)
    }
}

fn dack_sub_0x10(value: u32) -> u32 {
    if value >= 0x10 {
        value - 0x10
    } else {
        0x400 - (0x10 - value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_iqk_nctl_table_matches_devourer_shape() {
        assert_eq!(rtl_data::RTL8822C_IQK_NCTL.len(), 1801);
        assert_eq!(
            rtl_data::RTL8822C_IQK_NCTL[0],
            (0x1cd0, 0xf000_0000, 0x0000_0007)
        );
        assert_eq!(
            rtl_data::RTL8822C_IQK_NCTL[rtl_data::RTL8822C_IQK_NCTL.len() - 1],
            (0x1b80, 0xffff_ffff, 0x0000_0002)
        );
    }
}
