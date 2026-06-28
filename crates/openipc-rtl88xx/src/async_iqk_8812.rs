use crate::async_efuse::EfuseInfo;
use crate::async_iqk::IqkReport;
use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::types::{ChipInfo, DriverError};

const RF_REG_MASK: u32 = 0x000f_ffff;

impl RealtekDevice {
    pub(crate) async fn run_iqk_8812_async(
        &self,
        chip: ChipInfo,
        channel: u8,
    ) -> Result<IqkReport, DriverError> {
        let efuse = self.read_efuse_info_async(chip).await?;
        let macbb_regs = [
            0x0520u16, 0x0550, 0x0808, 0x0a04, 0x090c, 0x0c00, 0x0e00, 0x0838, 0x082c,
        ];
        let afe_regs = [
            0x0c5cu16, 0x0c60, 0x0c64, 0x0c68, 0x0cb0, 0x0cb4, 0x0e5c, 0x0e60, 0x0e64, 0x0e68,
            0x0eb0, 0x0eb4,
        ];
        let rf_regs = [0x65u16, 0x8f, 0x00];

        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        let mut macbb_backup = [0u32; 9];
        for (idx, register) in macbb_regs.into_iter().enumerate() {
            macbb_backup[idx] = self.read_u32_async(register).await?;
        }
        self.set_bb_reg_async(0x082c, bit(31), 1).await?;
        let reg_c1b8 = self.read_u32_async(0x0cb8).await?;
        let reg_e1b8 = self.read_u32_async(0x0eb8).await?;

        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        let mut afe_backup = [0u32; 12];
        for (idx, register) in afe_regs.into_iter().enumerate() {
            afe_backup[idx] = self.read_u32_async(register).await?;
        }
        let mut rfa_backup = [0u32; 3];
        let mut rfb_backup = [0u32; 3];
        for (idx, register) in rf_regs.into_iter().enumerate() {
            rfa_backup[idx] = self.query_rf_reg_async(chip, RfPath::A, register).await?;
            rfb_backup[idx] = self.query_rf_reg_async(chip, RfPath::B, register).await?;
        }

        self.configure_mac_iqk_8812_async().await?;
        self.do_tx_rx_calibration_8812_async(chip, &efuse, channel)
            .await?;

        for (idx, register) in rf_regs.into_iter().enumerate() {
            self.set_rf_reg_async(chip, RfPath::A, register, RF_REG_MASK, rfa_backup[idx])
                .await?;
            self.set_rf_reg_async(chip, RfPath::B, register, RF_REG_MASK, rfb_backup[idx])
                .await?;
        }
        self.set_rf_reg_async(chip, RfPath::A, 0xef, RF_REG_MASK, 0)
            .await?;
        self.set_rf_reg_async(chip, RfPath::B, 0xef, RF_REG_MASK, 0)
            .await?;

        for (idx, register) in afe_regs.into_iter().enumerate() {
            self.write_u32_async(register, afe_backup[idx]).await?;
        }
        self.restore_afe_iqk_8812_tail_async().await?;
        self.set_bb_reg_async(0x082c, bit(31), 1).await?;
        self.write_u32_async(0x0cb8, reg_c1b8).await?;
        self.write_u32_async(0x0eb8, reg_e1b8).await?;
        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        for (idx, register) in macbb_regs.into_iter().enumerate() {
            self.write_u32_async(register, macbb_backup[idx]).await?;
        }

        Ok(IqkReport {
            chip,
            channel,
            ran: true,
        })
    }

    async fn configure_mac_iqk_8812_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        self.write_u8_async(0x0522, 0x3f).await?;
        self.set_bb_reg_async(0x0550, bit(11) | bit(3), 0).await?;
        self.write_u8_async(0x0808, 0).await?;
        self.set_bb_reg_async(0x0838, 0x0f, 0x0c).await?;
        self.write_u8_async(0x0a07, 0x0f).await
    }

    async fn restore_afe_iqk_8812_tail_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x082c, bit(31), 1).await?;
        for (register, value) in [
            (0x0c80u16, 0),
            (0x0c84, 0),
            (0x0c88, 0),
            (0x0c8c, 0x3c00_0000),
            (0x0e80, 0),
            (0x0e84, 0),
            (0x0e88, 0),
            (0x0e8c, 0x3c00_0000),
        ] {
            self.write_u32_async(register, value).await?;
        }
        self.set_bb_reg_async(0x0c90, 0x80, 1).await?;
        self.set_bb_reg_async(0x0cc4, bit(18), 1).await?;
        self.set_bb_reg_async(0x0cc4, bit(29), 1).await?;
        self.set_bb_reg_async(0x0cc8, bit(29), 1).await?;
        self.set_bb_reg_async(0x0e90, 0x80, 1).await?;
        self.set_bb_reg_async(0x0ec4, bit(18), 1).await?;
        self.set_bb_reg_async(0x0ec4, bit(29), 1).await?;
        self.set_bb_reg_async(0x0ec8, bit(29), 1).await
    }

    async fn do_tx_rx_calibration_8812_async(
        &self,
        chip: ChipInfo,
        efuse: &EfuseInfo,
        channel: u8,
    ) -> Result<(), DriverError> {
        let mut tx_iqc_temp = [[0i32; 4]; 10];
        let mut rx_iqc_temp = [[0i32; 4]; 10];
        let mut tx_iqc = [0i32; 4];
        let mut rx_iqc = [0i32; 4];
        let mut tx0_average = 0usize;
        let mut tx1_average = 0usize;
        let mut rx0_average = 0usize;
        let mut rx1_average = 0usize;
        let mut cal0_retry = 0u8;
        let mut cal1_retry = 0u8;
        let mut tx0_finish = false;
        let mut tx1_finish = false;
        let mut rx0_finish = false;
        let mut rx1_finish = false;

        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        for register in [0x0c60u16, 0x0c64, 0x0e60, 0x0e64] {
            self.write_u32_async(register, 0x7777_7777).await?;
        }
        self.write_u32_async(0x0c68, 0x1979_1979).await?;
        self.write_u32_async(0x0e68, 0x1979_1979).await?;
        self.set_bb_reg_async(0x0c00, 0x0f, 4).await?;
        self.set_bb_reg_async(0x0e00, 0x0f, 4).await?;
        self.set_bb_reg_async(0x0c5c, bit(26) | bit(25) | bit(24), 7)
            .await?;
        self.set_bb_reg_async(0x0e5c, bit(26) | bit(25) | bit(24), 7)
            .await?;

        for path in [RfPath::A, RfPath::B] {
            self.set_rf_reg_async(chip, path, 0xef, RF_REG_MASK, 0x80002)
                .await?;
            self.set_rf_reg_async(chip, path, 0x30, RF_REG_MASK, 0x20000)
                .await?;
            self.set_rf_reg_async(chip, path, 0x31, RF_REG_MASK, 0x3fffd)
                .await?;
            self.set_rf_reg_async(chip, path, 0x32, RF_REG_MASK, 0xfe83f)
                .await?;
            self.set_rf_reg_async(chip, path, 0x65, RF_REG_MASK, 0x931d5)
                .await?;
            self.set_rf_reg_async(chip, path, 0x8f, RF_REG_MASK, 0x8a001)
                .await?;
        }

        self.write_u32_async(0x090c, 0x0000_8000).await?;
        self.set_bb_reg_async(0x0c94, bit(0), 1).await?;
        self.set_bb_reg_async(0x0e94, bit(0), 1).await?;
        self.write_u32_async(0x0978, 0x2900_2000).await?;
        self.write_u32_async(0x097c, 0xa900_2000).await?;
        self.write_u32_async(0x0984, 0x0046_2910).await?;
        self.set_bb_reg_async(0x082c, bit(31), 1).await?;

        if efuse.external_pa_5g {
            let value = if efuse.rfe_type == 1 {
                0x8214_03e3
            } else {
                0x8214_03f7
            };
            self.write_u32_async(0x0c88, value).await?;
            self.write_u32_async(0x0e88, value).await?;
        } else {
            self.write_u32_async(0x0c88, 0x8214_03f1).await?;
            self.write_u32_async(0x0e88, 0x8214_03f1).await?;
        }
        let band_5g = channel > 14;
        let c8c = if band_5g { 0x6816_3e96 } else { 0x2816_3e96 };
        self.write_u32_async(0x0c8c, c8c).await?;
        self.write_u32_async(0x0e8c, c8c).await?;
        if !band_5g && efuse.rfe_type == 3 {
            self.write_u32_async(
                0x0c88,
                if efuse.external_pa_2g {
                    0x8214_03e3
                } else {
                    0x8214_03f7
                },
            )
            .await?;
        }

        self.write_u32_async(0x0c80, 0x1800_8c10).await?;
        self.write_u32_async(0x0c84, 0x3800_8c10).await?;
        self.write_u32_async(0x0ce8, 0).await?;
        self.write_u32_async(0x0e80, 0x1800_8c10).await?;
        self.write_u32_async(0x0e84, 0x3800_8c10).await?;
        self.write_u32_async(0x0ee8, 0).await?;

        loop {
            self.write_u32_async(0x0cb8, 0x0010_0000).await?;
            self.write_u32_async(0x0eb8, 0x0010_0000).await?;
            self.write_u32_async(0x0980, 0xfa00_0000).await?;
            self.write_u32_async(0x0980, 0xf800_0000).await?;
            crate::time::sleep_ms(10).await;
            self.write_u32_async(0x0cb8, 0).await?;
            self.write_u32_async(0x0eb8, 0).await?;

            let delay_count = self.wait_iqk_ready_8812(tx0_finish, tx1_finish).await?;
            if delay_count < 20 {
                let tx0_fail = self.read_u32_async(0x0d00).await? & bit(12) != 0;
                let tx1_fail = self.read_u32_async(0x0d40).await? & bit(12) != 0;
                if !(tx0_fail || tx0_finish) {
                    self.write_u32_async(0x0cb8, 0x0200_0000).await?;
                    tx_iqc_temp[tx0_average][0] = iqk_sample(self.read_u32_async(0x0d00).await?);
                    self.write_u32_async(0x0cb8, 0x0400_0000).await?;
                    tx_iqc_temp[tx0_average][1] = iqk_sample(self.read_u32_async(0x0d00).await?);
                    tx0_average += 1;
                } else {
                    cal0_retry += 1;
                    if cal0_retry == 10 {
                        break;
                    }
                }
                if !(tx1_fail || tx1_finish) {
                    self.write_u32_async(0x0eb8, 0x0200_0000).await?;
                    tx_iqc_temp[tx1_average][2] = iqk_sample(self.read_u32_async(0x0d40).await?);
                    self.write_u32_async(0x0eb8, 0x0400_0000).await?;
                    tx_iqc_temp[tx1_average][3] = iqk_sample(self.read_u32_async(0x0d40).await?);
                    tx1_average += 1;
                } else {
                    cal1_retry += 1;
                    if cal1_retry == 10 {
                        break;
                    }
                }
            } else {
                cal0_retry += 1;
                cal1_retry += 1;
                if cal0_retry == 10 {
                    break;
                }
            }
            tx0_finish = average_iqk_pair(&tx_iqc_temp, tx0_average, 0, 1, &mut tx_iqc);
            tx1_finish = average_iqk_pair(&tx_iqc_temp, tx1_average, 2, 3, &mut tx_iqc);
            if (tx0_finish && tx1_finish)
                || (cal0_retry as usize + tx0_average) >= 10
                || (cal1_retry as usize + tx1_average) >= 10
            {
                break;
            }
        }

        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        let lok_a = (self.query_rf_reg_async(chip, RfPath::A, 0x08).await? & 0x0ffc00) >> 10;
        let lok_b = (self.query_rf_reg_async(chip, RfPath::B, 0x08).await? & 0x0ffc00) >> 10;
        self.set_rf_reg_async(chip, RfPath::A, 0x58, 0x7fe00, lok_a)
            .await?;
        self.set_rf_reg_async(chip, RfPath::B, 0x58, 0x7fe00, lok_b)
            .await?;
        self.configure_rx_iqk_8812_async(chip, efuse, tx0_finish, tx1_finish, &tx_iqc)
            .await?;

        cal0_retry = 0;
        cal1_retry = 0;
        loop {
            self.run_rx_iqk_one_shot_8812_async(efuse, tx0_finish, tx1_finish, &tx_iqc)
                .await?;
            let delay_count = self
                .wait_rx_iqk_ready_8812(rx0_finish, rx1_finish, tx0_finish, tx1_finish)
                .await?;
            if delay_count < 20 {
                let rx0_fail = self.read_u32_async(0x0d00).await? & bit(11) != 0;
                let rx1_fail = self.read_u32_async(0x0d40).await? & bit(11) != 0;
                if !(rx0_fail || rx0_finish) && tx0_finish {
                    self.write_u32_async(0x0cb8, 0x0600_0000).await?;
                    rx_iqc_temp[rx0_average][0] = iqk_sample(self.read_u32_async(0x0d00).await?);
                    self.write_u32_async(0x0cb8, 0x0800_0000).await?;
                    rx_iqc_temp[rx0_average][1] = iqk_sample(self.read_u32_async(0x0d00).await?);
                    rx0_average += 1;
                } else {
                    cal0_retry += 1;
                    if cal0_retry == 10 {
                        break;
                    }
                }
                if !(rx1_fail || rx1_finish) && tx1_finish {
                    self.write_u32_async(0x0eb8, 0x0600_0000).await?;
                    rx_iqc_temp[rx1_average][2] = iqk_sample(self.read_u32_async(0x0d40).await?);
                    self.write_u32_async(0x0eb8, 0x0800_0000).await?;
                    rx_iqc_temp[rx1_average][3] = iqk_sample(self.read_u32_async(0x0d40).await?);
                    rx1_average += 1;
                } else {
                    cal1_retry += 1;
                    if cal1_retry == 10 {
                        break;
                    }
                }
            } else {
                cal0_retry += 1;
                cal1_retry += 1;
                if cal0_retry == 10 {
                    break;
                }
            }
            rx0_finish = average_iqk_pair(&rx_iqc_temp, rx0_average, 0, 1, &mut rx_iqc);
            rx1_finish = average_iqk_pair(&rx_iqc_temp, rx1_average, 2, 3, &mut rx_iqc);
            if ((rx0_finish || !tx0_finish) && (rx1_finish || !tx1_finish))
                || (cal0_retry as usize + rx0_average) >= 10
                || (cal1_retry as usize + rx1_average) >= 10
                || rx0_average == 3
                || rx1_average == 3
            {
                break;
            }
        }

        self.fill_tx_iqc_8812_async(
            RfPath::A,
            if tx0_finish { tx_iqc[0] } else { 0x200 },
            if tx0_finish { tx_iqc[1] } else { 0 },
        )
        .await?;
        self.fill_rx_iqc_8812_async(
            RfPath::A,
            if rx0_finish { rx_iqc[0] } else { 0x200 },
            if rx0_finish { rx_iqc[1] } else { 0 },
        )
        .await?;
        self.fill_tx_iqc_8812_async(
            RfPath::B,
            if tx1_finish { tx_iqc[2] } else { 0x200 },
            if tx1_finish { tx_iqc[3] } else { 0 },
        )
        .await?;
        self.fill_rx_iqc_8812_async(
            RfPath::B,
            if rx1_finish { rx_iqc[2] } else { 0x200 },
            if rx1_finish { rx_iqc[3] } else { 0 },
        )
        .await
    }

    async fn wait_iqk_ready_8812(
        &self,
        tx0_finish: bool,
        tx1_finish: bool,
    ) -> Result<u8, DriverError> {
        let mut delay_count = 0u8;
        loop {
            let iqk0_ready = tx0_finish || self.read_u32_async(0x0d00).await? & bit(10) != 0;
            let iqk1_ready = tx1_finish || self.read_u32_async(0x0d40).await? & bit(10) != 0;
            if (iqk0_ready && iqk1_ready) || delay_count > 20 {
                break;
            }
            crate::time::sleep_ms(1).await;
            delay_count += 1;
        }
        Ok(delay_count)
    }

    async fn wait_rx_iqk_ready_8812(
        &self,
        rx0_finish: bool,
        rx1_finish: bool,
        tx0_finish: bool,
        tx1_finish: bool,
    ) -> Result<u8, DriverError> {
        let mut delay_count = 0u8;
        loop {
            let iqk0_ready =
                rx0_finish || !tx0_finish || self.read_u32_async(0x0d00).await? & bit(10) != 0;
            let iqk1_ready =
                rx1_finish || !tx1_finish || self.read_u32_async(0x0d40).await? & bit(10) != 0;
            if (iqk0_ready && iqk1_ready) || delay_count > 20 {
                break;
            }
            crate::time::sleep_ms(1).await;
            delay_count += 1;
        }
        Ok(delay_count)
    }

    async fn configure_rx_iqk_8812_async(
        &self,
        chip: ChipInfo,
        efuse: &EfuseInfo,
        tx0_finish: bool,
        tx1_finish: bool,
        tx_iqc: &[i32; 4],
    ) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        if tx0_finish {
            self.configure_rx_iqk_rf_8812_async(chip, RfPath::A).await?;
        }
        if tx1_finish {
            self.configure_rx_iqk_rf_8812_async(chip, RfPath::B).await?;
        }
        self.set_bb_reg_async(0x0978, bit(31), 1).await?;
        self.set_bb_reg_async(0x097c, bit(31), 0).await?;
        self.write_u32_async(0x090c, 0x0000_8000).await?;
        self.write_u32_async(0x0984, 0x0046_a890).await?;
        let inv = if efuse.rfe_type == 1 {
            0x0000_0077
        } else {
            0x0200_0077
        };
        self.write_u32_async(0x0cb0, 0x7777_7717).await?;
        self.write_u32_async(0x0cb4, inv).await?;
        self.write_u32_async(0x0eb0, 0x7777_7717).await?;
        self.write_u32_async(0x0eb4, inv).await?;
        self.set_bb_reg_async(0x082c, bit(31), 1).await?;
        if tx0_finish {
            self.write_u32_async(0x0c80, 0x3800_8c10).await?;
            self.write_u32_async(0x0c84, 0x1800_8c10).await?;
            self.write_u32_async(0x0c88, 0x8214_0119).await?;
        }
        if tx1_finish {
            self.write_u32_async(0x0e80, 0x3800_8c10).await?;
            self.write_u32_async(0x0e84, 0x1800_8c10).await?;
            self.write_u32_async(0x0e88, 0x8214_0119).await?;
        }
        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        if tx0_finish {
            self.set_bb_reg_async(0x0978, 0x03ff_8000, (tx_iqc[0] & 0x07ff) as u32)
                .await?;
            self.set_bb_reg_async(0x0978, 0x0000_07ff, (tx_iqc[1] & 0x07ff) as u32)
                .await?;
        }
        Ok(())
    }

    async fn run_rx_iqk_one_shot_8812_async(
        &self,
        efuse: &EfuseInfo,
        tx0_finish: bool,
        tx1_finish: bool,
        tx_iqc: &[i32; 4],
    ) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        if tx0_finish {
            self.set_bb_reg_async(0x0978, 0x03ff_8000, (tx_iqc[0] & 0x07ff) as u32)
                .await?;
            self.set_bb_reg_async(0x0978, 0x0000_07ff, (tx_iqc[1] & 0x07ff) as u32)
                .await?;
            self.set_bb_reg_async(0x082c, bit(31), 1).await?;
            self.write_u32_async(
                0x0c8c,
                if efuse.rfe_type == 1 {
                    0x2816_1500
                } else {
                    0x2816_0cc0
                },
            )
            .await?;
            self.write_u32_async(0x0cb8, 0x0030_0000).await?;
            self.write_u32_async(0x0cb8, 0x0010_0000).await?;
            crate::time::sleep_ms(5).await;
            self.write_u32_async(0x0c8c, 0x3c00_0000).await?;
            self.write_u32_async(0x0cb8, 0).await?;
        }
        if tx1_finish {
            self.set_bb_reg_async(0x082c, bit(31), 0).await?;
            self.set_bb_reg_async(0x0978, 0x03ff_8000, (tx_iqc[2] & 0x07ff) as u32)
                .await?;
            self.set_bb_reg_async(0x0978, 0x0000_07ff, (tx_iqc[3] & 0x07ff) as u32)
                .await?;
            self.set_bb_reg_async(0x082c, bit(31), 1).await?;
            self.write_u32_async(
                0x0e8c,
                if efuse.rfe_type == 1 {
                    0x2816_1500
                } else {
                    0x2816_0ca0
                },
            )
            .await?;
            self.write_u32_async(0x0eb8, 0x0030_0000).await?;
            self.write_u32_async(0x0eb8, 0x0010_0000).await?;
            crate::time::sleep_ms(5).await;
            self.write_u32_async(0x0e8c, 0x3c00_0000).await?;
            self.write_u32_async(0x0eb8, 0).await?;
        }
        Ok(())
    }

    async fn configure_rx_iqk_rf_8812_async(
        &self,
        chip: ChipInfo,
        path: RfPath,
    ) -> Result<(), DriverError> {
        self.set_rf_reg_async(chip, path, 0xef, RF_REG_MASK, 0x80000)
            .await?;
        self.set_rf_reg_async(chip, path, 0x30, RF_REG_MASK, 0x30000)
            .await?;
        self.set_rf_reg_async(chip, path, 0x31, RF_REG_MASK, 0x3f7ff)
            .await?;
        self.set_rf_reg_async(chip, path, 0x32, RF_REG_MASK, 0xfe7bf)
            .await?;
        self.set_rf_reg_async(chip, path, 0x8f, RF_REG_MASK, 0x88001)
            .await?;
        self.set_rf_reg_async(chip, path, 0x65, RF_REG_MASK, 0x931d1)
            .await?;
        self.set_rf_reg_async(chip, path, 0xef, RF_REG_MASK, 0)
            .await
    }

    async fn fill_tx_iqc_8812_async(
        &self,
        path: RfPath,
        x: i32,
        y: i32,
    ) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x082c, bit(31), 1).await?;
        match path {
            RfPath::A => {
                self.set_bb_reg_async(0x0c90, 0x80, 1).await?;
                self.set_bb_reg_async(0x0cc4, bit(18), 1).await?;
                self.set_bb_reg_async(0x0cc4, bit(29), 1).await?;
                self.set_bb_reg_async(0x0cc8, bit(29), 1).await?;
                self.set_bb_reg_async(0x0ccc, 0x07ff, y as u32).await?;
                self.set_bb_reg_async(0x0cd4, 0x07ff, x as u32).await
            }
            RfPath::B => {
                self.set_bb_reg_async(0x0e90, 0x80, 1).await?;
                self.set_bb_reg_async(0x0ec4, bit(18), 1).await?;
                self.set_bb_reg_async(0x0ec4, bit(29), 1).await?;
                self.set_bb_reg_async(0x0ec8, bit(29), 1).await?;
                self.set_bb_reg_async(0x0ecc, 0x07ff, y as u32).await?;
                self.set_bb_reg_async(0x0ed4, 0x07ff, x as u32).await
            }
            _ => Ok(()),
        }
    }

    async fn fill_rx_iqc_8812_async(
        &self,
        path: RfPath,
        x: i32,
        y: i32,
    ) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x082c, bit(31), 0).await?;
        let xs = (x as u32) >> 1;
        let ys = (y as u32) >> 1;
        let Some(register) = (match path {
            RfPath::A => Some(0x0c10),
            RfPath::B => Some(0x0e10),
            _ => None,
        }) else {
            return Ok(());
        };
        let (safe_x, safe_y) = if xs >= 0x112 || (0x12..=0x3ee).contains(&ys) {
            (0x100, 0)
        } else {
            (xs, ys)
        };
        self.set_bb_reg_async(register, 0x03ff, safe_x).await?;
        self.set_bb_reg_async(register, 0x03ff_0000, safe_y).await
    }
}

fn average_iqk_pair(
    samples: &[[i32; 4]; 10],
    count: usize,
    x_idx: usize,
    y_idx: usize,
    out: &mut [i32; 4],
) -> bool {
    if count < 2 {
        return false;
    }
    for i in 0..count {
        for ii in (i + 1)..count {
            let dx = (samples[i][x_idx] >> 21) - (samples[ii][x_idx] >> 21);
            let dy = (samples[i][y_idx] >> 21) - (samples[ii][y_idx] >> 21);
            if (-3..=3).contains(&dx) && (-3..=3).contains(&dy) {
                out[x_idx] = ((samples[i][x_idx] >> 21) + (samples[ii][x_idx] >> 21)) / 2;
                out[y_idx] = ((samples[i][y_idx] >> 21) + (samples[ii][y_idx] >> 21)) / 2;
                return true;
            }
        }
    }
    false
}

fn iqk_sample(register_value: u32) -> i32 {
    (((register_value & 0x07ff_0000) >> 16) as i32) << 21
}

const fn bit(n: u32) -> u32 {
    1u32 << n
}
