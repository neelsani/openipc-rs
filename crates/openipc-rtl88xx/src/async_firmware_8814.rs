use crate::device::RealtekDevice;
use crate::regs::*;
use crate::rtl_data;
use crate::time::{sleep_micros, sleep_ms, yield_now, DateNow};
use crate::types::DriverError;

impl RealtekDevice {
    pub(crate) async fn download_firmware_8814_async(&self) -> Result<(), DriverError> {
        let state = self.read_u32_async(REG_MCUFWDL).await?;
        if state & 0xff == 0x78 {
            return Ok(());
        }

        let firmware = rtl_data::RTL8814_FW_NIC;
        let dmem_size = le32_at(firmware, 36)? as usize + RTL8814_FW_CHECKSUM_DUMMY_SIZE;
        let iram_size = le32_at(firmware, 48)? as usize + RTL8814_FW_CHECKSUM_DUMMY_SIZE;
        if dmem_size + iram_size + RTL8814_FW_HEADER_SIZE != firmware.len() {
            return Err(DriverError::Nusb(format!(
                "RTL8814 firmware header mismatch: dmem={dmem_size} iram={iram_size} header={} blob={}",
                RTL8814_FW_HEADER_SIZE,
                firmware.len()
            )));
        }

        self.power_on_prefix_8814_async().await?;
        self.pre_firmware_staging_8814_async().await?;
        self.fwdl_kernel_path_8814_async(firmware, dmem_size, iram_size)
            .await
    }

    async fn power_on_prefix_8814_async(&self) -> Result<(), DriverError> {
        self.read_u32_discard_8814_async(0x00f0).await?;
        self.write_u8_async(0x001c, 0x00).await?;
        self.read_u32_discard_8814_async(0x0064).await?;
        self.write_u32_async(0x0064, 0x3020_0000).await?;
        self.read_u32_discard_8814_async(0x004c).await?;
        self.write_u32_async(0x004c, 0x6022_8282).await?;
        self.read_u32_discard_8814_async(0x0040).await?;
        self.write_u32_async(0x0040, 0x0000_0004).await?;
        self.read_u8_discard_8814_async(0x0002).await?;
        self.write_u8_async(0x0002, 0x1c).await?;
        self.read_u8_discard_8814_async(0x001f).await?;
        self.write_u8_async(0x001f, 0x00).await?;
        self.read_u32_discard_8814_async(0x00ec).await?;
        self.write_u32_async(0x00ec, 0x3045_3f15).await?;
        self.read_u8_discard_8814_async(0xfe58).await?;
        self.read_u16_discard_8814_async(0x0080).await?;
        self.read_u8_discard_8814_async(0x0100).await?;
        self.read_u8_discard_8814_async(0xfe58).await?;
        self.read_u16_discard_8814_async(0x0080).await?;
        self.read_u8_discard_8814_async(0x0100).await?;
        self.read_u8_discard_8814_async(0x0c00).await?;
        self.write_u8_async(0x0c00, 0x04).await?;
        self.read_u8_discard_8814_async(0x0e00).await?;
        self.write_u8_async(0x0e00, 0x04).await?;
        self.read_u8_discard_8814_async(0x1002).await?;
        self.write_u8_async(0x1002, 0xac).await?;
        self.read_u8_discard_8814_async(0x001f).await?;
        self.write_u8_async(0x001f, 0x00).await?;
        self.read_u8_discard_8814_async(0x0007).await?;
        self.write_u8_async(0x0007, 0x28).await?;
        self.read_u8_discard_8814_async(0x0008).await?;
        self.write_u8_async(0x0008, 0x21).await?;
        self.read_u8_discard_8814_async(0x0066).await?;
        self.write_u8_async(0x0066, 0x20).await?;
        self.read_u8_discard_8814_async(0x0041).await?;
        self.write_u8_async(0x0041, 0x00).await?;
        self.read_u8_discard_8814_async(0x0042).await?;
        self.write_u8_async(0x0042, 0x00).await?;
        self.read_u8_discard_8814_async(0x004e).await?;
        self.write_u8_async(0x004e, 0x22).await?;
        self.read_u8_discard_8814_async(0x0041).await?;
        self.write_u8_async(0x0041, 0x00).await?;
        self.read_u8_discard_8814_async(0x0005).await?;
        self.write_u8_async(0x0005, 0x02).await?;
        self.read_u8_discard_8814_async(0x0005).await?;
        self.read_u8_discard_8814_async(0x0005).await?;
        self.read_u8_discard_8814_async(0x0003).await?;
        self.write_u8_async(0x0003, 0x72).await?;
        self.read_u8_discard_8814_async(0x0080).await?;
        self.write_u8_async(0x0080, 0x01).await?;
        self.read_u8_discard_8814_async(0x0081).await?;
        self.write_u8_async(0x0081, 0x30).await?;
        self.read_u8_discard_8814_async(0x0045).await?;
        self.write_u8_async(0x0045, 0x00).await?;
        self.read_u8_discard_8814_async(0x0046).await?;
        self.write_u8_async(0x0046, 0xff).await?;
        self.read_u8_discard_8814_async(0x0047).await?;
        self.write_u8_async(0x0047, 0x00).await?;
        self.read_u8_discard_8814_async(0x0015).await?;
        self.write_u8_async(0x0015, 0xcf).await?;
        self.read_u8_discard_8814_async(0x0015).await?;
        self.write_u8_async(0x0015, 0xef).await?;
        self.read_u8_discard_8814_async(0x0012).await?;
        self.write_u8_async(0x0012, 0x83).await?;
        self.read_u8_discard_8814_async(0x0023).await?;
        self.write_u8_async(0x0023, 0x15).await?;
        self.read_u8_discard_8814_async(0x0008).await?;
        self.write_u8_async(0x0008, 0x21).await?;
        self.read_u8_discard_8814_async(0x0007).await?;
        self.write_u8_async(0x0007, 0x20).await?;
        self.read_u8_discard_8814_async(0x001f).await?;
        self.write_u8_async(0x001f, 0x00).await?;
        self.read_u8_discard_8814_async(0x0020).await?;
        self.write_u8_async(0x0020, 0x00).await?;
        self.read_u8_discard_8814_async(0x0021).await?;
        self.write_u8_async(0x0021, 0x00).await?;
        self.read_u8_discard_8814_async(0x0076).await?;
        self.write_u8_async(0x0076, 0x00).await?;
        self.read_u8_discard_8814_async(0x0091).await?;
        self.write_u8_async(0x0091, 0xe1).await?;
        self.read_u8_discard_8814_async(0x0070).await?;
        self.write_u8_async(0x0070, 0x08).await?;
        self.read_u8_discard_8814_async(0x0005).await?;
        self.write_u8_async(0x0005, 0x08).await?;
        self.write_u8_async(0x001c, 0x00).await?;
        self.read_u32_discard_8814_async(0x0064).await?;
        self.write_u32_async(0x0064, 0x3120_0000).await?;
        self.read_u32_discard_8814_async(0x004c).await?;
        self.write_u32_async(0x004c, 0x6022_8282).await?;
        self.read_u32_discard_8814_async(0x0040).await?;
        self.write_u32_async(0x0040, 0x0000_0004).await?;
        self.read_u8_discard_8814_async(0x0002).await?;
        self.write_u8_async(0x0002, 0x1c).await?;
        self.read_u8_discard_8814_async(0x001f).await?;
        self.write_u8_async(0x001f, 0x00).await?;
        self.read_u32_discard_8814_async(0x00ec).await?;
        self.write_u32_async(0x00ec, 0x3045_3f15).await?;
        self.read_u8_discard_8814_async(0xfe58).await?;
        self.read_u16_discard_8814_async(0x0080).await?;
        self.read_u8_discard_8814_async(0x0100).await?;
        self.read_u8_discard_8814_async(0x10c2).await?;
        self.write_u8_async(0x10c2, 0x22).await?;
        self.read_u8_discard_8814_async(0x0012).await?;
        self.write_u8_async(0x0012, 0xc3).await?;
        self.read_u8_discard_8814_async(0x0015).await?;
        self.write_u8_async(0x0015, 0xcf).await?;
        self.read_u8_discard_8814_async(0x0015).await?;
        self.write_u8_async(0x0015, 0x8f).await?;
        self.read_u8_discard_8814_async(0x0023).await?;
        self.write_u8_async(0x0023, 0x05).await?;
        self.read_u8_discard_8814_async(0x0046).await?;
        self.write_u8_async(0x0046, 0x00).await?;
        self.read_u8_discard_8814_async(0x0062).await?;
        self.write_u8_async(0x0062, 0x00).await?;
        self.read_u8_discard_8814_async(0x0005).await?;
        self.write_u8_async(0x0005, 0x00).await?;
        self.read_u8_discard_8814_async(0x0005).await?;
        self.write_u8_async(0x0005, 0x00).await?;
        self.read_u8_discard_8814_async(0x0006).await?;
        self.read_u8_discard_8814_async(0x0005).await?;
        self.write_u8_async(0x0005, 0x00).await?;
        self.read_u8_discard_8814_async(0x00f0).await?;
        self.write_u8_async(0x00f0, 0x35).await?;
        self.read_u8_discard_8814_async(0x0081).await?;
        self.write_u8_async(0x0081, 0x20).await?;
        self.read_u8_discard_8814_async(0x0005).await?;
        self.write_u8_async(0x0005, 0x01).await?;
        for _ in 0..75 {
            if self.read_u8_async(0x0005).await? & 0x01 == 0 {
                break;
            }
            yield_now().await;
        }
        Ok(())
    }

    async fn pre_firmware_staging_8814_async(&self) -> Result<(), DriverError> {
        self.read_u8_discard_8814_async(0x0003).await?;
        self.write_u8_async(0x0003, 0xfe).await?;
        self.read_u8_discard_8814_async(0x1103).await?;
        self.write_u8_async(0x1103, 0x0c).await?;
        self.read_u32_discard_8814_async(0x0080).await?;
        self.write_u8_async(0x01a0, 0xfd).await?;
        self.read_u8_discard_8814_async(0x001d).await?;
        self.write_u8_async(0x001d, 0x08).await?;
        self.read_u8_discard_8814_async(0x010d).await?;
        self.write_u8_async(0x010d, 0xc0).await?;
        self.read_u8_discard_8814_async(0x0100).await?;
        self.write_u8_async(0x0100, 0x05).await?;
        self.write_u32_async(0x1330, 0x8000_0000).await?;
        self.read_u16_discard_8814_async(0x0230).await?;
        self.read_u32_discard_8814_async(0x022c).await?;
        self.write_u16_async(0x0230, 0x0200).await?;
        self.write_u32_async(0x022c, 0x8000_0000).await?;
        self.read_u8_discard_8814_async(0x1082).await?;
        self.write_u8_async(0x1082, 0x80).await?;
        self.read_u8_discard_8814_async(0x0009).await?;
        self.write_u8_async(0x0009, 0xbc).await?;
        self.read_u8_discard_8814_async(0x1082).await?;
        self.write_u8_async(0x1082, 0x81).await?;
        self.read_u8_discard_8814_async(0x0009).await?;
        self.write_u8_async(0x0009, 0xfc).await
    }

    async fn fwdl_kernel_path_8814_async(
        &self,
        firmware: &[u8],
        dmem_size: usize,
        iram_size: usize,
    ) -> Result<(), DriverError> {
        self.firmware_download_enable_8814_async(true).await?;
        self.enable_3081_8814_async(false).await?;
        self.ddma_reset_8814_async().await?;

        let bcn_ctrl = self.read_u8_async(REG_BCN_CTRL).await?;
        let cr1 = self.read_u8_async(REG_CR + 1).await?;
        self.write_u8_async(REG_CR + 1, cr1 | BIT0 as u8).await?;
        self.write_u8_async(REG_BCN_CTRL, (bcn_ctrl & !EN_BCN_FUNCTION) | DIS_TSF_UDT)
            .await?;

        let fwhw_txq_ctrl2 = self.read_u8_async(REG_FWHW_TXQ_CTRL + 2).await?;
        self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, fwhw_txq_ctrl2 & !(BIT6 as u8))
            .await?;

        let txpktbuf_bndy = 0u16;
        self.write_u16_async(REG_FIFOPAGE_CTRL_2_8814, txpktbuf_bndy)
            .await?;
        let bcn_valid = self.read_u8_async(REG_FIFOPAGE_CTRL_2_8814 + 1).await?;
        self.write_u8_async(REG_FIFOPAGE_CTRL_2_8814 + 1, bcn_valid | BIT7 as u8)
            .await?;

        let source = OCPBASE_TXBUF_3081
            + u32::from(txpktbuf_bndy) * RTL8814_TX_PAGE_SIZE
            + RTL8814_TXDESC_OFFSET as u32;
        let dmem = &firmware[RTL8814_FW_HEADER_SIZE..RTL8814_FW_HEADER_SIZE + dmem_size];
        let iram_start = RTL8814_FW_HEADER_SIZE + dmem_size;
        let iram = &firmware[iram_start..iram_start + iram_size];

        let mut result = self
            .stream_firmware_section_8814_async(dmem, OCPBASE_DMEM_3081, source)
            .await;
        if result.is_ok() {
            result = self
                .stream_firmware_section_8814_async(iram, OCPBASE_IMEM_3081, source)
                .await;
        }

        if result.is_ok() {
            self.write_u8_async(REG_BCN_CTRL, bcn_ctrl).await?;
            if fwhw_txq_ctrl2 & BIT6 as u8 != 0 {
                self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, fwhw_txq_ctrl2)
                    .await?;
            }
            let cr1 = self.read_u8_async(REG_CR + 1).await?;
            self.write_u8_async(REG_CR + 1, cr1 & !(BIT0 as u8)).await?;

            let fwctrl = self.read_u8_async(REG_MCUFWDL).await?;
            if fwctrl & (DMEM_CHKSUM_OK_8814 | IMEM_CHKSUM_OK_8814)
                == DMEM_CHKSUM_OK_8814 | IMEM_CHKSUM_OK_8814
            {
                let fwctrl1 = self.read_u8_async(REG_MCUFWDL + 1).await?;
                self.write_u8_async(REG_MCUFWDL + 1, fwctrl1 | BIT6 as u8)
                    .await?;
            } else {
                result = Err(DriverError::FirmwareChecksumTimeout);
            }
        }

        self.enable_3081_8814_async(true).await?;
        self.firmware_download_enable_8814_async(false).await?;
        result?;
        self.firmware_free_to_go_8814_async().await?;
        self.write_u8_async(REG_HMETFR, 0x0f).await
    }

    async fn stream_firmware_section_8814_async(
        &self,
        section: &[u8],
        ocp_dest: u32,
        tx_fifo_source: u32,
    ) -> Result<(), DriverError> {
        let mut offset = 0usize;
        while offset < section.len() {
            let remaining = section.len() - offset;
            let mut chunk_len;
            let last_segment;
            if remaining > RTL8814_MAX_RSVD_PAGE_BUF_SIZE {
                chunk_len = RTL8814_MAX_RSVD_PAGE_BUF_SIZE;
                last_segment = false;
                let final_block_len = remaining - RTL8814_MAX_RSVD_PAGE_BUF_SIZE;
                if final_block_len < RTL8814_MAX_RSVD_PAGE_BUF_SIZE
                    && ((final_block_len + RTL8814_TXDESC_OFFSET) & 0x3f) == 0
                {
                    chunk_len -= 4;
                }
            } else {
                chunk_len = remaining;
                last_segment = true;
            }
            if chunk_len > RTL8814_MAX_RSVD_PAGE_CHUNK_SIZE {
                return Err(DriverError::Nusb(format!(
                    "RTL8814 firmware reserved-page chunk too large: {chunk_len}"
                )));
            }

            let chunk = &section[offset..offset + chunk_len];
            self.set_download_fw_rsvd_page_8814_async(chunk).await?;
            self.wait_download_rsvd_page_ok_8814_async().await?;
            self.iddma_download_fw_3081_async(
                tx_fifo_source,
                ocp_dest + offset as u32,
                chunk_len as u32,
                offset == 0,
                last_segment,
            )
            .await?;
            offset += chunk_len;
        }
        Ok(())
    }

    async fn set_download_fw_rsvd_page_8814_async(&self, chunk: &[u8]) -> Result<(), DriverError> {
        let packet = build_rsvd_page_packet_8814(chunk);
        let sent = self.write_tx_transfer_raw_async(&packet).await?;
        if sent != packet.len() {
            return Err(DriverError::Nusb(format!(
                "RTL8814 reserved-page bulk write sent {sent} of {} bytes",
                packet.len()
            )));
        }
        Ok(())
    }

    async fn wait_download_rsvd_page_ok_8814_async(&self) -> Result<(), DriverError> {
        for _ in 0..200 {
            let value = self.read_u8_async(REG_FIFOPAGE_CTRL_2_8814 + 1).await?;
            if value & BIT7 as u8 != 0 {
                self.write_u8_async(REG_FIFOPAGE_CTRL_2_8814 + 1, value | BIT7 as u8)
                    .await?;
                return Ok(());
            }
            sleep_micros(1000).await;
        }
        Err(DriverError::Nusb(
            "RTL8814 reserved-page download did not report completion".to_string(),
        ))
    }

    async fn iddma_download_fw_3081_async(
        &self,
        source: u32,
        dest: u32,
        length: u32,
        first_segment: bool,
        last_segment: bool,
    ) -> Result<(), DriverError> {
        self.wait_ddma_idle_8814_async("pre").await?;

        let mut ctrl = DDMA_CHKSUM_EN_8814 | DDMA_CH_OWN_8814 | (length & DDMA_LEN_MASK_8814);
        if !first_segment {
            ctrl |= DDMA_CH_CHKSUM_CNT_8814;
        }
        self.write_u32_async(REG_DDMA_CH0SA_8814, source).await?;
        self.write_u32_async(REG_DDMA_CH0DA_8814, dest).await?;
        self.write_u32_async(REG_DDMA_CH0CTRL_8814, ctrl).await?;

        self.wait_ddma_idle_8814_async("post").await?;

        if last_segment {
            let fwctrl = self.read_u8_async(REG_MCUFWDL).await?;
            let ddma = self.read_u32_async(REG_DDMA_CH0CTRL_8814).await?;
            if ddma & DDMA_CHKSUM_FAIL_8814 != 0 {
                self.write_u32_async(REG_DDMA_CH0CTRL_8814, ddma | DDMA_RST_CHKSUM_STS_8814)
                    .await?;
                let clear = if dest < OCPBASE_DMEM_3081 {
                    fwctrl & !(IMEM_DL_RDY_8814 | IMEM_CHKSUM_OK_8814)
                } else {
                    fwctrl & !(DMEM_DL_RDY_8814 | DMEM_CHKSUM_OK_8814)
                };
                self.write_u8_async(REG_MCUFWDL, clear).await?;
                return Err(DriverError::FirmwareChecksumTimeout);
            }

            let flags = if dest < OCPBASE_DMEM_3081 {
                IMEM_DL_RDY_8814 | IMEM_CHKSUM_OK_8814
            } else {
                DMEM_DL_RDY_8814 | DMEM_CHKSUM_OK_8814
            };
            self.write_u8_async(REG_MCUFWDL, fwctrl | flags).await?;
        }
        Ok(())
    }

    async fn wait_ddma_idle_8814_async(&self, phase: &'static str) -> Result<(), DriverError> {
        for _ in 0..20 {
            if self.read_u32_async(REG_DDMA_CH0CTRL_8814).await? & DDMA_CH_OWN_8814 == 0 {
                return Ok(());
            }
            sleep_ms(1).await;
        }
        Err(DriverError::Nusb(format!(
            "RTL8814 DDMA channel did not become idle during {phase}-transfer poll"
        )))
    }

    async fn firmware_download_enable_8814_async(&self, enable: bool) -> Result<(), DriverError> {
        if enable {
            let mut value = self.read_u16_async(REG_MCUFWDL).await?;
            value &= 0x3000;
            value &= !(BIT12 as u16);
            value |= BIT13 as u16;
            value |= BIT0 as u16;
            self.write_u16_async(REG_MCUFWDL, value).await
        } else {
            let value = self.read_u8_async(REG_MCUFWDL).await?;
            self.write_u8_async(REG_MCUFWDL, value & 0xfe).await
        }
    }

    async fn enable_3081_8814_async(&self, enable: bool) -> Result<(), DriverError> {
        let value = self.read_u8_async(REG_SYS_FUNC_EN + 1).await?;
        let next = if enable {
            value | BIT2 as u8
        } else {
            value & !(BIT2 as u8)
        };
        self.write_u8_async(REG_SYS_FUNC_EN + 1, next).await
    }

    async fn ddma_reset_8814_async(&self) -> Result<(), DriverError> {
        let value = self.read_u32_async(REG_CPU_DMEM_CON_8814).await?;
        self.write_u32_async(REG_CPU_DMEM_CON_8814, value & !BIT16)
            .await?;
        self.write_u32_async(REG_CPU_DMEM_CON_8814, value | BIT16)
            .await
    }

    async fn firmware_free_to_go_8814_async(&self) -> Result<(), DriverError> {
        let deadline = DateNow::deadline_ms(5000.0);
        let mut reads = 0;
        loop {
            reads += 1;
            let state = self.read_u32_async(REG_MCUFWDL).await?;
            if state & BIT15 != 0 {
                return Ok(());
            }
            if DateNow::expired(deadline) && reads >= 10 {
                return Err(DriverError::FirmwareReadyTimeout);
            }
            yield_now().await;
        }
    }

    async fn read_u8_discard_8814_async(&self, register: u16) -> Result<(), DriverError> {
        let _ = self.read_u8_async(register).await?;
        Ok(())
    }

    async fn read_u16_discard_8814_async(&self, register: u16) -> Result<(), DriverError> {
        let _ = self.read_u16_async(register).await?;
        Ok(())
    }

    async fn read_u32_discard_8814_async(&self, register: u16) -> Result<(), DriverError> {
        let _ = self.read_u32_async(register).await?;
        Ok(())
    }
}

fn le32_at(bytes: &[u8], offset: usize) -> Result<u32, DriverError> {
    let array: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| DriverError::Nusb("RTL8814 firmware blob is shorter than header".into()))?
        .try_into()
        .expect("slice length is checked");
    Ok(u32::from_le_bytes(array))
}

fn build_rsvd_page_packet_8814(chunk: &[u8]) -> Vec<u8> {
    let mut packet = vec![0; RTL8814_TXDESC_OFFSET + chunk.len()];
    set_bits_le32(&mut packet, 0, 0, 16, chunk.len() as u32);
    set_bits_le32(&mut packet, 0, 16, 8, RTL8814_TXDESC_OFFSET as u32);
    set_bits_le32(&mut packet, 0, 26, 1, 1);
    set_bits_le32(&mut packet, 4, 8, 5, 0x10);
    tx_desc_checksum(&mut packet[..RTL8814_TXDESC_OFFSET]);
    packet[RTL8814_TXDESC_OFFSET..].copy_from_slice(chunk);
    packet
}

fn set_bits_le32(bytes: &mut [u8], offset: usize, bit_offset: u8, bit_len: u8, value: u32) {
    let mut word = u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("descriptor offset is in range"),
    );
    let mask = if bit_len == 32 {
        u32::MAX
    } else {
        ((1u32 << bit_len) - 1) << bit_offset
    };
    word = (word & !mask) | ((value << bit_offset) & mask);
    bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
}

fn tx_desc_checksum(desc: &mut [u8]) {
    set_bits_le32(desc, 28, 0, 16, 0);
    let mut checksum = 0u16;
    for idx in 0..16 {
        let offset = idx * 2;
        checksum ^= u16::from_le_bytes([desc[offset], desc[offset + 1]]);
    }
    set_bits_le32(desc, 28, 0, 16, checksum as u32);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtl8814_firmware_header_sections_cover_blob() {
        let fw = rtl_data::RTL8814_FW_NIC;
        let dmem = le32_at(fw, 36).unwrap() as usize + RTL8814_FW_CHECKSUM_DUMMY_SIZE;
        let iram = le32_at(fw, 48).unwrap() as usize + RTL8814_FW_CHECKSUM_DUMMY_SIZE;
        assert_eq!(dmem + iram + RTL8814_FW_HEADER_SIZE, fw.len());
    }

    #[test]
    fn reserved_page_descriptor_matches_minimal_8814_shape() {
        let packet = build_rsvd_page_packet_8814(&[1, 2, 3, 4]);
        assert_eq!(packet.len(), RTL8814_TXDESC_OFFSET + 4);
        let word0 = u32::from_le_bytes(packet[0..4].try_into().unwrap());
        let word1 = u32::from_le_bytes(packet[4..8].try_into().unwrap());
        assert_eq!(word0 & 0xffff, 4);
        assert_eq!((word0 >> 16) & 0xff, RTL8814_TXDESC_OFFSET as u32);
        assert_eq!((word0 >> 26) & 0x1, 1);
        assert_eq!((word1 >> 8) & 0x1f, 0x10);
        assert_eq!(&packet[RTL8814_TXDESC_OFFSET..], &[1, 2, 3, 4]);
    }
}
