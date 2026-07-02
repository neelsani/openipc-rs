//! Rust Realtek rtl88xx USB receiver support for OpenIPC.
//!
//! The code in this crate is the hardware-facing half of `openipc-rs`: it owns
//! Realtek vendor-control register access, firmware download, PHY table loading,
//! monitor-mode RX setup, and bulk-IN receive transfers. Packet parsing and the
//! OpenIPC/WFB/RTP pipeline live in `openipc-core`.

mod async_diagnostics;
mod async_driver;
mod async_efuse;
mod async_firmware;
mod async_firmware_8814;
mod async_iqk;
mod async_iqk_8812;
mod async_jaguar3;
mod async_jaguar3_8822e;
mod async_jaguar3_iqk;
mod async_mac;
mod async_phydm;
mod async_power_tracking;
mod async_radio;
mod async_tables;
mod async_tx_power;
mod device;
mod firmware;
mod phy;
mod power;
mod regs;
mod rtl_data;
mod time;
mod tx;
mod tx_power_defaults;
mod types;
mod usb_recovery;
mod usb_transport;

pub use async_diagnostics::{BbDbgportRead, ThermalBucket, ThermalStatus};
pub use async_iqk::IqkReport;
pub use async_jaguar3_iqk::{Jaguar3PowerTrackingReport, Jaguar3PowerTrackingState};
pub use async_phydm::{FalseAlarmCounters, PhydmDigState, PhydmWatchdogReport};
pub use async_power_tracking::{PowerTrackingReport, PowerTrackingState};
pub use device::RealtekDevice;
pub use tx::{
    build_usb_tx_frame, RealtekTxDescriptor, RealtekTxError, RealtekTxOptions, TX_DESC_SIZE,
    TX_DESC_SIZE_8822C,
};
pub use types::{
    is_supported_id, list_devices, list_supported_devices, supported_device, supported_family_hint,
    ChannelWidth, ChipFamily, ChipInfo, DriverError, DriverOptions, Firmware8814Mode, InitReport,
    InitStatus, MonitorOptions, RadioConfig, RfType, SupportedDevice, UsbDeviceSummary,
    SUPPORTED_DEVICES,
};

/// Default native USB bulk-IN transfer size used for RX reads.
pub const DEFAULT_RX_TRANSFER_SIZE: usize = openipc_core::realtek::DEFAULT_RX_TRANSFER_SIZE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_8812_firmware_header() {
        let mut fw = vec![0; 40];
        fw[0] = 0x00;
        fw[1] = 0x95;
        assert_eq!(
            firmware::strip_firmware_header(ChipFamily::Rtl8812, &fw).len(),
            8
        );
    }

    #[test]
    fn detects_8821_pid() {
        assert!(types::is_rtl8821a_pid(0x2357, 0x0120));
        assert!(!types::is_rtl8821a_pid(0x0bda, 0x8812));
    }

    #[test]
    fn detects_jaguar3_pids_from_devourer() {
        assert!(types::is_rtl8822c_pid(0x0bda, 0xc812));
        assert!(types::is_rtl8822c_pid(0x0bda, 0xc82c));
        assert!(types::is_rtl8822c_pid(0x0bda, 0xc82e));
        assert_eq!(
            supported_family_hint(0x0bda, 0xc812),
            Some(ChipFamily::Rtl8822c)
        );
    }

    #[test]
    fn detects_rtl8822e_pids_and_shared_pid_chip_id() {
        for product_id in [0x881c, 0xa81a, 0xe822, 0xa82a] {
            assert!(types::is_rtl8822e_pid(0x0bda, product_id));
            assert_eq!(
                supported_family_hint(0x0bda, product_id),
                Some(ChipFamily::Rtl8822e)
            );
        }
        let eu = ChipInfo::from_probe(0x0bda, 0x8812, 0, 0x17);
        assert_eq!(eu.family, ChipFamily::Rtl8822e);
        assert_eq!(eu.rf_type, RfType::TwoTTwoR);

        let cu = ChipInfo::from_probe(0x0bda, 0x8812, 0, 0x13);
        assert_eq!(cu.family, ChipFamily::Rtl8822c);

        // SYS_CFG2 remains authoritative even when a PID hint points elsewhere.
        assert_eq!(
            ChipInfo::from_probe(0x0bda, 0xa81a, 0, 0x13).family,
            ChipFamily::Rtl8822c
        );
        assert_eq!(
            ChipInfo::from_probe(0x0bda, 0xc812, 0, 0x17).family,
            ChipFamily::Rtl8822e
        );
    }
}
