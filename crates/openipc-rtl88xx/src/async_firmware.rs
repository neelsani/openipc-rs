use crate::device::RealtekDevice;
use crate::firmware::strip_firmware_header;
use crate::power::{PowerCommand, PowerStep, RTL8812_POWER_ON_FLOW, RTL8821_POWER_ON_FLOW};
use crate::regs::*;
use crate::rtl_data;
use crate::time::{sleep_micros, sleep_ms, yield_now, DateNow};
use crate::types::{ChipFamily, ChipInfo, DriverError};

impl RealtekDevice {
    pub(crate) async fn power_on_jaguar_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        if chip.family != ChipFamily::Rtl8821 {
            self.write_u8_async(REG_RF_CTRL, 5).await?;
            self.write_u8_async(REG_RF_CTRL, 7).await?;
            self.write_u8_async(REG_RF_B_CTRL_8812, 5).await?;
            self.write_u8_async(REG_RF_B_CTRL_8812, 7).await?;
        }

        let flow = match chip.family {
            ChipFamily::Rtl8821 => RTL8821_POWER_ON_FLOW,
            _ => RTL8812_POWER_ON_FLOW,
        };
        self.run_power_sequence_async(flow).await?;

        let mut cr = self.read_u16_async(REG_CR).await.unwrap_or(0);
        cr |= HCI_TXDMA_EN
            | HCI_RXDMA_EN
            | TXDMA_EN
            | RXDMA_EN
            | PROTOCOL_EN
            | SCHEDULE_EN
            | ENSEC
            | CALTMR_EN;
        self.write_u16_async(REG_CR, cr).await?;
        let txdma = self.read_u32_async(REG_TXDMA_OFFSET_CHK).await.unwrap_or(0);
        self.write_u32_async(REG_TXDMA_OFFSET_CHK, txdma | DROP_DATA_EN)
            .await
    }

    async fn run_power_sequence_async(&self, flow: &[PowerStep]) -> Result<(), DriverError> {
        for step in flow {
            match step.cmd {
                PowerCommand::Write => {
                    let current = self.read_u8_async(step.offset).await?;
                    let next = (current & !step.mask) | (step.value & step.mask);
                    self.write_u8_async(step.offset, next).await?;
                }
                PowerCommand::Polling => {
                    let deadline = DateNow::deadline_ms(50.0);
                    loop {
                        let current = self.read_u8_async(step.offset).await?;
                        if current & step.mask == step.value & step.mask {
                            break;
                        }
                        if DateNow::expired(deadline) {
                            return Err(DriverError::Nusb(format!(
                                "power sequence poll 0x{:04x} mask=0x{:02x} value=0x{:02x} timed out",
                                step.offset, step.mask, step.value
                            )));
                        }
                        sleep_micros(10).await;
                    }
                }
                PowerCommand::DelayMs => sleep_ms(step.offset as u32).await,
            }
        }
        Ok(())
    }

    pub(crate) async fn init_llt_table_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        let txpktbuf_bndy = match chip.family {
            ChipFamily::Rtl8821 => TX_PAGE_BOUNDARY_8821 as u32,
            ChipFamily::Rtl8812 => TX_PAGE_BOUNDARY_8812 as u32,
            ChipFamily::Rtl8814 => return Ok(()),
        };

        for page in 0..txpktbuf_bndy.saturating_sub(1) {
            self.llt_write_async(page, page + 1).await?;
        }
        self.llt_write_async(txpktbuf_bndy - 1, 0xff).await?;

        for page in txpktbuf_bndy..LAST_ENTRY_OF_TX_PKT_BUFFER_8812 {
            self.llt_write_async(page, page + 1).await?;
        }
        self.llt_write_async(LAST_ENTRY_OF_TX_PKT_BUFFER_8812, txpktbuf_bndy)
            .await
    }

    async fn llt_write_async(&self, address: u32, data: u32) -> Result<(), DriverError> {
        const LLT_WRITE_ACCESS: u32 = 1;
        const POLLING_LLT_THRESHOLD: usize = 20;
        let value = ((address & 0xff) << 8) | (data & 0xff) | (LLT_WRITE_ACCESS << 30);
        self.write_u32_async(REG_LLT_INIT, value).await?;
        for _ in 0..=POLLING_LLT_THRESHOLD {
            let state = self.read_u32_async(REG_LLT_INIT).await?;
            if ((state >> 30) & 0x3) == 0 {
                return Ok(());
            }
            sleep_micros(10).await;
        }
        Err(DriverError::Nusb(format!(
            "LLT write timed out address=0x{address:02x} data=0x{data:02x}"
        )))
    }

    pub(crate) async fn init_hardware_drop_incorrect_bulk_out_async(
        &self,
    ) -> Result<(), DriverError> {
        let txdma = self.read_u32_async(REG_TXDMA_OFFSET_CHK).await.unwrap_or(0);
        self.write_u32_async(REG_TXDMA_OFFSET_CHK, txdma | DROP_DATA_EN)
            .await
    }

    pub(crate) async fn download_firmware_8812_family_async(
        &self,
        chip: ChipInfo,
    ) -> Result<(), DriverError> {
        let firmware = match chip.family {
            ChipFamily::Rtl8812 => rtl_data::RTL8812_FW_NIC,
            ChipFamily::Rtl8821 => rtl_data::RTL8821_FW_NIC,
            ChipFamily::Rtl8814 => return Err(DriverError::UnsupportedFirmwarePath(chip.family)),
        };
        let firmware = strip_firmware_header(chip.family, firmware);

        if self.read_u8_async(REG_MCUFWDL).await? & RAM_DL_SEL as u8 != 0 {
            self.write_u8_async(REG_MCUFWDL, 0).await?;
            self.reset_8051_async().await?;
        }

        self.firmware_download_enable_8812_async(true).await?;
        let mut ok = false;
        let start = DateNow::now();
        for attempt in 0..3 {
            let state = self.read_u8_async(REG_MCUFWDL).await?;
            self.write_u8_async(REG_MCUFWDL, state | FWDL_CHKSUM_RPT as u8)
                .await?;
            self.write_firmware_pages_async(firmware).await?;
            if self.poll_firmware_checksum_async(50.0, 5).await.is_ok() {
                ok = true;
                break;
            }
            if attempt == 2 && DateNow::now() - start < 500.0 {
                continue;
            }
        }
        self.firmware_download_enable_8812_async(false).await?;
        if !ok {
            return Err(DriverError::FirmwareChecksumTimeout);
        }
        self.firmware_free_to_go_8812_async().await?;
        self.write_u8_async(REG_HMETFR, 0x0f).await
    }

    async fn firmware_download_enable_8812_async(&self, enable: bool) -> Result<(), DriverError> {
        if enable {
            let tmp = self.read_u8_async(REG_MCUFWDL).await?;
            self.write_u8_async(REG_MCUFWDL, tmp | 0x01).await?;
            let tmp = self.read_u8_async(REG_MCUFWDL + 2).await?;
            self.write_u8_async(REG_MCUFWDL + 2, tmp & 0xf7).await?;
        } else {
            let tmp = self.read_u8_async(REG_MCUFWDL).await?;
            self.write_u8_async(REG_MCUFWDL, tmp & 0xfe).await?;
        }
        Ok(())
    }

    async fn write_firmware_pages_async(&self, firmware: &[u8]) -> Result<(), DriverError> {
        const MAX_PAGE: usize = 4096;
        for (page, chunk) in firmware.chunks(MAX_PAGE).enumerate() {
            let value = (self.read_u8_async(REG_MCUFWDL + 2).await? & 0xf8) | ((page as u8) & 0x07);
            self.write_u8_async(REG_MCUFWDL + 2, value).await?;
            self.block_write_firmware_async(chunk).await?;
        }
        Ok(())
    }

    async fn block_write_firmware_async(&self, chunk: &[u8]) -> Result<(), DriverError> {
        const MAX_BLOCK: usize = 196;
        let mut offset = 0usize;
        while offset + MAX_BLOCK <= chunk.len() {
            self.write_register_async(
                FW_START_ADDRESS + offset as u16,
                &chunk[offset..offset + MAX_BLOCK],
            )
            .await?;
            offset += MAX_BLOCK;
        }
        while offset + 8 <= chunk.len() {
            self.write_register_async(FW_START_ADDRESS + offset as u16, &chunk[offset..offset + 8])
                .await?;
            offset += 8;
        }
        while offset < chunk.len() {
            self.write_u8_async(FW_START_ADDRESS + offset as u16, chunk[offset])
                .await?;
            offset += 1;
        }
        Ok(())
    }

    async fn poll_firmware_checksum_async(
        &self,
        timeout_ms: f64,
        min_reads: u32,
    ) -> Result<(), DriverError> {
        let deadline = DateNow::deadline_ms(timeout_ms);
        let mut reads = 0;
        loop {
            reads += 1;
            let state = self.read_u32_async(REG_MCUFWDL).await?;
            if state & FWDL_CHKSUM_RPT != 0 {
                return Ok(());
            }
            if DateNow::expired(deadline) && reads >= min_reads {
                return Err(DriverError::FirmwareChecksumTimeout);
            }
            yield_now().await;
        }
    }

    async fn firmware_free_to_go_8812_async(&self) -> Result<(), DriverError> {
        let mut state = self.read_u32_async(REG_MCUFWDL).await?;
        state |= MCUFWDL_RDY;
        state &= !WINTINI_RDY;
        self.write_u32_async(REG_MCUFWDL, state).await?;
        self.reset_8051_async().await?;

        let deadline = DateNow::deadline_ms(200.0);
        let mut reads = 0;
        loop {
            reads += 1;
            let state = self.read_u32_async(REG_MCUFWDL).await?;
            if state & WINTINI_RDY != 0 {
                return Ok(());
            }
            if DateNow::expired(deadline) && reads >= 10 {
                return Err(DriverError::FirmwareReadyTimeout);
            }
            yield_now().await;
        }
    }

    async fn reset_8051_async(&self) -> Result<(), DriverError> {
        const REG_RSV_CTRL: u16 = 0x001c;
        let tmp = self.read_u8_async(REG_RSV_CTRL).await?;
        self.write_u8_async(REG_RSV_CTRL, tmp & !(BIT1 as u8))
            .await?;
        let tmp = self.read_u8_async(REG_RSV_CTRL + 1).await?;
        self.write_u8_async(REG_RSV_CTRL + 1, tmp & !(BIT3 as u8))
            .await?;

        let tmp = self.read_u8_async(REG_SYS_FUNC_EN + 1).await?;
        self.write_u8_async(REG_SYS_FUNC_EN + 1, tmp & !(BIT2 as u8))
            .await?;

        let tmp2 = self.read_u8_async(REG_RSV_CTRL).await?;
        self.write_u8_async(REG_RSV_CTRL, tmp2 & !(BIT1 as u8))
            .await?;
        let tmp2 = self.read_u8_async(REG_RSV_CTRL + 1).await?;
        self.write_u8_async(REG_RSV_CTRL + 1, tmp2 | BIT3 as u8)
            .await?;
        self.write_u8_async(REG_SYS_FUNC_EN + 1, tmp | BIT2 as u8)
            .await
    }
}
