//! RTL8822B/RTL8812BU (Jaguar2) cold-start and radio configuration.
//!
//! This follows devourer's validated HalMAC ordering. Jaguar2 shares the
//! HalMAC descriptor generation with Jaguar3, but its power, firmware-page,
//! system configuration, and PHY calibration sequences are intentionally kept
//! distinct because substituting the Jaguar3 values wedges firmware download.

use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::regs::*;
use crate::rtl_data;
use crate::time::{sleep_micros, sleep_ms};
use crate::tx::{build_firmware_page_8822b, TX_DESC_SIZE_8822C};
use crate::types::{
    ChannelWidth, ChipInfo, DriverError, InitReport, InitStatus, MonitorOptions, RadioConfig,
    RfType,
};

const RF_MASK: u32 = 0x000f_ffff;
const REG_SYS_CFG2_8822B: u16 = 0x00fc;
const REG_WLRF1_8822B: u16 = 0x00ec;
const REG_CPU_DMEM_CON_8822B: u16 = 0x1080;
const REG_SYS_CLK_CTRL_8822B: u16 = 0x0008;
const REG_H2CQ_CSR_8822B: u16 = 0x1330;
const REG_WMAC_CSIDMA_CFG_8822B: u16 = 0x169c;
const REG_DDMA_CH0SA_8822B: u16 = 0x1200;
const REG_DDMA_CH0DA_8822B: u16 = 0x1204;
const REG_DDMA_CH0CTRL_8822B: u16 = 0x1208;
const REG_FW_DBG7_8822B: u16 = 0x10fc;
const REG_TXDMA_STATUS_8822B: u16 = 0x0210;
const REG_FIFOPAGE_CTRL_2_8822B: u16 = 0x0204;
const REG_RQPN_CTRL_2_8822B: u16 = 0x022c;
const REG_FIFOPAGE_INFO_1_8822B: u16 = 0x0230;
const REG_FIFOPAGE_INFO_2_8822B: u16 = 0x0234;
const REG_FIFOPAGE_INFO_3_8822B: u16 = 0x0238;
const REG_FIFOPAGE_INFO_4_8822B: u16 = 0x023c;
const REG_FIFOPAGE_INFO_5_8822B: u16 = 0x0240;
const REG_H2C_HEAD_8822B: u16 = 0x0244;
const REG_H2C_TAIL_8822B: u16 = 0x0248;
const REG_H2C_READ_ADDR_8822B: u16 = 0x024c;
const REG_H2C_INFO_8822B: u16 = 0x0254;
const REG_FWFF_CTRL_8822B: u16 = 0x029c;
const REG_FWFF_PKT_INFO_8822B: u16 = 0x02a0;
const REG_RXDMA_MODE_8822B: u16 = 0x0290;
const REG_AUTO_LLT_8822B: u16 = 0x0208;
const REG_BCNQ1_BDNY_8822B: u16 = 0x0456;
const REG_USB_USBSTAT_8822B: u16 = 0xfe11;

const WLAN_FW_HDR_SIZE: usize = 64;
const WLAN_FW_HDR_CHKSUM_SIZE: usize = 8;
const WLAN_FW_HDR_MEM_USAGE: usize = 24;
const WLAN_FW_HDR_DMEM_ADDR: usize = 32;
const WLAN_FW_HDR_DMEM_SIZE: usize = 36;
const WLAN_FW_HDR_IMEM_SIZE: usize = 48;
const WLAN_FW_HDR_EMEM_SIZE: usize = 52;
const WLAN_FW_HDR_EMEM_ADDR: usize = 56;
const WLAN_FW_HDR_IMEM_ADDR: usize = 60;
const BIT_DDMACH0_CHKSUM_CONT: u32 = 1 << 24;
const BIT_DDMACH0_RESET_CHKSUM_STS: u32 = 1 << 25;
const BIT_DDMACH0_CHKSUM_STS: u32 = 1 << 27;
const BIT_DDMACH0_CHKSUM_EN: u32 = 1 << 29;
const BIT_DDMACH0_OWN: u32 = 1 << 31;
const BIT_MASK_DDMACH0_DLEN: u32 = 0x3ffff;
const OCPBASE_TXBUF_88XX: u32 = 0x1878_0000;
const OCPBASE_DMEM_88XX: u32 = 0x0020_0000;
const ILLEGAL_KEY_GROUP: u32 = 0xfaaa_aa00;
const RSVD_PAGE_BOUNDARY_8822B: u16 = 1938;
const DLFW_PACKET_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy)]
enum PowerCommand {
    Write,
    Poll,
    DelayMs,
}

#[derive(Debug, Clone, Copy)]
struct PowerStep {
    register: u16,
    command: PowerCommand,
    mask: u8,
    value: u8,
}

const fn write(register: u16, mask: u8, value: u8) -> PowerStep {
    PowerStep {
        register,
        command: PowerCommand::Write,
        mask,
        value,
    }
}

const fn poll(register: u16, mask: u8, value: u8) -> PowerStep {
    PowerStep {
        register,
        command: PowerCommand::Poll,
        mask,
        value,
    }
}

const fn delay_ms(value: u8) -> PowerStep {
    PowerStep {
        register: 0,
        command: PowerCommand::DelayMs,
        mask: 0,
        value,
    }
}

const POWER_ON_8822B_USB: &[PowerStep] = &[
    write(0x004a, 1 << 0, 0),
    write(0x0005, (1 << 3) | (1 << 4) | (1 << 7), 0),
    write(0xff0a, 0xff, 0),
    write(0xff0b, 0xff, 0),
    write(0x0012, 1 << 1, 0),
    write(0x0012, 1 << 0, 1 << 0),
    write(0x0020, 1 << 0, 1 << 0),
    delay_ms(1),
    write(0x0000, 1 << 5, 0),
    write(0x0005, (1 << 4) | (1 << 3) | (1 << 2), 0),
    poll(0x0006, 1 << 1, 1 << 1),
    write(0xff1a, 0xff, 0),
    write(0x0006, 1 << 0, 1 << 0),
    write(0x0005, 1 << 7, 0),
    write(0x0005, (1 << 4) | (1 << 3), 0),
    write(0x10c3, 1 << 0, 1 << 0),
    write(0x0005, 1 << 0, 1 << 0),
    poll(0x0005, 1 << 0, 0),
    write(0x0020, 1 << 3, 1 << 3),
    write(0x10a8, 0xff, 0),
    write(0x10a9, 0xff, 0xef),
    write(0x10aa, 0xff, 0x0c),
    write(0x0029, 0xff, 0xf9),
    write(0x0024, 1 << 2, 0),
    write(0x00af, 1 << 5, 1 << 5),
];

const POWER_OFF_8822B_USB: &[PowerStep] = &[
    write(0x0093, 0xff, 0xc4),
    write(0x001f, 0xff, 0),
    write(0x00ef, 0xff, 0),
    write(0xff1a, 0xff, 0x30),
    write(0x0049, 1 << 1, 0),
    write(0x0006, 1 << 0, 1 << 0),
    write(0x0002, 1 << 1, 0),
    write(0x10c3, 1 << 0, 0),
    write(0x0005, 1 << 1, 1 << 1),
    poll(0x0005, 1 << 1, 0),
    write(0x0020, 1 << 3, 0),
    write(0x0000, 1 << 5, 1 << 5),
    write(0x0007, 0xff, 0x20),
    write(0x0067, 1 << 5, 0),
    write(0x004a, 1 << 0, 0),
    write(0x0081, (1 << 7) | (1 << 6), 0),
    write(0x0005, (1 << 3) | (1 << 4), 1 << 3),
    write(0x0090, 1 << 1, 0),
];

impl RealtekDevice {
    pub(crate) async fn initialize_monitor_jaguar2_async(
        &self,
        _chip: ChipInfo,
        radio: RadioConfig,
        options: MonitorOptions,
    ) -> Result<InitReport, DriverError> {
        self.pre_init_system_cfg_8822b_async().await?;
        self.power_on_8822b_async().await?;
        let chip = self.probe_chip_async().await?;
        self.init_system_cfg_8822b_async(chip.cut_version).await?;
        self.download_firmware_8822b_async(rtl_data::RTL8822B_FW_NIC)
            .await?;
        self.init_mac_cfg_8822b_async().await?;
        self.init_usb_cfg_8822b_async().await?;
        self.enable_bb_rf_8822b_async(true).await?;

        let mut efuse = self.read_efuse_info_async(chip).await?;
        if let Some(rfe) = options.rfe_type_override {
            efuse.rfe_type = rfe;
        }
        let _ = self.efuse_info.set(efuse);
        self.phydm_pre_post_8822b_async(false).await?;
        self.load_phy_tables_async(chip, efuse).await?;
        self.load_rf_tables_async(chip, efuse).await?;
        self.phydm_pre_post_8822b_async(true).await?;
        self.config_trx_mode_8822b_async(chip).await?;
        self.set_channel_bw_8822b_async(chip, radio, efuse.rfe_type)
            .await?;
        self.lck_8822b_async(chip).await?;
        if options.should_run_iqk(chip.family) {
            self.run_iqk_8822b_async(chip, radio.channel <= 14).await?;
        }
        self.config_trx_mode_8822b_async(chip).await?;
        self.rfe_init_8822b_async().await?;
        self.bf_init_8822b_async().await?;
        if !options.skip_tx_power {
            self.apply_tx_power_8822b_async(chip, radio, efuse.rfe_type)
                .await?;
        }
        self.coex_wlan_only_8822b_async(radio.channel > 14).await?;
        self.enable_rx_8822b_async(options.accept_bad_fcs).await?;

        Ok(InitReport {
            chip,
            status: InitStatus::Initialized,
            firmware_downloaded: true,
        })
    }

    async fn run_power_sequence_8822b_async(
        &self,
        sequence: &[PowerStep],
        fatal_poll: bool,
    ) -> Result<(), DriverError> {
        for step in sequence {
            match step.command {
                PowerCommand::Write => {
                    let current = self.read_u8_async(step.register).await.unwrap_or(0);
                    self.write_u8_async(
                        step.register,
                        (current & !step.mask) | (step.value & step.mask),
                    )
                    .await?;
                }
                PowerCommand::DelayMs => sleep_ms(u32::from(step.value)).await,
                PowerCommand::Poll => {
                    let limit = if fatal_poll { 5000 } else { 2000 };
                    let mut matched = false;
                    for _ in 0..limit {
                        let current = self.read_u8_async(step.register).await.unwrap_or(0);
                        if current & step.mask == step.value & step.mask {
                            matched = true;
                            break;
                        }
                        sleep_micros(10).await;
                    }
                    if !matched && fatal_poll {
                        return Err(DriverError::Nusb(format!(
                            "RTL8822B power poll timed out at 0x{:04x}",
                            step.register
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    async fn power_on_8822b_async(&self) -> Result<(), DriverError> {
        self.run_power_sequence_8822b_async(POWER_OFF_8822B_USB, false)
            .await?;
        self.run_power_sequence_8822b_async(POWER_ON_8822B_USB, true)
            .await
    }

    pub(crate) async fn shutdown_monitor_jaguar2_async(&self) -> Result<(), DriverError> {
        self.write_u16_async(REG_CR, 0).await?;
        self.write_u32_async(REG_RCR, 0).await?;
        self.run_power_sequence_8822b_async(POWER_OFF_8822B_USB, false)
            .await
    }

    async fn pre_init_system_cfg_8822b_async(&self) -> Result<(), DriverError> {
        self.write_u8_async(REG_RSV_CTRL, 0).await?;
        if self
            .read_u8_async(REG_SYS_CFG2_8822B + 3)
            .await
            .unwrap_or(0)
            == 0x20
        {
            let value = self.read_u8_async(0xfe5b).await.unwrap_or(0) | (1 << 4);
            self.write_u8_async(0xfe5b, value).await?;
        }
        let pad = self.read_u32_async(0x0064).await.unwrap_or(0) | BIT28 | BIT29;
        self.write_u32_async(0x0064, pad).await?;
        let led = self.read_u32_async(0x004c).await.unwrap_or(0) & !(BIT25 | BIT26);
        self.write_u32_async(0x004c, led).await?;
        let gpio = self.read_u32_async(0x0040).await.unwrap_or(0) | BIT2;
        self.write_u32_async(0x0040, gpio).await?;
        self.enable_bb_rf_8822b_async(false).await
    }

    async fn init_system_cfg_8822b_async(&self, cut: u8) -> Result<(), DriverError> {
        // Unlike Jaguar3, DDMA enable (BIT8) must remain clear before DLFW.
        let dmem = self
            .read_u32_async(REG_CPU_DMEM_CON_8822B)
            .await
            .unwrap_or(0)
            | BIT16;
        self.write_u32_async(REG_CPU_DMEM_CON_8822B, dmem).await?;
        let sys = self.read_u8_async(REG_SYS_FUNC_EN + 1).await.unwrap_or(0) | 0xd8;
        self.write_u8_async(REG_SYS_FUNC_EN + 1, sys).await?;
        let mcu = self.read_u32_async(REG_MCUFWDL).await.unwrap_or(0);
        if mcu & BIT20 != 0 {
            self.write_u32_async(REG_MCUFWDL, mcu & !BIT20).await?;
            let gpio = self.read_u32_async(0x0040).await.unwrap_or(0) & !BIT19;
            self.write_u32_async(0x0040, gpio).await?;
        }
        if cut == 1 {
            let ana = self.read_u8_async(0x1018).await.unwrap_or(0) & !0x07;
            self.write_u8_async(0x1018, ana).await?;
        }
        Ok(())
    }

    async fn enable_bb_rf_8822b_async(&self, enable: bool) -> Result<(), DriverError> {
        let (sys, rf, wlrf) = if enable {
            (
                self.read_u8_async(REG_SYS_FUNC_EN).await.unwrap_or(0) | 0x03,
                self.read_u8_async(REG_RF_CTRL).await.unwrap_or(0) | 0x07,
                self.read_u32_async(REG_WLRF1_8822B).await.unwrap_or(0) | (0x7 << 24),
            )
        } else {
            (
                self.read_u8_async(REG_SYS_FUNC_EN).await.unwrap_or(0) & !0x03,
                self.read_u8_async(REG_RF_CTRL).await.unwrap_or(0) & !0x07,
                self.read_u32_async(REG_WLRF1_8822B).await.unwrap_or(0) & !(0x7 << 24),
            )
        };
        self.write_u8_async(REG_SYS_FUNC_EN, sys).await?;
        self.write_u8_async(REG_RF_CTRL, rf).await?;
        self.write_u32_async(REG_WLRF1_8822B, wlrf).await
    }

    async fn phydm_pre_post_8822b_async(&self, post: bool) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x0808, BIT28 | BIT29, if post { 3 } else { 0 })
            .await
    }

    async fn download_firmware_8822b_async(&self, firmware: &[u8]) -> Result<(), DriverError> {
        validate_firmware_8822b(firmware)?;
        self.wlan_cpu_enable_8822b_async(false).await?;

        let backups = [
            (
                REG_TRXDMA_CTRL + 1,
                1,
                u32::from(self.read_u8_async(REG_TRXDMA_CTRL + 1).await?),
            ),
            (REG_CR, 1, u32::from(self.read_u8_async(REG_CR).await?)),
            (REG_H2CQ_CSR_8822B, 4, BIT31),
            (
                REG_FIFOPAGE_INFO_1_8822B,
                2,
                u32::from(self.read_u16_async(REG_FIFOPAGE_INFO_1_8822B).await?),
            ),
            (
                REG_RQPN_CTRL_2_8822B,
                4,
                self.read_u32_async(REG_RQPN_CTRL_2_8822B).await? | BIT31,
            ),
            (
                REG_BCN_CTRL,
                1,
                u32::from(self.read_u8_async(REG_BCN_CTRL).await?),
            ),
        ];

        self.write_u8_async(REG_TRXDMA_CTRL + 1, 3 << 6).await?;
        self.write_u8_async(REG_CR, 0x05).await?;
        self.write_u32_async(REG_H2CQ_CSR_8822B, BIT31).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_1_8822B, 0x0200)
            .await?;
        self.write_u32_async(REG_RQPN_CTRL_2_8822B, backups[4].2)
            .await?;
        self.write_u8_async(REG_BCN_CTRL, (backups[5].2 as u8 & !(1 << 3)) | (1 << 4))
            .await?;
        self.platform_reset_8822b_async().await?;

        let result = self.start_dlfw_8822b_async(firmware).await;
        for (register, width, value) in backups {
            match width {
                1 => self.write_u8_async(register, value as u8).await?,
                2 => self.write_u16_async(register, value as u16).await?,
                _ => self.write_u32_async(register, value).await?,
            }
        }
        if let Err(error) = result {
            self.cleanup_failed_dlfw_8822b_async().await?;
            return Err(error);
        }
        if let Err(error) = self.finish_dlfw_8822b_async().await {
            self.cleanup_failed_dlfw_8822b_async().await?;
            return Err(error);
        }
        Ok(())
    }

    async fn wlan_cpu_enable_8822b_async(&self, enable: bool) -> Result<(), DriverError> {
        if enable {
            let rsv = self.read_u8_async(REG_RSV_CTRL + 1).await.unwrap_or(0) | 0x01;
            self.write_u8_async(REG_RSV_CTRL + 1, rsv).await?;
            let sys = self.read_u8_async(REG_SYS_FUNC_EN + 1).await.unwrap_or(0) | (1 << 2);
            self.write_u8_async(REG_SYS_FUNC_EN + 1, sys).await
        } else {
            let sys = self.read_u8_async(REG_SYS_FUNC_EN + 1).await.unwrap_or(0) & !(1 << 2);
            self.write_u8_async(REG_SYS_FUNC_EN + 1, sys).await?;
            let rsv = self.read_u8_async(REG_RSV_CTRL + 1).await.unwrap_or(0) & !0x01;
            self.write_u8_async(REG_RSV_CTRL + 1, rsv).await
        }
    }

    async fn platform_reset_8822b_async(&self) -> Result<(), DriverError> {
        let dmem = self
            .read_u8_async(REG_CPU_DMEM_CON_8822B + 2)
            .await
            .unwrap_or(0)
            & !0x01;
        self.write_u8_async(REG_CPU_DMEM_CON_8822B + 2, dmem)
            .await?;
        let clock = self
            .read_u8_async(REG_SYS_CLK_CTRL_8822B + 1)
            .await
            .unwrap_or(0)
            & !(1 << 6);
        self.write_u8_async(REG_SYS_CLK_CTRL_8822B + 1, clock)
            .await?;
        self.write_u8_async(REG_CPU_DMEM_CON_8822B + 2, dmem | 0x01)
            .await?;
        self.write_u8_async(REG_SYS_CLK_CTRL_8822B + 1, clock | (1 << 6))
            .await
    }

    async fn start_dlfw_8822b_async(&self, firmware: &[u8]) -> Result<(), DriverError> {
        let dmem = le32_at(firmware, WLAN_FW_HDR_DMEM_SIZE)? as usize + WLAN_FW_HDR_CHKSUM_SIZE;
        let imem = le32_at(firmware, WLAN_FW_HDR_IMEM_SIZE)? as usize + WLAN_FW_HDR_CHKSUM_SIZE;
        let emem = if firmware[WLAN_FW_HDR_MEM_USAGE] & (1 << 4) != 0 {
            le32_at(firmware, WLAN_FW_HDR_EMEM_SIZE)? as usize + WLAN_FW_HDR_CHKSUM_SIZE
        } else {
            0
        };
        let fw_ctrl = (self.read_u16_async(REG_MCUFWDL).await.unwrap_or(0) & 0x3800) | 0x01;
        self.write_u16_async(REG_MCUFWDL, fw_ctrl).await?;

        let dmem_addr = le32_at(firmware, WLAN_FW_HDR_DMEM_ADDR)? & !BIT31;
        let imem_addr = le32_at(firmware, WLAN_FW_HDR_IMEM_ADDR)? & !BIT31;
        self.download_segment_8822b_async(
            &firmware[WLAN_FW_HDR_SIZE..WLAN_FW_HDR_SIZE + dmem],
            dmem_addr,
        )
        .await?;
        self.download_segment_8822b_async(
            &firmware[WLAN_FW_HDR_SIZE + dmem..WLAN_FW_HDR_SIZE + dmem + imem],
            imem_addr,
        )
        .await?;
        if emem != 0 {
            let emem_addr = le32_at(firmware, WLAN_FW_HDR_EMEM_ADDR)? & !BIT31;
            self.download_segment_8822b_async(
                &firmware[WLAN_FW_HDR_SIZE + dmem + imem..],
                emem_addr,
            )
            .await?;
        }
        Ok(())
    }

    async fn download_segment_8822b_async(
        &self,
        segment: &[u8],
        destination: u32,
    ) -> Result<(), DriverError> {
        let control = self
            .read_u32_async(REG_DDMA_CH0CTRL_8822B)
            .await
            .unwrap_or(0)
            | BIT_DDMACH0_RESET_CHKSUM_STS;
        self.write_u32_async(REG_DDMA_CH0CTRL_8822B, control)
            .await?;
        let mut first = true;
        let mut offset = 0usize;
        for chunk in segment.chunks(DLFW_PACKET_SIZE) {
            let (frame, packet_offset) = build_firmware_page_8822b(chunk);
            self.send_firmware_page_8822b_async(&frame, destination, chunk.len())
                .await?;
            self.iddma_dlfw_8822b_async(
                OCPBASE_TXBUF_88XX + TX_DESC_SIZE_8822C as u32 + packet_offset as u32,
                destination + offset as u32,
                chunk.len() as u32,
                first,
            )
            .await?;
            first = false;
            offset += chunk.len();
        }
        self.check_fw_checksum_8822b_async(destination).await
    }

    async fn send_firmware_page_8822b_async(
        &self,
        frame: &[u8],
        _destination: u32,
        chunk_len: usize,
    ) -> Result<(), DriverError> {
        // Every chunk reuses TX-buffer page zero before IDDMA copies it to its
        // final DMEM/IMEM offset.
        self.write_u16_async(REG_FIFOPAGE_CTRL_2_8822B, 1 << 15)
            .await?;
        let cr1 = self.read_u8_async(REG_CR + 1).await.unwrap_or(0);
        let txq2 = self.read_u8_async(REG_FWHW_TXQ_CTRL + 2).await.unwrap_or(0);
        self.write_u8_async(REG_CR + 1, cr1 | 0x01).await?;
        self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, txq2 & !(1 << 6))
            .await?;

        let sent = self.write_tx_transfer_raw_async(frame).await;
        let result = match sent {
            Ok(actual) if actual == frame.len() => {
                let mut valid = false;
                for _ in 0..1000 {
                    if self
                        .read_u8_async(REG_FIFOPAGE_CTRL_2_8822B + 1)
                        .await
                        .unwrap_or(0)
                        & (1 << 7)
                        != 0
                    {
                        valid = true;
                        break;
                    }
                    sleep_micros(10).await;
                }
                if valid {
                    Ok(())
                } else {
                    Err(DriverError::Nusb(format!(
                        "RTL8822B firmware beacon-valid timeout for {chunk_len} bytes"
                    )))
                }
            }
            Ok(actual) => Err(DriverError::Nusb(format!(
                "RTL8822B firmware short bulk write: {actual}/{}",
                frame.len()
            ))),
            Err(error) => Err(error),
        };
        self.write_u16_async(
            REG_FIFOPAGE_CTRL_2_8822B,
            RSVD_PAGE_BOUNDARY_8822B | (1 << 15),
        )
        .await?;
        self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, txq2).await?;
        self.write_u8_async(REG_CR + 1, cr1).await?;
        result
    }

    async fn iddma_dlfw_8822b_async(
        &self,
        source: u32,
        destination: u32,
        len: u32,
        first: bool,
    ) -> Result<(), DriverError> {
        for _ in 0..1000 {
            if self
                .read_u32_async(REG_DDMA_CH0CTRL_8822B)
                .await
                .unwrap_or(0)
                & BIT_DDMACH0_OWN
                == 0
            {
                let mut control = BIT_DDMACH0_CHKSUM_EN | BIT_DDMACH0_OWN;
                control |= len & BIT_MASK_DDMACH0_DLEN;
                if !first {
                    control |= BIT_DDMACH0_CHKSUM_CONT;
                }
                self.write_u32_async(REG_DDMA_CH0SA_8822B, source).await?;
                self.write_u32_async(REG_DDMA_CH0DA_8822B, destination)
                    .await?;
                self.write_u32_async(REG_DDMA_CH0CTRL_8822B, control)
                    .await?;
                for _ in 0..1000 {
                    if self
                        .read_u32_async(REG_DDMA_CH0CTRL_8822B)
                        .await
                        .unwrap_or(0)
                        & BIT_DDMACH0_OWN
                        == 0
                    {
                        return Ok(());
                    }
                }
                break;
            }
        }
        Err(DriverError::Nusb(
            "RTL8822B firmware IDDMA channel timed out".to_owned(),
        ))
    }

    async fn check_fw_checksum_8822b_async(&self, memory: u32) -> Result<(), DriverError> {
        let mut fw = self.read_u8_async(REG_MCUFWDL).await.unwrap_or(0);
        if self
            .read_u32_async(REG_DDMA_CH0CTRL_8822B)
            .await
            .unwrap_or(0)
            & BIT_DDMACH0_CHKSUM_STS
            != 0
        {
            if memory < OCPBASE_DMEM_88XX {
                fw = (fw | (1 << 3)) & !(1 << 4);
            } else {
                fw = (fw | (1 << 5)) & !(1 << 6);
            }
            self.write_u8_async(REG_MCUFWDL, fw).await?;
            return Err(DriverError::FirmwareChecksumTimeout);
        }
        fw |= if memory < OCPBASE_DMEM_88XX {
            (1 << 3) | (1 << 4)
        } else {
            (1 << 5) | (1 << 6)
        };
        self.write_u8_async(REG_MCUFWDL, fw).await
    }

    async fn finish_dlfw_8822b_async(&self) -> Result<(), DriverError> {
        self.write_u32_async(REG_TXDMA_STATUS_8822B, 1 << 2).await?;
        let fw = self.read_u16_async(REG_MCUFWDL).await?;
        if fw & 0x50 != 0x50 {
            return Err(DriverError::FirmwareChecksumTimeout);
        }
        self.write_u16_async(REG_MCUFWDL, (fw | (1 << 14)) & !0x01)
            .await?;
        self.wlan_cpu_enable_8822b_async(true).await?;
        for _ in 0..5000 {
            if self.read_u16_async(REG_MCUFWDL).await.unwrap_or(0) == 0xc078 {
                return Ok(());
            }
            sleep_micros(50).await;
        }
        if self.read_u32_async(REG_FW_DBG7_8822B).await.unwrap_or(0) & 0xffff_ff00
            == ILLEGAL_KEY_GROUP
        {
            log::error!(target: "openipc_rtl88xx::firmware", "RTL8822B firmware reported illegal key group");
        }
        Err(DriverError::FirmwareReadyTimeout)
    }

    async fn cleanup_failed_dlfw_8822b_async(&self) -> Result<(), DriverError> {
        let fw = self.read_u8_async(REG_MCUFWDL).await.unwrap_or(0) & !0x01;
        self.write_u8_async(REG_MCUFWDL, fw).await?;
        let sys = self.read_u8_async(REG_SYS_FUNC_EN + 1).await.unwrap_or(0) | (1 << 2);
        self.write_u8_async(REG_SYS_FUNC_EN + 1, sys).await
    }

    async fn init_mac_cfg_8822b_async(&self) -> Result<(), DriverError> {
        self.init_trx_cfg_8822b_async(true).await?;
        self.init_protocol_cfg_8822b_async().await?;
        self.init_edca_cfg_8822b_async().await?;
        self.init_wmac_cfg_8822b_async().await
    }

    async fn init_trx_cfg_8822b_async(&self, set_boundary: bool) -> Result<(), DriverError> {
        // VO/VI -> NQ, BE/BK -> LQ, management/high -> HQ.
        let queue_map = (3 << 14) | (3 << 12) | (1 << 10) | (1 << 8) | (2 << 6) | (2 << 4);
        self.write_u16_async(REG_TRXDMA_CTRL, queue_map).await?;
        let fwff = self.read_u8_async(0x0601).await.unwrap_or(0) & 0x80;
        if fwff != 0 {
            self.write_u8_async(
                0x0601,
                self.read_u8_async(0x0601).await.unwrap_or(0) & !0x80,
            )
            .await?;
        }
        self.write_u8_async(REG_CR, 0).await?;
        self.write_u16_async(
            REG_FWFF_CTRL_8822B,
            self.read_u16_async(REG_FWFF_PKT_INFO_8822B)
                .await
                .unwrap_or(0),
        )
        .await?;
        self.write_u8_async(REG_CR, 0x0f).await?;
        if fwff != 0 {
            self.write_u8_async(0x0601, self.read_u8_async(0x0601).await.unwrap_or(0) | 0x80)
                .await?;
        }
        self.write_u32_async(REG_H2CQ_CSR_8822B, BIT31).await?;
        self.priority_queue_cfg_8822b_async(set_boundary).await?;
        self.init_h2c_8822b_async().await
    }

    async fn priority_queue_cfg_8822b_async(&self, set_boundary: bool) -> Result<(), DriverError> {
        const TX_FIFO_PAGES: u16 = 2048;
        const RSVD_CSI: u16 = 1998;
        const PUBLIC_PAGES: u16 = 1745;

        self.write_u16_async(REG_FIFOPAGE_INFO_1_8822B, 64).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_2_8822B, 64).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_3_8822B, 64).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_4_8822B, 0).await?;
        self.write_u16_async(REG_FIFOPAGE_INFO_5_8822B, PUBLIC_PAGES)
            .await?;
        self.write_u32_async(
            REG_RQPN_CTRL_2_8822B,
            self.read_u32_async(REG_RQPN_CTRL_2_8822B)
                .await
                .unwrap_or(0)
                | BIT31,
        )
        .await?;
        self.write_u16_async(REG_WMAC_CSIDMA_CFG_8822B, RSVD_CSI)
            .await?;
        self.write_u8_async(
            REG_FWHW_TXQ_CTRL + 2,
            self.read_u8_async(REG_FWHW_TXQ_CTRL + 2).await.unwrap_or(0) | (1 << 4),
        )
        .await?;
        if set_boundary {
            self.write_u16_async(REG_FIFOPAGE_CTRL_2_8822B, RSVD_PAGE_BOUNDARY_8822B)
                .await?;
            self.write_u16_async(REG_BCNQ_BDNY, RSVD_PAGE_BOUNDARY_8822B)
                .await?;
            self.write_u16_async(REG_FIFOPAGE_CTRL_2_8822B + 2, RSVD_PAGE_BOUNDARY_8822B)
                .await?;
            self.write_u16_async(REG_BCNQ1_BDNY_8822B, RSVD_PAGE_BOUNDARY_8822B)
                .await?;
        }
        self.write_u32_async(REG_RXFF_PTR_8814, 24576 - 256 - 1)
            .await?;
        let auto =
            (self.read_u8_async(REG_AUTO_LLT_8822B).await.unwrap_or(0) & !(0x0f << 4)) | (3 << 4);
        self.write_u8_async(REG_AUTO_LLT_8822B, auto).await?;
        self.write_u8_async(REG_AUTO_LLT_8822B + 3, 3).await?;
        self.write_u8_async(
            REG_TXDMA_OFFSET_CHK + 1,
            self.read_u8_async(REG_TXDMA_OFFSET_CHK + 1)
                .await
                .unwrap_or(0)
                | (1 << 1),
        )
        .await?;
        self.write_u8_async(REG_AUTO_LLT_8822B, auto | 0x01).await?;
        for _ in 0..1000 {
            if self.read_u8_async(REG_AUTO_LLT_8822B).await.unwrap_or(0) & 0x01 == 0 {
                self.write_u8_async(REG_CR + 3, 0).await?;
                return Ok(());
            }
            sleep_micros(10).await;
        }
        Err(DriverError::Nusb(format!(
            "RTL8822B LLT auto-init timed out ({TX_FIFO_PAGES} TX FIFO pages)"
        )))
    }

    async fn init_h2c_8822b_async(&self) -> Result<(), DriverError> {
        // 2048 - CSI(50) - FW TX(4) - CPU(0) - H2C(8) = page 1986.
        let h2c_address = 1986u32 << 7;
        let h2c_size = 8u32 << 7;
        self.write_u32_async(
            REG_H2C_HEAD_8822B,
            (self.read_u32_async(REG_H2C_HEAD_8822B).await.unwrap_or(0) & 0xfffc_0000)
                | h2c_address,
        )
        .await?;
        self.write_u32_async(
            REG_H2C_READ_ADDR_8822B,
            (self
                .read_u32_async(REG_H2C_READ_ADDR_8822B)
                .await
                .unwrap_or(0)
                & 0xfffc_0000)
                | h2c_address,
        )
        .await?;
        self.write_u32_async(
            REG_H2C_TAIL_8822B,
            (self.read_u32_async(REG_H2C_TAIL_8822B).await.unwrap_or(0) & 0xfffc_0000)
                | (h2c_address + h2c_size),
        )
        .await?;
        let info = (self.read_u8_async(REG_H2C_INFO_8822B).await.unwrap_or(0) & 0xfc) | 0x01;
        self.write_u8_async(REG_H2C_INFO_8822B, info).await?;
        self.write_u8_async(REG_H2C_INFO_8822B, (info & 0xfb) | 0x04)
            .await?;
        self.write_u8_async(
            REG_TXDMA_OFFSET_CHK + 1,
            (self
                .read_u8_async(REG_TXDMA_OFFSET_CHK + 1)
                .await
                .unwrap_or(0)
                & 0x7f)
                | 0x80,
        )
        .await
    }

    async fn init_protocol_cfg_8822b_async(&self) -> Result<(), DriverError> {
        self.write_u8_async(
            0x04bc,
            self.read_u8_async(0x04bc).await.unwrap_or(0) & !(1 << 6),
        )
        .await?;
        self.write_u8_async(0x0455, 0x70).await?;
        self.write_u8_async(
            0x045e,
            self.read_u8_async(0x045e).await.unwrap_or(0) | (1 << 2),
        )
        .await?;
        self.write_u32_async(0x04c8, 0x2020_08ff).await?;
        self.write_u16_async(0x04ce, 0x0801).await?;
        for (register, value) in [
            (0x1448, 0x06),
            (0x144a, 0x06),
            (0x144c, 0x06),
            (0x144e, 0x06),
        ] {
            self.write_u8_async(register, value).await?;
        }
        self.write_u8_async(
            0x0480,
            self.read_u8_async(0x0480).await.unwrap_or(0) | (1 << 5),
        )
        .await
    }

    async fn init_edca_cfg_8822b_async(&self) -> Result<(), DriverError> {
        self.write_u8_async(
            0x05b4,
            self.read_u8_async(0x05b4).await.unwrap_or(0) & !0x70,
        )
        .await?;
        self.write_u16_async(0x0522, 0).await?;
        self.write_u8_async(0x051b, 0x09).await?;
        self.write_u8_async(0x0512, 0x19).await?;
        self.write_u32_async(0x0514, 0x1010_0e0a).await?;
        self.write_u16_async(0x0502, 0x0186).await?;
        self.write_u16_async(0x0506, 0x03bc).await?;
        self.write_u32_async(0x0544, 0x001b_0005).await?;
        self.write_u16_async(0x055e, 0x3030).await?;
        self.write_u8_async(
            REG_BCN_CTRL,
            self.read_u8_async(REG_BCN_CTRL).await.unwrap_or(0) | (1 << 3),
        )
        .await?;
        self.write_u32_async(0x0540, 0x0000_6404).await?;
        self.write_u8_async(0x0558, 0x04).await?;
        self.write_u8_async(0x0559, 0x02).await?;
        self.write_u8_async(
            0x0521,
            self.read_u8_async(0x0521).await.unwrap_or(0) & !(1 << 4),
        )
        .await
    }

    async fn init_wmac_cfg_8822b_async(&self) -> Result<(), DriverError> {
        self.write_u32_async(0x06a0, 0x0fff_ffff).await?;
        self.write_u16_async(0x06a4, 0xffff).await?;
        self.write_u32_async(REG_RCR, 0xe400_220e).await?;
        self.write_u8_async(0x060c, 24).await?;
        self.write_u8_async(0x0606, 0x30).await?;
        self.write_u8_async(0x0605, 0x30).await?;
        self.write_u8_async(
            0x066c,
            self.read_u8_async(0x066c).await.unwrap_or(0) | (1 << 1),
        )
        .await?;
        self.write_u8_async(
            0x0718,
            self.read_u8_async(0x0718).await.unwrap_or(0) | (1 << 6),
        )
        .await?;
        self.write_u32_async(0x07d8, 0x3081_0041).await?;
        self.write_u8_async(0x07d4, 0x98).await
    }

    async fn init_usb_cfg_8822b_async(&self) -> Result<(), DriverError> {
        let mut mode = (1 << 1) | (3 << 2);
        if self
            .read_u8_async(REG_SYS_CFG2_8822B + 3)
            .await
            .unwrap_or(0)
            == 0x20
        {
            mode |= 0 << 4;
        } else if self.read_u8_async(REG_USB_USBSTAT_8822B).await.unwrap_or(0) & 0x03 == 1 {
            mode |= 1 << 4;
        } else {
            mode |= 2 << 4;
        }
        self.write_u8_async(REG_RXDMA_MODE_8822B, mode).await?;
        self.write_u16_async(
            REG_TXDMA_OFFSET_CHK,
            self.read_u16_async(REG_TXDMA_OFFSET_CHK).await.unwrap_or(0) | (1 << 9),
        )
        .await?;
        let agg_register = 0x0280;
        let agg_hi = self.read_u8_async(agg_register + 3).await.unwrap_or(0) & !(1 << 7);
        let queue = self.read_u8_async(REG_TRXDMA_CTRL).await.unwrap_or(0) | (1 << 2);
        self.write_u32_async(
            agg_register,
            self.read_u32_async(agg_register).await.unwrap_or(0) & !BIT29,
        )
        .await?;
        self.write_u8_async(REG_TRXDMA_CTRL, queue).await?;
        self.write_u8_async(agg_register + 3, agg_hi).await?;
        let superspeed = self
            .read_u8_async(REG_SYS_CFG2_8822B + 3)
            .await
            .unwrap_or(0)
            == 0x20;
        self.write_u16_async(
            agg_register,
            0x05 | (if superspeed { 0x0a } else { 0x20 } << 8),
        )
        .await
    }

    async fn rfe_ifem_8822b_async(&self, channel: u8) -> Result<(), DriverError> {
        let is_2g = channel <= 14;
        let (rfe, switch) = if is_2g {
            (0x745774, 0x57)
        } else {
            (0x477547, 0x75)
        };
        for register in [0x0cb0, 0x0eb0] {
            self.set_bb_reg_async(register, 0x00ff_ffff, rfe).await?;
        }
        for register in [0x0cb4, 0x0eb4] {
            self.set_bb_reg_async(register, 0x0000_ff00, switch).await?;
        }
        for register in [0x0cbc, 0x0ebc] {
            self.set_bb_reg_async(register, 0x3f, 0).await?;
            self.set_bb_reg_async(register, BIT10 | BIT11, 0).await?;
        }
        let antenna = if is_2g { 0xa501 } else { 0xa5a5 };
        for register in [0x0ca0, 0x0ea0] {
            self.set_bb_reg_async(register, 0x0000_ffff, antenna)
                .await?;
        }
        Ok(())
    }

    async fn config_trx_mode_8822b_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        let path = if chip.rf_type == RfType::TwoTTwoR {
            3
        } else {
            1
        };
        self.set_bb_reg_async(0x0c08, 0x0000_ffff, 0x3231).await?;
        if chip.total_rf_paths() >= 2 {
            self.set_bb_reg_async(0x0e08, 0x0000_ffff, 0x3231).await?;
        }
        self.set_bb_reg_async(0x093c, BIT18 | BIT19, 3).await?;
        self.set_bb_reg_async(0x080c, BIT28 | BIT29, 1).await?;
        self.set_bb_reg_async(0x080c, BIT30, 1).await?;
        self.set_bb_reg_async(0x080c, 0xff, (path << 4) | path)
            .await?;
        self.set_bb_reg_async(0x0a04, 0xf000_0000, 8).await?;
        self.set_bb_reg_async(0x093c, 0xfff0_0000, 1).await?;
        if chip.total_rf_paths() >= 2 {
            self.set_bb_reg_async(0x0940, 0x0000_fff0, 0x043).await?;
        } else {
            self.set_bb_reg_async(0x0940, 0xf0, 1).await?;
            self.set_bb_reg_async(0x0940, 0xff00, 0).await?;
        }
        self.set_bb_reg_async(0x0a2c, BIT22, 0).await?;
        self.set_bb_reg_async(0x0a2c, BIT18, 0).await?;
        self.set_bb_reg_async(0x0a04, 0x0f00_0000, 0).await?;
        self.set_bb_reg_async(0x0808, 0xff, (path << 4) | path)
            .await?;
        for (register, value) in [
            (0x1904, u32::from(chip.total_rf_paths() >= 2)),
            (0x0800, u32::from(chip.total_rf_paths() >= 2)),
            (0x0850, u32::from(chip.total_rf_paths() >= 2)),
        ] {
            let mask = if register == 0x1904 {
                BIT16
            } else if register == 0x0800 {
                BIT28
            } else {
                1 << 23
            };
            self.set_bb_reg_async(register, mask, value).await?;
        }
        for _ in 0..100 {
            self.set_rf_reg_async(chip, RfPath::A, 0xef, RF_MASK, 0x80000)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x33, RF_MASK, 0x00001)
                .await?;
            sleep_micros(2).await;
            if self.query_rf_reg_async(chip, RfPath::A, 0x33).await? == 1 {
                break;
            }
        }
        for (register, value) in [
            (0xef, 0x80000),
            (0x33, 0x00001),
            (0x3e, 0x00034),
            (0x3f, 0x4080c),
            (0xef, 0x00000),
        ] {
            self.set_rf_reg_async(chip, RfPath::A, register, RF_MASK, value)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn set_channel_bw_8822b_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
        rfe_type: u8,
    ) -> Result<(), DriverError> {
        let width = match radio.channel_width {
            ChannelWidth::Mhz20 => 0u8,
            ChannelWidth::Mhz40 => 1,
            ChannelWidth::Mhz80 => 2,
            ChannelWidth::Mhz5 | ChannelWidth::Mhz10 => {
                return Err(DriverError::Nusb(
                    "RTL8822B supports 20/40/80 MHz; 5/10 MHz is a Jaguar3 mode".to_owned(),
                ));
            }
        };
        let primary_index = radio.channel_offset;
        let center = center_channel_8822b(radio.channel, width, primary_index);
        let is_2g = center <= 14;
        self.rfe_ifem_8822b_async(center).await?;

        let mut rf18 = self.query_rf_reg_async(chip, RfPath::A, 0x18).await?;
        rf18 &= !(BIT18 | BIT17 | 0xff);
        rf18 |= u32::from(center);
        if is_2g {
            self.set_bb_reg_async(0x0958, 0x1f, 0).await?;
            self.set_bb_reg_async(0x0860, 0x1ffe_0000, 0x96a).await?;
            if center == 14 {
                self.write_u32_async(0x0a24, 0x0000_6577).await?;
                self.set_bb_reg_async(0x0a28, 0x0000_ffff, 0).await?;
            } else {
                self.write_u32_async(0x0a24, 0x384f_6577).await?;
                self.set_bb_reg_async(0x0a28, 0x0000_ffff, 0x1525).await?;
            }
        } else {
            let agc = if center <= 64 {
                Some(1)
            } else if center <= 144 {
                Some(2)
            } else {
                Some(3)
            };
            if let Some(agc) = agc {
                self.set_bb_reg_async(0x0958, 0x1f, agc).await?;
            }
            let cfo = match center {
                36..=48 => Some(0x494),
                52..=64 => Some(0x453),
                100..=116 => Some(0x452),
                118..=177 => Some(0x412),
                _ => None,
            };
            if let Some(cfo) = cfo {
                self.set_bb_reg_async(0x0860, 0x1ffe_0000, cfo).await?;
            }
        }

        if let Some(phase_noise) = rf_be_8822b(center) {
            self.set_rf_reg_async(
                chip,
                RfPath::A,
                0xbe,
                BIT15 | BIT16 | BIT17,
                u32::from(phase_noise),
            )
            .await?;
        }
        if center == 144 {
            self.set_rf_reg_async(chip, RfPath::A, 0xdf, BIT18, 1)
                .await?;
            rf18 |= BIT17;
        } else {
            self.set_rf_reg_async(chip, RfPath::A, 0xdf, BIT18, 0)
                .await?;
            if center > 144 {
                rf18 |= BIT18;
            } else if center >= 80 {
                rf18 |= BIT17;
            }
        }

        let mut bb8ac = self.read_u32_async(0x08ac).await.unwrap_or(0);
        let subchannel = u32::from((primary_index & 0x0f) << 2);
        match width {
            1 => {
                self.set_bb_reg_async(0x0a00, BIT4, u32::from(primary_index == 1))
                    .await?;
                bb8ac = (bb8ac & 0xff3f_f300) | subchannel | 1;
                self.write_u32_async(0x08ac, bb8ac).await?;
                self.set_bb_reg_async(0x08c4, BIT30, 1).await?;
                rf18 = (rf18 & !(BIT11 | BIT10)) | BIT11;
            }
            2 => {
                bb8ac = (bb8ac & 0xfcef_cf00) | subchannel | 2;
                self.write_u32_async(0x08ac, bb8ac).await?;
                self.set_bb_reg_async(0x08c4, BIT30, 1).await?;
                rf18 = (rf18 & !(BIT11 | BIT10)) | BIT10;
                if matches!(rfe_type, 2 | 3 | 17) {
                    self.set_bb_reg_async(0x0840, 0x0000_f000, 6).await?;
                    self.set_bb_reg_async(0x08c8, BIT10, 1).await?;
                }
            }
            _ => {
                bb8ac &= 0xffcf_fc00;
                self.write_u32_async(0x08ac, bb8ac).await?;
                self.set_bb_reg_async(0x08c4, BIT30, 1).await?;
                rf18 |= BIT11 | BIT10;
            }
        }
        if is_2g {
            rf18 &= 0x0006_0cff;
        }
        self.set_rf_reg_async(chip, RfPath::A, 0x18, RF_MASK, rf18)
            .await?;
        if chip.total_rf_paths() >= 2 {
            self.set_rf_reg_async(chip, RfPath::B, 0x18, RF_MASK, rf18)
                .await?;
        }
        self.set_rf_reg_async(chip, RfPath::A, 0xb8, BIT19, 0)
            .await?;
        self.set_rf_reg_async(chip, RfPath::A, 0xb8, BIT19, 1)
            .await?;

        for register in [0x0948, 0x094c] {
            self.set_bb_reg_async(register, BIT28 | BIT29, 2).await?;
        }
        for register in [0x0c20, 0x0e20] {
            self.set_bb_reg_async(register, BIT31, 1).await?;
        }
        let cca_column = if is_2g {
            usize::from(chip.total_rf_paths() >= 2)
        } else {
            2 + usize::from(chip.total_rf_paths() >= 2)
        };
        const CCA: [[u32; 4]; 3] = [
            [0x75c9_7010, 0x75c9_7010, 0x75c9_7010, 0x75c9_7010],
            [0x79a0_eaaa, 0x79a0_eaac, 0x79a0_eaaa, 0x79a0_eaaa],
            [0x8776_5541, 0x8774_6341, 0x8776_5541, 0x8774_6341],
        ];
        for (register, values) in [(0x082c, CCA[0]), (0x0830, CCA[1]), (0x0838, CCA[2])] {
            self.write_u32_async(register, values[cca_column]).await?;
        }
        let rx_path = if chip.total_rf_paths() >= 2 { 3 } else { 1 };
        self.set_bb_reg_async(0x0808, 0xff, 0).await?;
        self.set_bb_reg_async(0x0808, 0xff, rx_path | (rx_path << 4))
            .await?;
        self.toggle_igi_8822b_async().await
    }

    async fn toggle_igi_8822b_async(&self) -> Result<(), DriverError> {
        let igi = self.read_u32_async(0x0c50).await.unwrap_or(0) & 0x7f;
        for register in [0x0c50, 0x0e50] {
            self.set_bb_reg_async(register, 0x7f, igi.saturating_sub(2))
                .await?;
            self.set_bb_reg_async(register, 0x7f, igi).await?;
        }
        Ok(())
    }

    async fn lck_8822b_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        let aac = (self.query_rf_reg_async(chip, RfPath::A, 0xc9).await? & 0xf8) >> 3;
        if !(4..=7).contains(&aac) {
            self.set_rf_reg_async(chip, RfPath::A, 0xca, BIT19, 0)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0xb2, 0x7c000, 6)
                .await?;
        }
        let c00 = self.read_u32_async(0x0c00).await?;
        let e00 = self.read_u32_async(0x0e00).await?;
        self.write_u32_async(0x0c00, 4).await?;
        self.write_u32_async(0x0e00, 4).await?;
        for path in RfPath::iter(chip.total_rf_paths()) {
            self.set_rf_reg_async(chip, path, 0, RF_MASK, 0x10000)
                .await?;
        }
        let channel = self.query_rf_reg_async(chip, RfPath::A, 0x18).await?;
        self.set_rf_reg_async(chip, RfPath::A, 0xc4, RF_MASK, 0x01402)
            .await?;
        self.set_rf_reg_async(chip, RfPath::A, 0x18, RF_MASK, channel | 0x08000)
            .await?;
        sleep_ms(100).await;
        for _ in 0..5 {
            if self.query_rf_reg_async(chip, RfPath::A, 0x18).await? & 0x8000 == 0 {
                break;
            }
            sleep_ms(10).await;
        }
        self.set_rf_reg_async(chip, RfPath::A, 0x18, RF_MASK, channel)
            .await?;
        self.set_rf_reg_async(chip, RfPath::A, 0xc4, RF_MASK, 0x81402)
            .await?;
        self.write_u32_async(0x0c00, c00).await?;
        self.write_u32_async(0x0e00, e00).await?;
        for path in RfPath::iter(chip.total_rf_paths()) {
            self.set_rf_reg_async(chip, path, 0, RF_MASK, 0x3ffff)
                .await?;
        }
        Ok(())
    }

    async fn rfe_init_8822b_async(&self) -> Result<(), DriverError> {
        self.set_bb_reg_async(0x0064, BIT28 | BIT29, 3).await?;
        self.set_bb_reg_async(0x004c, BIT25 | BIT26, 0).await?;
        self.set_bb_reg_async(0x0040, BIT2, 1).await?;
        self.set_bb_reg_async(0x1990, 0x3f, 0x30).await?;
        self.set_bb_reg_async(0x1990, BIT10 | BIT11, 3).await?;
        self.set_bb_reg_async(0x0974, 0x3f, 0x3f).await?;
        self.set_bb_reg_async(0x0974, BIT10 | BIT11, 3).await
    }

    async fn bf_init_8822b_async(&self) -> Result<(), DriverError> {
        let mut value = self.read_u32_async(0x14c0).await.unwrap_or(0);
        value = ((value | BIT16) & !(0x0f << 12)) | (0x0a << 12);
        value &= !BIT7;
        value &= !0x3f;
        self.write_u32_async(0x14c0, value).await?;
        self.write_u8_async(0x167c, (3 << 4) | (1 << 6)).await?;
        self.write_u16_async(0x1680, 0).await?;
        self.write_u8_async(
            0x042f,
            self.read_u8_async(0x042f).await.unwrap_or(0) | (1 << 6),
        )
        .await?;
        self.write_u8_async(0x045f, 0x10).await?;
        self.write_u8_async(
            0x06df,
            (self.read_u8_async(0x06df).await.unwrap_or(0) & 0xc0) | 4,
        )
        .await?;
        self.write_u32_async(0x1c94, 0xafff_afff).await
    }

    async fn coex_wlan_only_8822b_async(&self, is_5g: bool) -> Result<(), DriverError> {
        for (register, mask, value) in [
            (0x004c, 0x0180_0000, 2),
            (0x0cb4, 0xff, 0x77),
            (0x0974, 0x300, 3),
            (0x1990, 0x300, 0),
            (0x0cbc, 0x80000, 0),
            (0x0070, 0xff00_0000, 0x0e),
            (0x1704, u32::MAX, 0x0000_7700),
            (0x1700, u32::MAX, 0xc00f_0038),
            (0x0cbc, 0x300, if is_5g { 1 } else { 2 }),
        ] {
            self.set_bb_reg_async(register, mask, value).await?;
        }
        Ok(())
    }

    async fn enable_rx_8822b_async(&self, accept_bad_fcs: bool) -> Result<(), DriverError> {
        self.write_u16_async(REG_CR, 0x06ff).await?;
        let mut rcr = 0x7000_382f;
        if accept_bad_fcs {
            rcr |= (1 << 8) | (1 << 9);
        }
        self.write_u32_async(REG_RCR, rcr).await?;
        for register in [0x0c50, 0x0e50] {
            self.set_bb_reg_async(register, 0x7f, 0x40).await?;
        }
        Ok(())
    }

    async fn apply_tx_power_8822b_async(
        &self,
        chip: ChipInfo,
        radio: RadioConfig,
        rfe_type: u8,
    ) -> Result<(), DriverError> {
        let Some(map) = self.efuse_logical_map.get() else {
            return Ok(());
        };
        let channel = radio.channel;
        if channel == 0 {
            return Ok(());
        }
        let is_5g = channel > 14;
        let bandwidth = match radio.channel_width {
            ChannelWidth::Mhz20 | ChannelWidth::Mhz5 | ChannelWidth::Mhz10 => 0,
            ChannelWidth::Mhz40 => 1,
            ChannelWidth::Mhz80 => 2,
        };
        let group = if is_5g {
            channel_group_5g_8822b(channel)
        } else {
            channel_group_2g_8822b(channel)
        };
        let cck_group = if channel == 14 { 5 } else { group };
        let band = u8::from(is_5g);
        let limit_cck = if is_5g {
            63
        } else {
            tx_power_limit_8822b(rfe_type, band, bandwidth, 0, 1, channel)
        };
        let limit_ofdm = tx_power_limit_8822b(rfe_type, band, bandwidth, 1, 1, channel);
        let limit_ht = tx_power_limit_8822b(rfe_type, band, bandwidth, 2, 1, channel);

        for path in 0..chip.total_rf_paths() {
            let block = 0x10 + path * 42 + usize::from(is_5g) * 18;
            let cck_base = if is_5g {
                0
            } else {
                map[block + cck_group as usize]
            };
            let bw40_base = if is_5g {
                map[block + group as usize]
            } else {
                map[block + 6 + group as usize]
            };
            if bw40_base == 0xff || (!is_5g && cck_base == 0xff) {
                continue;
            }
            let diff0 = map[block + if is_5g { 14 } else { 11 }];
            let diff1 = map[block + if is_5g { 15 } else { 12 }];
            let ofdm_diff = signed_nibble_8822b(diff0 & 0x0f);
            let bw20_diff0 = signed_nibble_8822b(diff0 >> 4);
            let bw20_diff1 = signed_nibble_8822b(diff1 & 0x0f);
            let ht_diff0 = if bandwidth == 0 { bw20_diff0 } else { 0 };
            let ht_diff1 = if bandwidth == 0 { bw20_diff1 } else { 0 };
            let clamp = |value: i16, limit: i8| value.clamp(0, i16::from(limit.min(63))) as u8;
            let cck = clamp(i16::from(cck_base), limit_cck);
            let ofdm = clamp(i16::from(bw40_base) + i16::from(ofdm_diff), limit_ofdm);
            let ht1 = clamp(i16::from(bw40_base) + i16::from(ht_diff0), limit_ht);
            let ht2 = clamp(
                i16::from(bw40_base) + i16::from(ht_diff0) + i16::from(ht_diff1),
                limit_ht,
            );
            let base = 0x1d00 + path as u16 * 0x80;
            if !is_5g {
                self.write_u32_async(base, u32::from(cck) * 0x0101_0101)
                    .await?;
            }
            for (offset, value) in [
                (0x04, ofdm),
                (0x08, ofdm),
                (0x0c, ht1),
                (0x10, ht1),
                (0x14, ht2),
                (0x18, ht2),
            ] {
                self.write_u32_async(base + offset, u32::from(value) * 0x0101_0101)
                    .await?;
            }
        }
        Ok(())
    }

    /// Apply a flat 8822B TXAGC index to every rate on active paths.
    pub async fn set_tx_power_flat_8822b_async(&self, index: u8) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        let value = u32::from(index.min(0x3f)) * 0x0101_0101;
        for path in 0..chip.total_rf_paths() {
            let base = 0x1d00 + path as u16 * 0x80;
            for offset in (0..=0x54).step_by(4) {
                self.write_u32_async(base + offset, value).await?;
            }
        }
        Ok(())
    }

    /// Run one 8822B dynamic-initial-gain update.
    pub async fn run_jaguar2_dig_step_async(&self) -> Result<u32, DriverError> {
        let false_alarms = (self.read_u32_async(0x0f48).await? & 0xffff)
            + (self.read_u32_async(0x0a5c).await? & 0xffff);
        self.set_bb_reg_async(0x09a4, BIT17, 1).await?;
        self.set_bb_reg_async(0x09a4, BIT17, 0).await?;
        self.set_bb_reg_async(0x0a2c, BIT15, 0).await?;
        self.set_bb_reg_async(0x0a2c, BIT15, 1).await?;
        self.set_bb_reg_async(0x0b58, BIT0, 1).await?;
        self.set_bb_reg_async(0x0b58, BIT0, 0).await?;
        let igi = self.read_u8_async(0x0c50).await? & 0x7f;
        let next = if false_alarms > 750 {
            igi.saturating_add(2)
        } else if false_alarms > 500 {
            igi.saturating_add(1)
        } else if false_alarms < 250 {
            igi.saturating_sub(2)
        } else {
            igi
        }
        .clamp(0x1c, 0x3e);
        if next != igi {
            for register in [0x0c50, 0x0e50] {
                self.set_bb_reg_async(register, 0x7f, u32::from(next))
                    .await?;
            }
        }
        Ok(false_alarms)
    }
}

fn center_channel_8822b(primary: u8, width: u8, primary_index: u8) -> u8 {
    match width {
        1 if primary_index == 2 => primary.saturating_sub(2),
        1 => primary.saturating_add(2),
        2 => {
            let offset = [0i16, 6, 2, -2, -6]
                .get(primary_index as usize)
                .copied()
                .unwrap_or(0);
            (i16::from(primary) + offset).clamp(0, u8::MAX as i16) as u8
        }
        _ => primary,
    }
}

fn rf_be_8822b(channel: u8) -> Option<u8> {
    const LOW: [u8; 15] = [7, 6, 6, 5, 0, 0, 7, 0xff, 6, 5, 0, 0, 7, 6, 6];
    const MIDDLE: [u8; 23] = [
        6, 5, 0, 0, 7, 6, 6, 0xff, 0, 0, 7, 6, 6, 5, 0, 0xff, 7, 6, 6, 5, 0, 0, 7,
    ];
    const HIGH: [u8; 15] = [5, 5, 0, 7, 7, 6, 5, 0xff, 0, 7, 7, 6, 5, 5, 0];
    let value = match channel {
        1..=14 => 0,
        36..=64 => LOW[((channel - 36) >> 1) as usize],
        100..=144 => MIDDLE[((channel - 100) >> 1) as usize],
        149..=177 => HIGH[((channel - 149) >> 1) as usize],
        _ => return None,
    };
    (value != 0xff).then_some(value)
}

fn signed_nibble_8822b(value: u8) -> i8 {
    let value = value & 0x0f;
    if value & 0x08 != 0 {
        value as i8 - 16
    } else {
        value as i8
    }
}

fn channel_group_2g_8822b(channel: u8) -> u8 {
    match channel {
        1..=2 => 0,
        3..=5 => 1,
        6..=8 => 2,
        9..=11 => 3,
        _ => 4,
    }
}

fn channel_group_5g_8822b(channel: u8) -> u8 {
    match channel {
        0..=42 => 0,
        43..=48 => 1,
        49..=58 => 2,
        59..=80 => 3,
        81..=106 => 4,
        107..=114 => 5,
        115..=122 => 6,
        123..=130 => 7,
        131..=138 => 8,
        139..=144 => 9,
        145..=155 => 10,
        156..=161 => 11,
        162..=171 => 12,
        _ => 13,
    }
}

fn tx_power_limit_8822b(
    rfe_type: u8,
    band: u8,
    bandwidth: u8,
    section: u8,
    streams: u8,
    channel: u8,
) -> i8 {
    let table = if rfe_type == 3 {
        rtl_data::RTL8822B_TX_POWER_LIMITS_TYPE3_WW
    } else {
        rtl_data::RTL8822B_TX_POWER_LIMITS_WW
    };
    let mut best = 63;
    let mut distance = u8::MAX;
    for entry in table {
        if entry.band != band
            || entry.bandwidth != bandwidth
            || entry.section != section
            || entry.streams != streams
        {
            continue;
        }
        if entry.channel == channel {
            return entry.limit;
        }
        let candidate = entry.channel.abs_diff(channel);
        if candidate < distance {
            distance = candidate;
            best = entry.limit;
        }
    }
    best
}

fn le32_at(bytes: &[u8], offset: usize) -> Result<u32, DriverError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| DriverError::Nusb("RTL8822B firmware header is truncated".to_owned()))
}

fn validate_firmware_8822b(firmware: &[u8]) -> Result<(), DriverError> {
    if firmware.len() < WLAN_FW_HDR_SIZE {
        return Err(DriverError::Nusb(
            "RTL8822B firmware is shorter than its header".to_owned(),
        ));
    }
    let dmem = le32_at(firmware, WLAN_FW_HDR_DMEM_SIZE)? as usize + WLAN_FW_HDR_CHKSUM_SIZE;
    let imem = le32_at(firmware, WLAN_FW_HDR_IMEM_SIZE)? as usize + WLAN_FW_HDR_CHKSUM_SIZE;
    let emem = if firmware[WLAN_FW_HDR_MEM_USAGE] & (1 << 4) != 0 {
        le32_at(firmware, WLAN_FW_HDR_EMEM_SIZE)? as usize + WLAN_FW_HDR_CHKSUM_SIZE
    } else {
        0
    };
    let expected = WLAN_FW_HDR_SIZE + dmem + imem + emem;
    if firmware.len() != expected {
        return Err(DriverError::Nusb(format!(
            "RTL8822B firmware size mismatch: {} bytes, header describes {expected}",
            firmware.len()
        )));
    }
    Ok(())
}
