use std::sync::atomic::Ordering;

use crate::{AmpduMode, ChipFamily, DriverError, RealtekDevice};

const REG_NET_TYPE: u16 = 0x0102;
const REG_AMPDU_MAX_TIME_JAGUAR1: u16 = 0x0456;
const REG_AMPDU_MAX_TIME_HALMAC: u16 = 0x0455;
const REG_SW_AMPDU_BURST_MODE_CTRL: u16 = 0x04bc;
const REG_TDECTRL: u16 = 0x0208;
const REG_MACID: u16 = 0x0610;
const REG_BSSID: u16 = 0x0618;
const NET_TYPE_MASK: u8 = 0x03;
const NET_TYPE_AP: u8 = 0x03;
const BURST_MODE_BIT: u8 = 1 << 6;

impl RealtekDevice {
    /// Arm the SIFS-timed hardware ACK and BlockAck responder for one MAC.
    ///
    /// Monitor RX and injection remain enabled. The address must be unicast;
    /// enabling this changes a passive monitor adapter into an active peer.
    pub async fn set_ack_responder_async(&self, mac: [u8; 6]) -> Result<(), DriverError> {
        if mac[0] & 1 != 0 {
            return Err(DriverError::InvalidAckResponderMac(mac));
        }
        let low = u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]);
        let high = u16::from_le_bytes([mac[4], mac[5]]);
        self.write_u32_async(REG_MACID, low).await?;
        self.write_u16_async(REG_MACID + 4, high).await?;
        self.write_u32_async(REG_BSSID, low).await?;
        self.write_u16_async(REG_BSSID + 4, high).await?;
        let net_type = self.read_u8_async(REG_NET_TYPE).await?;
        self.write_u8_async(REG_NET_TYPE, (net_type & !NET_TYPE_MASK) | NET_TYPE_AP)
            .await?;
        log::info!(target: "openipc_rtl88xx::tx", "hardware ACK/BlockAck responder armed mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
        Ok(())
    }

    /// Return the hardware ACK/BlockAck responder to monitor-mode NoLink.
    pub async fn clear_ack_responder_async(&self) -> Result<(), DriverError> {
        let net_type = self.read_u8_async(REG_NET_TYPE).await?;
        self.write_u8_async(REG_NET_TYPE, net_type & !NET_TYPE_MASK)
            .await?;
        log::info!(target: "openipc_rtl88xx::tx", "hardware ACK/BlockAck responder disarmed");
        Ok(())
    }

    /// Apply persistent A-MPDU descriptor state and generation-specific pacing.
    pub async fn set_ampdu_mode_async(&self, mode: AmpduMode) -> Result<(), DriverError> {
        let family = self.probe_chip_async().await?.family;
        if mode.enabled {
            if mode.max_time != 0 {
                self.write_u8_async(ampdu_time_register(family), mode.max_time)
                    .await?;
            }
            if mode.clear_burst_mode
                && matches!(
                    family,
                    ChipFamily::Rtl8814 | ChipFamily::Rtl8822b | ChipFamily::Rtl8821c
                )
            {
                let value = self.read_u8_async(REG_SW_AMPDU_BURST_MODE_CTRL).await?;
                self.write_u8_async(REG_SW_AMPDU_BURST_MODE_CTRL, value & !BURST_MODE_BIT)
                    .await?;
            }
        } else {
            let default_time = if family == ChipFamily::Rtl8821 {
                0x5e
            } else {
                0x70
            };
            self.write_u8_async(ampdu_time_register(family), default_time)
                .await?;
            if matches!(
                family,
                ChipFamily::Rtl8814 | ChipFamily::Rtl8822b | ChipFamily::Rtl8821c
            ) {
                let value = self.read_u8_async(REG_SW_AMPDU_BURST_MODE_CTRL).await?;
                self.write_u8_async(REG_SW_AMPDU_BURST_MODE_CTRL, value | BURST_MODE_BIT)
                    .await?;
            }
        }
        *self
            .ampdu_mode
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)? = mode;
        log::info!(target: "openipc_rtl88xx::tx", "A-MPDU mode enabled={} tid={} max={} density={} no_ack={} max_time=0x{:02x}", mode.enabled, mode.tid, mode.max_num, mode.density, mode.no_ack, mode.max_time);
        Ok(())
    }

    /// Disable A-MPDU descriptors and restore generation bring-up pacing.
    pub async fn clear_ampdu_mode_async(&self) -> Result<(), DriverError> {
        self.set_ampdu_mode_async(AmpduMode::disabled()).await
    }

    /// Current persistent A-MPDU state used by packet builders.
    pub fn ampdu_mode(&self) -> AmpduMode {
        self.ampdu_mode
            .lock()
            .map_or_else(|_| AmpduMode::disabled(), |mode| *mode)
    }

    /// Enable or disable firmware CCX reports for subsequently built frames.
    pub fn set_tx_reports(&self, enabled: bool) {
        self.tx_reports_enabled.store(enabled, Ordering::Release);
    }

    /// Configure TXDMA and the maximum frames packed into one USB transfer.
    pub async fn set_usb_tx_aggregation_async(&self, max_frames: usize) -> Result<(), DriverError> {
        let family = self.probe_chip_async().await?.family;
        if max_frames > 0 && family.is_jaguar1() {
            let descriptors_per_bulk = match family {
                ChipFamily::Rtl8812 => 1u8,
                ChipFamily::Rtl8814 => 3u8,
                ChipFamily::Rtl8821 => 6u8,
                _ => unreachable!("Jaguar1 family matched above"),
            };
            let value = self.read_u32_async(REG_TDECTRL).await?;
            self.write_u32_async(
                REG_TDECTRL,
                (value & !0xf0) | (u32::from(descriptors_per_bulk) << 4),
            )
            .await?;
            if family == ChipFamily::Rtl8814 {
                self.write_u8_async(REG_TDECTRL + 3, descriptors_per_bulk << 1)
                    .await?;
            }
        }
        self.usb_tx_aggregate_max
            .store(max_frames, Ordering::Release);
        log::info!(target: "openipc_rtl88xx::tx", "USB TX aggregation max_frames={max_frames}");
        Ok(())
    }

    /// Current configured maximum frames per USB TX aggregate.
    pub fn usb_tx_aggregation(&self) -> usize {
        self.usb_tx_aggregate_max.load(Ordering::Acquire)
    }
}

const fn ampdu_time_register(family: ChipFamily) -> u16 {
    if family.is_jaguar1() {
        REG_AMPDU_MAX_TIME_JAGUAR1
    } else {
        REG_AMPDU_MAX_TIME_HALMAC
    }
}
