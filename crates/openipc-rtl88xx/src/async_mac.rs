use crate::async_efuse::EfuseInfo;
use crate::device::RealtekDevice;
use crate::regs::*;
use crate::types::{ChipFamily, ChipInfo, DriverError};

impl RealtekDevice {
    pub(crate) async fn init_queue_fifo_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        match chip.family {
            ChipFamily::Rtl8812 => {
                self.init_queue_reserved_page_async(
                    TX_TOTAL_PAGE_NUMBER_8812,
                    NORMAL_PAGE_NUM_HPQ_8812,
                    NORMAL_PAGE_NUM_LPQ_8812,
                    NORMAL_PAGE_NUM_NPQ_8812,
                )
                .await?;
                self.init_tx_buffer_boundary_async(TX_PAGE_BOUNDARY_8812)
                    .await?;
                self.init_queue_priority_async(chip).await?;
                self.write_u16_async(REG_TRXFF_BNDY + 2, RX_DMA_BOUNDARY_8812)
                    .await?;
                self.write_u8_async(REG_PBP, PBP_512 << 4).await?;
            }
            ChipFamily::Rtl8821 => {
                self.init_queue_reserved_page_async(
                    TX_TOTAL_PAGE_NUMBER_8821,
                    NORMAL_PAGE_NUM_HPQ_8821,
                    NORMAL_PAGE_NUM_LPQ_8821,
                    NORMAL_PAGE_NUM_NPQ_8821,
                )
                .await?;
                self.init_tx_buffer_boundary_async(TX_PAGE_BOUNDARY_8821)
                    .await?;
                self.init_queue_priority_async(chip).await?;
                self.write_u16_async(REG_TRXFF_BNDY + 2, RX_DMA_BOUNDARY_8812)
                    .await?;
            }
            ChipFamily::Rtl8814 => {
                self.init_queue_reserved_page_8814_async().await?;
                self.init_queue_priority_async(chip).await?;
                self.write_u16_async(REG_RXFF_PTR_8814, RX_DMA_BOUNDARY_8814)
                    .await?;
                self.init_auto_llt_8814_async().await?;
            }
            ChipFamily::Rtl8822c => {
                return Err(DriverError::UnsupportedFirmwarePath(chip.family));
            }
        }
        Ok(())
    }

    async fn init_queue_reserved_page_8814_async(&self) -> Result<(), DriverError> {
        let dup16 = |value: u32| (value & 0xffff) | ((value & 0xffff) << 16);
        self.write_u32_async(REG_FIFOPAGE_INFO_1_8814, dup16(HPQ_PGNUM_8814))
            .await?;
        self.write_u32_async(REG_FIFOPAGE_INFO_2_8814, dup16(LPQ_PGNUM_8814))
            .await?;
        self.write_u32_async(REG_FIFOPAGE_INFO_3_8814, dup16(NPQ_PGNUM_8814))
            .await?;
        self.write_u32_async(REG_FIFOPAGE_INFO_4_8814, dup16(EPQ_PGNUM_8814))
            .await?;
        self.write_u32_async(REG_FIFOPAGE_INFO_5_8814, dup16(PUB_PGNUM_8814))
            .await?;
        self.write_u32_async(REG_RQPN_CTRL_2_8814, BIT31).await?;

        self.write_u16_async(REG_TXPKTBUF_BCNQ_BDNY_8814, TX_PAGE_BOUNDARY_8814)
            .await?;
        self.write_u16_async(REG_TXPKTBUF_BCNQ1_BDNY_8814, TX_PAGE_BOUNDARY_8814)
            .await?;
        self.write_u16_async(REG_MGQ_PGBNDY_8814, TX_PAGE_BOUNDARY_8814)
            .await?;
        self.write_u16_async(REG_FIFOPAGE_CTRL_2_8814, TX_PAGE_BOUNDARY_8814)
            .await?;
        self.write_u16_async(REG_FIFOPAGE_CTRL_2_8814 + 2, TX_PAGE_BOUNDARY_8814)
            .await
    }

    async fn init_auto_llt_8814_async(&self) -> Result<(), DriverError> {
        let mut value = self.read_u32_async(REG_TDECTRL).await.unwrap_or(0);
        self.write_u32_async(REG_TDECTRL, value | BIT16).await?;
        for _ in 0..200 {
            value = self.read_u32_async(REG_TDECTRL).await?;
            if value & BIT16 == 0 {
                return Ok(());
            }
            crate::time::sleep_ms(2).await;
        }
        Err(DriverError::Nusb(format!(
            "RTL8814 auto-LLT did not complete, REG_TDECTRL=0x{value:08x}"
        )))
    }

    async fn init_queue_reserved_page_async(
        &self,
        total_pages: u32,
        hpq_pages: u32,
        lpq_pages: u32,
        npq_pages: u32,
    ) -> Result<(), DriverError> {
        let queue_sel = self.out_ep_queue_sel_async();
        let num_hq = if queue_sel & TX_SELE_HQ != 0 {
            hpq_pages
        } else {
            0
        };
        let num_lq = if queue_sel & TX_SELE_LQ != 0 {
            lpq_pages
        } else {
            0
        };
        let num_nq = if queue_sel & TX_SELE_NQ != 0 {
            npq_pages
        } else {
            0
        };
        let num_pubq = total_pages.saturating_sub(num_hq + num_lq + num_nq);

        self.write_u8_async(REG_RQPN_NPQ, (num_nq & 0xff) as u8)
            .await?;
        self.write_u32_async(
            REG_RQPN,
            (num_hq & 0xff) | ((num_lq & 0xff) << 8) | ((num_pubq & 0xff) << 16) | BIT31,
        )
        .await
    }

    async fn init_tx_buffer_boundary_async(&self, boundary: u8) -> Result<(), DriverError> {
        self.write_u8_async(REG_BCNQ_BDNY, boundary).await?;
        self.write_u8_async(REG_MGQ_BDNY, boundary).await?;
        self.write_u8_async(REG_WMAC_LBK_BF_HD, boundary).await?;
        self.write_u8_async(REG_TRXFF_BNDY, boundary).await?;
        self.write_u8_async(REG_TDECTRL + 1, boundary).await
    }

    async fn init_queue_priority_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        match self.bulk_out_ep_count {
            0 | 1 => Ok(()),
            2 => {
                self.init_normal_chip_reg_priority_async(
                    chip,
                    [
                        QUEUE_NORMAL,
                        QUEUE_NORMAL,
                        QUEUE_HIGH,
                        QUEUE_HIGH,
                        QUEUE_HIGH,
                        QUEUE_HIGH,
                    ],
                )
                .await
            }
            3 => {
                self.init_normal_chip_reg_priority_async(
                    chip,
                    [
                        QUEUE_LOW,
                        QUEUE_LOW,
                        QUEUE_NORMAL,
                        QUEUE_HIGH,
                        QUEUE_HIGH,
                        QUEUE_HIGH,
                    ],
                )
                .await
            }
            _ => {
                self.init_normal_chip_reg_priority_async(
                    chip,
                    [
                        QUEUE_LOW,
                        QUEUE_LOW,
                        QUEUE_NORMAL,
                        QUEUE_NORMAL,
                        QUEUE_EXTRA,
                        QUEUE_HIGH,
                    ],
                )
                .await?;
                self.write_u8_async(REG_HIQ_NO_LMT_EN, 0xff).await
            }
        }
    }

    async fn init_normal_chip_reg_priority_async(
        &self,
        chip: ChipInfo,
        queues: [u16; 6],
    ) -> Result<(), DriverError> {
        let [beq, bkq, viq, voq, mgq, hiq] = queues;
        let mut value = self.read_u16_async(REG_TRXDMA_CTRL).await.unwrap_or(0) & 0x7;
        value |= txdma_beq_map(beq)
            | txdma_bkq_map(bkq)
            | txdma_viq_map(viq)
            | txdma_voq_map(voq)
            | txdma_mgq_map(mgq)
            | txdma_hiq_map(hiq);
        if chip.family == ChipFamily::Rtl8814 {
            value |= BIT2 as u16;
        }
        self.write_u16_async(REG_TRXDMA_CTRL, value).await
    }

    fn out_ep_queue_sel_async(&self) -> u8 {
        match self.bulk_out_ep_count {
            0 => 0,
            1 => TX_SELE_HQ,
            2 => TX_SELE_HQ | TX_SELE_NQ,
            3 => TX_SELE_HQ | TX_SELE_LQ | TX_SELE_NQ,
            _ => TX_SELE_HQ | TX_SELE_LQ | TX_SELE_NQ | TX_SELE_EQ,
        }
    }

    pub(crate) async fn init_mac_rx_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        self.write_u8_async(REG_RX_DRVINFO_SZ, 4).await?;
        self.init_interrupt_async(chip).await?;
        self.init_network_type_async(chip).await?;
        self.init_wmac_setting_async().await?;
        self.init_adaptive_ctrl_async().await?;
        if chip.family == ChipFamily::Rtl8814 {
            self.write_u8_async(REG_MAX_AGGR_NUM, 0x36).await?;
            self.write_u8_async(REG_RTS_MAX_AGGR_NUM_8814, 0x36).await?;
        }
        self.init_edca_async(chip).await?;
        self.init_retry_function_async().await?;
        self.init_usb_aggregation_async(chip).await?;
        self.init_beacon_parameters_async().await?;
        self.write_u8_async(REG_BCN_MAX_ERR, 0xff).await?;
        self.init_burst_packet_length_async(chip).await?;

        let mut cr = self.read_u8_async(REG_CR).await.unwrap_or(0);
        cr |= (MACTXEN | MACRXEN) as u8;
        self.write_u8_async(REG_CR, cr).await?;
        self.write_u16_async(REG_PKT_VO_VI_LIFE_TIME, 0x0400)
            .await?;
        self.write_u16_async(REG_PKT_BE_BK_LIFE_TIME, 0x0400).await
    }

    async fn init_interrupt_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        if chip.family == ChipFamily::Rtl8812 {
            self.write_u32_async(REG_HIMR0_8812, 0).await?;
            self.write_u32_async(REG_HIMR1_8812, 0).await?;
        }
        Ok(())
    }

    async fn init_network_type_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        let nettype = if chip.family == ChipFamily::Rtl8814 {
            0
        } else {
            NETTYPE_LINK_AP
        };
        let cr = (self.read_u32_async(REG_CR).await.unwrap_or(0) & !MASK_NETTYPE) | nettype;
        self.write_u32_async(REG_CR, cr).await?;
        if chip.family != ChipFamily::Rtl8814 {
            return Ok(());
        }
        let txqctl_b2 = self.read_u8_async(REG_FWHW_TXQ_CTRL + 2).await.unwrap_or(0);
        self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, txqctl_b2 & !(BIT6 as u8))
            .await?;
        self.write_u8_async(REG_TBTT_PROHIBIT + 1, 0x64).await?;
        let tbtt_b2 = self.read_u8_async(REG_TBTT_PROHIBIT + 2).await.unwrap_or(0);
        self.write_u8_async(REG_TBTT_PROHIBIT + 2, tbtt_b2 & 0xf0)
            .await
    }

    async fn init_wmac_setting_async(&self) -> Result<(), DriverError> {
        let rcr = RCR_APM
            | RCR_AM
            | RCR_AB
            | RCR_CBSSID_DATA
            | RCR_CBSSID_BCN
            | RCR_APP_ICV
            | RCR_AMF
            | RCR_HTC_LOC_CTRL
            | RCR_APP_MIC
            | RCR_APP_PHYST_RXFF
            | RCR_APPFCS
            | FORCEACK;
        self.write_u32_async(REG_RCR, rcr).await?;
        self.write_u32_async(REG_MAR, 0xffff_ffff).await?;
        self.write_u32_async(REG_MAR + 4, 0xffff_ffff).await?;
        self.write_u16_async(REG_RXFLTMAP1, (BIT10 | BIT5) as u16)
            .await
    }

    async fn init_adaptive_ctrl_async(&self) -> Result<(), DriverError> {
        let mut rrsr = self.read_u32_async(REG_RRSR).await.unwrap_or(0);
        rrsr &= !RATE_BITMAP_ALL;
        rrsr |= RATE_RRSR_WITHOUT_CCK | RATE_RRSR_CCK_ONLY_1M;
        self.write_u32_async(REG_RRSR, rrsr).await?;
        self.write_u16_async(REG_SPEC_SIFS, 0x1010).await?;
        self.write_u16_async(REG_RL, (RL_VAL_STA & 0x3f) | ((RL_VAL_STA & 0x3f) << 8))
            .await
    }

    async fn init_edca_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        self.write_u16_async(REG_SPEC_SIFS, 0x100a).await?;
        self.write_u16_async(REG_MAC_SPEC_SIFS, 0x100a).await?;
        self.write_u16_async(REG_SIFS_CTX, 0x100a).await?;
        self.write_u16_async(REG_SIFS_TRX, 0x100a).await?;
        self.write_u32_async(REG_EDCA_BE_PARAM, 0x005e_a42b).await?;
        self.write_u32_async(REG_EDCA_BK_PARAM, 0x0000_a44f).await?;
        self.write_u32_async(REG_EDCA_VI_PARAM, 0x005e_a324).await?;
        self.write_u32_async(REG_EDCA_VO_PARAM, 0x002f_a226).await?;
        if chip.family != ChipFamily::Rtl8814 {
            self.write_u8_async(REG_USTIME_TSF, 0x50).await?;
            self.write_u8_async(REG_USTIME_EDCA, 0x50).await?;
        }
        Ok(())
    }

    async fn init_retry_function_async(&self) -> Result<(), DriverError> {
        let txq = self.read_u8_async(REG_FWHW_TXQ_CTRL).await.unwrap_or(0);
        self.write_u8_async(REG_FWHW_TXQ_CTRL, txq | EN_AMPDU_RTY_NEW)
            .await?;
        self.write_u8_async(REG_ACKTO, 0x80).await
    }

    async fn init_usb_aggregation_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        let mut trxdma = self.read_u8_async(REG_TRXDMA_CTRL).await.unwrap_or(0);
        trxdma |= RXDMA_AGG_EN;
        let (size, timeout) = match chip.family {
            ChipFamily::Rtl8814 => (8u8, 6u8),
            _ if self.device.speed() == Some(nusb::Speed::Super) => (3u8, 1u8),
            _ => (1u8, 1u8),
        };
        self.write_u16_async(REG_RXDMA_AGG_PG_TH, size as u16 | ((timeout as u16) << 8))
            .await?;
        if chip.family == ChipFamily::Rtl8814 {
            let agg = self
                .read_u8_async(REG_RXDMA_AGG_PG_TH + 3)
                .await
                .unwrap_or(0);
            self.write_u8_async(REG_RXDMA_AGG_PG_TH + 3, agg & !(BIT7 as u8))
                .await?;
        }
        self.write_u8_async(REG_TRXDMA_CTRL, trxdma).await
    }

    async fn init_beacon_parameters_async(&self) -> Result<(), DriverError> {
        let bcn_ctrl = DIS_TSF_UDT as u16 | ((DIS_TSF_UDT as u16) << 8);
        self.write_u16_async(REG_BCN_CTRL, bcn_ctrl).await?;
        self.write_u8_async(REG_TBTT_PROHIBIT, 0x04).await?;
        self.write_u8_async(REG_TBTT_PROHIBIT + 1, 0x64).await?;
        let tbtt_b2 = self.read_u8_async(REG_TBTT_PROHIBIT + 2).await.unwrap_or(0);
        self.write_u8_async(REG_TBTT_PROHIBIT + 2, tbtt_b2 & 0xf0)
            .await?;
        self.write_u8_async(REG_DRVERLYINT, DRIVER_EARLY_INT_TIME_8812)
            .await?;
        self.write_u8_async(REG_BCNDMATIM, BCN_DMA_ATIME_INT_TIME_8812)
            .await?;
        self.write_u16_async(REG_BCNTCFG, 0x4413).await
    }

    async fn init_burst_packet_length_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        if chip.family == ChipFamily::Rtl8814 {
            return self.init_burst_packet_length_8814_async().await;
        }

        self.write_u8_async(0xf050, 0x01).await?;
        self.write_u16_async(REG_RXDMA_STATUS, 0x7400).await?;
        self.write_u8_async(0x0289, 0xf5).await?;
        self.write_u8_async(
            REG_AMPDU_MAX_TIME_8812,
            if chip.family == ChipFamily::Rtl8821 {
                0x5e
            } else {
                0x70
            },
        )
        .await?;
        self.write_u32_async(REG_AMPDU_MAX_LENGTH_8812, 0xffff_ffff)
            .await?;
        self.write_u8_async(REG_USTIME_TSF, 0x50).await?;
        self.write_u8_async(REG_USTIME_EDCA, 0x50).await?;

        let speedvalue = if chip.family == ChipFamily::Rtl8821 {
            BIT7 as u8
        } else {
            self.read_u8_async(0x00ff).await.unwrap_or(BIT7 as u8)
        };
        let pro = self.read_u8_async(REG_RXDMA_PRO_8812).await.unwrap_or(0);
        if speedvalue & BIT7 as u8 != 0 {
            let usb_info = self.read_u8_async(REG_USB_INFO).await.unwrap_or(0);
            let next = if ((usb_info >> 4) & 0x03) == 0 {
                (pro | (BIT4 | BIT3 | BIT2 | BIT1) as u8) & !(BIT5 as u8)
            } else {
                (pro | (BIT5 | BIT3 | BIT2 | BIT1) as u8) & !(BIT4 as u8)
            };
            self.write_u8_async(REG_RXDMA_PRO_8812, next).await?;
        } else {
            self.write_u8_async(
                REG_RXDMA_PRO_8812,
                (pro | (BIT3 | BIT2 | BIT1) as u8) & 0b1100_1111,
            )
            .await?;
            let u1u2 = self.read_u8_async(0xf008).await.unwrap_or(0);
            self.write_u8_async(0xf008, u1u2 & 0xe7).await?;
        }

        let sys_func = self.read_u8_async(REG_SYS_FUNC_EN).await.unwrap_or(0);
        self.write_u8_async(REG_SYS_FUNC_EN, sys_func & !(BIT10 as u8))
            .await?;

        let ampdu = self
            .read_u8_async(REG_HT_SINGLE_AMPDU_8812)
            .await
            .unwrap_or(0);
        self.write_u8_async(REG_HT_SINGLE_AMPDU_8812, ampdu | BIT7 as u8)
            .await?;
        self.write_u8_async(REG_RX_PKT_LIMIT, 0x18).await?;
        self.write_u8_async(REG_PIFS, 0x00).await?;
        self.write_u16_async(REG_MAX_AGGR_NUM, 0x1f1f).await?;
        let txq = self.read_u8_async(REG_FWHW_TXQ_CTRL).await.unwrap_or(0);
        self.write_u8_async(REG_FWHW_TXQ_CTRL, txq & !(BIT7 as u8))
            .await?;
        if chip.family == ChipFamily::Rtl8821 {
            self.write_u8_async(REG_FWHW_TXQ_CTRL, 0x80).await?;
            self.write_u32_async(REG_FAST_EDCA_CTRL, 0x0308_7777)
                .await?;
        }

        let rsv = self.read_u8_async(0x001c).await.unwrap_or(0);
        self.write_u8_async(0x001c, rsv | (BIT5 | BIT6) as u8)
            .await?;
        self.write_u32_async(REG_ARFR0_8812, 0x0000_0010).await?;
        self.write_u32_async(REG_ARFR0_8812 + 4, 0xffff_f000)
            .await?;
        self.write_u32_async(REG_ARFR1_8812, 0x0000_0010).await?;
        self.write_u32_async(REG_ARFR1_8812 + 4, 0x003f_f000)
            .await?;
        self.write_u32_async(REG_ARFR2_8812, 0x0000_0015).await?;
        self.write_u32_async(REG_ARFR2_8812 + 4, 0x003f_f000)
            .await?;
        self.write_u32_async(REG_ARFR3_8812, 0x0000_0015).await?;
        self.write_u32_async(REG_ARFR3_8812 + 4, 0xffcf_f000).await
    }

    async fn init_burst_packet_length_8814_async(&self) -> Result<(), DriverError> {
        self.write_u32_async(REG_FAST_EDCA_VOVI_SETTING_8814, 0x0807_0807)
            .await?;
        self.write_u32_async(REG_FAST_EDCA_BEBK_SETTING_8814, 0x0807_0807)
            .await?;

        let usb3_indicator = self.read_u8_async(0x00ff).await.unwrap_or(BIT7 as u8);
        if usb3_indicator & BIT7 as u8 != 0 {
            let mode = if self.device.speed() == Some(nusb::Speed::High) {
                0x1e
            } else {
                0x2e
            };
            self.write_u8_async(REG_RXDMA_PRO_8812, mode).await?;
            self.write_u16_async(REG_RXDMA_AGG_PG_TH, 0x2005).await
        } else {
            self.write_u8_async(REG_RXDMA_PRO_8812, 0x0e).await?;
            self.write_u16_async(REG_RXDMA_AGG_PG_TH, 0x0a05).await?;
            let u1u2 = self.read_u8_async(0xf008).await.unwrap_or(0);
            self.write_u8_async(0xf008, u1u2 & 0xe7).await?;
            self.write_u16_async(0xf002, 0).await?;
            let burst = self
                .read_u8_async(REG_SW_AMPDU_BURST_MODE_CTRL_8814)
                .await
                .unwrap_or(0);
            self.write_u8_async(REG_SW_AMPDU_BURST_MODE_CTRL_8814, burst & !(BIT6 as u8))
                .await
        }
    }

    pub(crate) async fn enable_bb_rf_domain_8814_async(
        &self,
        chip: ChipInfo,
    ) -> Result<(), DriverError> {
        if chip.family != ChipFamily::Rtl8814 {
            return Ok(());
        }

        let sys_func = self.read_u8_async(REG_SYS_FUNC_EN).await.unwrap_or(0);
        self.write_u8_async(REG_SYS_FUNC_EN, sys_func | BIT2 as u8)
            .await?;
        let bb_reset = self.read_u8_async(0x1002).await.unwrap_or(0);
        self.write_u8_async(0x1002, bb_reset | 0x03).await?;
        for register in [0x001f, 0x0020, 0x0021, 0x0076] {
            self.write_u8_async(register, 0x07).await?;
        }
        Ok(())
    }

    pub(crate) async fn finalize_mac_rx_async(
        &self,
        chip: ChipInfo,
        efuse: EfuseInfo,
    ) -> Result<(), DriverError> {
        let mut cr = self.read_u16_async(REG_CR).await.unwrap_or(0);
        cr |= HCI_TXDMA_EN
            | HCI_RXDMA_EN
            | TXDMA_EN
            | RXDMA_EN
            | PROTOCOL_EN
            | SCHEDULE_EN
            | MACTXEN
            | MACRXEN;
        if chip.family == ChipFamily::Rtl8814 {
            cr |= ENSEC | CALTMR_EN;
            self.init_hardware_drop_incorrect_bulk_out_async().await?;
        }
        self.write_u16_async(REG_CR, cr).await?;
        self.write_u16_async(REG_RXFLTMAP2, 0xffff).await?;
        if chip.family == ChipFamily::Rtl8814 {
            let hwseq = self.read_u32_async(REG_FWHW_TXQ_CTRL).await.unwrap_or(0);
            self.write_u32_async(REG_FWHW_TXQ_CTRL, (hwseq & 0x00ff_ffff) | 0xff00_0000)
                .await?;
        } else {
            self.write_u8_async(REG_HWSEQ_CTRL, 0xff).await?;
        }
        self.write_u32_async(REG_BAR_MODE_CTRL, 0x0201_ffff).await?;
        if chip.family == ChipFamily::Rtl8814 {
            self.write_u8_async(REG_SECONDARY_CCA_CTRL_8814, 0x03)
                .await?;
        }
        self.write_u8_async(REG_NAV_UPPER, 0x00).await?;
        if chip.family == ChipFamily::Rtl8814 {
            self.write_u8_async(REG_NAV_UPPER, 0xeb).await?;
        }
        let queue = self.read_u8_async(REG_QUEUE_CTRL).await.unwrap_or(0);
        self.write_u8_async(REG_QUEUE_CTRL, queue & 0xf7).await?;
        self.write_u8_async(REG_FWHW_TXQ_CTRL + 1, 0x0f).await?;
        if chip.family == ChipFamily::Rtl8814 {
            let txq = self.read_u8_async(REG_FWHW_TXQ_CTRL).await.unwrap_or(0);
            self.write_u8_async(REG_FWHW_TXQ_CTRL, txq | EN_AMPDU_RTY_NEW)
                .await?;
            let txq2 = self.read_u8_async(REG_FWHW_TXQ_CTRL + 2).await.unwrap_or(0);
            self.write_u8_async(REG_FWHW_TXQ_CTRL + 2, txq2 & !(BIT6 as u8))
                .await?;
            self.write_u8_async(REG_ACKTO, 0x80).await?;
        } else {
            self.write_u8_async(REG_EARLY_MODE_CONTROL_8812 + 3, 0x01)
                .await?;
            self.write_u16_async(REG_TX_RPT_TIME, 0x3df0).await?;
            self.write_u8_async(REG_USB_HRPWM, 0).await?;
            let txq = self.read_u32_async(REG_FWHW_TXQ_CTRL).await.unwrap_or(0);
            self.write_u32_async(REG_FWHW_TXQ_CTRL, txq | BIT12).await?;
        }
        self.write_u8_async(REG_SDIO_CTRL_8812, 0).await?;
        self.write_u8_async(REG_ACLK_MON, 0).await?;

        if let Some(mac) = efuse.mac {
            self.program_macid_async(mac).await?;
        } else if chip.family == ChipFamily::Rtl8814 {
            self.program_macid_async([0x02, 0x0d, 0xb0, 0xc7, 0xe4, 0xb3])
                .await?;
        }

        if chip.family == ChipFamily::Rtl8814 {
            self.write_u32_async(REG_RRSR, 0x0000_0fff).await?;
            self.write_u8_async(REG_SW_AMPDU_BURST_MODE_CTRL_8814, 0x00)
                .await?;
            self.write_u8_async(REG_QUEUE_CTRL, 0x04).await?;
            self.write_u32_async(REG_TX_PTCL_CTRL, 0x0000_2f0f).await?;
            self.write_u32_async(REG_RD_CTRL, 0x0f4f_ff00).await?;
            self.write_u32_async(REG_CAMCMD, 0xc000_0000).await?;
            self.write_u32_async(0x0990, 0x2710_0000).await?;
            self.write_u32_async(0x0994, 0x4c48_0100).await?;
            self.write_u32_async(0x0998, 0x302c_2824).await?;
            self.write_u32_async(0x099c, 0x403c_3834).await?;
            self.write_u32_async(0x09a0, 0x0000_0044).await?;
            self.write_u32_async(0x09a4, 0x0008_0080).await?;
        }
        Ok(())
    }

    async fn program_macid_async(&self, mac: [u8; 6]) -> Result<(), DriverError> {
        for (idx, byte) in mac.iter().enumerate() {
            self.write_u8_async(REG_MACID + idx as u16, *byte).await?;
        }
        Ok(())
    }
}
