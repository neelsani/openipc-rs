//! RTL8812EU/RTL8822EU (rtl8822e) Jaguar3 calibration routines.
//!
//! These sequences are a Rust port of devourer's `Halrf8822e` path. The EU
//! silicon shares Jaguar3 MAC, firmware-download, descriptor, and RF-window
//! plumbing with RTL8822C, but its DACK, IQK, TXGAPK, and thermal tracking are
//! different and must not use the CU calibration implementation.

use crate::async_iqk::IqkReport;
use crate::async_jaguar3_iqk::{Jaguar3PowerTrackingReport, Jaguar3PowerTrackingState};
use crate::device::RealtekDevice;
use crate::time::{sleep_micros, sleep_ms};
use crate::types::{ChannelWidth, ChipInfo, DriverError};

const RF_REGISTER_MASK: u32 = 0x000f_ffff;
const RF_WINDOW: [u16; 2] = [0x3c00, 0x4c00];
const PATHS: usize = 2;
const IQK_TX: usize = 0;
const IQK_RX: usize = 1;
const IQK_RX1: u8 = 1;
const IQK_RX2: u8 = 2;
const IQK_NB_TX: u8 = 3;
const IQK_NB_RX: u8 = 4;
const IQK_LOK1: u8 = 5;
const IQK_LOK2: u8 = 6;
const IQK_BAND_2G: u8 = 0;
const TXGAPK_GAIN_COUNT: usize = 12;

const IQK_MAC_BACKUP: [u16; 4] = [0x0520, 0x001c, 0x00ec, 0x0070];
const IQK_BB_BACKUP: [u16; 21] = [
    0x0820, 0x0824, 0x1c38, 0x1c68, 0x1d60, 0x180c, 0x410c, 0x1c3c, 0x1a14, 0x1d58, 0x1d70, 0x1864,
    0x4164, 0x186c, 0x416c, 0x1a14, 0x1e70, 0x080c, 0x1e7c, 0x18a4, 0x41a4,
];
const IQK_RF_BACKUP: [u16; 3] = [0x19, 0x9e, 0x00];

impl RealtekDevice {
    pub(crate) async fn dack_soft_reset_8822e_async(&self) -> Result<(), DriverError> {
        Jaguar3EuCal::new(self).dack_soft_reset().await
    }

    pub(crate) async fn dac_calibrate_8822e_async(&self) -> Result<(), DriverError> {
        Jaguar3EuCal::new(self).dac_calibrate().await
    }

    pub(crate) async fn run_iqk_8822e_async(
        &self,
        chip: ChipInfo,
        width: ChannelWidth,
        channel: u8,
        skip_txgapk: bool,
    ) -> Result<IqkReport, DriverError> {
        let mut calibration = Jaguar3EuCal::new(self);
        calibration
            .phy_iq_calibrate(width, channel, skip_txgapk)
            .await?;
        self.write_u16_async(0x0522, 0).await?;
        Ok(IqkReport {
            chip,
            channel,
            ran: true,
        })
    }

    pub(crate) async fn tick_power_tracking_8822e_inner_async(
        &self,
        chip: ChipInfo,
        state: &mut Jaguar3PowerTrackingState,
    ) -> Result<Jaguar3PowerTrackingReport, DriverError> {
        if state.chip_family != Some(chip.family) {
            let efuse = self.read_efuse_info_async(chip).await?;
            let channel = self.query_bb_reg_async(0x3c60, 0xff).await? as u8;
            *state = Jaguar3PowerTrackingState {
                chip_family: Some(chip.family),
                thermal_ref: efuse.thermal_meter_paths.map(|value| {
                    if value == 0xff {
                        -1
                    } else {
                        i16::from(value)
                    }
                }),
                channel,
                ..Jaguar3PowerTrackingState::default()
            };
        }

        let mut report = Jaguar3PowerTrackingReport {
            thermal_raw: [0; 2],
            thermal_ref: state.thermal_ref,
            compensation_index: [0; 2],
            lck_ran: false,
        };
        if state.channel <= 14 || state.thermal_ref.iter().all(|value| *value < 0) {
            return Ok(report);
        }
        let calibration = Jaguar3EuCal::new(self);
        if !state.thermal_meter_triggered {
            for path in 0..PATHS {
                calibration.rf_write(path, 0x42, 1 << 19, 1).await?;
                calibration.rf_write(path, 0x42, 1 << 19, 0).await?;
                calibration.rf_write(path, 0x42, 1 << 19, 1).await?;
            }
            state.thermal_meter_triggered = true;
            sleep_micros(300).await;
        }
        let band = if state.channel <= 64 {
            0
        } else if state.channel <= 144 {
            1
        } else {
            2
        };
        for path in 0..PATHS {
            let baseline = state.thermal_ref[path];
            if baseline < 0 {
                continue;
            }
            let current = calibration.rf_read(path, 0x42, 0x7e).await? as u8;
            report.thermal_raw[path] = current;
            if current == 0 {
                continue;
            }
            let slot = usize::from(state.thermal_average_index[path]);
            state.thermal_average[path][slot] = current;
            state.thermal_average_index[path] = (state.thermal_average_index[path] + 1) & 3;
            state.thermal_average_count[path] =
                state.thermal_average_count[path].saturating_add(1).min(4);
            let count = usize::from(state.thermal_average_count[path]);
            let average = state.thermal_average[path][..count]
                .iter()
                .map(|value| u32::from(*value))
                .sum::<u32>()
                / count as u32;
            let average = average as i16;
            let delta = (average - baseline).unsigned_abs().min(29) as usize;
            let swing = if average > baseline {
                i16::from(EU_THERMAL_POSITIVE[path][band][delta])
            } else {
                -i16::from(EU_THERMAL_NEGATIVE[path][band][delta])
            };
            report.compensation_index[path] = swing as i8;
            calibration
                .bb_set(
                    if path == 0 { 0x18a0 } else { 0x41a0 },
                    0xff,
                    u32::from(swing as i8 as u8),
                )
                .await?;
        }
        Ok(report)
    }
}

const EU_THERMAL_POSITIVE: [[[u8; 30]; 3]; 2] = [
    [
        [
            0, 1, 1, 2, 2, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
            13, 13, 13,
        ],
        [
            0, 1, 2, 2, 3, 4, 4, 5, 6, 7, 7, 8, 9, 9, 10, 11, 12, 12, 13, 14, 14, 15, 16, 17, 17,
            18, 19, 19, 20, 21,
        ],
        [
            0, 1, 2, 3, 3, 4, 5, 6, 6, 7, 8, 9, 9, 10, 11, 12, 12, 13, 14, 15, 15, 16, 17, 18, 18,
            19, 20, 21, 22, 22,
        ],
    ],
    [
        [
            0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6, 7, 7, 8, 8, 9, 9, 10, 11, 11, 12, 12, 13, 14, 14, 15,
            15, 16, 16, 17,
        ],
        [
            0, 1, 2, 3, 3, 4, 5, 6, 7, 8, 8, 9, 10, 11, 12, 12, 13, 14, 15, 16, 16, 17, 18, 19, 20,
            21, 21, 22, 23, 24,
        ],
        [
            0, 1, 2, 2, 3, 4, 4, 5, 6, 6, 7, 8, 8, 9, 10, 11, 11, 12, 13, 13, 14, 15, 15, 16, 17,
            17, 18, 19, 19, 20,
        ],
    ],
];

const EU_THERMAL_NEGATIVE: [[[u8; 30]; 3]; 2] = [
    [
        [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1,
        ],
        [
            0, 1, 1, 2, 2, 2, 3, 3, 4, 4, 5, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 9, 10, 10, 11, 11, 12,
            12, 12, 13,
        ],
        [
            0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6, 7, 7, 8, 8, 8, 9, 9, 9, 10, 10,
            10, 11,
        ],
    ],
    [
        [0; 30],
        [
            0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
            13, 14, 14,
        ],
        [
            0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 7, 7, 7, 8, 8, 8, 9, 9, 10, 10, 10,
            11, 11,
        ],
    ],
];

struct Jaguar3EuCal<'a> {
    device: &'a RealtekDevice,
    iqk: EuIqkInfo,
    dack: EuDackInfo,
    txgapk: EuTxgapkInfo,
    mac_backup: [u32; 4],
    bb_backup: [u32; 21],
    rf_backup: [[u32; 2]; 3],
}

impl<'a> Jaguar3EuCal<'a> {
    fn new(device: &'a RealtekDevice) -> Self {
        Self {
            device,
            iqk: EuIqkInfo::default(),
            dack: EuDackInfo::default(),
            txgapk: EuTxgapkInfo::default(),
            mac_backup: [0; 4],
            bb_backup: [0; 21],
            rf_backup: [[0; 2]; 3],
        }
    }

    async fn bb_read(&self, register: u16) -> Result<u32, DriverError> {
        self.device.read_u32_async(register).await
    }

    async fn bb_write(&self, register: u16, value: u32) -> Result<(), DriverError> {
        self.device.write_u32_async(register, value).await
    }

    async fn bb_get(&self, register: u16, mask: u32) -> Result<u32, DriverError> {
        self.device.query_bb_reg_async(register, mask).await
    }

    async fn bb_set(&self, register: u16, mask: u32, value: u32) -> Result<(), DriverError> {
        self.device.set_bb_reg_async(register, mask, value).await
    }

    async fn mac_write8(&self, register: u16, value: u8) -> Result<(), DriverError> {
        self.device.write_u8_async(register, value).await
    }

    async fn rf_read(&self, path: usize, register: u16, mask: u32) -> Result<u32, DriverError> {
        let direct = RF_WINDOW[path & 1] + ((register & 0xff) << 2);
        self.bb_get(direct, mask & RF_REGISTER_MASK).await
    }

    async fn rf_write(
        &self,
        path: usize,
        register: u16,
        mask: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        let direct = RF_WINDOW[path & 1] + ((register & 0xff) << 2);
        self.bb_set(direct, mask & RF_REGISTER_MASK, value).await
    }

    async fn wdack(&self, register: u16, mask: u32, value: u32) -> Result<(), DriverError> {
        self.bb_set(register, mask, value).await?;
        self.bb_set(register, mask, value).await
    }

    async fn write_check_afe(&self, register: u16, value: u32) -> Result<(), DriverError> {
        let fallback = if matches!(register, 0x3800 | 0x3900) {
            value
        } else {
            0xee32_001f
        };
        let check_register = if register >> 8 == 0x38 {
            0x3800
        } else {
            0x3900
        };
        for _ in 0..100 {
            self.bb_write(0x2dd4, 0).await?;
            self.bb_write(register, value).await?;
            self.bb_write(register, value).await?;
            self.bb_write(0x2dd4, 0).await?;
            if self.bb_read(check_register).await? != 0 {
                return Ok(());
            }
            self.bb_write(check_register, fallback).await?;
            self.bb_write(check_register, fallback).await?;
        }
        Ok(())
    }

    async fn dack_soft_reset(&self) -> Result<(), DriverError> {
        for register in [0x3800, 0x382c, 0x3900, 0x392c] {
            self.write_check_afe(register, 0xee30_001f).await?;
            self.write_check_afe(register, 0xee32_001f).await?;
        }
        Ok(())
    }

    async fn dack_reset(&self) -> Result<(), DriverError> {
        for register in [0x1818, 0x4118] {
            self.bb_set(register, 1 << 25, 0).await?;
            self.bb_set(register, 1 << 25, 1).await?;
            sleep_ms(1).await;
        }
        Ok(())
    }

    async fn dac_fifo_reset(&self) -> Result<(), DriverError> {
        // The two AFE banks are reset independently in the vendor sequence.
        for registers in [[0x3800, 0x382c], [0x3900, 0x392c]] {
            for register in registers {
                self.bb_set(register, 1 << 21, 0).await?;
            }
            for register in registers {
                self.bb_set(register, 1 << 21, 1).await?;
            }
        }
        Ok(())
    }

    async fn check_addc(&mut self, path: usize) -> Result<(), DriverError> {
        let offset = if path == 0 { 0 } else { 0x100 };
        self.wdack(0x381c + offset, 0x0006_0000, 3).await?;
        self.wdack(0x381c + offset, 1 << 16, 0).await?;
        self.wdack(0x381c + offset, 1 << 16, 1).await?;
        self.wdack(0x381c + offset, 1 << 16, 1).await?;
        let mut count = 0;
        while self.bb_get(0x3878 + offset, 1 << 12).await? == 0
            || self.bb_get(0x38a8 + offset, 1 << 12).await? == 0
        {
            count += 1;
            if count > 10_000 {
                self.dack.addck_timeout[path] = true;
                break;
            }
            sleep_micros(1).await;
        }
        self.dack.addc[path][0] = self.bb_get(0x3878 + offset, 0xfff).await? as u16;
        self.dack.addc[path][1] = self.bb_get(0x38a8 + offset, 0xfff).await? as u16;
        Ok(())
    }

    async fn addck(&mut self, path: usize) -> Result<(), DriverError> {
        let (base, register60, register10, output) = if path == 0 {
            (0x1830, 0x1860, 0x1810, 0x1868)
        } else {
            (0x4130, 0x4160, 0x4110, 0x4168)
        };
        self.bb_set(base, 1 << 30, 0).await?;
        self.bb_set(register60, 0xf000_0000, 0xf).await?;
        self.bb_set(register60, 1 << 26, 0).await?;
        self.bb_set(register60, 1 << 12, 0).await?;
        self.bb_set(register10, 1 << 19, 1).await?;
        self.check_addc(path).await?;
        let i = 0x800u32.wrapping_sub(u32::from(self.dack.addc[path][0]));
        let q = 0x800u32.wrapping_sub(u32::from(self.dack.addc[path][1]));
        self.dack.addck_d[path] = [i as u16, q as u16];
        self.bb_write(output, (i & 0x3ff) | ((q & 0x3ff) << 10))
            .await?;
        self.bb_set(register10, 1 << 19, 0).await?;
        self.bb_set(register60, 1 << 12, 1).await?;
        self.bb_set(base, 1 << 30, 1).await
    }

    async fn dack_backup(&mut self, path: usize) -> Result<(), DriverError> {
        let (i_select, q_select, i_data, q_data, bias, dadck_i, dadck_q) = if path == 0 {
            (0x3800, 0x382c, 0x3870, 0x38a0, 0x3878, 0x3874, 0x38a4)
        } else {
            (0x3900, 0x392c, 0x3970, 0x39a0, 0x3978, 0x3974, 0x39a4)
        };
        for index in 0..16 {
            self.wdack(i_select, 0x1e, index as u32).await?;
            self.dack.new_msbk_d[path][0][index] = self.bb_get(i_data, 0xff00_0000).await? as u8;
            self.wdack(q_select, 0x1e, index as u32).await?;
            self.dack.new_msbk_d[path][1][index] = self.bb_get(q_data, 0xff00_0000).await? as u8;
        }
        self.dack.new_biask_d[path] = self.bb_get(bias, 0xffc0_0000).await? as u16;
        self.dack.dadck_d[path][0] = self.bb_get(dadck_i, 0xff00_0000).await? as u8;
        self.dack.dadck_d[path][1] = self.bb_get(dadck_q, 0xff00_0000).await? as u8;
        Ok(())
    }

    async fn dack_reload_index(&self, path: usize, index: usize) -> Result<(), DriverError> {
        let offset = (if index == 0 { 0 } else { 0x14 }) + if path == 0 { 0 } else { 0x100 };
        for (register, base) in [(0x38c0, 12), (0x38c4, 8), (0x38c8, 4), (0x38cc, 0)] {
            let mut packed = 0u32;
            for byte in 0..4 {
                packed |= u32::from(self.dack.new_msbk_d[path][index][base + byte]) << (byte * 8);
            }
            self.wdack(register + offset, u32::MAX, packed).await?;
        }
        let tail = (u32::from(self.dack.new_biask_d[path]) << 16)
            | (u32::from(self.dack.dadck_d[path][index]) << 8);
        self.wdack(0x38d0 + offset, u32::MAX, tail).await
    }

    async fn dack_reload(&self, path: usize) -> Result<(), DriverError> {
        self.dack_reload_index(path, 0).await?;
        self.dack_reload_index(path, 1).await
    }

    async fn dack_path(&mut self, path: usize) -> Result<(), DriverError> {
        let (base30, register60, register10, register18, auto, dc, done_a, done_b) = if path == 0 {
            (
                0x1830, 0x1860, 0x1810, 0x1818, 0x3804, 0x380c, 0x385c, 0x388c,
            )
        } else {
            (
                0x4130, 0x4160, 0x4110, 0x4118, 0x3904, 0x390c, 0x395c, 0x398c,
            )
        };
        let (bias_q, dadck_a, dadck_b) = if path == 0 {
            (0x3830, 0x3870, 0x38a0)
        } else {
            (0x3930, 0x3970, 0x39a0)
        };
        let clock = self.bb_read(0x09b4).await?;
        self.bb_set(0x09b4, 0x0001_ff00, 0xdb).await?;
        self.bb_set(base30, 1 << 30, 0).await?;
        self.bb_set(register60, 1 << 30, 1).await?;
        self.bb_set(register60, 1 << 27, 0).await?;
        self.bb_set(register10, 1 << 15, 1).await?;
        self.bb_set(register18, 0x0c00_0000, 3).await?;
        self.wdack(auto, 0x3ff0_0000, 0x58).await?;
        self.wdack(bias_q, 0x3ff0_0000, 0x58).await?;
        self.dac_fifo_reset().await?;
        self.wdack(dc, 1 << 1, 0).await?;
        self.wdack(auto, 1, 1).await?;
        sleep_micros(1).await;
        let mut count = 0;
        while self.bb_get(done_a, 1 << 1).await? == 0 || self.bb_get(done_b, 1 << 1).await? == 0 {
            count += 1;
            if count > 10_000 {
                self.dack.msbk_timeout[path] = true;
                break;
            }
            sleep_micros(1).await;
        }
        self.bb_set(register18, 0x0c00_0000, 0).await?;
        self.wdack(dc, 1 << 1, 1).await?;
        sleep_micros(1).await;
        count = 0;
        while self.bb_get(dadck_a, 1 << 2).await? == 0 || self.bb_get(dadck_b, 1 << 2).await? == 0 {
            count += 1;
            if count > 10_000 {
                self.dack.dadck_timeout[path] = true;
                break;
            }
            sleep_micros(1).await;
        }
        self.wdack(auto, 1, 0).await?;
        self.bb_set(register10, 1 << 15, 0).await?;
        self.dack_backup(path).await?;
        self.dack_reload(path).await?;
        self.bb_write(0x09b4, clock).await?;
        self.dac_fifo_reset().await?;
        self.bb_set(base30, 1 << 30, 1).await
    }

    async fn dack_use_register_values(&self, enabled: bool) -> Result<(), DriverError> {
        for register in [0x38d0, 0x38e4, 0x39d0, 0x39e4] {
            self.wdack(register, 1, u32::from(enabled)).await?;
        }
        Ok(())
    }

    async fn dack_failed(&self) -> Result<bool, DriverError> {
        Ok(self.bb_get(0x3800, 1 << 17).await? == 0 || self.bb_get(0x3900, 1 << 17).await? == 0)
    }

    async fn dac_calibrate(&mut self) -> Result<(), DriverError> {
        self.addck(0).await?;
        self.addck(1).await?;
        self.dack_reset().await?;
        self.dack_use_register_values(false).await?;
        self.dack_path(0).await?;
        self.dack_path(1).await?;
        for _ in 0..10 {
            if !self.dack_failed().await? {
                break;
            }
            self.dack_reset().await?;
            self.dack_use_register_values(false).await?;
            self.dack_path(0).await?;
            self.dack_path(1).await?;
        }
        self.dack_use_register_values(true).await
    }
}

impl Jaguar3EuCal<'_> {
    async fn iqk_check_cal(&self, command: u8) -> Result<bool, DriverError> {
        let mut failed = true;
        sleep_micros(1).await;
        for _ in 0..3000 {
            if self.bb_get(0x2d9c, 0xff).await? == 0x55 {
                failed = if matches!(command, IQK_LOK1 | IQK_LOK2) {
                    false
                } else {
                    self.bb_get(0x1b08, 1 << 26).await? != 0
                };
                break;
            }
            sleep_micros(1).await;
        }
        sleep_micros(1).await;
        let mut handshake_ready = false;
        for _ in 0..500 {
            if self.bb_get(0x1bfc, 0xffff).await? == 0x8000 {
                handshake_ready = true;
                break;
            }
            sleep_micros(1).await;
        }
        if !handshake_ready {
            failed = true;
        }
        sleep_micros(50).await;
        self.bb_set(0x1b10, 0xff, 0).await?;
        self.bb_set(0x1b08, 1 << 26, 0).await?;
        Ok(failed)
    }

    async fn btc_wait_ready(&self) -> Result<(), DriverError> {
        for _ in 0..10 {
            if self.bb_get(0x1700, 0xff00_0000).await? & (1 << 5) != 0 {
                break;
            }
            sleep_micros(100).await;
        }
        Ok(())
    }

    async fn btc_read(&self, register: u16) -> Result<u32, DriverError> {
        self.btc_wait_ready().await?;
        self.bb_write(0x1700, 0x800f_0000 | u32::from(register))
            .await?;
        self.bb_read(0x1708).await
    }

    async fn btc_write(&self, register: u16, mask: u32, value: u32) -> Result<(), DriverError> {
        if mask == 0 {
            return Ok(());
        }
        let value = if mask == u32::MAX {
            value
        } else {
            let current = self.btc_read(register).await?;
            (current & !mask) | (value << mask.trailing_zeros())
        };
        self.btc_wait_ready().await?;
        self.bb_write(0x1704, value).await?;
        self.bb_write(0x1700, 0xc00f_0000 | u32::from(register))
            .await
    }

    async fn set_gnt_wl(&self, before_calibration: bool) -> Result<(), DriverError> {
        if before_calibration {
            self.btc_write(0x38, 0xff00, 0x77).await
        } else {
            self.btc_write(0x38, u32::MAX, self.iqk.tmp_gntwl).await
        }
    }

    async fn backup_tx_cfir(&mut self, path: usize) -> Result<(), DriverError> {
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.bb_set(0x1b20, 0xc000_0000, 3).await?;
        self.bb_write(0x1bd8, 0x0000_0071).await?;
        self.bb_set(0x1bd4, 0x0020_0000, 1).await?;
        self.bb_set(0x1bd4, 0x001f_0000, 0x10).await?;
        for tap in 0..17 {
            self.bb_set(0x1bd8, 0x01f0_0000, tap as u32).await?;
            let value = self.bb_read(0x1bfc).await?;
            self.iqk.iqk_cfir_real[0][path][IQK_TX][tap] = ((value & 0x0fff_0000) >> 16) as u16;
            self.iqk.iqk_cfir_imag[0][path][IQK_TX][tap] = (value & 0x0fff) as u16;
        }
        self.bb_set(0x1b20, 0xc000_0000, 0).await?;
        self.bb_write(0x1bd8, 0x0000_0070).await
    }

    async fn backup_rx_cfir(&mut self, path: usize) -> Result<(), DriverError> {
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.bb_set(0x1b20, 0xc000_0000, 1).await?;
        self.bb_write(0x1bd8, 0x0000_0071).await?;
        self.bb_set(0x1bd4, 0x0020_0000, 1).await?;
        self.bb_set(0x1bd4, 0x001f_0000, 0x10).await?;
        for tap in 0..17 {
            self.bb_set(0x1bd8, 0x01f0_0000, tap as u32).await?;
            let value = self.bb_read(0x1bfc).await?;
            self.iqk.iqk_cfir_real[0][path][IQK_RX][tap] = ((value & 0x0fff_0000) >> 16) as u16;
            self.iqk.iqk_cfir_imag[0][path][IQK_RX][tap] = (value & 0x0fff) as u16;
        }
        self.bb_set(0x1b20, 0xc000_0000, 0).await?;
        self.bb_write(0x1bd8, 0x0000_0070).await
    }

    async fn backup_cfir(&mut self, path: usize, kind: u8) -> Result<(), DriverError> {
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        if matches!(kind, 0 | IQK_NB_TX) {
            self.iqk.txxy[0][path] = self.bb_read(0x1b38).await?;
            self.backup_tx_cfir(path).await?;
        } else if matches!(kind, IQK_RX1 | IQK_RX2 | IQK_NB_RX) {
            self.iqk.rxxy[0][path] = self.bb_read(0x1b3c).await?;
            self.backup_rx_cfir(path).await?;
        }
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.iqk.cfir_en[0][path] = self.bb_read(0x1b70).await?;
        self.iqk.iqk_tab[0] = self.iqk.rf_reg18;
        Ok(())
    }

    async fn iqk_afe_setting(&self) -> Result<(), DriverError> {
        self.bb_write(0x1c38, 0).await?;
        self.bb_set(0x1830, 0x4000_0000, 0).await?;
        self.bb_set(0x1860, 0xffff_f000, 0xf0001).await?;
        self.bb_set(0x4130, 0x4000_0000, 0).await?;
        self.bb_set(0x4160, 0xffff_f000, 0xf0001).await?;
        const SEQUENCE: [u32; 18] = [
            0x700f0001, 0x700f0001, 0x701f0001, 0x702f0001, 0x703f0001, 0x704f0001, 0x705f0001,
            0x706f0001, 0x707f0001, 0x708f0001, 0x709f0001, 0x70af0001, 0x70bf0001, 0x70cf0001,
            0x70df0001, 0x70ef0001, 0x70ff0001, 0x70ff0001,
        ];
        for value in SEQUENCE {
            self.bb_write(0x1830, value).await?;
        }
        for value in SEQUENCE {
            self.bb_write(0x4130, value).await?;
        }
        self.bb_write(0x1c38, u32::MAX).await?;
        self.dack_soft_reset().await
    }

    async fn iqk_afe_restore(&self) -> Result<(), DriverError> {
        self.bb_write(0x1c38, 0).await?;
        self.bb_set(0x1830, 0x4000_0000, 1).await?;
        self.bb_set(0x4130, 0x4000_0000, 1).await?;
        const SEQUENCE_A: [u32; 19] = [
            0x700f0001, 0x700f0001, 0x700f0001, 0x70144001, 0x70244001, 0x70344001, 0x70444001,
            0x705b0001, 0x70644001, 0x707b0001, 0x708f0001, 0x709f0001, 0x70af0001, 0x70bf0001,
            0x70cb0001, 0x70db0001, 0x70eb0001, 0x70fb0001, 0x70fb0001,
        ];
        const SEQUENCE_B: [u32; 18] = [
            0x700f0001, 0x700f0001, 0x70144001, 0x70244001, 0x70344001, 0x70444001, 0x705b0001,
            0x70644001, 0x707b0001, 0x708f0001, 0x709f0001, 0x70af0001, 0x70bf0001, 0x70cb0001,
            0x70db0001, 0x70eb0001, 0x70fb0001, 0x70fb0001,
        ];
        for value in SEQUENCE_A {
            self.bb_write(0x1830, value).await?;
        }
        for value in SEQUENCE_B {
            self.bb_write(0x4130, value).await?;
        }
        self.bb_write(0x1c38, 0xffa1_005e).await?;
        self.dack_soft_reset().await
    }

    async fn backup_iqk_registers(&mut self) -> Result<(), DriverError> {
        for (index, register) in IQK_MAC_BACKUP.into_iter().enumerate() {
            self.mac_backup[index] = self.device.read_u32_async(register).await?;
        }
        for (index, register) in IQK_BB_BACKUP.into_iter().enumerate() {
            self.bb_backup[index] = self.bb_read(register).await?;
        }
        for (index, register) in IQK_RF_BACKUP.into_iter().enumerate() {
            self.rf_backup[index][0] = self.rf_read(0, register, RF_REGISTER_MASK).await?;
            self.rf_backup[index][1] = self.rf_read(1, register, RF_REGISTER_MASK).await?;
        }
        Ok(())
    }

    async fn restore_iqk_registers(&self) -> Result<(), DriverError> {
        self.rf_write(0, 0xef, RF_REGISTER_MASK, 0).await?;
        self.rf_write(1, 0xef, RF_REGISTER_MASK, 0).await?;
        self.rf_write(0, 0xdf, 0x10, 0).await?;
        self.rf_write(1, 0xdf, 0x10, 0).await?;
        for (index, register) in IQK_RF_BACKUP.into_iter().enumerate() {
            self.rf_write(0, register, RF_REGISTER_MASK, self.rf_backup[index][0])
                .await?;
            self.rf_write(1, register, RF_REGISTER_MASK, self.rf_backup[index][1])
                .await?;
        }
        self.rf_write(0, 0xde, 1 << 16, 0).await?;
        self.rf_write(1, 0xde, 1 << 16, 0).await?;

        self.bb_write(0x1d70, 0x5050_5050).await?;
        for (index, register) in IQK_MAC_BACKUP.into_iter().enumerate() {
            self.device
                .write_u32_async(register, self.mac_backup[index])
                .await?;
        }
        for (index, register) in IQK_BB_BACKUP.into_iter().enumerate() {
            self.bb_write(register, self.bb_backup[index]).await?;
        }
        self.bb_set(
            0x180c,
            1 << 31,
            u32::from(!self.iqk.iqk_fail_report[0][0][IQK_RX]),
        )
        .await?;
        self.bb_set(
            0x410c,
            1 << 31,
            u32::from(!self.iqk.iqk_fail_report[0][1][IQK_RX]),
        )
        .await
    }

    fn switch_iqk_table(&mut self) {
        self.iqk.iqk_tab[1] = self.iqk.iqk_tab[0];
        self.iqk.lok_idac[1] = self.iqk.lok_idac[0];
        self.iqk.txxy[1] = self.iqk.txxy[0];
        self.iqk.rxxy[1] = self.iqk.rxxy[0];
        self.iqk.cfir_en[1] = self.iqk.cfir_en[0];
        self.iqk.iqk_fail_report[1] = self.iqk.iqk_fail_report[0];
        self.iqk.iqk_cfir_real[1] = self.iqk.iqk_cfir_real[0];
        self.iqk.iqk_cfir_imag[1] = self.iqk.iqk_cfir_imag[0];
    }

    async fn iqk_tx_pause(&self) -> Result<(), DriverError> {
        self.mac_write8(0x0522, 0xff).await?;
        self.bb_set(0x1e70, 0x0f, 2).await?;
        for _ in 0..2500 {
            let path_a = self.rf_read(0, 0, 0xf0000).await? as u8;
            let path_b = self.rf_read(1, 0, 0xf0000).await? as u8;
            if path_a == 3 || path_b == 3 {
                break;
            }
            sleep_micros(2).await;
        }
        Ok(())
    }

    async fn iqk_macbb_setting(&self) -> Result<(), DriverError> {
        self.bb_set(0x0824, 0x0003_0000, 3).await?;
        self.iqk_tx_pause().await?;
        for (register, mask, value) in [
            (0x0070, 0xff00_0000, 0x06),
            (0x1e24, 0x0002_0000, 1),
            (0x1cd0, 0x1000_0000, 1),
            (0x1cd0, 0x2000_0000, 1),
            (0x1cd0, 0x4000_0000, 1),
            (0x1cd0, 0x8000_0000, 0),
            (0x1864, 0x8000_0000, 1),
            (0x4164, 0x8000_0000, 1),
            (0x180c, 0x0800_0000, 1),
            (0x410c, 0x0800_0000, 1),
            (0x186c, 0x80, 1),
            (0x416c, 0x80, 1),
            (0x180c, 0x03, 0),
            (0x410c, 0x03, 0),
            (0x1a00, 0x03, 2),
        ] {
            self.bb_set(register, mask, value).await?;
        }
        self.bb_write(0x1b08, 0x80).await
    }

    async fn iqk_macbb_restore(&self) -> Result<(), DriverError> {
        self.rf_write(0, 0xde, 0x10000, 0).await?;
        self.rf_write(1, 0xde, 0x10000, 0).await?;
        for path in 0..2 {
            self.bb_set(0x1b00, 0x06, path).await?;
            self.bb_set(0x1bcc, 0x3f, 0).await?;
            self.bb_set(0x1b20, 1 << 25, 0).await?;
        }
        self.bb_set(0x1b00, 0x06, 0).await?;
        self.bb_write(0x1b08, 0).await?;
        self.bb_set(0x1d0c, 1 << 16, 1).await?;
        self.bb_set(0x1d0c, 1 << 16, 0).await?;
        self.bb_set(0x1d0c, 1 << 16, 1).await?;
        for (register, mask, value) in [
            (0x1864, 0x8000_0000, 0),
            (0x4164, 0x8000_0000, 0),
            (0x180c, 0x0800_0000, 0),
            (0x410c, 0x0800_0000, 0),
            (0x186c, 0x80, 0),
            (0x416c, 0x80, 0),
            (0x180c, 0x03, 3),
            (0x410c, 0x03, 3),
            (0x1a00, 0x03, 0),
        ] {
            self.bb_set(register, mask, value).await?;
        }
        Ok(())
    }

    async fn lok_failed(&self, path: usize) -> Result<bool, DriverError> {
        let value = self.rf_read(path, 0x58, RF_REGISTER_MASK).await?;
        let idac_i = (value & 0xf8000) >> 15;
        let idac_q = (value & 0x07c00) >> 10;
        Ok(idac_i <= 3 || idac_i >= 0x1c || idac_q <= 3 || idac_q >= 0x1c)
    }

    async fn iqk_one_shot(&self, path: usize, kind: u8) -> Result<bool, DriverError> {
        let command = match kind {
            IQK_NB_TX => {
                self.bb_set(0x1b2c, 0x0000_0fff, 0x0c).await?;
                0x200 | (1 << (4 + path)) | 8
            }
            IQK_NB_RX => {
                self.bb_set(0x1b2c, 0x0fff_0000, 0x18).await?;
                0x300 | (1 << (4 + path)) | 8
            }
            0 => ((u32::from(self.iqk.iqk_bw) + 4) << 8) | (1 << (path + 4)) | 8,
            IQK_RX1 => 0xf00 | (1 << (4 + path)) | 8,
            IQK_RX2 => ((u32::from(self.iqk.iqk_bw) + 0x0a) << 8) | (1 << (path + 4)) | 8,
            IQK_LOK1 => (1 << (4 + path)) | 8,
            IQK_LOK2 => 0x100 | (1 << (4 + path)) | 8,
            _ => 0,
        };
        self.bb_write(0x1b00, command).await?;
        self.bb_write(0x1b00, command + 1).await?;
        self.iqk_check_cal(1).await
    }

    async fn txk(&mut self, path: usize, is_2g: bool) -> Result<bool, DriverError> {
        self.rf_write(path, 0xdf, 0x10, 0).await?;
        self.rf_write(path, 0xde, 0x10000, 1).await?;
        self.rf_write(path, 0x56, 0x00c00, 0).await?;
        self.rf_write(path, 0x56, 0x003e0, if is_2g { 0x0f } else { 0x07 })
            .await?;
        self.rf_write(path, 0x56, 0x0001f, if is_2g { 0x05 } else { 0x0c })
            .await?;
        self.rf_write(path, 0x57, 0x08000, 1).await?;
        if is_2g {
            self.rf_write(path, 0x53, 0x000e0, 0).await?;
        } else {
            self.rf_write(path, 0x64, 0x07000, 0).await?;
        }
        self.rf_write(path, 0xef, 0x10, 1).await?;
        self.rf_write(path, 0x33, 0x7f, if is_2g { 0 } else { 0x20 })
            .await?;

        let mac_register = if path == 0 { 0x001c } else { 0x00ec };
        let mac_value = self.device.read_u32_async(mac_register).await?;
        self.device
            .write_u32_async(mac_register, mac_value & !0xc000_0000)
            .await?;

        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.bb_set(0x1b10, 0xff, 0).await?;
        self.bb_set(0x1bcc, 0x3f, 0).await?;
        self.bb_set(0x1bcc, 0x0fc0, 0x09).await?;
        self.bb_set(0x1b2c, 0x0fff, 0x038).await?;
        if self.iqk_one_shot(path, IQK_LOK1).await? {
            self.iqk.fail_step |= 1 << 0;
        }

        self.bb_set(0x1b10, 0xff, 0).await?;
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.bb_set(0x1bcc, 0x3f, 0).await?;
        self.bb_set(0x1bcc, 0x0fc0, 0x09).await?;
        self.bb_set(0x1b2c, 0x0fff, 0x038).await?;
        let _ = self.iqk_one_shot(path, IQK_LOK2).await?;

        self.device.write_u32_async(mac_register, mac_value).await?;
        self.rf_write(path, 0xef, 0x10, 0).await?;
        self.iqk.lok_fail[path] = self.lok_failed(path).await?;

        if is_2g {
            self.rf_write(path, 0x56, 0x003e0, 0x07).await?;
            self.rf_write(path, 0x56, 0x0001f, 0x0c).await?;
            self.rf_write(path, 0x53, 0x000e0, 0x01).await?;
        } else {
            self.rf_write(path, 0x56, 0x0001f, 0x13).await?;
            self.rf_write(path, 0x64, 0x07000, 0x01).await?;
        }
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.bb_set(0x1bcc, 0x3f, 0).await?;
        self.bb_set(0x1bcc, 0x0fc0, 0x12).await?;
        let failed = self
            .iqk_one_shot(path, if self.iqk.is_nbiqk { IQK_NB_TX } else { 0 })
            .await?;
        self.iqk.iqk_fail_report[0][path][IQK_TX] = failed;
        if failed {
            self.iqk.fail_step |= 1 << 1;
            self.bb_set(0x1b00, 0x06, path as u32).await?;
            self.bb_write(0x1b38, 0x4000_0000).await?;
            self.bb_set(0x1b70, 1 << 8, 0).await?;
        }
        Ok(failed)
    }

    async fn rx_gain_search(&self, path: usize, is_2g: bool) -> Result<bool, DriverError> {
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.bb_set(0x1b18, 0x02, 1).await?;
        self.bb_set(0x1b24, 0x000f_ffff, 0x70408).await?;
        let mut failed = true;
        let mut bounded = true;
        let limit = if is_2g { 5 } else { 4 };
        for _ in 0..limit {
            self.bb_set(0x1b00, 0x06, path as u32).await?;
            self.bb_set(0x1bcc, 0x3f, if is_2g { 0x12 } else { 0x1b })
                .await?;
            self.bb_set(0x1b2c, 0x0fff_0000, 0x18).await?;
            let command = (0x0f << 8) | (1 << (path + 4)) | 8;
            self.bb_write(0x1b00, command).await?;
            self.bb_write(0x1b00, command + 1).await?;
            sleep_micros(20).await;
            failed = self.iqk_check_cal(1).await?;
            self.bb_set(0x1b00, 0x06, path as u32).await?;
            let rf0 = self.rf_read(path, 0, RF_REGISTER_MASK).await?;
            let mut lna = (rf0 & 0x01c00) >> 10;
            let rxbb = (rf0 & 0x003e0) >> 5;
            if rxbb > 9 {
                lna = lna.wrapping_add(1);
                bounded = true;
            } else if rxbb > 1 {
                bounded = false;
            } else {
                lna = lna.wrapping_sub(1);
                bounded = true;
            }
            if lna < 1 {
                lna = 0;
            }
            if lna >= 7 {
                lna = 7;
            }
            if bounded {
                sleep_micros(10).await;
            }
            self.bb_set(0x1b00, 0x06, path as u32).await?;
            self.bb_set(0x1b24, 0x1c00, lna).await?;
            self.bb_set(0x1b24, 0x03e0, rxbb).await?;
            if !bounded {
                break;
            }
        }
        if bounded {
            failed = true;
        }
        Ok(failed)
    }

    async fn rxk(&mut self, path: usize, is_2g: bool) -> Result<(), DriverError> {
        self.rf_write(path, 0x9e, 0x20, 0).await?;
        self.rf_write(path, 0x9e, 0x400, 0).await?;
        self.rf_write(path, 0x56, 0x03e0, if is_2g { 0x04 } else { 0 })
            .await?;
        let gain_failed = self.rx_gain_search(path, is_2g).await?;
        if gain_failed {
            self.iqk.fail_step |= 1 << 2;
            if is_2g {
                self.bb_set(0x1b24, 0x000f_ffff, 0x70108).await?;
            }
        }
        let failed = if !is_2g && gain_failed {
            true
        } else {
            self.bb_set(0x1b00, 0x06, path as u32).await?;
            self.bb_set(0x1bcc, 0x3f, if is_2g { 0x12 } else { 0x1b })
                .await?;
            self.iqk_one_shot(
                path,
                if self.iqk.is_nbiqk {
                    IQK_NB_RX
                } else {
                    IQK_RX2
                },
            )
            .await?
        };
        self.iqk.iqk_fail_report[0][path][IQK_RX] = failed;
        if failed {
            self.bb_write(0x1b3c, 0x4000_0000).await?;
            self.iqk.fail_step |= 1 << 3;
            self.bb_set(0x1b70, 1, 0).await?;
        }
        Ok(())
    }

    async fn iqk_by_path(&mut self) -> Result<(), DriverError> {
        let is_2g = self.iqk.iqk_band == IQK_BAND_2G;
        for path in 0..PATHS {
            self.rf_write(1 - path, 0, 0xf0000, 1).await?;
            self.iqk.is_nbiqk = is_2g;
            if !self.txk(path, is_2g).await? {
                self.rxk(path, is_2g).await?;
            }
        }
        for path in 0..PATHS {
            self.iqk.lok_idac[0][path] = if self.iqk.lok_fail[path] {
                0x84220
            } else {
                self.rf_read(path, 0x58, RF_REGISTER_MASK).await?
            };
            if !self.iqk.iqk_fail_report[0][path][IQK_TX] {
                self.backup_cfir(path, 0).await?;
            } else {
                self.bb_set(0x1b00, 0x06, path as u32).await?;
                self.bb_write(0x1b38, 0x4000_0000).await?;
                self.bb_set(0x1b70, 1 << 8, 0).await?;
                self.bb_write(0x1b3c, 0x4000_0000).await?;
                self.bb_set(0x1b70, 1, 0).await?;
            }
            if !self.iqk.iqk_fail_report[0][path][IQK_RX] {
                self.backup_cfir(path, IQK_RX2).await?;
            } else {
                self.bb_set(0x1b00, 0x06, path as u32).await?;
                self.bb_write(0x1b3c, 0x4000_0000).await?;
                self.bb_set(0x1b70, 1, 0).await?;
            }
        }
        Ok(())
    }

    fn init_iqk_state(&mut self) {
        self.iqk.is_nbiqk = false;
        self.iqk.fail_step = 0;
        self.iqk.iqk_times = 0;
        self.iqk.kcount = 0;
        self.iqk.fail_count = 0;
        for path in 0..PATHS {
            self.iqk.lok_fail[path] = true;
            for channel in 0..2 {
                self.iqk.lok_idac[channel][path] = 0x84220;
                for kind in 0..2 {
                    self.iqk.iqk_fail_report[channel][path][kind] = true;
                }
            }
        }
    }

    async fn iqk_information(&mut self, width: ChannelWidth) -> Result<(), DriverError> {
        self.iqk.rf_reg18 = self.rf_read(0, 0x18, RF_REGISTER_MASK).await?;
        self.iqk.iqk_band = ((self.iqk.rf_reg18 >> 16) & 1) as u8;
        self.iqk.iqk_ch = self.iqk.rf_reg18 as u8;
        self.iqk.iqk_bw = match (self.iqk.rf_reg18 >> 12) & 3 {
            3 => 0,
            2 => 1,
            1 => 2,
            _ => {
                self.iqk.is_nbiqk = true;
                0
            }
        };
        if matches!(width, ChannelWidth::Mhz5 | ChannelWidth::Mhz10) {
            self.iqk.is_nbiqk = true;
        }
        Ok(())
    }

    async fn phy_iq_calibrate(
        &mut self,
        width: ChannelWidth,
        channel: u8,
        skip_txgapk: bool,
    ) -> Result<(), DriverError> {
        self.init_iqk_state();
        self.iqk_information(width).await?;
        self.iqk.fail_step = 0;
        self.iqk.iqk_times = self.iqk.iqk_times.saturating_add(1);
        self.iqk.kcount = self.iqk.kcount.saturating_add(1);
        self.iqk.tmp_gntwl = self.btc_read(0x38).await?;
        self.backup_iqk_registers().await?;
        self.switch_iqk_table();
        for _ in 0..2 {
            self.iqk_macbb_setting().await?;
            self.iqk_afe_setting().await?;
            self.set_gnt_wl(true).await?;
            self.iqk_by_path().await?;
            self.set_gnt_wl(false).await?;
            self.iqk_afe_restore().await?;
            self.iqk_macbb_restore().await?;
            if !self.iqk.iqk_fail_report[0][0][IQK_TX] && !self.iqk.iqk_fail_report[0][1][IQK_TX] {
                break;
            }
        }
        self.restore_iqk_registers().await?;
        if self.iqk.fail_step != 0 {
            self.iqk.fail_count = self.iqk.fail_count.saturating_add(1);
        }
        if skip_txgapk {
            Ok(())
        } else {
            self.do_txgapk(channel).await
        }
    }
}

impl Jaguar3EuCal<'_> {
    async fn txgapk_tx_pause(&self) -> Result<(), DriverError> {
        self.mac_write8(0x0522, 0xff).await?;
        self.bb_set(0x1e70, 0x0f, 2).await?;
        for _ in 0..2500 {
            let a = self.rf_read(0, 0, 0xf0000).await? as u8;
            let b = self.rf_read(1, 0, 0xf0000).await? as u8;
            if a != 2 && b != 2 {
                break;
            }
            sleep_micros(2).await;
        }
        Ok(())
    }

    async fn txgapk_bb_iqk(&self, path: usize) -> Result<(), DriverError> {
        for (register, mask, value) in [
            (0x1e24, 0x0002_0000, 1),
            (0x1cd0, 0x1000_0000, 1),
            (0x1cd0, 0x2000_0000, 1),
            (0x1cd0, 0x4000_0000, 1),
            (0x1cd0, 0x8000_0000, 0),
        ] {
            self.bb_set(register, mask, value).await?;
        }
        if path == 0 {
            for (register, mask, value) in [
                (0x1864, 0x8000_0000, 1),
                (0x180c, 0x0800_0000, 1),
                (0x186c, 0x80, 1),
                (0x180c, 0x03, 0),
            ] {
                self.bb_set(register, mask, value).await?;
            }
        } else {
            for (register, mask, value) in [
                (0x4164, 0x8000_0000, 1),
                (0x410c, 0x0800_0000, 1),
                (0x416c, 0x80, 1),
                (0x410c, 0x03, 0),
            ] {
                self.bb_set(register, mask, value).await?;
            }
        }
        self.bb_set(0x1a00, 0x03, 2).await?;
        self.bb_write(0x1b08, 0x80).await
    }

    async fn txgapk_afe_iqk(&self, path: usize) -> Result<(), DriverError> {
        let register = if path == 0 { 0x1830 } else { 0x4130 };
        self.bb_write(0x1c38, u32::MAX).await?;
        self.bb_write(register, 0x700f_0001).await?;
        for value in 0..=0x0f {
            self.bb_write(register, 0x700f_0001 | (value << 20)).await?;
        }
        self.bb_write(register, 0x70ff_0001).await?;
        self.dack_soft_reset().await
    }

    async fn txgapk_afe_restore(&self, path: usize) -> Result<(), DriverError> {
        let register = if path == 0 { 0x1830 } else { 0x4130 };
        const VALUES: [u32; 16] = [
            0x700b8041, 0x70144041, 0x70244041, 0x70344041, 0x70444041, 0x705b8041, 0x70644041,
            0x707b8041, 0x708b8041, 0x709b8041, 0x70ab8041, 0x70bb8041, 0x70cb8041, 0x70db8041,
            0x70eb8041, 0x70fb8041,
        ];
        self.bb_write(0x1c38, 0xffa1_005e).await?;
        for value in VALUES {
            self.bb_write(register, value).await?;
        }
        self.dack_soft_reset().await
    }

    async fn txgapk_bb_restore(&self, path: usize) -> Result<(), DriverError> {
        self.rf_write(path, 0xde, 0x10000, 0).await?;
        self.bb_set(0x1b00, 0x06, 0).await?;
        self.bb_write(0x1b08, 0).await?;
        self.bb_set(0x1d0c, 1 << 16, 1).await?;
        self.bb_set(0x1d0c, 1 << 16, 0).await?;
        self.bb_set(0x1d0c, 1 << 16, 1).await?;
        let registers = if path == 0 {
            [
                (0x1864, 0x8000_0000, 0),
                (0x180c, 0x0800_0000, 0),
                (0x186c, 0x80, 0),
                (0x180c, 0x03, 3),
            ]
        } else {
            [
                (0x4164, 0x8000_0000, 0),
                (0x410c, 0x0800_0000, 0),
                (0x416c, 0x80, 0),
                (0x410c, 0x03, 3),
            ]
        };
        for (register, mask, value) in registers {
            self.bb_set(register, mask, value).await?;
        }
        self.bb_set(0x1a00, 0x03, 0).await
    }

    async fn txgapk_calculate_offset(
        &mut self,
        path: usize,
        channel: u8,
    ) -> Result<(), DriverError> {
        const PI_REGISTER: [u16; 2] = [0x001c, 0x00ec];
        const CAL_COMMAND: [u32; 2] = [0x0000_0d19, 0x0000_0d29];
        let is_2g = channel <= 14;
        if is_2g {
            self.set_gnt_wl(true).await?;
        }
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.rf_write(path, 0xde, 0x10000, 1).await?;
        self.rf_write(path, 0x00, 0xf0000, 5).await?;
        if is_2g {
            self.rf_write(path, 0x88, 0x70, 1).await?;
            self.rf_write(path, 0x88, 0x0f, 1).await?;
            self.rf_write(path, 0xdf, 0x10000, 1).await?;
            self.rf_write(path, 0x87, 0xc0000, 3).await?;
            self.rf_write(path, 0x00, 0x03e0, 0x0f).await?;
            self.bb_set(0x1b98, 0x7000, 0).await?;
        } else {
            self.rf_write(path, 0x8b, 0x0700, 0).await?;
            self.rf_write(path, 0xdf, 0x20000, 1).await?;
            self.rf_write(path, 0x89, 0x03, 3).await?;
            self.rf_write(path, 0x00, 0x03e0, 0x0f).await?;
            let band = if channel <= 64 {
                2
            } else if channel <= 144 {
                3
            } else {
                4
            };
            self.bb_set(0x1b98, 0x7000, band).await?;
        }

        let pi = self.device.read_u32_async(PI_REGISTER[path]).await?;
        let pi_backup = pi & 0xc000_0000;
        self.device
            .write_u32_async(PI_REGISTER[path], pi & !0xc000_0000)
            .await?;
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.bb_set(0x1bcc, 0x3f, 0x12).await?;
        self.bb_set(0x1b2c, 0x0fff, 0x038).await?;
        self.bb_write(0x1b00, CAL_COMMAND[path]).await?;
        sleep_micros(10_000).await;
        for _ in 0..30 {
            sleep_micros(100).await;
            if self.bb_get(0x2d9c, 0xff).await? == 0x55 {
                break;
            }
        }
        for _ in 0..30 {
            sleep_micros(100).await;
            if self.bb_get(0x1bfc, 0xffff).await? == 0x8000 {
                break;
            }
        }
        self.bb_set(0x1b10, 0xff, 0).await?;
        sleep_micros(100).await;
        let pi = self.device.read_u32_async(PI_REGISTER[path]).await?;
        self.device
            .write_u32_async(PI_REGISTER[path], (pi & !0xc000_0000) | pi_backup)
            .await?;
        if is_2g {
            self.set_gnt_wl(false).await?;
        }
        self.bb_set(0x1b00, 0x06, path as u32).await?;
        self.bb_set(0x1bd4, 0x0020_0000, 1).await?;
        self.bb_set(0x1bd4, 0x001f_0000, 0x12).await?;
        self.bb_set(0x1b9c, 0x0f00, 3).await?;
        let value = self.bb_read(0x1bfc).await?;
        for index in 0..8 {
            self.txgapk.offset[index][path] =
                sign_extend_4bit(((value >> (index * 4)) & 0x0f) as u8);
        }
        self.bb_set(0x1b9c, 0x0f00, 4).await?;
        let value = self.bb_get(0x1bfc, 0xff).await?;
        self.txgapk.offset[8][path] = sign_extend_4bit((value & 0x0f) as u8);
        self.txgapk.offset[9][path] = sign_extend_4bit(((value >> 4) & 0x0f) as u8);
        Ok(())
    }

    async fn txgapk_rf_restore(&self, path: usize) -> Result<(), DriverError> {
        self.rf_write(path, 0x00, 0xf0000, 3).await?;
        self.rf_write(path, 0xde, 0x10000, 0).await?;
        self.rf_write(path, 0xdf, 0x30000, 0).await
    }

    async fn write_gain_bb_table(&self) -> Result<(), DriverError> {
        for band in 0..5 {
            for path in 0..PATHS {
                self.bb_set(0x1b00, 0x06, path as u32).await?;
                let selector = match band {
                    1 => Some(0),
                    2 => Some(2),
                    3 => Some(3),
                    4 => Some(4),
                    _ => None,
                };
                if let Some(selector) = selector {
                    self.bb_set(0x1b98, 0x7000, selector).await?;
                }
                self.bb_set(0x1b9c, 0xff, 0x88).await?;
                for gain in 0..11 {
                    self.bb_set(
                        0x1b98,
                        0x0fff,
                        self.txgapk.rf3f_bp[band][gain][path] & 0x0fff,
                    )
                    .await?;
                    self.bb_set(0x1b98, 0x000f_0000, gain as u32).await?;
                    self.bb_set(0x1b98, 0x8000, 1).await?;
                    self.bb_set(0x1b98, 0x8000, 0).await?;
                }
            }
        }
        Ok(())
    }

    async fn save_all_tx_gains(&mut self) -> Result<(), DriverError> {
        if self.txgapk.read_txgain {
            return self.write_gain_bb_table().await;
        }
        const THREE_WIRE: [u16; 2] = [0x180c, 0x410c];
        const CHANNEL: [u8; 5] = [1, 1, 36, 100, 149];
        const CHANNEL_SETTING: [u32; 5] = [0, 0, 1, 1, 1];
        const BAND: [u32; 5] = [0, 0, 1, 3, 5];
        const CCK: [u32; 5] = [1, 0, 0, 0, 0];
        for band in 0..5 {
            for (path, three_wire) in THREE_WIRE.into_iter().enumerate() {
                let rf18 = self.rf_read(path, 0x18, RF_REGISTER_MASK).await?;
                self.bb_set(three_wire, 0x03, 0).await?;
                self.rf_write(path, 0x18, 0xff, u32::from(CHANNEL[band]))
                    .await?;
                self.rf_write(path, 0x18, 0x70000, BAND[band]).await?;
                self.rf_write(path, 0x18, 0x00100, CHANNEL_SETTING[band])
                    .await?;
                self.rf_write(path, 0x1a, 0x00001, CCK[band]).await?;
                self.rf_write(path, 0x1a, 0x10000, CCK[band]).await?;
                for (gain, rf0) in (1u32..32).step_by(3).enumerate() {
                    self.rf_write(path, 0, 0xff, rf0).await?;
                    self.txgapk.rf3f_bp[band][gain][path] =
                        self.rf_read(path, 0x5f, RF_REGISTER_MASK).await?;
                }
                self.rf_write(path, 0x18, RF_REGISTER_MASK, rf18).await?;
                self.bb_set(three_wire, 0x03, 3).await?;
            }
        }
        self.write_gain_bb_table().await?;
        for band in 0..5 {
            for path in 0..PATHS {
                for gain in 0..TXGAPK_GAIN_COUNT - 1 {
                    self.txgapk.rf3f_same[band][gain][path] = u8::from(
                        self.txgapk.rf3f_bp[band][gain][path] & 0x0fe0
                            == self.txgapk.rf3f_bp[band][gain + 1][path] & 0x0fe0,
                    );
                }
            }
        }
        self.txgapk.read_txgain = true;
        Ok(())
    }

    async fn write_tx_gain(&mut self, channel: u8) -> Result<(), DriverError> {
        let (base, band) = if channel <= 14 {
            (0x20, 1)
        } else if channel <= 64 {
            (0x200, 2)
        } else if channel <= 144 {
            (0x280, 3)
        } else {
            (0x300, 4)
        };
        for path in 0..PATHS {
            if self.txgapk.rf3f_bp[band][..11]
                .iter()
                .all(|gain| gain[path] & 0x0fff == 0)
            {
                continue;
            }
            let mut accumulated = [0i8; 11];
            for (start, accumulated_offset) in accumulated.iter_mut().take(10).enumerate() {
                for index in start..10 {
                    if self.txgapk.rf3f_same[band][index][path] == 0 {
                        *accumulated_offset =
                            accumulated_offset.wrapping_add(self.txgapk.offset[index][path]);
                        self.txgapk.final_offset[start][path] = *accumulated_offset;
                    }
                }
            }
            self.rf_write(path, 0xee, RF_REGISTER_MASK, 0x10000).await?;
            for (index, offset) in accumulated.into_iter().enumerate() {
                self.rf_write(path, 0x33, RF_REGISTER_MASK, base + index as u32)
                    .await?;
                let gain =
                    calculate_tx_gain(self.txgapk.rf3f_bp[band][index][path], offset) & 0x1fff;
                self.rf_write(path, 0x3f, 0x7ffff, gain << 6).await?;
            }
            self.rf_write(path, 0xee, RF_REGISTER_MASK, 0).await?;
        }
        Ok(())
    }

    async fn do_txgapk(&mut self, channel: u8) -> Result<(), DriverError> {
        self.save_all_tx_gains().await?;
        if !self.txgapk.read_txgain {
            return Ok(());
        }
        const BB_REGISTERS: [u16; 3] = [0x0520, 0x1e70, 0x1b00];
        const KIP_REGISTERS: [u16; 2] = [0x1b38, 0x1b20];
        let mut bb_backup = [0u32; 3];
        let mut kip_backup = [[0u32; 2]; 2];
        for (index, register) in BB_REGISTERS.into_iter().enumerate() {
            bb_backup[index] = self.bb_read(register).await?;
        }
        for (index, register) in KIP_REGISTERS.into_iter().enumerate() {
            for (path, backup) in kip_backup[index].iter_mut().enumerate() {
                self.bb_set(0x1b00, 0x06, path as u32).await?;
                *backup = self.bb_read(register).await?;
            }
        }
        self.txgapk_tx_pause().await?;
        for path in 0..PATHS {
            self.txgapk_bb_iqk(path).await?;
            self.txgapk_afe_iqk(path).await?;
            self.txgapk_calculate_offset(path, channel).await?;
            self.txgapk_rf_restore(path).await?;
            self.txgapk_afe_restore(path).await?;
            self.txgapk_bb_restore(path).await?;
        }
        self.write_tx_gain(channel).await?;
        for (index, register) in KIP_REGISTERS.into_iter().enumerate() {
            for (path, backup) in kip_backup[index].into_iter().enumerate() {
                self.bb_set(0x1b00, 0x06, path as u32).await?;
                self.bb_write(register, backup).await?;
            }
        }
        for (index, register) in BB_REGISTERS.into_iter().enumerate() {
            self.bb_write(register, bb_backup[index]).await?;
        }
        self.txgapk.is_txgapk_ok = true;
        Ok(())
    }
}

fn sign_extend_4bit(value: u8) -> i8 {
    if value & 0x08 != 0 {
        value as i8 - 16
    } else {
        value as i8
    }
}

fn calculate_tx_gain(original: u32, offset: i8) -> u32 {
    let half = i32::from(offset) / 2;
    let adjusted = if offset < 0 && offset % 2 != 0 {
        i64::from(original) + 0x1000 + i64::from(half) - 1
    } else if offset > 0 && offset % 2 != 0 {
        i64::from(original) + 0x1000 + i64::from(half)
    } else {
        i64::from(original) + i64::from(half)
    };
    adjusted as u32
}

#[derive(Debug, Default)]
struct EuDackInfo {
    new_msbk_d: [[[u8; 16]; 2]; 2],
    new_biask_d: [u16; 2],
    dadck_d: [[u8; 2]; 2],
    addc: [[u16; 2]; 2],
    addck_d: [[u16; 2]; 2],
    addck_timeout: [bool; 2],
    dadck_timeout: [bool; 2],
    msbk_timeout: [bool; 2],
}

#[derive(Debug, Default)]
struct EuIqkInfo {
    tmp_gntwl: u32,
    rf_reg18: u32,
    iqk_cfir_real: [[[[u16; 17]; 2]; 2]; 3],
    iqk_cfir_imag: [[[[u16; 17]; 2]; 2]; 3],
    txxy: [[u32; 2]; 3],
    rxxy: [[u32; 2]; 3],
    cfir_en: [[u32; 2]; 3],
    iqk_tab: [u32; 2],
    lok_idac: [[u32; 2]; 2],
    lok_fail: [bool; 2],
    iqk_fail_report: [[[bool; 2]; 2]; 3],
    iqk_band: u8,
    iqk_ch: u8,
    iqk_bw: u8,
    is_nbiqk: bool,
    kcount: u8,
    fail_count: u8,
    fail_step: u8,
    iqk_times: u8,
}

#[derive(Debug, Default)]
struct EuTxgapkInfo {
    rf3f_bp: [[[u32; 2]; TXGAPK_GAIN_COUNT]; 5],
    rf3f_same: [[[u8; 2]; TXGAPK_GAIN_COUNT]; 5],
    offset: [[i8; 2]; TXGAPK_GAIN_COUNT],
    final_offset: [[i8; 2]; TXGAPK_GAIN_COUNT],
    read_txgain: bool,
    is_txgapk_ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_extends_vendor_four_bit_offsets() {
        assert_eq!(sign_extend_4bit(0x0), 0);
        assert_eq!(sign_extend_4bit(0x7), 7);
        assert_eq!(sign_extend_4bit(0x8), -8);
        assert_eq!(sign_extend_4bit(0xf), -1);
    }

    #[test]
    fn tx_gain_half_steps_match_devourer() {
        assert_eq!(calculate_tx_gain(0x500, 4), 0x502);
        assert_eq!(calculate_tx_gain(0x500, 3), 0x1501);
        assert_eq!(calculate_tx_gain(0x500, -4), 0x4fe);
        assert_eq!(calculate_tx_gain(0x500, -3), 0x14fe);
    }
}
