use std::time::Duration;

pub(crate) const USB_TIMEOUT: Duration = Duration::from_millis(500);
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) const USB_FIRMWARE_TIMEOUT: Duration = Duration::from_millis(2000);
pub(crate) const REALTEK_VENDOR_READ_REQUEST: u8 = 0x05;
pub(crate) const REALTEK_VENDOR_WRITE_REQUEST: u8 = 0x05;

pub(crate) const REG_SYS_ISO_CTRL: u16 = 0x0000;
pub(crate) const REG_SYS_FUNC_EN: u16 = 0x0002;
pub(crate) const REG_APS_FSMCO: u16 = 0x0004;
pub(crate) const REG_SYS_CLKR: u16 = 0x0008;
pub(crate) const REG_RSV_CTRL: u16 = 0x001c;
pub(crate) const REG_RF_CTRL: u16 = 0x001f;
pub(crate) const REG_MAC_PHY_CTRL: u16 = 0x002c;
pub(crate) const REG_EFUSE_CTRL: u16 = 0x0030;
pub(crate) const REG_EFUSE_TEST: u16 = 0x0034;
pub(crate) const REG_ACLK_MON: u16 = 0x003e;
pub(crate) const REG_RF_B_CTRL_8812: u16 = 0x0076;
pub(crate) const REG_MCUFWDL: u16 = 0x0080;
pub(crate) const REG_HIMR0_8812: u16 = 0x00b0;
pub(crate) const REG_HIMR1_8812: u16 = 0x00b8;
pub(crate) const REG_EFUSE_BURN_GNT_8812: u16 = 0x00cf;
pub(crate) const REG_HMETFR: u16 = 0x01cc;
pub(crate) const REG_SYS_CFG: u16 = 0x00f0;
pub(crate) const REG_CR: u16 = 0x0100;
pub(crate) const REG_PBP: u16 = 0x0104;
pub(crate) const REG_TRXDMA_CTRL: u16 = 0x010c;
pub(crate) const REG_TRXFF_BNDY: u16 = 0x0114;
pub(crate) const REG_RXFF_PTR_8814: u16 = 0x011c;
pub(crate) const REG_LLT_INIT: u16 = 0x01e0;
pub(crate) const REG_RQPN: u16 = 0x0200;
pub(crate) const REG_FIFOPAGE_CTRL_2_8814: u16 = 0x0204;
pub(crate) const REG_TDECTRL: u16 = 0x0208;
pub(crate) const REG_TXDMA_OFFSET_CHK: u16 = 0x020c;
pub(crate) const REG_RQPN_NPQ: u16 = 0x0214;
pub(crate) const REG_RQPN_CTRL_2_8814: u16 = 0x022c;
pub(crate) const REG_FIFOPAGE_INFO_1_8814: u16 = 0x0230;
pub(crate) const REG_FIFOPAGE_INFO_2_8814: u16 = 0x0234;
pub(crate) const REG_FIFOPAGE_INFO_3_8814: u16 = 0x0238;
pub(crate) const REG_FIFOPAGE_INFO_4_8814: u16 = 0x023c;
pub(crate) const REG_FIFOPAGE_INFO_5_8814: u16 = 0x0240;
pub(crate) const REG_RXDMA_AGG_PG_TH: u16 = 0x0280;
pub(crate) const REG_RXDMA_STATUS: u16 = 0x0288;
pub(crate) const REG_RXDMA_PRO_8812: u16 = 0x0290;
pub(crate) const REG_EARLY_MODE_CONTROL_8812: u16 = 0x02bc;
pub(crate) const REG_FWHW_TXQ_CTRL: u16 = 0x0420;
pub(crate) const REG_HWSEQ_CTRL: u16 = 0x0423;
pub(crate) const REG_BCNQ_BDNY: u16 = 0x0424;
pub(crate) const REG_TXPKTBUF_BCNQ_BDNY_8814: u16 = 0x0424;
pub(crate) const REG_MGQ_BDNY: u16 = 0x0425;
pub(crate) const REG_SPEC_SIFS: u16 = 0x0428;
pub(crate) const REG_RL: u16 = 0x042a;
pub(crate) const REG_RRSR: u16 = 0x0440;
pub(crate) const REG_ARFR0_8812: u16 = 0x0444;
pub(crate) const REG_ARFR1_8812: u16 = 0x044c;
pub(crate) const REG_TXPKT_EMPTY: u16 = 0x041a;
pub(crate) const REG_CCK_CHECK: u16 = 0x0454;
pub(crate) const REG_AMPDU_MAX_TIME_8812: u16 = 0x0456;
pub(crate) const REG_TXPKTBUF_BCNQ1_BDNY_8814: u16 = 0x0456;
pub(crate) const REG_AMPDU_MAX_LENGTH_8812: u16 = 0x0458;
pub(crate) const REG_WMAC_LBK_BF_HD: u16 = 0x045d;
pub(crate) const REG_FAST_EDCA_CTRL: u16 = 0x0460;
pub(crate) const REG_SDIO_CTRL_8812: u16 = 0x0070;
pub(crate) const REG_MGQ_PGBNDY_8814: u16 = 0x047a;
pub(crate) const REG_ARFR2_8812: u16 = 0x048c;
pub(crate) const REG_ARFR3_8812: u16 = 0x0494;
pub(crate) const REG_SW_AMPDU_BURST_MODE_CTRL_8814: u16 = 0x04bc;
pub(crate) const REG_PKT_VO_VI_LIFE_TIME: u16 = 0x04c0;
pub(crate) const REG_PKT_BE_BK_LIFE_TIME: u16 = 0x04c2;
pub(crate) const REG_QUEUE_CTRL: u16 = 0x04c6;
pub(crate) const REG_HT_SINGLE_AMPDU_8812: u16 = 0x04c7;
pub(crate) const REG_MAX_AGGR_NUM: u16 = 0x04ca;
pub(crate) const REG_RTS_MAX_AGGR_NUM_8814: u16 = 0x04cb;
pub(crate) const REG_BAR_MODE_CTRL: u16 = 0x04cc;
pub(crate) const REG_TX_RPT_TIME: u16 = 0x04f0;
pub(crate) const REG_EDCA_VO_PARAM: u16 = 0x0500;
pub(crate) const REG_EDCA_VI_PARAM: u16 = 0x0504;
pub(crate) const REG_EDCA_BE_PARAM: u16 = 0x0508;
pub(crate) const REG_EDCA_BK_PARAM: u16 = 0x050c;
pub(crate) const REG_BCNTCFG: u16 = 0x0510;
pub(crate) const REG_PIFS: u16 = 0x0512;
pub(crate) const REG_SIFS_CTX: u16 = 0x0514;
pub(crate) const REG_SIFS_TRX: u16 = 0x0516;
pub(crate) const REG_TX_PTCL_CTRL: u16 = 0x0520;
pub(crate) const REG_RD_CTRL: u16 = 0x0524;
pub(crate) const REG_TBTT_PROHIBIT: u16 = 0x0540;
pub(crate) const REG_BCN_CTRL: u16 = 0x0550;
pub(crate) const REG_DRVERLYINT: u16 = 0x0558;
pub(crate) const REG_BCNDMATIM: u16 = 0x0559;
pub(crate) const REG_USTIME_TSF: u16 = 0x055c;
pub(crate) const REG_BCN_MAX_ERR: u16 = 0x055d;
pub(crate) const REG_HIQ_NO_LMT_EN: u16 = 0x05a7;
pub(crate) const REG_SECONDARY_CCA_CTRL_8814: u16 = 0x0577;
pub(crate) const REG_RCR: u16 = 0x0608;
pub(crate) const REG_RX_PKT_LIMIT: u16 = 0x060c;
pub(crate) const REG_RX_DRVINFO_SZ: u16 = 0x060f;
pub(crate) const REG_RXFLTMAP1: u16 = 0x06a2;
pub(crate) const REG_RXFLTMAP2: u16 = 0x06a4;
pub(crate) const REG_MACID: u16 = 0x0610;
pub(crate) const REG_MAR: u16 = 0x0620;
pub(crate) const REG_USTIME_EDCA: u16 = 0x0638;
pub(crate) const REG_MAC_SPEC_SIFS: u16 = 0x063a;
pub(crate) const REG_ACKTO: u16 = 0x0640;
pub(crate) const REG_NAV_UPPER: u16 = 0x0652;
pub(crate) const REG_CAMCMD: u16 = 0x0670;
pub(crate) const REG_CPU_DMEM_CON_8814: u16 = 0x1080;
pub(crate) const REG_DDMA_CH0SA_8814: u16 = 0x1200;
pub(crate) const REG_DDMA_CH0DA_8814: u16 = 0x1204;
pub(crate) const REG_DDMA_CH0CTRL_8814: u16 = 0x1208;
pub(crate) const REG_FAST_EDCA_VOVI_SETTING_8814: u16 = 0x1448;
pub(crate) const REG_FAST_EDCA_BEBK_SETTING_8814: u16 = 0x144c;
pub(crate) const FW_START_ADDRESS: u16 = 0x1000;

pub(crate) const BIT0: u32 = 1 << 0;
pub(crate) const BIT1: u32 = 1 << 1;
pub(crate) const BIT2: u32 = 1 << 2;
pub(crate) const BIT3: u32 = 1 << 3;
pub(crate) const BIT4: u32 = 1 << 4;
pub(crate) const BIT5: u32 = 1 << 5;
pub(crate) const BIT6: u32 = 1 << 6;
pub(crate) const BIT7: u32 = 1 << 7;
pub(crate) const BIT8: u32 = 1 << 8;
pub(crate) const BIT9: u32 = 1 << 9;
pub(crate) const BIT10: u32 = 1 << 10;
pub(crate) const BIT11: u32 = 1 << 11;
pub(crate) const BIT12: u32 = 1 << 12;
pub(crate) const BIT13: u32 = 1 << 13;
pub(crate) const BIT14: u32 = 1 << 14;
pub(crate) const BIT15: u32 = 1 << 15;
pub(crate) const BIT16: u32 = 1 << 16;
pub(crate) const BIT17: u32 = 1 << 17;
pub(crate) const BIT18: u32 = 1 << 18;
pub(crate) const BIT20: u32 = 1 << 20;
pub(crate) const BIT22: u32 = 1 << 22;
pub(crate) const BIT24: u32 = 1 << 24;
pub(crate) const BIT25: u32 = 1 << 25;
pub(crate) const BIT26: u32 = 1 << 26;
pub(crate) const BIT27: u32 = 1 << 27;
pub(crate) const BIT28: u32 = 1 << 28;
pub(crate) const BIT29: u32 = 1 << 29;
pub(crate) const BIT30: u32 = 1 << 30;
pub(crate) const BIT31: u32 = 1 << 31;

pub(crate) const MCUFWDL_RDY: u32 = BIT1;
pub(crate) const FWDL_CHKSUM_RPT: u32 = BIT2;
pub(crate) const WINTINI_RDY: u32 = BIT6;
pub(crate) const RAM_DL_SEL: u32 = BIT7;
pub(crate) const RF_TYPE_ID: u32 = BIT27;
pub(crate) const CHIP_VER_RTL_SHIFT: u32 = 12;
pub(crate) const CHIP_VER_RTL_MASK: u32 = 0x0000_f000;

pub(crate) const HCI_TXDMA_EN: u16 = 1 << 0;
pub(crate) const HCI_RXDMA_EN: u16 = 1 << 1;
pub(crate) const TXDMA_EN: u16 = 1 << 2;
pub(crate) const RXDMA_EN: u16 = 1 << 3;
pub(crate) const PROTOCOL_EN: u16 = 1 << 4;
pub(crate) const SCHEDULE_EN: u16 = 1 << 5;
pub(crate) const MACTXEN: u16 = 1 << 6;
pub(crate) const MACRXEN: u16 = 1 << 7;
pub(crate) const ENSEC: u16 = 1 << 9;
pub(crate) const CALTMR_EN: u16 = 1 << 10;
pub(crate) const MASK_NETTYPE: u32 = 0x0003_0000;
pub(crate) const NETTYPE_LINK_AP: u32 = 0x0002_0000;
pub(crate) const DROP_DATA_EN: u32 = BIT9;
pub(crate) const PWC_EV12V: u16 = BIT15 as u16;
pub(crate) const FEN_ELDR: u16 = BIT12 as u16;
pub(crate) const ANA8M: u16 = BIT1 as u16;
pub(crate) const LOADER_CLK_EN: u16 = BIT5 as u16;

pub(crate) const RCR_APPFCS: u32 = BIT31;
pub(crate) const RCR_APP_PHYST_RXFF: u32 = BIT28;
pub(crate) const RCR_AMF: u32 = BIT13;
pub(crate) const RCR_ACF: u32 = BIT12;
pub(crate) const RCR_ADF: u32 = BIT11;
pub(crate) const RCR_AICV: u32 = BIT9;
pub(crate) const RCR_ACRC32: u32 = BIT8;
pub(crate) const RCR_APWRMGT: u32 = BIT5;
pub(crate) const RCR_AB: u32 = BIT3;
pub(crate) const RCR_AM: u32 = BIT2;
pub(crate) const RCR_APM: u32 = BIT1;
pub(crate) const RCR_AAP: u32 = BIT0;

pub(crate) const B_MASK_DWORD: u32 = 0xffff_ffff;
pub(crate) const B_MASK_BYTE0: u32 = 0x0000_00ff;
pub(crate) const B_LSSI_WRITE_DATA: u32 = 0x000f_ffff;
pub(crate) const RF_CHNLBW_JAGUAR: u16 = 0x18;
pub(crate) const R_FC_AREA_JAGUAR: u16 = 0x0860;
pub(crate) const R_CCA_ON_SEC_JAGUAR: u16 = 0x0838;
pub(crate) const R_PWED_TH_JAGUAR: u16 = 0x0830;
pub(crate) const R_BW_INDICATION_JAGUAR: u16 = 0x0834;
pub(crate) const R_RFMOD_JAGUAR: u16 = 0x08ac;
pub(crate) const R_FPGA0_XCD_RF_PARA: u16 = 0x08b4;
pub(crate) const R_OFDMCCKEN_JAGUAR: u16 = 0x0808;
pub(crate) const R_TX_PATH_JAGUAR: u16 = 0x080c;
pub(crate) const R_AGC_TABLE_JAGUAR: u16 = 0x082c;
pub(crate) const R_AGC_TABLE_JAGUAR2: u16 = 0x0958;
pub(crate) const R_CCK0_TXFILTER1: u16 = 0x0a20;
pub(crate) const R_CCK0_TXFILTER2: u16 = 0x0a24;
pub(crate) const R_CCK0_DEBUGPORT: u16 = 0x0a28;
pub(crate) const R_CCK_RX_JAGUAR: u16 = 0x0a04;
pub(crate) const R_A_TX_SCALE_JAGUAR: u16 = 0x0c1c;
pub(crate) const R_B_TX_SCALE_JAGUAR: u16 = 0x0e1c;
pub(crate) const R_C_TX_SCALE_JAGUAR: u16 = 0x181c;
pub(crate) const R_D_TX_SCALE_JAGUAR: u16 = 0x1a1c;
pub(crate) const R_A_RFE_PINMUX_JAGUAR: u16 = 0x0cb0;
pub(crate) const R_B_RFE_PINMUX_JAGUAR: u16 = 0x0eb0;
pub(crate) const R_A_RFE_INV_JAGUAR: u16 = 0x0cb4;
pub(crate) const R_B_RFE_INV_JAGUAR: u16 = 0x0eb4;
pub(crate) const R_C_RFE_PINMUX_8814: u16 = 0x18b4;
pub(crate) const R_D_RFE_PINMUX_8814: u16 = 0x1ab4;
pub(crate) const R_D_RFE_INV_8814: u16 = 0x1abc;
pub(crate) const R_ANTSEL_SW_JAGUAR: u16 = 0x0900;
pub(crate) const REG_DATA_SC_8812: u16 = 0x0483;
pub(crate) const REG_WMAC_TRXPTCL_CTL: u16 = 0x0668;
pub(crate) const REG_USB_INFO: u16 = 0xfe17;
pub(crate) const REG_USB_HRPWM: u16 = 0xfe58;

pub(crate) const RXDMA_AGG_EN: u8 = BIT2 as u8;
pub(crate) const RX_DMA_BOUNDARY_8812: u16 = 0x3e7f;
pub(crate) const TX_TOTAL_PAGE_NUMBER_8812: u32 = 0xf8;
pub(crate) const TX_PAGE_BOUNDARY_8812: u8 = 0xf9;
pub(crate) const LAST_ENTRY_OF_TX_PKT_BUFFER_8812: u32 = 255;
pub(crate) const NORMAL_PAGE_NUM_HPQ_8812: u32 = 0x10;
pub(crate) const NORMAL_PAGE_NUM_LPQ_8812: u32 = 0x10;
pub(crate) const NORMAL_PAGE_NUM_NPQ_8812: u32 = 0x00;
pub(crate) const TX_TOTAL_PAGE_NUMBER_8821: u32 = 0xf7;
pub(crate) const TX_PAGE_BOUNDARY_8821: u8 = 0xf8;
pub(crate) const NORMAL_PAGE_NUM_HPQ_8821: u32 = 0x08;
pub(crate) const NORMAL_PAGE_NUM_LPQ_8821: u32 = 0x08;
pub(crate) const NORMAL_PAGE_NUM_NPQ_8821: u32 = 0x00;
pub(crate) const DRIVER_EARLY_INT_TIME_8812: u8 = 0x05;
pub(crate) const BCN_DMA_ATIME_INT_TIME_8812: u8 = 0x02;
pub(crate) const RATE_BITMAP_ALL: u32 = 0x000f_ffff;
pub(crate) const RATE_RRSR_WITHOUT_CCK: u32 = 0x000f_fff0;
pub(crate) const RATE_RRSR_CCK_ONLY_1M: u32 = 0x000f_fff1;
pub(crate) const RL_VAL_STA: u16 = 0x30;
pub(crate) const FORCEACK: u32 = BIT26;
pub(crate) const RCR_APP_MIC: u32 = BIT30;
pub(crate) const RCR_APP_ICV: u32 = BIT29;
pub(crate) const RCR_HTC_LOC_CTRL: u32 = BIT14;
pub(crate) const RCR_CBSSID_BCN: u32 = BIT7;
pub(crate) const RCR_CBSSID_DATA: u32 = BIT6;
pub(crate) const B_OFDM_EN_JAGUAR: u32 = BIT29;
pub(crate) const B_CCK_EN_JAGUAR: u32 = BIT28;
pub(crate) const B_MASK_RFE_INV_JAGUAR: u32 = 0x3ff0_0000;
pub(crate) const DIS_TSF_UDT: u8 = BIT4 as u8;
pub(crate) const EN_BCN_FUNCTION: u8 = BIT3 as u8;
pub(crate) const EN_AMPDU_RTY_NEW: u8 = BIT7 as u8;
pub(crate) const PBP_512: u8 = 0x3;

pub(crate) const RTL8814_FW_HEADER_SIZE: usize = 64;
pub(crate) const RTL8814_FW_CHECKSUM_DUMMY_SIZE: usize = 8;
pub(crate) const RTL8814_TXDESC_OFFSET: usize = 40;
pub(crate) const RTL8814_TX_PAGE_SIZE: u32 = 128;
pub(crate) const RTL8814_MAX_RSVD_PAGE_BUF_SIZE: usize = 1536 - 48;
pub(crate) const RTL8814_MAX_RSVD_PAGE_CHUNK_SIZE: usize = 4096;
pub(crate) const OCPBASE_TXBUF_3081: u32 = 0x1878_0000;
pub(crate) const OCPBASE_DMEM_3081: u32 = 0x0020_0000;
pub(crate) const OCPBASE_IMEM_3081: u32 = 0x0000_0000;

pub(crate) const IMEM_DL_RDY_8814: u8 = BIT3 as u8;
pub(crate) const IMEM_CHKSUM_OK_8814: u8 = BIT4 as u8;
pub(crate) const DMEM_DL_RDY_8814: u8 = BIT5 as u8;
pub(crate) const DMEM_CHKSUM_OK_8814: u8 = BIT6 as u8;
pub(crate) const DDMA_LEN_MASK_8814: u32 = 0x0001_ffff;
pub(crate) const DDMA_CH_CHKSUM_CNT_8814: u32 = BIT24;
pub(crate) const DDMA_RST_CHKSUM_STS_8814: u32 = BIT25;
pub(crate) const DDMA_CHKSUM_FAIL_8814: u32 = BIT27;
pub(crate) const DDMA_CHKSUM_EN_8814: u32 = BIT29;
pub(crate) const DDMA_CH_OWN_8814: u32 = BIT31;

pub(crate) const TXPKT_PGNUM_8814: u16 = 2048 - 0x0a;
pub(crate) const HPQ_PGNUM_8814: u32 = 0x20;
pub(crate) const LPQ_PGNUM_8814: u32 = 0x20;
pub(crate) const NPQ_PGNUM_8814: u32 = 0x20;
pub(crate) const EPQ_PGNUM_8814: u32 = 0x20;
pub(crate) const PUB_PGNUM_8814: u32 =
    TXPKT_PGNUM_8814 as u32 - HPQ_PGNUM_8814 - LPQ_PGNUM_8814 - NPQ_PGNUM_8814 - EPQ_PGNUM_8814;
pub(crate) const TX_PAGE_BOUNDARY_8814: u16 = TXPKT_PGNUM_8814;
pub(crate) const RX_DMA_BOUNDARY_8814: u16 = 0x5c00 - 1;

pub(crate) const TX_SELE_HQ: u8 = 1 << 0;
pub(crate) const TX_SELE_LQ: u8 = 1 << 1;
pub(crate) const TX_SELE_NQ: u8 = 1 << 2;
pub(crate) const TX_SELE_EQ: u8 = 1 << 3;
pub(crate) const QUEUE_EXTRA: u16 = 0;
pub(crate) const QUEUE_LOW: u16 = 1;
pub(crate) const QUEUE_NORMAL: u16 = 2;
pub(crate) const QUEUE_HIGH: u16 = 3;

pub(crate) const fn txdma_voq_map(queue: u16) -> u16 {
    (queue & 0x3) << 4
}

pub(crate) const fn txdma_viq_map(queue: u16) -> u16 {
    (queue & 0x3) << 6
}

pub(crate) const fn txdma_beq_map(queue: u16) -> u16 {
    (queue & 0x3) << 8
}

pub(crate) const fn txdma_bkq_map(queue: u16) -> u16 {
    (queue & 0x3) << 10
}

pub(crate) const fn txdma_mgq_map(queue: u16) -> u16 {
    (queue & 0x3) << 12
}

pub(crate) const fn txdma_hiq_map(queue: u16) -> u16 {
    (queue & 0x3) << 14
}
