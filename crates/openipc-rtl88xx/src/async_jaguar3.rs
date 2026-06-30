use std::future::Future;

use crate::device::RealtekDevice;
use crate::regs::*;
use crate::rtl_data;
use crate::time::{sleep_micros, sleep_ms};
use crate::tx::TX_DESC_SIZE_8822C;
use crate::types::{
    ChannelWidth, ChipInfo, DriverError, InitReport, InitStatus, MonitorOptions, RadioConfig,
};

const MASKDWORD: u32 = 0xffff_ffff;
const RFREG_MASK: u32 = 0x000f_ffff;

const REG_FIFOPAGE_CTRL_2: u16 = 0x0204;
const REG_FIFOPAGE_INFO_1: u16 = 0x0230;
const REG_FIFOPAGE_INFO_2: u16 = 0x0234;
const REG_FIFOPAGE_INFO_3: u16 = 0x0238;
const REG_FIFOPAGE_INFO_4: u16 = 0x023c;
const REG_FIFOPAGE_INFO_5: u16 = 0x0240;
const REG_H2C_HEAD: u16 = 0x0244;
const REG_H2C_TAIL: u16 = 0x0248;
const REG_H2C_READ_ADDR: u16 = 0x024c;
const REG_H2C_INFO: u16 = 0x0254;
const REG_FWFF_CTRL: u16 = 0x029c;
const REG_FWFF_PKT_INFO: u16 = 0x02a0;
const REG_RXDMA_MODE_8822C: u16 = 0x0290;
const REG_TXDMA_STATUS: u16 = 0x0210;
const REG_RQPN_CTRL_2: u16 = 0x022c;
const REG_BCNQ1_BDNY_V1: u16 = 0x0456;
const REG_LIFETIME_EN: u16 = 0x0426;
const REG_DARFRC: u16 = 0x0430;
const REG_DARFRCH: u16 = 0x0434;
const REG_RARFRCH: u16 = 0x043c;
const REG_ARFR0: u16 = 0x0444;
const REG_ARFRH0: u16 = 0x0448;
const REG_ARFR1_V1: u16 = 0x044c;
const REG_ARFRH1_V1: u16 = 0x0450;
const REG_AMPDU_MAX_TIME_V1: u16 = 0x0455;
const REG_TX_HANG_CTRL: u16 = 0x045e;
const REG_INIRTS_RATE_SEL: u16 = 0x0480;
const REG_ARFR4: u16 = 0x049c;
const REG_ARFRH4: u16 = 0x04a0;
const REG_ARFR5: u16 = 0x04a4;
const REG_ARFRH5: u16 = 0x04a8;
const REG_PROT_MODE_CTRL: u16 = 0x04c8;
const REG_PRECNT_CTRL: u16 = 0x04e5;
const REG_RD_NAV_NXT: u16 = 0x0544;
const REG_BCN_CTRL_CLINT0: u16 = 0x0551;
const REG_RXTSF_OFFSET_CCK: u16 = 0x055e;
const REG_TIMER0_SRC_SEL: u16 = 0x05b4;
const REG_TCR: u16 = 0x0604;
const REG_RXFLTMAP0: u16 = 0x06a0;
const REG_BBPSF_CTRL: u16 = 0x06dc;
const REG_ACKTO_CCK: u16 = 0x0639;
const REG_RESP_SIFS_CCK: u16 = 0x063c;
const REG_RESP_SIFS_OFDM: u16 = 0x063e;
const REG_SND_PTCL_CTRL: u16 = 0x0718;
const REG_WMAC_OPTION_FUNCTION_1: u16 = 0x07d4;
const REG_WMAC_OPTION_FUNCTION_2: u16 = 0x07d8;
const REG_AFE_CTRL1: u16 = 0x0024;
const REG_PAD_CTRL1: u16 = 0x0064;
const REG_GPIO_MUXCFG: u16 = 0x0040;
const REG_LED_CFG: u16 = 0x004c;
const REG_SYS_CFG2: u16 = 0x00fc;
const REG_WLRF1: u16 = 0x00ec;
const REG_CPU_DMEM_CON: u16 = 0x1080;
const REG_CR_EXT: u16 = 0x1100;
const REG_H2CQ_CSR: u16 = 0x1330;
const REG_CPUMGQ_PARAMETER: u16 = 0x1518;
const REG_RXPSF_CTRL: u16 = 0x1610;
const REG_GENERAL_OPTION: u16 = 0x1664;
const REG_WMAC_CSIDMA_CFG: u16 = 0x169c;
const REG_MU_TX_CTL: u16 = 0x14c0;
const REG_MU_BF_OPTION: u16 = 0x167c;
const REG_WMAC_MU_BF_CTL: u16 = 0x1680;
const REG_TXBF_CTRL: u16 = 0x042c;
const REG_NDPA_OPT_CTRL: u16 = 0x045f;
const REG_DDMA_CH0SA: u16 = 0x1200;
const REG_DDMA_CH0DA: u16 = 0x1204;
const REG_DDMA_CH0CTRL: u16 = 0x1208;
const REG_FW_DBG7: u16 = 0x10fc;
const REG_USB_USBSTAT: u16 = 0xfe11;

const WLAN_FW_HDR_SIZE: usize = 64;
const WLAN_FW_HDR_CHKSUM_SIZE: u32 = 8;
const WLAN_FW_HDR_MEM_USAGE: usize = 24;
const WLAN_FW_HDR_DMEM_ADDR: usize = 32;
const WLAN_FW_HDR_DMEM_SIZE: usize = 36;
const WLAN_FW_HDR_IMEM_SIZE: usize = 48;
const WLAN_FW_HDR_EMEM_SIZE: usize = 52;
const WLAN_FW_HDR_EMEM_ADDR: usize = 56;
const WLAN_FW_HDR_IMEM_ADDR: usize = 60;
const BIT_FW_DW_RDY: u16 = 1 << 14;
const BIT_DDMACH0_CHKSUM_CONT: u32 = 1 << 24;
const BIT_DDMACH0_RESET_CHKSUM_STS: u32 = 1 << 25;
const BIT_DDMACH0_CHKSUM_STS: u32 = 1 << 27;
const BIT_DDMACH0_CHKSUM_EN: u32 = 1 << 29;
const BIT_DDMACH0_OWN: u32 = 1 << 31;
const BIT_MASK_DDMACH0_DLEN: u32 = 0x3ffff;
const OCPBASE_TXBUF_88XX: u32 = 0x1878_0000;
const OCPBASE_DMEM_88XX: u32 = 0x0020_0000;
const ILLEGAL_KEY_GROUP: u32 = 0xfaaaaa00;
const HALMC_DDMA_POLLING_COUNT: u32 = 1000;
const QSEL_BEACON: u32 = 0x10;
const RSV_PG_BOUNDARY_8822C: u16 = 1938;

impl RealtekDevice {
    pub(crate) async fn initialize_monitor_jaguar3_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
        options: MonitorOptions,
        firmware_already_running: bool,
    ) -> Result<InitReport, DriverError> {
        let mut firmware_downloaded = false;
        let status = if firmware_already_running {
            InitStatus::AlreadyRunning
        } else {
            InitStatus::Initialized
        };

        let efuse_info = self.read_efuse_info_async(chip).await?;
        self.pre_init_system_cfg_8822c_async().await?;
        self.power_on_8822c_async().await?;
        self.init_system_cfg_8822c_async(radio.channel_width, chip.cut_version)
            .await?;
        if !firmware_already_running {
            self.download_firmware_8822c_async(rtl_data::RTL8822C_FW_NIC)
                .await?;
            firmware_downloaded = true;
        }
        self.init_mac_cfg_8822c_async(radio.channel_width).await?;
        self.init_usb_cfg_8822c_async().await?;
        self.enable_bb_rf_8822c_async(true).await?;
        self.load_phy_tables_8822c_async(chip, efuse_info.rfe_type)
            .await?;
        self.config_phydm_parameter_init_8822c_async().await?;
        self.init_rfk_8822c_async().await?;
        self.dac_calibrate_8822c_async().await?;
        self.bf_init_8822c_async().await?;
        self.set_channel_bwmode_8822c_async(radio.channel, radio.channel_width)
            .await?;
        self.run_iqk_8822c_async(chip, radio.channel_width, radio.channel)
            .await?;
        self.monitor_rx_cfg_8822c_async(options.accept_bad_fcs)
            .await?;
        self.enable_tx_path_8822c_async().await?;
        self.coex_wlan_only_init_8822c_async().await?;
        let _ = self.fw_set_pwr_mode_active_8822c_async().await;
        let _ = self.fw_coex_query_bt_info_8822c_async().await;
        let _ = self.fw_coex_tdma_off_8822c_async().await;

        Ok(InitReport {
            chip,
            status,
            firmware_downloaded,
        })
    }

    pub(crate) async fn shutdown_monitor_jaguar3_async(&self) -> Result<(), DriverError> {
        self.write_u16_async(REG_CR, 0x0000).await?;
        self.write_u32_async(REG_RCR, 0x0000_0000).await?;
        self.power_off_8822c_async().await
    }

    async fn pre_init_system_cfg_8822c_async(&self) -> Result<(), DriverError> {
        self.write_u8_async(REG_RSV_CTRL, 0).await?;
        if self.read_u8_async(REG_SYS_CFG2 + 3).await.unwrap_or(0) == 0x20 {
            let v = self.read_u8_async(0xfe5b).await.unwrap_or(0);
            self.write_u8_async(0xfe5b, v | BIT4 as u8).await?;
        }
        let v = self.read_u32_async(REG_PAD_CTRL1).await.unwrap_or(0) & !(BIT28 | BIT29)
            | BIT28
            | BIT29;
        self.write_u32_async(REG_PAD_CTRL1, v).await?;
        let v = self.read_u32_async(REG_LED_CFG).await.unwrap_or(0) & !(BIT25 | BIT26);
        self.write_u32_async(REG_LED_CFG, v).await?;
        let v = self.read_u32_async(REG_GPIO_MUXCFG).await.unwrap_or(0) | BIT2;
        self.write_u32_async(REG_GPIO_MUXCFG, v).await?;
        self.enable_bb_rf_8822c_async(false).await
    }

    async fn init_system_cfg_8822c_async(
        &self,
        width: ChannelWidth,
        cut: u8,
    ) -> Result<(), DriverError> {
        let v = self.read_u32_async(REG_CPU_DMEM_CON).await.unwrap_or(0) | BIT16 | BIT8;
        self.write_u32_async(REG_CPU_DMEM_CON, v).await?;
        let sys = self.read_u8_async(REG_SYS_FUNC_EN + 1).await.unwrap_or(0) | 0xd8;
        self.write_u8_async(REG_SYS_FUNC_EN + 1, sys).await?;

        let delay = match width {
            ChannelWidth::Mhz5 => 0x0e,
            ChannelWidth::Mhz10 => 0x0a,
            _ => 0x0c,
        };
        let cr_ext = (self.read_u8_async(REG_CR_EXT + 3).await.unwrap_or(0) & 0xf0) | delay;
        self.write_u8_async(REG_CR_EXT + 3, cr_ext).await?;

        let mcu = self.read_u32_async(REG_MCUFWDL).await.unwrap_or(0);
        if mcu & BIT20 != 0 {
            self.write_u32_async(REG_MCUFWDL, mcu & !BIT20).await?;
            let gpio = self.read_u32_async(REG_GPIO_MUXCFG).await.unwrap_or(0) & !BIT19;
            self.write_u32_async(REG_GPIO_MUXCFG, gpio).await?;
        }
        if cut == 1 {
            let ana = self.read_u8_async(0x1018).await.unwrap_or(0) & !0x07;
            self.write_u8_async(0x1018, ana).await?;
        }
        Ok(())
    }

    async fn enable_bb_rf_8822c_async(&self, enable: bool) -> Result<(), DriverError> {
        if enable {
            let sys = self.read_u8_async(REG_SYS_FUNC_EN).await.unwrap_or(0) | 0x03;
            self.write_u8_async(REG_SYS_FUNC_EN, sys).await?;
            let rf = self.read_u8_async(REG_RF_CTRL).await.unwrap_or(0) | 0x07;
            self.write_u8_async(REG_RF_CTRL, rf).await?;
            let wlrf = self.read_u32_async(REG_WLRF1).await.unwrap_or(0) | (0x7 << 24);
            self.write_u32_async(REG_WLRF1, wlrf).await
        } else {
            let sys = self.read_u8_async(REG_SYS_FUNC_EN).await.unwrap_or(0) & !0x03;
            self.write_u8_async(REG_SYS_FUNC_EN, sys).await?;
            let rf = self.read_u8_async(REG_RF_CTRL).await.unwrap_or(0) & !0x07;
            self.write_u8_async(REG_RF_CTRL, rf).await?;
            let wlrf = self.read_u32_async(REG_WLRF1).await.unwrap_or(0) & !(0x7 << 24);
            self.write_u32_async(REG_WLRF1, wlrf).await
        }
    }

    async fn power_on_8822c_async(&self) -> Result<(), DriverError> {
        self.power_off_8822c_async().await?;
        for step in PWR_ON_8822C_USB {
            self.run_pwr_step_8822c_async(*step, true).await?;
        }
        Ok(())
    }

    async fn power_off_8822c_async(&self) -> Result<(), DriverError> {
        for step in PWR_OFF_8822C_USB {
            self.run_pwr_step_8822c_async(*step, false).await?;
        }
        Ok(())
    }

    async fn run_pwr_step_8822c_async(
        &self,
        step: PwrStep8822c,
        fatal_poll: bool,
    ) -> Result<(), DriverError> {
        match step.cmd {
            PwrCmd8822c::Write => {
                let current = self.read_u8_async(step.offset).await.unwrap_or(0);
                self.write_u8_async(
                    step.offset,
                    (current & !step.mask) | (step.value & step.mask),
                )
                .await
            }
            PwrCmd8822c::Poll => {
                let limit = if fatal_poll { 5000 } else { 2000 };
                for _ in 0..limit {
                    let current = self.read_u8_async(step.offset).await.unwrap_or(0);
                    if current & step.mask == step.value & step.mask {
                        return Ok(());
                    }
                    sleep_micros(10).await;
                }
                if fatal_poll {
                    Err(DriverError::Nusb(format!(
                        "RTL8822C power poll 0x{:04x} mask=0x{:02x} value=0x{:02x} timed out",
                        step.offset, step.mask, step.value
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }

    async fn init_usb_cfg_8822c_async(&self) -> Result<(), DriverError> {
        let mut value = (1u8 << 1) | (0x3u8 << 2);
        if self.read_u8_async(REG_SYS_CFG2 + 3).await.unwrap_or(0) == 0x20 {
            value |= 0x0 << 4;
        } else if self.read_u8_async(REG_USB_USBSTAT).await.unwrap_or(0) & 0x3 == 0x1 {
            value |= 0x1 << 4;
        } else {
            value |= 0x2 << 4;
        }
        self.write_u8_async(REG_RXDMA_MODE_8822C, value).await?;
        let txdma = self.read_u16_async(REG_TXDMA_OFFSET_CHK).await.unwrap_or(0) | BIT9 as u16;
        self.write_u16_async(REG_TXDMA_OFFSET_CHK, txdma).await
    }

    async fn init_mac_cfg_8822c_async(&self, width: ChannelWidth) -> Result<(), DriverError> {
        self.init_trx_cfg_8822c_async().await?;
        self.init_protocol_cfg_8822c_async().await?;
        self.init_edca_cfg_8822c_async(width).await?;
        self.init_wmac_cfg_8822c_async(width).await
    }

    async fn init_trx_cfg_8822c_async(&self) -> Result<(), DriverError> {
        self.write_u16_async(REG_TRXDMA_CTRL, 0xf5a0).await?;
        let fwff = self.read_u8_async(0x0601).await.unwrap_or(0) & 0x80;
        if fwff != 0 {
            let v = self.read_u8_async(0x0601).await.unwrap_or(0) & !0x80;
            self.write_u8_async(0x0601, v).await?;
        }
        self.write_u8_async(REG_CR, 0).await?;
        self.write_u16_async(
            REG_FWFF_CTRL,
            self.read_u16_async(REG_FWFF_PKT_INFO).await.unwrap_or(0),
        )
        .await?;
        self.write_u8_async(REG_CR, 0x0f).await?;
        if fwff != 0 {
            let v = self.read_u8_async(0x0601).await.unwrap_or(0) | 0x80;
            self.write_u8_async(0x0601, v).await?;
        }
        self.write_u32_async(REG_H2CQ_CSR, BIT31).await?;
        self.priority_queue_cfg_8822c_async().await?;
        self.init_h2c_8822c_async().await
    }

    async fn priority_queue_cfg_8822c_async(&self) -> Result<(), DriverError> {
        const TX_FIFO_PAGES: u16 = 2048;
        const RSVD_CSIBUF: u16 = 1998;
        const RSVD_H2CQ: u16 = 1986;
        const PUB_PG: u16 = 1745;

        self.write_u16_async(REG_FIFOPAGE_INFO_1, 64).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_2, 64).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_3, 64).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_4, 0).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_5, PUB_PG).await?;
        self.write_u32_async(
            REG_RQPN_CTRL_2,
            self.read_u32_async(REG_RQPN_CTRL_2).await.unwrap_or(0) | BIT31,
        )
        .await?;
        self.write_u16_async(REG_FIFOPAGE_CTRL_2, RSV_PG_BOUNDARY_8822C)
            .await?;
        self.write_u16_async(REG_WMAC_CSIDMA_CFG, RSVD_CSIBUF)
            .await?;
        let fwhw = self.read_u8_async(REG_FWHW_TXQ_CTRL + 2).await.unwrap_or(0) | BIT4 as u8;
        self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, fwhw).await?;
        self.write_u16_async(REG_BCNQ_BDNY, RSV_PG_BOUNDARY_8822C)
            .await?;
        self.write_u16_async(REG_FIFOPAGE_CTRL_2 + 2, RSV_PG_BOUNDARY_8822C)
            .await?;
        self.write_u16_async(REG_BCNQ1_BDNY_V1, RSV_PG_BOUNDARY_8822C)
            .await?;
        self.write_u32_async(REG_RXFF_PTR_8814, 24576 - 256 - 1)
            .await?;
        let auto = (self.read_u8_async(REG_TDECTRL).await.unwrap_or(0) & !(0xf << 4)) | (3 << 4);
        self.write_u8_async(REG_TDECTRL, auto).await?;
        self.write_u8_async(REG_TDECTRL + 3, 3).await?;
        let txdma = self
            .read_u8_async(REG_TXDMA_OFFSET_CHK + 1)
            .await
            .unwrap_or(0)
            | BIT1 as u8;
        self.write_u8_async(REG_TXDMA_OFFSET_CHK + 1, txdma).await?;
        self.write_u8_async(
            REG_TDECTRL,
            self.read_u8_async(REG_TDECTRL).await.unwrap_or(0) | BIT0 as u8,
        )
        .await?;
        for _ in 0..1000 {
            if self.read_u8_async(REG_TDECTRL).await.unwrap_or(0) & BIT0 as u8 == 0 {
                self.write_u8_async(REG_CR + 3, 0).await?;
                return Ok(());
            }
            sleep_micros(10).await;
        }
        Err(DriverError::Nusb(format!(
            "RTL8822C auto LLT did not complete, tx_fifo_pages={TX_FIFO_PAGES}, h2cq={RSVD_H2CQ}"
        )))
    }

    async fn init_h2c_8822c_async(&self) -> Result<(), DriverError> {
        let h2cq_addr = u32::from(RSV_PG_BOUNDARY_8822C + 48) << 7;
        let h2cq_size = 8u32 << 7;
        let v = (self.read_u32_async(REG_H2C_HEAD).await.unwrap_or(0) & 0xfffc_0000) | h2cq_addr;
        self.write_u32_async(REG_H2C_HEAD, v).await?;
        let v =
            (self.read_u32_async(REG_H2C_READ_ADDR).await.unwrap_or(0) & 0xfffc_0000) | h2cq_addr;
        self.write_u32_async(REG_H2C_READ_ADDR, v).await?;
        let v = (self.read_u32_async(REG_H2C_TAIL).await.unwrap_or(0) & 0xfffc_0000)
            | (h2cq_addr + h2cq_size);
        self.write_u32_async(REG_H2C_TAIL, v).await?;
        let v = (self.read_u8_async(REG_H2C_INFO).await.unwrap_or(0) & 0xfc) | 0x01;
        self.write_u8_async(REG_H2C_INFO, v).await?;
        let v = (self.read_u8_async(REG_H2C_INFO).await.unwrap_or(0) & 0xfb) | 0x04;
        self.write_u8_async(REG_H2C_INFO, v).await?;
        let v = (self
            .read_u8_async(REG_TXDMA_OFFSET_CHK + 1)
            .await
            .unwrap_or(0)
            & 0x7f)
            | 0x80;
        self.write_u8_async(REG_TXDMA_OFFSET_CHK + 1, v).await
    }

    async fn init_protocol_cfg_8822c_async(&self) -> Result<(), DriverError> {
        let fwhw = self.read_u8_async(REG_FWHW_TXQ_CTRL).await.unwrap_or(0) | 0x80;
        self.write_u8_async(REG_FWHW_TXQ_CTRL, fwhw & !0x06).await?;
        self.write_u8_async(REG_FWHW_TXQ_CTRL + 1, 0x1f).await?;
        self.init_sifs_ctrl_8822c_async(ChannelWidth::Mhz20).await?;
        self.write_u32_async(REG_DARFRC, 0x0100_0000).await?;
        self.write_u32_async(REG_DARFRCH, 0x0807_0504).await?;
        self.write_u32_async(REG_RARFRCH, 0x0807_0504).await?;
        self.write_u32_async(REG_ARFR0, 0xfe01_f010).await?;
        self.write_u32_async(REG_ARFRH0, 0x4000_0000).await?;
        self.write_u32_async(REG_ARFR1_V1, 0x003f_f010).await?;
        self.write_u32_async(REG_ARFRH1_V1, 0x4000_0000).await?;
        self.write_u32_async(REG_ARFR4, 0x0600_f010).await?;
        self.write_u32_async(REG_ARFRH4, 0x4000_03e0).await?;
        self.write_u32_async(REG_ARFR5, 0x0600_f015).await?;
        self.write_u32_async(REG_ARFRH5, 0x0000_00e0).await?;
        self.write_u8_async(REG_AMPDU_MAX_TIME_V1, 0x70).await?;
        self.write_u8_async(
            REG_TX_HANG_CTRL,
            self.read_u8_async(REG_TX_HANG_CTRL).await.unwrap_or(0) | 0x04,
        )
        .await?;
        self.write_u8_async(REG_PRECNT_CTRL, 0xe4).await?;
        self.write_u8_async(REG_PRECNT_CTRL + 1, 0x09).await?;
        self.write_u32_async(REG_PROT_MODE_CTRL, 0x203f_08ff)
            .await?;
        self.write_u16_async(REG_BAR_MODE_CTRL + 2, 0x0801).await?;
        self.write_u8_async(0x1448, 0x06).await?;
        self.write_u8_async(0x144a, 0x06).await?;
        self.write_u8_async(0x144c, 0x06).await?;
        self.write_u8_async(0x144e, 0x06).await?;
        self.write_u8_async(
            REG_LIFETIME_EN,
            self.read_u8_async(REG_LIFETIME_EN).await.unwrap_or(0) & !(BIT5 as u8),
        )
        .await?;
        let v = self.read_u32_async(0x1428).await.unwrap_or(0) & !BIT29 | BIT28;
        self.write_u32_async(0x1428, v).await?;
        let v = self.read_u32_async(0x142c).await.unwrap_or(0) & !BIT29 | BIT28;
        self.write_u32_async(0x142c, v).await?;
        let v = self.read_u32_async(0x1430).await.unwrap_or(0) & !BIT0 & !BIT1;
        self.write_u32_async(0x1430, v).await?;
        let v = self.read_u32_async(REG_RRSR).await.unwrap_or(0) & !(0x3 << 21);
        self.write_u32_async(REG_RRSR, v).await?;
        self.write_u8_async(
            REG_INIRTS_RATE_SEL,
            self.read_u8_async(REG_INIRTS_RATE_SEL).await.unwrap_or(0) | BIT5 as u8,
        )
        .await
    }

    async fn init_sifs_ctrl_8822c_async(&self, width: ChannelWidth) -> Result<(), DriverError> {
        match width {
            ChannelWidth::Mhz5 => {
                self.write_u16_async(REG_RESP_SIFS_OFDM, 0x3838).await?;
                self.write_u16_async(REG_SPEC_SIFS, 0x400a).await?;
                self.write_u32_async(REG_SIFS_CTX, 0x400a_380a).await?;
                self.write_u16_async(REG_MAC_SPEC_SIFS + 1, 0x40).await?;
            }
            ChannelWidth::Mhz10 => {
                self.write_u16_async(REG_RESP_SIFS_OFDM, 0x1c1c).await?;
                self.write_u16_async(REG_SPEC_SIFS, 0x200a).await?;
                self.write_u32_async(REG_SIFS_CTX, 0x200a_1c0a).await?;
                self.write_u16_async(REG_MAC_SPEC_SIFS + 1, 0x20).await?;
            }
            _ => {
                self.write_u16_async(REG_RESP_SIFS_OFDM, 0x0e0e).await?;
                self.write_u16_async(REG_SPEC_SIFS, 0x100a).await?;
                self.write_u32_async(REG_SIFS_CTX, 0x100a_0e0a).await?;
            }
        }
        self.write_u16_async(REG_RESP_SIFS_CCK, 0x0a0a).await
    }

    async fn init_edca_cfg_8822c_async(&self, width: ChannelWidth) -> Result<(), DriverError> {
        match width {
            ChannelWidth::Mhz5 => {
                self.write_u8_async(REG_PIFS + 9, 0x15).await?;
                self.write_u8_async(REG_PIFS, 0x55).await?;
                self.write_u32_async(REG_TBTT_PROHIBIT, 0x0001_900f).await?;
                self.write_u32_async(REG_EDCA_VO_PARAM, 0x002f_a27a).await?;
                self.write_u32_async(REG_EDCA_VI_PARAM, 0x005e_a382).await?;
                self.write_u32_async(REG_EDCA_BE_PARAM, 0x005e_a48e).await?;
                self.write_u32_async(REG_EDCA_BK_PARAM, 0x0000_a4d3).await?;
                self.write_u8_async(REG_CPUMGQ_PARAMETER, 0x72).await?;
            }
            ChannelWidth::Mhz10 => {
                self.write_u8_async(REG_PIFS + 9, 0x0d).await?;
                self.write_u8_async(REG_PIFS, 0x2d).await?;
                self.write_u32_async(REG_TBTT_PROHIBIT, 0x0000_c808).await?;
                self.write_u32_async(REG_EDCA_VO_PARAM, 0x002f_a242).await?;
                self.write_u32_async(REG_EDCA_VI_PARAM, 0x005e_a346).await?;
                self.write_u32_async(REG_EDCA_BE_PARAM, 0x005e_a44c).await?;
                self.write_u32_async(REG_EDCA_BK_PARAM, 0x0000_a47b).await?;
                self.write_u8_async(REG_CPUMGQ_PARAMETER, 0x3e).await?;
            }
            _ => {
                self.write_u8_async(REG_PIFS + 9, 0x09).await?;
                self.write_u8_async(REG_PIFS, 0x1c).await?;
                self.write_u32_async(REG_TBTT_PROHIBIT, 0x6404).await?;
                self.write_u32_async(REG_EDCA_VO_PARAM, 0x002f_a226).await?;
                self.write_u32_async(REG_EDCA_VI_PARAM, 0x005e_a328).await?;
                self.write_u32_async(REG_EDCA_BE_PARAM, 0x005e_a42b).await?;
                self.write_u32_async(REG_EDCA_BK_PARAM, 0x0000_a44f).await?;
            }
        }
        self.write_u8_async(
            REG_TX_PTCL_CTRL + 1,
            self.read_u8_async(REG_TX_PTCL_CTRL + 1).await.unwrap_or(0) & !BIT4 as u8,
        )
        .await?;
        self.write_u8_async(
            REG_RD_CTRL + 1,
            self.read_u8_async(REG_RD_CTRL + 1).await.unwrap_or(0) | 0x07,
        )
        .await?;
        self.write_u32_async(
            REG_AFE_CTRL1,
            self.read_u32_async(REG_AFE_CTRL1).await.unwrap_or(0) & !(0x3 << 20),
        )
        .await?;
        self.write_u8_async(REG_USTIME_TSF, 80).await?;
        self.write_u8_async(REG_USTIME_EDCA, 80).await?;
        self.write_u8_async(0x0577, self.read_u8_async(0x0577).await.unwrap_or(0) | 0x0b)
            .await?;
        self.write_u8_async(
            REG_TIMER0_SRC_SEL,
            self.read_u8_async(REG_TIMER0_SRC_SEL).await.unwrap_or(0) & !0x70,
        )
        .await?;
        self.write_u16_async(REG_TX_PTCL_CTRL + 2, 0).await?;
        self.write_u32_async(REG_RD_NAV_NXT, 0x001b_0005).await?;
        self.write_u16_async(REG_RXTSF_OFFSET_CCK, 0x3030).await?;
        self.write_u8_async(
            REG_BCN_CTRL,
            self.read_u8_async(REG_BCN_CTRL).await.unwrap_or(0) | BIT3 as u8,
        )
        .await?;
        self.write_u8_async(REG_DRVERLYINT, 0x04).await?;
        self.write_u8_async(REG_BCN_CTRL_CLINT0, 0x10).await?;
        self.write_u8_async(REG_BCNDMATIM, 0x02).await?;
        self.write_u8_async(REG_BCN_MAX_ERR, 0xff).await?;
        self.write_u8_async(
            REG_BAR_MODE_CTRL + 4,
            self.read_u8_async(REG_BAR_MODE_CTRL + 4).await.unwrap_or(0) | 0x01,
        )
        .await
    }

    async fn init_wmac_cfg_8822c_async(&self, width: ChannelWidth) -> Result<(), DriverError> {
        match width {
            ChannelWidth::Mhz5 => {
                self.write_u8_async(REG_ACKTO, 0x75).await?;
                self.write_u8_async(REG_ACKTO + 1, 0x50).await?;
                self.write_u16_async(REG_ACKTO + 2, 0x00e2).await?;
            }
            ChannelWidth::Mhz10 => {
                self.write_u8_async(REG_ACKTO, 0x3d).await?;
                self.write_u8_async(REG_ACKTO + 1, 0x28).await?;
                self.write_u16_async(REG_ACKTO + 2, 0x0076).await?;
            }
            _ => {
                self.write_u8_async(REG_ACKTO, 0x21).await?;
                self.write_u16_async(REG_ACKTO + 2, 0x0040).await?;
            }
        }
        self.write_u32_async(REG_MAR, 0xffff_ffff).await?;
        self.write_u32_async(REG_MAR + 4, 0xffff_ffff).await?;
        self.write_u8_async(REG_BBPSF_CTRL + 2, 0x84).await?;
        self.write_u8_async(REG_ACKTO_CCK, 0x6a).await?;
        self.write_u8_async(REG_NAV_UPPER, 0xc8).await?;
        self.write_u8_async(
            0x066c,
            self.read_u8_async(0x066c).await.unwrap_or(0) | BIT1 as u8,
        )
        .await?;
        self.write_u8_async(0x066e, 0x05).await?;
        self.write_u32_async(REG_RXFLTMAP0, 0xffff_ffff).await?;
        self.write_u16_async(REG_RXFLTMAP2, 0xffff).await?;
        self.write_u32_async(REG_RCR, 0xe410_220e).await?;
        self.write_u8_async(
            REG_RXPSF_CTRL + 2,
            self.read_u8_async(REG_RXPSF_CTRL + 2).await.unwrap_or(0) | 0x0e,
        )
        .await?;
        self.write_u8_async(REG_RX_PKT_LIMIT, 24).await?;
        self.write_u8_async(REG_TCR + 2, 0x30).await?;
        self.write_u8_async(REG_TCR + 1, 0x30).await?;
        self.write_u16_async(
            REG_GENERAL_OPTION,
            self.read_u16_async(REG_GENERAL_OPTION).await.unwrap_or(0) | BIT9 as u16 | BIT8 as u16,
        )
        .await?;
        self.write_u8_async(
            REG_SND_PTCL_CTRL,
            self.read_u8_async(REG_SND_PTCL_CTRL).await.unwrap_or(0) | BIT6 as u8,
        )
        .await?;
        self.write_u32_async(REG_WMAC_OPTION_FUNCTION_2, 0xb181_0041)
            .await?;
        self.write_u8_async(REG_WMAC_OPTION_FUNCTION_1, 0x98).await
    }

    async fn load_phy_tables_8822c_async(
        &self,
        chip: ChipInfo,
        rfe_type: u8,
    ) -> Result<(), DriverError> {
        let ctx = Jaguar3TableContext {
            cut_version: chip.cut_version,
            rfe_type,
        };
        load_jaguar3_table_async(rtl_data::RTL8822C_PHY_REG, ctx, |addr, value| async move {
            self.write_bb_8822c_async(addr, value).await
        })
        .await?;
        load_jaguar3_table_async(rtl_data::RTL8822C_AGC_TAB, ctx, |addr, value| async move {
            self.write_bb_8822c_async(addr, value).await
        })
        .await?;
        self.set_bb_reg_async(0x1c90, BIT8, 0).await?;
        load_jaguar3_table_async(rtl_data::RTL8822C_RADIO_A, ctx, |addr, value| async move {
            self.write_rf_direct_8822c_async(0x3c00, addr, value).await
        })
        .await?;
        load_jaguar3_table_async(rtl_data::RTL8822C_RADIO_B, ctx, |addr, value| async move {
            self.write_rf_direct_8822c_async(0x4c00, addr, value).await
        })
        .await?;
        self.set_bb_reg_async(0x1c90, BIT8, 1).await?;
        self.set_bb_reg_async(0x1830, BIT29, 1).await?;
        self.set_bb_reg_async(0x4130, BIT29, 1).await
    }

    async fn write_bb_8822c_async(&self, addr: u32, value: u32) -> Result<(), DriverError> {
        match addr {
            0xfe => sleep_ms(50).await,
            0xfd => sleep_ms(5).await,
            0xfc => sleep_ms(1).await,
            0xfb => sleep_micros(50).await,
            0xfa => sleep_micros(5).await,
            0xf9 => sleep_micros(1).await,
            _ => self.set_bb_reg_async(addr as u16, MASKDWORD, value).await?,
        }
        Ok(())
    }

    async fn write_rf_direct_8822c_async(
        &self,
        base: u16,
        addr: u32,
        value: u32,
    ) -> Result<(), DriverError> {
        match addr {
            0xffe => sleep_ms(50).await,
            0xfe => sleep_micros(100).await,
            0xffff => sleep_micros(1).await,
            _ => {
                let register = base + (((addr & 0xff) as u16) << 2);
                self.set_bb_reg_async(register, RFREG_MASK, value).await?;
            }
        }
        Ok(())
    }

    async fn config_phydm_parameter_init_8822c_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x180c, 0x3, 0x3).await?;
        self.set_bb_reg_async(0x180c, BIT28, 1).await?;
        self.set_bb_reg_async(0x410c, 0x3, 0x3).await?;
        self.set_bb_reg_async(0x410c, BIT28, 1).await?;
        self.set_bb_reg_async(0x1c3c, 0x3, 0x3).await?;
        self.bb_reset_8822c_async().await
    }

    async fn init_rfk_8822c_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x1cd0, BIT28, 1).await?;
        self.set_bb_reg_async(0x1cd0, BIT29, 1).await?;
        self.set_bb_reg_async(0x1cd0, BIT30, 1).await?;
        self.set_bb_reg_async(0x1cd0, BIT31, 0).await?;
        for pair in rtl_data::RTL8822C_CAL_INIT.chunks_exact(2) {
            self.write_bb_8822c_async(pair[0], pair[1]).await?;
        }
        Ok(())
    }

    async fn bf_init_8822c_async(&self) -> Result<(), DriverError> {
        let mut value = self.read_u32_async(REG_MU_TX_CTL).await.unwrap_or(0);
        value |= BIT16;
        value = (value & !(0xf << 12)) | (0x0a << 12);
        value &= !BIT7;
        value &= !0x3f;
        self.write_u32_async(REG_MU_TX_CTL, value).await?;
        self.write_u8_async(REG_MU_BF_OPTION, (3 << 4) | (1 << 6))
            .await?;
        self.write_u16_async(REG_WMAC_MU_BF_CTL, 0).await?;
        self.write_u8_async(
            REG_TXBF_CTRL + 3,
            self.read_u8_async(REG_TXBF_CTRL + 3).await.unwrap_or(0) | 0x40,
        )
        .await?;
        self.write_u8_async(REG_NDPA_OPT_CTRL, 0x10).await?;
        self.write_u8_async(
            0x06df,
            (self.read_u8_async(0x06df).await.unwrap_or(0) & 0xc0) | 0x04,
        )
        .await
    }

    async fn set_channel_bwmode_8822c_async(
        &self,
        channel: u8,
        width: ChannelWidth,
    ) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x0810, 0x3ff0, 0x19b).await?;
        self.set_bb_reg_async(0x09b0, 0xffc0, 0x0).await?;
        self.set_bb_reg_async(0x09b4, 0x0000_0700, 0x7).await?;
        self.set_bb_reg_async(0x09b4, 0x0070_0000, 0x6).await?;
        self.set_bb_reg_async(0x09b0, 0xf, 0x0).await?;
        self.set_bb_reg_async(0x0cbc, BIT21, 0x0).await?;
        self.set_bb_reg_async(0x1abc, BIT30, 0x0).await?;
        self.set_bb_reg_async(0x1ae8, BIT31, 0x1).await?;
        self.set_bb_reg_async(0x1aec, 0xf, 0x6).await?;
        self.set_bb_reg_async(0x088c, 0xf000, 0x1).await?;

        let mut rf18 = u32::from(channel) | 0x3000;
        if channel > 14 {
            rf18 |= BIT16 | BIT8;
            if channel > 144 {
                rf18 |= BIT18;
            } else if channel >= 80 {
                rf18 |= BIT17;
            }
        }
        self.write_rf_direct_8822c_async(0x3c00, 0x18, rf18).await?;
        self.write_rf_direct_8822c_async(0x4c00, 0x18, rf18).await?;
        self.write_rf_direct_8822c_async(0x3c00, 0x3f, 0x18).await?;
        self.write_rf_direct_8822c_async(0x4c00, 0x3f, 0x18).await?;
        self.set_bb_reg_async(0x3f7c, BIT18, if channel <= 14 { 1 } else { 0 })
            .await?;

        if channel <= 14 {
            self.write_u8_async(
                0x0454,
                self.read_u8_async(0x0454).await.unwrap_or(0) & !0x80,
            )
            .await?;
            self.set_bb_reg_async(0x1a80, BIT18, 0).await?;
            self.set_bb_reg_async(0x1c80, 0x3f00_0000, 0x0f).await?;
        } else {
            self.write_u8_async(0x0454, self.read_u8_async(0x0454).await.unwrap_or(0) | 0x80)
                .await?;
            self.set_bb_reg_async(0x1a80, BIT18, 1).await?;
            self.set_bb_reg_async(0x1c80, 0x3f00_0000, 0x22).await?;
        }

        self.set_bb_reg_async(0x0c30, 0xfff, sco_value_8822c(channel))
            .await?;
        if channel <= 14 {
            self.set_bb_reg_async(0x0808, 0x0070_0000, if channel == 11 { 0x3 } else { 0x1 })
                .await?;
            self.set_bb_reg_async(0x0808, 0x70, if channel == 13 { 0x3 } else { 0x1 })
                .await?;
        } else {
            self.set_bb_reg_async(0x0808, 0x0070_0000, 0x1).await?;
            self.set_bb_reg_async(0x0808, 0x70, 0x3).await?;
        }
        self.bb_reset_8822c_async().await?;
        if matches!(width, ChannelWidth::Mhz5 | ChannelWidth::Mhz10) {
            self.set_bandwidth_dividers_8822c_async(width).await?;
        }
        Ok(())
    }

    async fn set_bandwidth_dividers_8822c_async(
        &self,
        width: ChannelWidth,
    ) -> Result<(), DriverError> {
        let (small_bw, dac, adc, dfir) = match width {
            ChannelWidth::Mhz10 => (0x2, 0x6, 0x5, 0x2ab),
            ChannelWidth::Mhz5 => (0x1, 0x4, 0x4, 0x2ab),
            _ => (0x0, 0x7, 0x6, 0x19b),
        };
        self.set_bb_reg_async(0x0810, 0x3ff0, dfir).await?;
        self.set_bb_reg_async(0x09b0, 0xc0, small_bw).await?;
        self.set_bb_reg_async(0x09b4, 0x0000_0700, dac).await?;
        self.set_bb_reg_async(0x09b4, 0x0070_0000, adc).await?;
        self.bb_reset_8822c_async().await
    }

    async fn monitor_rx_cfg_8822c_async(&self, accept_bad_fcs: bool) -> Result<(), DriverError> {
        self.write_u16_async(REG_CR, 0x06ff).await?;
        let mut rcr = 0xf410_400e | BIT28;
        if !accept_bad_fcs {
            rcr &= !(RCR_ACRC32 | RCR_AICV);
        }
        self.write_u32_async(REG_RCR, rcr).await?;
        self.write_u8_async(REG_RX_DRVINFO_SZ, 0x04).await?;
        self.write_u16_async(REG_RXFLTMAP0, 0xffff).await?;
        self.write_u16_async(REG_RXFLTMAP1, 0xffff).await?;
        self.write_u16_async(REG_RXFLTMAP2, 0xffff).await
    }

    async fn enable_tx_path_8822c_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x1c3c, 0x0000_0003, 0x3).await?;
        self.set_bb_reg_async(0x1c80, 0x3f00_0000, 0x22).await?;
        self.set_bb_reg_async(0x1c90, 0x0000_8000, 0x0).await?;
        self.set_bb_reg_async(0x1cd0, 0x7000_0000, 0x7).await
    }

    /// Re-assert the Jaguar3 WiFi-only coex/PTA state and firmware keepalives.
    ///
    /// Devourer runs this kind of work periodically while transmitting so the
    /// firmware coexistence state does not steal the antenna from WLAN during
    /// long sessions. Callers can invoke this from their normal RX/TX loop; no
    /// dedicated thread is required by the driver.
    pub async fn run_jaguar3_coex_keepalive_async(&self) -> Result<(), DriverError> {
        self.coex_run_5g_8822c_async().await?;
        self.fw_update_wl_phy_info_8822c_async().await?;
        self.fw_set_pwr_mode_active_8822c_async().await?;
        self.fw_coex_query_bt_info_8822c_async().await
    }

    async fn coex_wlan_only_init_8822c_async(&self) -> Result<(), DriverError> {
        self.write_u8_mask_8822c_async(REG_BCN_CTRL, 0x08, 1)
            .await?;
        self.write_u8_mask_8822c_async(0x0790, 0x3f, 0x05).await?;
        self.write_u8_async(0x0778, 0x01).await?;
        self.write_u32_async(
            REG_GPIO_MUXCFG,
            self.read_u32_async(REG_GPIO_MUXCFG).await.unwrap_or(0) | BIT5 | BIT9,
        )
        .await?;
        self.write_u8_mask_8822c_async(REG_QUEUE_CTRL, 0x10, 1)
            .await?;
        self.write_u8_mask_8822c_async(REG_QUEUE_CTRL, 0x20, 0)
            .await?;
        self.write_u16_async(
            0x0762,
            self.read_u16_async(0x0762).await.unwrap_or(0) | BIT12 as u16,
        )
        .await?;
        self.write_u8_mask_8822c_async(0x04fc, 0x03, 0).await?;

        self.btc_write_indirect_8822c_async(0x38, 0x80, 0x0).await?;
        self.btc_write_indirect_8822c_async(0xa0, 0xffff, 0xffff)
            .await?;
        self.btc_write_indirect_8822c_async(0xa4, 0xffff, 0xffff)
            .await?;

        self.write_rf_direct_8822c_async(0x4c00, 0x01, 0x42000)
            .await?;
        self.write_u8_mask_8822c_async(0x1c32, 0x40, 1).await?;
        self.write_u8_mask_8822c_async(0x1c39, 0x10, 0).await?;
        self.write_u8_mask_8822c_async(0x1c3b, 0x10, 1).await?;
        self.write_u8_mask_8822c_async(0x4160, 0x08, 1).await?;
        self.write_u8_mask_8822c_async(0x1860, 0x08, 0).await?;
        self.write_u8_mask_8822c_async(0x1ca7, 0x08, 1).await?;
        self.force_wl_antenna_8822c_async().await
    }

    async fn coex_run_5g_8822c_async(&self) -> Result<(), DriverError> {
        self.btc_write_indirect_8822c_async(0x38, 0xc000, 0x0)
            .await?;
        self.btc_write_indirect_8822c_async(0x38, 0x0c00, 0x0)
            .await?;
        self.btc_write_indirect_8822c_async(0x38, 0x3000, 0x3)
            .await?;
        self.btc_write_indirect_8822c_async(0x38, 0x0300, 0x3)
            .await?;
        self.write_u8_async(0x0073, self.read_u8_async(0x0073).await.unwrap_or(0) | 0x04)
            .await?;
        self.write_u32_async(0x06c0, 0xffff_ffff).await?;
        self.write_u32_async(0x06c4, 0xffff_ffff).await?;
        self.write_u32_async(0x06c8, 0xf0ff_ffff).await?;
        self.write_u16_async(0x00aa, 0x8003).await
    }

    async fn force_wl_antenna_8822c_async(&self) -> Result<(), DriverError> {
        self.write_u16_async(0x00aa, 0x8003).await?;
        self.btc_write_indirect_8822c_async(0x38, 0xff00, 0x77)
            .await?;
        self.write_u8_async(0x0073, self.read_u8_async(0x0073).await.unwrap_or(0) | 0x04)
            .await
    }

    async fn fw_update_wl_phy_info_8822c_async(&self) -> Result<(), DriverError> {
        const TX_THROUGHPUT_MBPS: u32 = 100;
        self.send_h2c_raw_8822c_async(0x58 | ((TX_THROUGHPUT_MBPS & 0x3ff) << 8), 0)
            .await
    }

    async fn fw_set_pwr_mode_active_8822c_async(&self) -> Result<(), DriverError> {
        self.send_h2c_raw_8822c_async(0x20 | (1 << 24), 0x0c << 8)
            .await
    }

    async fn fw_coex_query_bt_info_8822c_async(&self) -> Result<(), DriverError> {
        self.send_h2c_raw_8822c_async(0x61 | (1 << 8), 0).await
    }

    async fn fw_coex_tdma_off_8822c_async(&self) -> Result<(), DriverError> {
        self.send_h2c_raw_8822c_async(0x60, 0).await
    }

    async fn send_h2c_raw_8822c_async(&self, msg: u32, msg_ext: u32) -> Result<(), DriverError> {
        let box_reg = 0x01d0;
        let box_ex_reg = 0x01f0;
        for _ in 0..30 {
            if self.read_u8_async(REG_HMETFR).await.unwrap_or(0) & 0x01 == 0 {
                break;
            }
            sleep_micros(100).await;
        }
        self.write_u32_async(box_ex_reg, msg_ext).await?;
        self.write_u32_async(box_reg, msg).await
    }

    async fn btc_wait_ready_8822c_async(&self) -> Result<(), DriverError> {
        for _ in 0..10 {
            if self.read_u8_async(0x1703).await.unwrap_or(0) & BIT5 as u8 != 0 {
                break;
            }
            sleep_ms(10).await;
        }
        Ok(())
    }

    async fn btc_read_indirect_8822c_async(&self, register: u16) -> Result<u32, DriverError> {
        self.btc_wait_ready_8822c_async().await?;
        self.write_u32_async(0x1700, 0x800f_0000 | register as u32)
            .await?;
        self.read_u32_async(0x1708).await
    }

    async fn btc_write_indirect_8822c_async(
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
            let shift = mask_shift(mask);
            let current = self.btc_read_indirect_8822c_async(register).await?;
            (current & !mask) | ((value << shift) & mask)
        };
        self.btc_wait_ready_8822c_async().await?;
        self.write_u32_async(0x1704, value).await?;
        self.write_u32_async(0x1700, 0xc00f_0000 | register as u32)
            .await
    }

    async fn write_u8_mask_8822c_async(
        &self,
        register: u16,
        mask: u8,
        value: u8,
    ) -> Result<(), DriverError> {
        if mask == 0 {
            return Ok(());
        }
        let shift = mask.trailing_zeros() as u8;
        let current = self.read_u8_async(register).await.unwrap_or(0);
        self.write_u8_async(register, (current & !mask) | ((value << shift) & mask))
            .await
    }

    async fn bb_reset_8822c_async(&self) -> Result<(), DriverError> {
        let v = self.read_u32_async(0x0000).await.unwrap_or(0);
        self.write_u32_async(0x0000, v | BIT16).await?;
        self.write_u32_async(0x0000, v & !BIT16).await?;
        self.write_u32_async(0x0000, v | BIT16).await
    }

    async fn download_firmware_8822c_async(&self, firmware: &[u8]) -> Result<(), DriverError> {
        validate_8822c_firmware_size(firmware)?;
        self.wlan_cpu_en_8822c_async(false).await?;

        let backups = [
            BackupReg::U8(
                REG_TRXDMA_CTRL + 1,
                self.read_u8_async(REG_TRXDMA_CTRL + 1).await.unwrap_or(0) as u32,
            ),
            BackupReg::U8(REG_CR, self.read_u8_async(REG_CR).await.unwrap_or(0) as u32),
            BackupReg::U32(REG_H2CQ_CSR, BIT31),
            BackupReg::U16(
                REG_FIFOPAGE_INFO_1,
                self.read_u16_async(REG_FIFOPAGE_INFO_1).await.unwrap_or(0) as u32,
            ),
            BackupReg::U32(
                REG_RQPN_CTRL_2,
                self.read_u32_async(REG_RQPN_CTRL_2).await.unwrap_or(0) | BIT31,
            ),
            BackupReg::U8(
                REG_BCN_CTRL,
                self.read_u8_async(REG_BCN_CTRL).await.unwrap_or(0) as u32,
            ),
        ];

        self.write_u8_async(REG_TRXDMA_CTRL + 1, 3 << 6).await?;
        self.write_u8_async(REG_CR, (BIT0 | BIT2) as u8).await?;
        self.write_u32_async(REG_H2CQ_CSR, BIT31).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_1, 0x0200).await?;
        if let BackupReg::U32(_, value) = backups[4] {
            self.write_u32_async(REG_RQPN_CTRL_2, value).await?;
        }
        if let BackupReg::U8(_, bcn) = backups[5] {
            self.write_u8_async(REG_BCN_CTRL, ((bcn as u8) & !(BIT3 as u8)) | BIT4 as u8)
                .await?;
        }
        self.pltfm_reset_8822c_async().await?;

        let result = self.start_dlfw_8822c_async(firmware).await;
        for backup in backups {
            self.restore_backup_8822c_async(backup).await?;
        }
        result?;
        self.dlfw_end_flow_8822c_async().await
    }

    async fn start_dlfw_8822c_async(&self, firmware: &[u8]) -> Result<(), DriverError> {
        let dmem = le32(&firmware[WLAN_FW_HDR_DMEM_SIZE..]) + WLAN_FW_HDR_CHKSUM_SIZE;
        let imem = le32(&firmware[WLAN_FW_HDR_IMEM_SIZE..]) + WLAN_FW_HDR_CHKSUM_SIZE;
        let emem = if firmware[WLAN_FW_HDR_MEM_USAGE] & BIT4 as u8 != 0 {
            le32(&firmware[WLAN_FW_HDR_EMEM_SIZE..]) + WLAN_FW_HDR_CHKSUM_SIZE
        } else {
            0
        };

        let fw_ctrl = (self.read_u16_async(REG_MCUFWDL).await.unwrap_or(0) & 0x3800) | 0x0001;
        self.write_u16_async(REG_MCUFWDL, fw_ctrl).await?;

        let mut cur = WLAN_FW_HDR_SIZE;
        let dmem_addr = le32(&firmware[WLAN_FW_HDR_DMEM_ADDR..]) & !BIT31;
        self.dlfw_to_mem_8822c_async(&firmware[cur..cur + dmem as usize], 0, dmem_addr)
            .await?;
        cur += dmem as usize;

        let imem_addr = le32(&firmware[WLAN_FW_HDR_IMEM_ADDR..]) & !BIT31;
        self.dlfw_to_mem_8822c_async(&firmware[cur..cur + imem as usize], 0, imem_addr)
            .await?;
        cur += imem as usize;

        if emem != 0 {
            let emem_addr = le32(&firmware[WLAN_FW_HDR_EMEM_ADDR..]) & !BIT31;
            self.dlfw_to_mem_8822c_async(&firmware[cur..cur + emem as usize], 0, emem_addr)
                .await?;
        }
        Ok(())
    }

    async fn dlfw_to_mem_8822c_async(
        &self,
        data: &[u8],
        src: u32,
        dest: u32,
    ) -> Result<(), DriverError> {
        self.write_u32_async(REG_DDMA_CH0CTRL, BIT_DDMACH0_RESET_CHKSUM_STS)
            .await?;
        let mut offset = 0usize;
        let mut first = true;
        while offset < data.len() {
            let end = (offset + 4096).min(data.len());
            let chunk = &data[offset..end];
            self.send_fw_page_8822c_async((src >> 7) as u16, chunk)
                .await?;
            self.iddma_dlfw_8822c_async(
                OCPBASE_TXBUF_88XX + src + TX_DESC_SIZE_8822C as u32,
                dest + offset as u32,
                chunk.len() as u32,
                first,
            )
            .await?;
            first = false;
            offset = end;
        }
        self.check_fw_chksum_8822c_async(dest).await
    }

    async fn send_fw_page_8822c_async(
        &self,
        page_addr: u16,
        chunk: &[u8],
    ) -> Result<(), DriverError> {
        self.write_u16_async(REG_FIFOPAGE_CTRL_2, (page_addr & 0x0fff) | BIT15 as u16)
            .await?;
        let cr1 = self.read_u8_async(REG_CR + 1).await.unwrap_or(0);
        self.write_u8_async(REG_CR + 1, cr1 | BIT0 as u8).await?;
        let txq2 = self.read_u8_async(REG_FWHW_TXQ_CTRL + 2).await.unwrap_or(0);
        self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, txq2 & !(BIT6 as u8))
            .await?;

        let frame = build_fw_page_frame_8822c(chunk)?;
        self.write_tx_transfer_raw_async(&frame).await?;
        for _ in 0..1000 {
            if self
                .read_u8_async(REG_FIFOPAGE_CTRL_2 + 1)
                .await
                .unwrap_or(0)
                & BIT7 as u8
                != 0
            {
                self.write_u16_async(REG_FIFOPAGE_CTRL_2, RSV_PG_BOUNDARY_8822C | BIT15 as u16)
                    .await?;
                self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, txq2).await?;
                self.write_u8_async(REG_CR + 1, cr1).await?;
                return Ok(());
            }
            sleep_micros(10).await;
        }
        Err(DriverError::Nusb(
            "RTL8822C firmware reserved-page download did not become valid".to_owned(),
        ))
    }

    async fn iddma_dlfw_8822c_async(
        &self,
        src: u32,
        dest: u32,
        len: u32,
        first: bool,
    ) -> Result<(), DriverError> {
        for _ in 0..HALMC_DDMA_POLLING_COUNT {
            if self.read_u32_async(REG_DDMA_CH0CTRL).await.unwrap_or(0) & BIT_DDMACH0_OWN == 0 {
                let mut ctrl =
                    BIT_DDMACH0_CHKSUM_EN | BIT_DDMACH0_OWN | (len & BIT_MASK_DDMACH0_DLEN);
                if !first {
                    ctrl |= BIT_DDMACH0_CHKSUM_CONT;
                }
                self.write_u32_async(REG_DDMA_CH0SA, src).await?;
                self.write_u32_async(REG_DDMA_CH0DA, dest).await?;
                self.write_u32_async(REG_DDMA_CH0CTRL, ctrl).await?;
                for _ in 0..HALMC_DDMA_POLLING_COUNT {
                    if self.read_u32_async(REG_DDMA_CH0CTRL).await.unwrap_or(0) & BIT_DDMACH0_OWN
                        == 0
                    {
                        return Ok(());
                    }
                    sleep_micros(10).await;
                }
                break;
            }
            sleep_micros(10).await;
        }
        Err(DriverError::Nusb(
            "RTL8822C firmware IDDMA transfer timed out".to_owned(),
        ))
    }

    async fn check_fw_chksum_8822c_async(&self, mem_addr: u32) -> Result<(), DriverError> {
        let mut fw_ctrl = self.read_u8_async(REG_MCUFWDL).await.unwrap_or(0);
        if self.read_u32_async(REG_DDMA_CH0CTRL).await.unwrap_or(0) & BIT_DDMACH0_CHKSUM_STS != 0 {
            if mem_addr < OCPBASE_DMEM_88XX {
                fw_ctrl |= BIT3 as u8;
                fw_ctrl &= !(BIT4 as u8);
            } else {
                fw_ctrl |= BIT5 as u8;
                fw_ctrl &= !(BIT6 as u8);
            }
            self.write_u8_async(REG_MCUFWDL, fw_ctrl).await?;
            return Err(DriverError::FirmwareChecksumTimeout);
        }
        if mem_addr < OCPBASE_DMEM_88XX {
            fw_ctrl |= (BIT3 | BIT4) as u8;
        } else {
            fw_ctrl |= (BIT5 | BIT6) as u8;
        }
        self.write_u8_async(REG_MCUFWDL, fw_ctrl).await
    }

    async fn dlfw_end_flow_8822c_async(&self) -> Result<(), DriverError> {
        self.write_u32_async(REG_TXDMA_STATUS, BIT2).await?;
        let fw_ctrl = self.read_u16_async(REG_MCUFWDL).await.unwrap_or(0);
        if fw_ctrl & 0x50 != 0x50 {
            return Err(DriverError::FirmwareChecksumTimeout);
        }
        self.write_u16_async(REG_MCUFWDL, (fw_ctrl | BIT_FW_DW_RDY) & !0x0001)
            .await?;
        self.wlan_cpu_en_8822c_async(true).await?;
        for _ in 0..5000 {
            if self.read_u16_async(REG_MCUFWDL).await.unwrap_or(0) == 0xc078 {
                return Ok(());
            }
            sleep_micros(50).await;
        }
        if self.read_u32_async(REG_FW_DBG7).await.unwrap_or(0) & 0xffff_ff00 == ILLEGAL_KEY_GROUP {
            return Err(DriverError::Nusb(
                "RTL8822C firmware boot failed: illegal key group".to_owned(),
            ));
        }
        Err(DriverError::FirmwareReadyTimeout)
    }

    async fn wlan_cpu_en_8822c_async(&self, enable: bool) -> Result<(), DriverError> {
        if enable {
            self.write_u8_async(
                REG_RSV_CTRL + 1,
                self.read_u8_async(REG_RSV_CTRL + 1).await.unwrap_or(0) | BIT0 as u8,
            )
            .await?;
            self.write_u8_async(
                REG_SYS_FUNC_EN + 1,
                self.read_u8_async(REG_SYS_FUNC_EN + 1).await.unwrap_or(0) | BIT2 as u8,
            )
            .await
        } else {
            self.write_u8_async(
                REG_SYS_FUNC_EN + 1,
                self.read_u8_async(REG_SYS_FUNC_EN + 1).await.unwrap_or(0) & !(BIT2 as u8),
            )
            .await?;
            self.write_u8_async(
                REG_RSV_CTRL + 1,
                self.read_u8_async(REG_RSV_CTRL + 1).await.unwrap_or(0) & !(BIT0 as u8),
            )
            .await
        }
    }

    async fn pltfm_reset_8822c_async(&self) -> Result<(), DriverError> {
        let v = self.read_u8_async(REG_CPU_DMEM_CON + 2).await.unwrap_or(0) & !0x01;
        self.write_u8_async(REG_CPU_DMEM_CON + 2, v).await?;
        self.write_u8_async(REG_CPU_DMEM_CON + 2, v | 0x01).await
    }

    async fn restore_backup_8822c_async(&self, backup: BackupReg) -> Result<(), DriverError> {
        match backup {
            BackupReg::U8(register, value) => self.write_u8_async(register, value as u8).await,
            BackupReg::U16(register, value) => self.write_u16_async(register, value as u16).await,
            BackupReg::U32(register, value) => self.write_u32_async(register, value).await,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum BackupReg {
    U8(u16, u32),
    U16(u16, u32),
    U32(u16, u32),
}

#[derive(Debug, Clone, Copy)]
struct Jaguar3TableContext {
    cut_version: u8,
    rfe_type: u8,
}

async fn load_jaguar3_table_async<F, Fut>(
    table: &[u32],
    ctx: Jaguar3TableContext,
    mut write: F,
) -> Result<(), DriverError>
where
    F: FnMut(u32, u32) -> Fut,
    Fut: Future<Output = Result<(), DriverError>>,
{
    let Some((headline_size, headline_idx)) = select_jaguar3_headline(table, ctx) else {
        return Ok(());
    };
    let cfg_target = if headline_size == 0 {
        0
    } else {
        table[headline_idx * 2] & 0x0fff_ffff
    };

    let mut cfg_para = 0;
    let mut is_matched = true;
    let mut find_target = false;
    let mut idx = headline_size;
    while idx + 1 < table.len() {
        let v1 = table[idx];
        let v2 = table[idx + 1];
        idx += 2;
        match v1 >> 28 {
            0x8 | 0x9 => cfg_para = v1 & 0x0fff_ffff,
            0xa => {
                is_matched = false;
                if !find_target {
                    return Ok(());
                }
            }
            0xb => {
                is_matched = true;
                find_target = false;
            }
            0x4 => {
                if find_target {
                    is_matched = false;
                } else if cfg_para == cfg_target {
                    is_matched = true;
                    find_target = true;
                } else {
                    is_matched = false;
                    find_target = false;
                }
            }
            _ if is_matched => write(v1, v2).await?,
            _ => {}
        }
    }
    Ok(())
}

fn select_jaguar3_headline(table: &[u32], ctx: Jaguar3TableContext) -> Option<(usize, usize)> {
    let mut headline_size = 0usize;
    while headline_size + 1 < table.len() && (table[headline_size] >> 28) == 0xf {
        headline_size += 2;
    }
    if headline_size == 0 {
        return Some((0, 0));
    }

    let target = ((u32::from(ctx.cut_version) & 0x0f) << 24) | u32::from(ctx.rfe_type);
    for idx in (0..headline_size).step_by(2) {
        if table[idx] & 0x0f00_00ff == target {
            return Some((headline_size, idx / 2));
        }
    }

    let target = (0x0f << 24) | u32::from(ctx.rfe_type);
    for idx in (0..headline_size).step_by(2) {
        if table[idx] & 0x0f00_00ff == target {
            return Some((headline_size, idx / 2));
        }
    }

    let mut best = None;
    let mut cut_max = 0;
    for idx in (0..headline_size).step_by(2) {
        let rfe = table[idx] & 0xff;
        let cut = (table[idx] & 0x0f00_0000) >> 24;
        if rfe == u32::from(ctx.rfe_type) && cut >= cut_max {
            cut_max = cut;
            best = Some(idx / 2);
        }
    }
    if let Some(idx) = best {
        return Some((headline_size, idx));
    }

    let mut best = None;
    cut_max = 0;
    for idx in (0..headline_size).step_by(2) {
        let rfe = table[idx] & 0xff;
        let cut = (table[idx] & 0x0f00_0000) >> 24;
        if rfe == 0xff && cut >= cut_max {
            cut_max = cut;
            best = Some(idx / 2);
        }
    }
    best.map(|idx| (headline_size, idx))
}

fn build_fw_page_frame_8822c(chunk: &[u8]) -> Result<Vec<u8>, DriverError> {
    if chunk.len() > u16::MAX as usize {
        return Err(DriverError::Nusb(
            "RTL8822C firmware chunk is too large for TX descriptor".to_owned(),
        ));
    }
    let mut frame = vec![0; TX_DESC_SIZE_8822C + chunk.len()];
    set_bits_le32(&mut frame, 0, 0, 16, chunk.len() as u32);
    set_bits_le32(&mut frame, 0, 16, 8, TX_DESC_SIZE_8822C as u32);
    set_bits_le32(&mut frame, 0, 26, 1, 1);
    set_bits_le32(&mut frame, 4, 8, 5, QSEL_BEACON);
    set_bits_le32(&mut frame, 12, 8, 1, 1);
    set_bits_le32(&mut frame, 16, 0, 7, 0);
    set_bits_le32(&mut frame, 12, 10, 1, 1);
    frame[TX_DESC_SIZE_8822C..].copy_from_slice(chunk);
    tx_desc_checksum_8822c(&mut frame[..TX_DESC_SIZE_8822C]);
    Ok(frame)
}

fn tx_desc_checksum_8822c(desc: &mut [u8]) {
    set_bits_le32(desc, 28, 0, 16, 0);
    let pkt_offset = bits(le32(&desc[4..]), 24, 5) as usize;
    let pairs = (pkt_offset + (TX_DESC_SIZE_8822C >> 3)) << 1;
    let mut checksum = 0u16;
    for idx in 0..pairs {
        checksum ^= le16(desc, 2 * idx) ^ le16(desc, 2 * idx + 1);
    }
    set_bits_le32(desc, 28, 0, 16, checksum as u32);
}

fn validate_8822c_firmware_size(firmware: &[u8]) -> Result<(), DriverError> {
    if firmware.len() < WLAN_FW_HDR_SIZE {
        return Err(DriverError::Nusb(
            "RTL8822C firmware image is shorter than its WLAN header".to_owned(),
        ));
    }
    let dmem = le32(&firmware[WLAN_FW_HDR_DMEM_SIZE..]) + WLAN_FW_HDR_CHKSUM_SIZE;
    let imem = le32(&firmware[WLAN_FW_HDR_IMEM_SIZE..]) + WLAN_FW_HDR_CHKSUM_SIZE;
    let emem = if firmware[WLAN_FW_HDR_MEM_USAGE] & BIT4 as u8 != 0 {
        le32(&firmware[WLAN_FW_HDR_EMEM_SIZE..]) + WLAN_FW_HDR_CHKSUM_SIZE
    } else {
        0
    };
    let expected = WLAN_FW_HDR_SIZE + (dmem + imem + emem) as usize;
    if firmware.len() != expected {
        return Err(DriverError::Nusb(format!(
            "RTL8822C firmware size {} does not match header-computed size {expected}",
            firmware.len()
        )));
    }
    Ok(())
}

fn set_bits_le32(bytes: &mut [u8], offset: usize, bit_offset: u8, bit_len: u8, value: u32) {
    let mut word = u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("offset valid"));
    let mask = ((1u32 << bit_len) - 1) << bit_offset;
    word = (word & !mask) | ((value << bit_offset) & mask);
    bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
}

fn le16(bytes: &[u8], word: usize) -> u16 {
    let offset = word * 2;
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().expect("length checked"))
}

fn bits(word: u32, offset: u8, len: u8) -> u32 {
    (word >> offset) & ((1u32 << len) - 1)
}

fn mask_shift(mask: u32) -> u32 {
    mask.trailing_zeros()
}

fn sco_value_8822c(channel: u8) -> u32 {
    match channel {
        36..=51 => 0x494,
        52..=55 => 0x493,
        56..=111 => 0x453,
        112..=119 => 0x452,
        120..=172 => 0x412,
        173..=u8::MAX => 0x411,
        1..=10 => 0x9aa,
        11 | 12 => 0x96a,
        _ => 0x969,
    }
}

#[derive(Clone, Copy)]
struct PwrStep8822c {
    offset: u16,
    cmd: PwrCmd8822c,
    mask: u8,
    value: u8,
}

#[derive(Clone, Copy)]
enum PwrCmd8822c {
    Write,
    Poll,
}

const fn b(bit: u8) -> u8 {
    1u8 << bit
}

const PWR_ON_8822C_USB: &[PwrStep8822c] = &[
    pwr(0x002e, PwrCmd8822c::Write, b(2), b(2)),
    pwr(0x002d, PwrCmd8822c::Write, b(0), 0),
    pwr(0x007f, PwrCmd8822c::Write, b(7), 0),
    pwr(0x004a, PwrCmd8822c::Write, b(0), 0),
    pwr(0x0005, PwrCmd8822c::Write, b(3) | b(4), 0),
    pwr(0x0000, PwrCmd8822c::Write, b(5), 0),
    pwr(0x0005, PwrCmd8822c::Write, b(4) | b(3) | b(2), 0),
    pwr(0x0006, PwrCmd8822c::Poll, b(1), b(1)),
    pwr(0xff1a, PwrCmd8822c::Write, 0xff, 0),
    pwr(0x002e, PwrCmd8822c::Write, b(3), 0),
    pwr(0x0006, PwrCmd8822c::Write, b(0), b(0)),
    pwr(0x0005, PwrCmd8822c::Write, b(4) | b(3), 0),
    pwr(0x1018, PwrCmd8822c::Write, b(2), b(2)),
    pwr(0x0005, PwrCmd8822c::Write, b(0), b(0)),
    pwr(0x0005, PwrCmd8822c::Poll, b(0), 0),
    pwr(0x001f, PwrCmd8822c::Write, b(7) | b(6), b(7)),
    pwr(0x00ef, PwrCmd8822c::Write, b(7) | b(6), b(7)),
    pwr(0x1045, PwrCmd8822c::Write, b(4), b(4)),
    pwr(0x0010, PwrCmd8822c::Write, b(2), b(2)),
    pwr(0x1064, PwrCmd8822c::Write, b(1), b(1)),
];

const PWR_OFF_8822C_USB: &[PwrStep8822c] = &[
    pwr(0x0093, PwrCmd8822c::Write, b(3), 0),
    pwr(0x001f, PwrCmd8822c::Write, 0xff, 0),
    pwr(0x00ef, PwrCmd8822c::Write, 0xff, 0),
    pwr(0x1045, PwrCmd8822c::Write, b(4), 0),
    pwr(0xff1a, PwrCmd8822c::Write, 0xff, 0x30),
    pwr(0x0049, PwrCmd8822c::Write, b(1), 0),
    pwr(0x0006, PwrCmd8822c::Write, b(0), b(0)),
    pwr(0x0002, PwrCmd8822c::Write, b(1), 0),
    pwr(0x0005, PwrCmd8822c::Write, b(1), b(1)),
    pwr(0x0005, PwrCmd8822c::Poll, b(1), 0),
    pwr(0x0000, PwrCmd8822c::Write, b(5), b(5)),
    pwr(0x0007, PwrCmd8822c::Write, 0xff, 0),
    pwr(0x0067, PwrCmd8822c::Write, b(5), 0),
    pwr(0x004a, PwrCmd8822c::Write, b(0), 0),
    pwr(0x0081, PwrCmd8822c::Write, b(7) | b(6), 0),
    pwr(0x0090, PwrCmd8822c::Write, b(1), 0),
    pwr(0x0005, PwrCmd8822c::Write, b(3) | b(4), b(3)),
];

const fn pwr(offset: u16, cmd: PwrCmd8822c, mask: u8, value: u8) -> PwrStep8822c {
    PwrStep8822c {
        offset,
        cmd,
        mask,
        value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_jaguar3_headline_like_devourer() {
        let table = [
            0xf000_0015,
            0,
            0xf100_00ff,
            0,
            0x8123_4567,
            0,
            0x4000_0000,
            0,
            0x0100,
            1,
        ];
        assert_eq!(
            select_jaguar3_headline(
                &table,
                Jaguar3TableContext {
                    cut_version: 0,
                    rfe_type: 0x15
                }
            ),
            Some((4, 0))
        );
    }

    #[test]
    fn validates_generated_firmware_header_size() {
        validate_8822c_firmware_size(rtl_data::RTL8822C_FW_NIC).unwrap();
    }
}
