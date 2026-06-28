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
mod types;

pub use async_diagnostics::{BbDbgportRead, ThermalBucket, ThermalStatus};
pub use async_iqk::IqkReport;
pub use async_phydm::{FalseAlarmCounters, PhydmDigState, PhydmWatchdogReport};
pub use async_power_tracking::{PowerTrackingReport, PowerTrackingState};
pub use device::RealtekDevice;
pub use types::{
    is_supported_id, list_devices, list_supported_devices, ChannelWidth, ChipFamily, ChipInfo,
    DriverError, DriverOptions, Firmware8814Mode, InitReport, InitStatus, MonitorOptions,
    RadioConfig, RfType, SupportedDevice, UsbDeviceSummary, SUPPORTED_DEVICES,
};

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
}
