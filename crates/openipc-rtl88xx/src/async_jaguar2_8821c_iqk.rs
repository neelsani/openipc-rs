//! RTL8821C one-path software IQK calibration.

use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::time::{sleep_micros, sleep_ms};
use crate::types::{ChipInfo, DriverError};

const RF_MASK: u32 = 0x000f_ffff;
const TX_IQK: usize = 0;
const RX_IQK1: usize = 1;
const RX_IQK2: usize = 2;
const BTG_LNA: [u8; 5] = [0, 4, 8, 12, 15];
const WLG_LNA: [u8; 5] = [0, 1, 2, 3, 5];
const WLA_LNA: [u8; 5] = [0, 1, 3, 4, 5];

struct Iqk8821cState {
    band_2g: bool,
    is_btg: bool,
    iqk_step: u8,
    rx_step: u8,
    tmp_1bcc: u8,
    lna_index: usize,
    boundary: bool,
    grant: u32,
    retries: [u8; 3],
    gain_retries: [u8; 2],
    lok_failed: bool,
    rx_fail_code: u8,
    fail_report: [bool; 2],
    rx_agc: [u16; 2],
}

impl Iqk8821cState {
    fn new(band_2g: bool, is_btg: bool) -> Self {
        Self {
            band_2g,
            is_btg,
            iqk_step: 1,
            rx_step: 1,
            tmp_1bcc: 0x12,
            lna_index: 0,
            boundary: false,
            grant: 0,
            retries: [0; 3],
            gain_retries: [0; 2],
            lok_failed: true,
            rx_fail_code: 0,
            fail_report: [true; 2],
            rx_agc: [0; 2],
        }
    }
}

impl RealtekDevice {
    pub(crate) async fn run_iqk_8821c_async(
        &self,
        chip: ChipInfo,
        band_2g: bool,
    ) -> Result<(), DriverError> {
        const MAC: [u16; 3] = [0x0520, 0x0550, 0x1518];
        const BB: [u16; 16] = [
            0x0808, 0x090c, 0x0c00, 0x0cb0, 0x0cb4, 0x0cbc, 0x1990, 0x09a4, 0x0a04, 0x0838, 0x0c94,
            0x0b00, 0x0c58, 0x0c5c, 0x0c60, 0x0c6c,
        ];
        const RF: [u16; 5] = [0xdf, 0xde, 0x8f, 0x00, 0x01];

        let mut state = Iqk8821cState::new(
            band_2g,
            self.query_bb_reg_async(0x0cb8, 1 << 16).await? != 0,
        );
        state.grant = self.iqk_indirect_read_8821c_async(0x38).await?;
        let mut mac_backup = [0u32; 3];
        let mut bb_backup = [0u32; 16];
        let mut rf_backup = [0u32; 5];
        for (slot, register) in mac_backup.iter_mut().zip(MAC) {
            *slot = self.read_u32_async(register).await?;
        }
        for (slot, register) in bb_backup.iter_mut().zip(BB) {
            *slot = self.read_u32_async(register).await?;
        }
        for (slot, register) in rf_backup.iter_mut().zip(RF) {
            *slot = self.query_rf_reg_async(chip, RfPath::A, register).await?;
        }

        for _ in 0..=4 {
            self.iqk_configure_8821c_async().await?;
            self.iqk_afe_8821c_async(chip, true).await?;
            self.iqk_rfe_8821c_async(false).await?;
            for value in [0xf800_0008, 0xf80a_7008, 0xf801_5008, 0xf800_0008] {
                self.write_u32_async(0x1b00, value).await?;
            }
            self.iqk_rf_setting_8821c_async(chip, &state).await?;
            self.iqk_start_8821c_async(chip, &mut state).await?;
            self.iqk_afe_8821c_async(chip, false).await?;
            for (register, value) in MAC.into_iter().zip(mac_backup) {
                self.write_u32_async(register, value).await?;
            }
            for (register, value) in BB.into_iter().zip(bb_backup) {
                self.write_u32_async(register, value).await?;
            }
            self.set_rf_reg_async(chip, RfPath::A, 0xef, RF_MASK, 0)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0xee, RF_MASK, 0)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0xdf, RF_MASK, rf_backup[0] & !(1 << 4))
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0xde, RF_MASK, rf_backup[1] & !(1 << 4))
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x8f, RF_MASK, rf_backup[2])
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x00, RF_MASK, rf_backup[3])
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x01, RF_MASK, rf_backup[4] & !1)
                .await?;
            if state.iqk_step == 4 {
                break;
            }
            sleep_ms(50).await;
        }
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        self.set_bb_reg_async(
            0x1bf0,
            0x00ff_ffff,
            u32::from(state.fail_report[0])
                | (u32::from(state.fail_report[1]) << 4)
                | (u32::from(state.rx_fail_code) << 8),
        )
        .await?;
        self.write_u32_async(
            0x1be8,
            (u32::from(state.rx_agc[1]) << 16) | u32::from(state.rx_agc[0]),
        )
        .await?;
        log::debug!(target: "openipc_rtl88xx::iqk", "RTL8821C IQK complete btg={} lok_fail={} retries={:?} gain_retries={:?} fail={:?} rx_code={}", state.is_btg, state.lok_failed, state.retries, state.gain_retries, state.fail_report, state.rx_fail_code);
        Ok(())
    }

    async fn iqk_configure_8821c_async(&self) -> Result<(), DriverError> {
        self.write_u8_async(0x0522, 0x7f).await?;
        self.set_bb_reg_async(0x1518, 1 << 16, 1).await?;
        self.set_bb_reg_async(0x0550, (1 << 11) | (1 << 3), 0)
            .await?;
        self.set_bb_reg_async(0x090c, 1 << 15, 1).await?;
        self.set_bb_reg_async(0x0c94, 1, 1).await?;
        self.set_bb_reg_async(0x0c94, (1 << 11) | (1 << 10), 1)
            .await?;
        self.write_u32_async(0x0c00, 4).await?;
        self.set_bb_reg_async(0x0b00, 1 << 8, 0).await?;
        self.set_bb_reg_async(0x0808, 1 << 28, 0).await?;
        self.set_bb_reg_async(0x0838, 0x0e, 7).await
    }

    async fn iqk_afe_8821c_async(&self, chip: ChipInfo, enable: bool) -> Result<(), DriverError> {
        if enable {
            for (register, value) in [
                (0x0c60, 0x5000_0000),
                (0x0c60, 0x700f_0040),
                (0x0c58, 0xd800_0402),
                (0x0c5c, 0xd100_0120),
                (0x0c6c, 0x0000_0a15),
            ] {
                self.write_u32_async(register, value).await?;
            }
            self.iqk_bb_reset_8821c_async(chip).await?;
        } else {
            for (register, value) in [
                (0x0c60, 0x5000_0000),
                (0x0c60, 0x700b_8040),
                (0x0c58, 0xd802_0402),
                (0x0c5c, 0xde00_0120),
                (0x0c6c, 0x0000_122a),
            ] {
                self.write_u32_async(register, value).await?;
            }
        }
        self.set_bb_reg_async(0x09a4, 1 << 31, 0).await
    }

    async fn iqk_bb_reset_8821c_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        self.set_rf_reg_async(chip, RfPath::A, 0, RF_MASK, 0x10000)
            .await?;
        self.set_bb_reg_async(0x08f8, 0x0ff0_0000, 0).await?;
        for count in 0..=30 {
            self.write_u32_async(0x08fc, 0).await?;
            self.set_bb_reg_async(0x198c, 7, 7).await?;
            if self.query_bb_reg_async(0x0fa0, 1 << 3).await? == 0 || count == 30 {
                self.write_u8_async(0x0808, 0).await?;
                self.set_bb_reg_async(0x0a04, 0x0f00_0000, 0).await?;
                self.set_bb_reg_async(0, 1 << 16, 0).await?;
                self.set_bb_reg_async(0, 1 << 16, 1).await?;
                if self.query_bb_reg_async(0x0660, 1 << 16).await? != 0 {
                    self.write_u32_async(0x06b4, 0x8900_0006).await?;
                }
                break;
            }
            sleep_ms(1).await;
        }
        Ok(())
    }

    async fn iqk_rfe_8821c_async(&self, external_pa: bool) -> Result<(), DriverError> {
        let values = if external_pa {
            [0x7777_7777, 0x0000_7777, 0x0000_083b]
        } else {
            [0x7717_1117, 0x0000_1177, 0x0000_0404]
        };
        for (register, value) in [0x0cb0, 0x0cb4, 0x0cbc].into_iter().zip(values) {
            self.write_u32_async(register, value).await?;
        }
        Ok(())
    }

    async fn iqk_rf_setting_8821c_async(
        &self,
        chip: ChipInfo,
        state: &Iqk8821cState,
    ) -> Result<(), DriverError> {
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        self.write_u32_async(0x1bb8, 0).await?;
        let mut df = self.query_rf_reg_async(chip, RfPath::A, 0xdf).await?;
        df = (df & !(1 << 4)) | (1 << 1) | (1 << 11);
        self.set_rf_reg_async(chip, RfPath::A, 0xdf, RF_MASK, df)
            .await?;
        if state.is_btg {
            let de =
                (self.query_rf_reg_async(chip, RfPath::A, 0xde).await? & !(1 << 4)) | (1 << 15);
            self.set_rf_reg_async(chip, RfPath::A, 0xde, RF_MASK, de)
                .await?;
            for (register, value) in [(0xee, 0x01000), (0x33, 0x00004), (0x3f, 0x01ec1), (0xee, 0)]
            {
                self.set_rf_reg_async(chip, RfPath::A, register, RF_MASK, value)
                    .await?;
            }
        } else {
            for (register, value) in [
                (0xef, 0x80000),
                (0x33, 0x00024),
                (0x3e, 0x0003f),
                (0x3f, 0xe0fde),
                (0xef, 0),
            ] {
                self.set_rf_reg_async(chip, RfPath::A, register, RF_MASK, value)
                    .await?;
            }
            self.set_rf_reg_async(chip, RfPath::A, 0xef, 1 << 19, 1)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x33, RF_MASK, 0x00026)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x3e, RF_MASK, 0x00037)
                .await?;
            self.set_rf_reg_async(
                chip,
                RfPath::A,
                0x3f,
                RF_MASK,
                if state.band_2g { 0x5efce } else { 0xdefce },
            )
            .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0xef, 1 << 19, 0)
                .await?;
        }
        Ok(())
    }

    async fn iqk_start_8821c_async(
        &self,
        chip: ChipInfo,
        state: &mut Iqk8821cState,
    ) -> Result<(), DriverError> {
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        self.write_u32_async(0x1bb8, 0).await?;
        let mut rf1 = self.query_rf_reg_async(chip, RfPath::A, 1).await?;
        rf1 = if state.is_btg {
            (rf1 & !(1 << 3)) | 1 | (1 << 2) | (1 << 5)
        } else {
            (rf1 & !(1 << 3) & !(1 << 5)) | 1 | (1 << 2)
        };
        self.set_rf_reg_async(chip, RfPath::A, 1, RF_MASK, rf1)
            .await?;
        self.iqk_by_path_8821c_async(chip, state).await
    }

    async fn iqk_by_path_8821c_async(
        &self,
        chip: ChipInfo,
        state: &mut Iqk8821cState,
    ) -> Result<(), DriverError> {
        for _ in 0..=100 {
            match state.iqk_step {
                1 => {
                    for pad in 0..8 {
                        self.iqk_lok_setting_8821c_async(chip, state, pad).await?;
                        state.lok_failed = self.iqk_lok_once_8821c_async(chip, state).await?;
                    }
                    state.iqk_step += 1;
                }
                2 => {
                    self.iqk_tx_setting_8821c_async(chip, state).await?;
                    let failed = self.iqk_once_8821c_async(chip, state, TX_IQK).await?;
                    if failed && state.retries[TX_IQK] < 3 {
                        state.retries[TX_IQK] += 1;
                    } else {
                        state.iqk_step += 1;
                    }
                }
                3 => {
                    self.iqk_rx_steps_8821c_async(chip, state).await?;
                    if state.rx_step == 5 {
                        state.iqk_step += 1;
                        state.rx_step = 1;
                    }
                }
                _ => break,
            }
            if state.iqk_step == 4 {
                self.write_u32_async(0x1b00, 0xf800_0008).await?;
                self.write_u32_async(0x1b2c, 7).await?;
                self.write_u32_async(0x1bcc, 0).await?;
                self.write_u32_async(0x1b38, 0x2000_0000).await?;
                break;
            }
        }
        Ok(())
    }

    async fn iqk_lok_setting_8821c_async(
        &self,
        chip: ChipInfo,
        state: &Iqk8821cState,
        pad: u8,
    ) -> Result<(), DriverError> {
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        if state.is_btg {
            self.write_u32_async(0x1bcc, 0x1b).await?;
            self.write_u8_async(0x1b23, 0).await?;
            self.write_u8_async(0x1b2b, 0x80).await?;
            self.set_rf_reg_async(
                chip,
                RfPath::A,
                0x78,
                RF_MASK,
                0xbcbba & (0xe3fff | (u32::from(pad) << 14)),
            )
            .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x5c, RF_MASK, 0x05320)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x8f, RF_MASK, 0xac018)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0xee, 1 << 4, 1)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x33, 1 << 3, 0)
                .await?;
        } else {
            self.write_u32_async(0x1bcc, 9).await?;
            self.write_u8_async(0x1b23, 0).await?;
            self.write_u8_async(0x1b2b, 0).await?;
            let value =
                (if state.band_2g { 0x50ef3 } else { 0x50ee8 }) & (0xfff1f | (u32::from(pad) << 5));
            self.set_rf_reg_async(chip, RfPath::A, 0x56, RF_MASK, value)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x8f, RF_MASK, 0xadc18)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0xef, 1 << 4, 1)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x33, 1 << 3, u32::from(!state.band_2g))
                .await?;
        }
        self.set_rf_reg_async(chip, RfPath::A, 0x33, 7, u32::from(pad))
            .await
    }

    async fn iqk_tx_setting_8821c_async(
        &self,
        chip: ChipInfo,
        state: &Iqk8821cState,
    ) -> Result<(), DriverError> {
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        if state.is_btg {
            self.write_u32_async(0x1bcc, 0x1b).await?;
            self.write_u32_async(0x1b20, 0x0084_0008).await?;
            for (register, value) in [(0x78, 0xbcbba), (0x5c, 0x04320), (0x8f, 0xac018)] {
                self.set_rf_reg_async(chip, RfPath::A, register, RF_MASK, value)
                    .await?;
            }
            self.write_u8_async(0x1b2b, 0x80).await
        } else {
            self.write_u32_async(0x1bcc, 9).await?;
            self.write_u32_async(0x1b20, 0x0144_0008).await?;
            self.set_rf_reg_async(
                chip,
                RfPath::A,
                0x56,
                RF_MASK,
                if state.band_2g { 0x50ef3 } else { 0x5004e },
            )
            .await?;
            self.set_rf_reg_async(
                chip,
                RfPath::A,
                0x8f,
                RF_MASK,
                if state.band_2g { 0xadc18 } else { 0xa9c18 },
            )
            .await?;
            self.write_u8_async(0x1b2b, 0).await
        }
    }

    async fn iqk_rx1_setting_8821c_async(
        &self,
        chip: ChipInfo,
        state: &Iqk8821cState,
    ) -> Result<(), DriverError> {
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        if state.is_btg {
            self.write_u8_async(0x1b2b, 0x80).await?;
            self.write_u32_async(0x1bcc, 9).await?;
            self.write_u32_async(0x1b20, 0x0145_0008).await?;
            self.write_u32_async(0x1b24, 0x0146_0c88).await?;
            for (register, value) in [(0x78, 0x8cbba), (0x5c, 0x00320), (0x8f, 0xa8018)] {
                self.set_rf_reg_async(chip, RfPath::A, register, RF_MASK, value)
                    .await?;
            }
        } else {
            self.write_u8_async(0x1bcc, if state.band_2g { 0x12 } else { 9 })
                .await?;
            self.write_u8_async(0x1b2b, 0).await?;
            self.write_u32_async(
                0x1b20,
                if state.band_2g {
                    0x0145_0008
                } else {
                    0x0045_0008
                },
            )
            .await?;
            self.write_u32_async(
                0x1b24,
                if state.band_2g {
                    0x0146_1068
                } else {
                    0x0046_1468
                },
            )
            .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x56, RF_MASK, 0x510f3)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x8f, RF_MASK, 0xa9c00)
                .await?;
        }
        Ok(())
    }

    async fn iqk_rx2_setting_8821c_async(
        &self,
        chip: ChipInfo,
        state: &mut Iqk8821cState,
        gain_search: bool,
    ) -> Result<(), DriverError> {
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        let lna = if state.is_btg {
            &BTG_LNA
        } else if state.band_2g {
            &WLG_LNA
        } else {
            &WLA_LNA
        };
        if gain_search {
            state.tmp_1bcc = if state.is_btg {
                0x1b
            } else if state.band_2g {
                0x12
            } else {
                9
            };
            state.lna_index = 2;
        }
        self.write_u8_async(0x1b2b, if state.is_btg { 0x80 } else { 0 })
            .await?;
        self.write_u32_async(0x1bcc, u32::from(state.tmp_1bcc))
            .await?;
        self.write_u32_async(
            0x1b20,
            if !state.is_btg && !state.band_2g {
                0x0045_0008
            } else {
                0x0145_0008
            },
        )
        .await?;
        self.write_u32_async(
            0x1b24,
            0x0146_0048 | (u32::from(lna[state.lna_index]) << 10),
        )
        .await?;
        if state.is_btg {
            for (register, value) in [(0x78, 0x8cbba), (0x5c, 0x00320), (0x8f, 0xa8018)] {
                self.set_rf_reg_async(chip, RfPath::A, register, RF_MASK, value)
                    .await?;
            }
        } else {
            self.set_rf_reg_async(
                chip,
                RfPath::A,
                0x56,
                RF_MASK,
                if state.band_2g { 0x510f3 } else { 0x51060 },
            )
            .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x8f, RF_MASK, 0xa9c00)
                .await?;
        }
        Ok(())
    }

    async fn iqk_rx_steps_8821c_async(
        &self,
        chip: ChipInfo,
        state: &mut Iqk8821cState,
    ) -> Result<(), DriverError> {
        match state.rx_step {
            1 => {
                self.iqk_rx1_setting_8821c_async(chip, state).await?;
                loop {
                    let failed = self
                        .iqk_gain_search_8821c_async(chip, state, RX_IQK1)
                        .await?;
                    if failed && state.gain_retries[0] < 2 {
                        state.gain_retries[0] += 1;
                    } else {
                        if failed {
                            state.rx_fail_code = 0;
                            state.rx_step = 5;
                        } else {
                            state.rx_step += 1;
                        }
                        break;
                    }
                }
            }
            2 => {
                self.iqk_rx2_setting_8821c_async(chip, state, true).await?;
                state.boundary = false;
                loop {
                    let failed = self
                        .iqk_gain_search_8821c_async(chip, state, RX_IQK2)
                        .await?;
                    if failed && state.gain_retries[1] < 6 {
                        state.gain_retries[1] += 1;
                    } else {
                        state.rx_step += 1;
                        break;
                    }
                }
            }
            3 => {
                self.iqk_rx1_setting_8821c_async(chip, state).await?;
                loop {
                    let failed = self.iqk_once_8821c_async(chip, state, RX_IQK1).await?;
                    if failed && state.retries[RX_IQK1] < 2 {
                        state.retries[RX_IQK1] += 1;
                    } else {
                        if failed {
                            state.rx_fail_code = 1;
                            state.rx_step = 5;
                        } else {
                            state.rx_step += 1;
                        }
                        break;
                    }
                }
            }
            4 => {
                self.iqk_rx2_setting_8821c_async(chip, state, false).await?;
                loop {
                    let failed = self.iqk_once_8821c_async(chip, state, RX_IQK2).await?;
                    if failed && state.retries[RX_IQK2] < 2 {
                        state.retries[RX_IQK2] += 1;
                    } else {
                        if failed {
                            state.rx_fail_code = 2;
                        }
                        state.rx_step = 5;
                        break;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn iqk_gain_search_8821c_async(
        &self,
        chip: ChipInfo,
        state: &mut Iqk8821cState,
        step: usize,
    ) -> Result<bool, DriverError> {
        const IQMUX: [u8; 4] = [9, 0x12, 0x1b, 0x24];
        let lna = if state.is_btg {
            &BTG_LNA
        } else if state.band_2g {
            &WLG_LNA
        } else {
            &WLA_LNA
        };
        let mut mux = IQMUX
            .iter()
            .position(|value| *value == state.tmp_1bcc)
            .unwrap_or(0);
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        self.write_u32_async(0x1bcc, u32::from(state.tmp_1bcc))
            .await?;
        let command = if step == RX_IQK1 {
            0xf800_0208
        } else {
            0xf800_0308
        } | (1 << 4);
        self.iqk_force_grant_8821c_async(state, true).await?;
        self.write_u32_async(0x1b00, command).await?;
        self.write_u32_async(0x1b00, command + 1).await?;
        let mut failed = self.iqk_check_cal_8821c_async(command).await?;
        self.iqk_force_grant_8821c_async(state, false).await?;
        if step == RX_IQK2 {
            let rxbb = (self.query_rf_reg_async(chip, RfPath::A, 0).await? >> 5) & 0x1f;
            if rxbb == 1 {
                if mux != 3 {
                    mux += 1;
                } else if state.lna_index != 0 {
                    state.lna_index -= 1;
                } else {
                    state.boundary = true;
                }
                failed = true;
            } else if rxbb == 0x0a {
                if mux != 0 {
                    mux -= 1;
                } else if state.lna_index != 4 {
                    state.lna_index += 1;
                } else {
                    state.boundary = true;
                }
                failed = true;
            } else {
                failed = false;
            }
            if state.boundary {
                failed = false;
            }
            state.tmp_1bcc = IQMUX[mux];
            if failed {
                self.write_u32_async(0x1b00, 0xf800_0008).await?;
                self.write_u32_async(
                    0x1b24,
                    (self.read_u32_async(0x1b24).await? & 0xffff_c3ff)
                        | (u32::from(lna[state.lna_index]) << 10),
                )
                .await?;
            }
        }
        Ok(failed)
    }

    async fn iqk_once_8821c_async(
        &self,
        chip: ChipInfo,
        state: &mut Iqk8821cState,
        index: usize,
    ) -> Result<bool, DriverError> {
        let command = match index {
            TX_IQK => 0xf800_0408 | (1 << 4),
            RX_IQK1 => 0xf800_0708 | (1 << 4),
            _ => 0xf800_0908 | (1 << 4),
        };
        self.iqk_force_grant_8821c_async(state, true).await?;
        self.write_u32_async(0x1bc8, 0x8000_0000).await?;
        self.write_u32_async(0x08f8, 0x4140_0080).await?;
        if self.query_rf_reg_async(chip, RfPath::A, 8).await? != 0 {
            self.set_rf_reg_async(chip, RfPath::A, 8, RF_MASK, 0)
                .await?;
        }
        self.write_u32_async(0x1b00, command).await?;
        self.write_u32_async(0x1b00, command + 1).await?;
        let failed = self.iqk_check_nctl_8821c_async(chip, command).await?;
        self.iqk_force_grant_8821c_async(state, false).await?;
        self.write_u32_async(0x1b00, 0xf800_0008).await?;
        if index == TX_IQK {
            if failed {
                self.set_bb_reg_async(0x0c94, 1, 0).await?;
            }
            state.fail_report[0] = failed;
        } else if index == RX_IQK2 {
            state.rx_agc[0] = (((self.query_rf_reg_async(chip, RfPath::A, 0).await? >> 5) & 0xff)
                | (u32::from(state.tmp_1bcc) << 8)) as u16;
            self.write_u32_async(0x1b38, 0x2000_0000).await?;
            if failed {
                self.set_bb_reg_async(0x0c94, (1 << 11) | (1 << 10), 0)
                    .await?;
            }
            state.fail_report[1] = failed;
        }
        Ok(failed)
    }

    async fn iqk_lok_once_8821c_async(
        &self,
        chip: ChipInfo,
        state: &Iqk8821cState,
    ) -> Result<bool, DriverError> {
        let command = 0xf800_0018;
        self.iqk_force_grant_8821c_async(state, true).await?;
        self.write_u32_async(0x1b00, command).await?;
        self.write_u32_async(0x1b00, command + 1).await?;
        sleep_micros(10).await;
        let mut failed = true;
        for _ in 0..50_000 {
            if self.query_rf_reg_async(chip, RfPath::A, 8).await? == 0x12345 {
                failed = false;
                break;
            }
            sleep_micros(10).await;
        }
        self.set_rf_reg_async(chip, RfPath::A, 8, RF_MASK, 0)
            .await?;
        self.iqk_force_grant_8821c_async(state, false).await?;
        Ok(failed)
    }

    async fn iqk_check_cal_8821c_async(&self, command: u32) -> Result<bool, DriverError> {
        for _ in 0..50_000 {
            if self.read_u32_async(0x1b00).await? == command & 0xffff_ff0f {
                return Ok(self.query_bb_reg_async(0x1b08, 1 << 26).await? != 0);
            }
            sleep_micros(10).await;
        }
        Ok(true)
    }

    async fn iqk_check_nctl_8821c_async(
        &self,
        chip: ChipInfo,
        command: u32,
    ) -> Result<bool, DriverError> {
        let done = if (command & 0xf00) >> 8 == 0x0c {
            0x1a3b5
        } else {
            0x12345
        };
        let mut failed = true;
        for _ in 0..50_000 {
            if self.query_rf_reg_async(chip, RfPath::A, 8).await? == done {
                failed = self.query_bb_reg_async(0x1b08, 1 << 26).await? != 0;
                break;
            }
            sleep_micros(10).await;
        }
        self.set_rf_reg_async(chip, RfPath::A, 8, RF_MASK, 0)
            .await?;
        Ok(failed)
    }

    async fn iqk_indirect_read_8821c_async(&self, register: u16) -> Result<u32, DriverError> {
        self.write_u32_async(0x1700, 0x800f_0000 | u32::from(register))
            .await?;
        for _ in 0..30_000 {
            if self.read_u8_async(0x1703).await? & (1 << 5) != 0 {
                break;
            }
        }
        self.read_u32_async(0x1708).await
    }

    async fn iqk_indirect_write_8821c_async(
        &self,
        register: u16,
        mask: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        let data = if mask == u32::MAX {
            value
        } else {
            let current = self.iqk_indirect_read_8821c_async(register).await?;
            (current & !mask) | (value << mask.trailing_zeros())
        };
        self.write_u32_async(0x1704, data).await?;
        for _ in 0..30_000 {
            if self.read_u8_async(0x1703).await? & (1 << 5) != 0 {
                break;
            }
        }
        self.write_u32_async(0x1700, 0xc00f_0000 | u32::from(register))
            .await
    }

    async fn iqk_force_grant_8821c_async(
        &self,
        state: &Iqk8821cState,
        before: bool,
    ) -> Result<(), DriverError> {
        if before {
            for (mask, value) in [(0x3000, 3), (0x0300, 3), (0xc000, 1), (0x0c00, 1)] {
                self.iqk_indirect_write_8821c_async(0x38, mask, value)
                    .await?;
            }
            Ok(())
        } else {
            self.iqk_indirect_write_8821c_async(0x38, u32::MAX, state.grant)
                .await
        }
    }
}
