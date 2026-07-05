#[cfg(not(target_arch = "wasm32"))]
use crate::tx::{build_usb_tx_frame, RealtekTxDescriptor, RealtekTxOptions};
use nusb::descriptors::TransferType;
#[cfg(not(target_arch = "wasm32"))]
use nusb::transfer::Buffer;
use nusb::transfer::{Bulk, In, Out};
#[cfg(not(target_arch = "wasm32"))]
use nusb::MaybeFuture;
use openipc_core::realtek::{parse_rx_aggregate_with_kind, RealtekRxPacket, RxDescriptorKind};
use std::sync::atomic::AtomicU8;
use std::sync::{Mutex, OnceLock};

use crate::async_continuous_tx::ContinuousTxState;
use crate::async_cw::CwToneState;
use crate::async_efuse::EfuseInfo;

#[cfg(not(target_arch = "wasm32"))]
use crate::regs::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::types::{
    is_supported_id, ChipInfo, DriverOptions, InitReport, MonitorOptions, RadioConfig,
};
use crate::types::{supported_family_hint, ChipFamily, DriverError};
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use crate::usb_lock::UsbDeviceLock;
#[cfg(not(target_arch = "wasm32"))]
use crate::usb_recovery::{
    retry_delay_ms, should_retry_transfer_error, transfer_error, BULK_RETRY_ATTEMPTS,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::usb_transport::{read_register_with_recovery, write_register_with_recovery};
#[cfg(not(target_arch = "wasm32"))]
use crate::{
    BbDbgportRead, FalseAlarmCounters, IqkReport, Jaguar3PowerTrackingReport,
    Jaguar3PowerTrackingState, PhydmDigState, PhydmWatchdogReport, PowerTrackingReport,
    PowerTrackingState, ThermalStatus,
};

/// Claimed Realtek rtl88xx USB adapter.
///
/// Use this type for native monitor-mode initialization, bulk receive, driver
/// diagnostics, and adaptive-link transmit. Browser/WASM callers normally
/// construct the same driver through the `openipc-web` WebUSB bindings.
pub struct RealtekDevice {
    pub(crate) device: nusb::Device,
    pub(crate) interface: nusb::Interface,
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    /// Selected bulk-IN endpoint address.
    pub bulk_in_ep: u8,
    /// Selected bulk-OUT endpoint address.
    pub bulk_out_ep: u8,
    pub(crate) bulk_out_ep_count: usize,
    pub(crate) detected_family: OnceLock<ChipFamily>,
    pub(crate) efuse_logical_map: OnceLock<[u8; 512]>,
    pub(crate) efuse_info: OnceLock<EfuseInfo>,
    pub(crate) cck_filter_8821c: OnceLock<[u32; 3]>,
    pub(crate) h2c_box: AtomicU8,
    pub(crate) cw_tone: Mutex<CwToneState>,
    pub(crate) continuous_tx: Mutex<ContinuousTxState>,
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    pub(crate) _usb_lock: Option<UsbDeviceLock>,
}

impl RealtekDevice {
    /// USB vendor identifier reported by the opened adapter.
    pub const fn vendor_id(&self) -> u16 {
        self.vendor_id
    }

    /// USB product identifier reported by the opened adapter.
    pub const fn product_id(&self) -> u16 {
        self.product_id
    }

    /// Selected bulk-IN endpoint address.
    pub const fn bulk_in_endpoint_address(&self) -> u8 {
        self.bulk_in_ep
    }

    /// Selected bulk-OUT endpoint address.
    pub const fn bulk_out_endpoint_address(&self) -> u8 {
        self.bulk_out_ep
    }

    /// Open the selected bulk-IN endpoint without changing endpoint state.
    ///
    /// Long-running receive loops should keep the returned endpoint alive and
    /// continuously recycle its buffers. Clearing a halt belongs in the
    /// transfer-error recovery path, not at every receive iteration.
    pub fn open_bulk_in_endpoint(&self) -> Result<nusb::Endpoint<Bulk, In>, DriverError> {
        self.interface
            .endpoint::<Bulk, In>(self.bulk_in_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk IN endpoint failed: {err}")))
    }

    /// Open the selected bulk-OUT endpoint without changing endpoint state.
    ///
    /// Long-running transmitters should retain this endpoint and drain
    /// completions instead of reopening and clearing it for every packet.
    pub fn open_bulk_out_endpoint(&self) -> Result<nusb::Endpoint<Bulk, Out>, DriverError> {
        self.interface
            .endpoint::<Bulk, Out>(self.bulk_out_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk OUT endpoint failed: {err}")))
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    /// Open the first visible adapter matching [`DriverOptions`].
    pub fn open_first(options: DriverOptions) -> Result<Self, DriverError> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("list_devices failed: {err}")))?
            .find(|dev| device_matches_options(dev.vendor_id(), dev.product_id(), options))
            .ok_or(DriverError::DeviceNotFound)?;

        Self::open_device_info(info, options)
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    /// Open one exact physical adapter returned by [`crate::list_supported_devices`].
    ///
    /// The stable id contains USB topology in addition to VID/PID, allowing
    /// callers to independently open multiple adapters of the same model.
    pub fn open_by_id(stable_id: &str, options: DriverOptions) -> Result<Self, DriverError> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("list_devices failed: {err}")))?
            .find(|device| {
                let legacy_id = format!("{:04x}:{:04x}", device.vendor_id(), device.product_id());
                (crate::types::nusb_device_id(device) == stable_id || legacy_id == stable_id)
                    && device_matches_options(device.vendor_id(), device.product_id(), options)
            })
            .ok_or(DriverError::DeviceNotFound)?;
        Self::open_device_info(info, options)
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    /// Open every visible adapter matching [`DriverOptions`].
    ///
    /// This is intended for mirror/transmit workflows where one WFB packet
    /// should be injected through all attached matching radios.
    pub fn open_all(options: DriverOptions) -> Result<Vec<Self>, DriverError> {
        let infos: Vec<_> = nusb::list_devices()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("list_devices failed: {err}")))?
            .filter(|dev| device_matches_options(dev.vendor_id(), dev.product_id(), options))
            .collect();

        if infos.is_empty() {
            return Err(DriverError::DeviceNotFound);
        }

        let mut devices = Vec::new();
        let mut errors = Vec::new();
        for info in infos {
            let vendor_id = info.vendor_id();
            let product_id = info.product_id();
            match Self::open_device_info(info, options) {
                Ok(device) => devices.push(device),
                Err(err) => errors.push(format!("{vendor_id:04x}:{product_id:04x}: {err}")),
            }
        }

        if devices.is_empty() {
            return Err(DriverError::Nusb(format!(
                "no matching Realtek adapters could be opened: {}",
                errors.join("; ")
            )));
        }

        Ok(devices)
    }

    #[cfg(target_os = "android")]
    /// Android does not support desktop enumeration; use `nusb::Device::from_fd`.
    pub fn open_first(_options: DriverOptions) -> Result<Self, DriverError> {
        Err(DriverError::Nusb(
            "Android USB discovery must use UsbManager and nusb::Device::from_fd".to_owned(),
        ))
    }

    #[cfg(target_os = "android")]
    /// Android does not support desktop enumeration; open devices from granted fds.
    pub fn open_all(_options: DriverOptions) -> Result<Vec<Self>, DriverError> {
        Err(DriverError::Nusb(
            "Android USB discovery must use UsbManager and nusb::Device::from_fd".to_owned(),
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Build a driver from an already-open `nusb::Device`.
    ///
    /// This is the path used by Android after `UsbManager` grants permission
    /// and the app passes a file descriptor to `nusb::Device::from_fd`.
    pub fn from_nusb_device(
        device: nusb::Device,
        options: DriverOptions,
    ) -> Result<Self, DriverError> {
        let descriptor = device.device_descriptor();
        let vendor_id = descriptor.vendor_id();
        let product_id = descriptor.product_id();
        if !device_matches_options(vendor_id, product_id, options) {
            return Err(DriverError::Nusb(format!(
                "unsupported or unexpected USB device {vendor_id:04x}:{product_id:04x}"
            )));
        }
        if !options.skip_reset {
            // nusb invalidates a Device after reset. An externally supplied
            // handle cannot be rediscovered safely here (notably Android's
            // UsbManager fd), so reset ownership remains with its caller.
            log::warn!(target: "openipc_rtl88xx::usb", "skip_reset=false ignored for externally opened nusb device; use open_first/open_by_id for reset and re-enumeration");
        }
        let interface = device
            .detach_and_claim_interface(0)
            .wait()
            .map_err(|err| DriverError::Nusb(format!("claim interface 0 failed: {err}")))?;
        #[cfg(target_os = "android")]
        return Self::from_claimed_device(device, interface, options);
        #[cfg(not(target_os = "android"))]
        Self::from_claimed_device(device, interface, options, None)
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn open_device_info(
        info: nusb::DeviceInfo,
        options: DriverOptions,
    ) -> Result<Self, DriverError> {
        let vendor_id = info.vendor_id();
        let product_id = info.product_id();
        let stable_id = crate::types::nusb_device_id(&info);
        let bus_id = info.bus_id().to_owned();
        let port_chain = info.port_chain().to_vec();
        let usb_lock = UsbDeviceLock::acquire(&info)?;

        let (mut device, mut interface) = Self::open_and_claim(&info)?;
        let reset_requested = !options.skip_reset && !cfg!(target_os = "windows");
        if !options.skip_reset && cfg!(target_os = "windows") {
            log::debug!(target: "openipc_rtl88xx::usb", "USB reset skipped because nusb does not support device reset on Windows");
        }
        if reset_requested {
            device
                .reset()
                .wait()
                .map_err(|err| DriverError::Nusb(format!("device reset failed: {err}")))?;
            drop(interface);
            drop(device);

            let mut reopened = None;
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let Ok(devices) = nusb::list_devices().wait() else {
                    continue;
                };
                let candidate = devices.into_iter().find(|candidate| {
                    if !port_chain.is_empty() {
                        candidate.bus_id() == bus_id && candidate.port_chain() == port_chain
                    } else {
                        candidate.bus_id() == bus_id
                            && candidate.vendor_id() == vendor_id
                            && candidate.product_id() == product_id
                    }
                });
                if let Some(candidate) = candidate {
                    reopened = Some(Self::open_and_claim(&candidate)?);
                    break;
                }
            }
            (device, interface) = reopened.ok_or_else(|| {
                DriverError::Nusb(format!(
                    "USB adapter {stable_id} did not re-enumerate after reset"
                ))
            })?;
        }

        Self::from_claimed_device(device, interface, options, Some(usb_lock))
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn open_and_claim(
        info: &nusb::DeviceInfo,
    ) -> Result<(nusb::Device, nusb::Interface), DriverError> {
        let id = crate::types::nusb_device_id(info);
        let device = info
            .open()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("open adapter {id} failed: {err}")))?;
        let interface = device
            .detach_and_claim_interface(0)
            .wait()
            .map_err(|err| DriverError::Nusb(format!("claim interface 0 failed: {err}")))?;
        Ok((device, interface))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn from_claimed_device(
        device: nusb::Device,
        interface: nusb::Interface,
        options: DriverOptions,
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))] usb_lock: Option<
            UsbDeviceLock,
        >,
    ) -> Result<Self, DriverError> {
        let descriptor = device.device_descriptor();
        let vendor_id = descriptor.vendor_id();
        let product_id = descriptor.product_id();
        let (bulk_in_ep, bulk_out_ep, bulk_out_ep_count) =
            discover_bulk_endpoints_with_override(&interface, options.tx_endpoint_override)?;

        Ok(Self {
            device,
            interface,
            vendor_id,
            product_id,
            bulk_in_ep,
            bulk_out_ep,
            bulk_out_ep_count,
            detected_family: OnceLock::new(),
            efuse_logical_map: OnceLock::new(),
            efuse_info: OnceLock::new(),
            cck_filter_8821c: OnceLock::new(),
            h2c_box: AtomicU8::new(0),
            cw_tone: Mutex::new(CwToneState::default()),
            continuous_tx: Mutex::new(ContinuousTxState::default()),
            #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
            _usb_lock: usb_lock,
        })
    }

    /// Return the USB connection speed reported by `nusb`, if known.
    pub fn device_speed(&self) -> Option<nusb::Speed> {
        self.device.speed()
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Probe chip family, RF path count, and cut version from hardware.
    pub fn probe_chip(&self) -> Result<ChipInfo, DriverError> {
        let sys_cfg = self.read_u32(REG_SYS_CFG)?;
        let chip_id = self.read_u8(0x00fc).unwrap_or(0);
        let chip = ChipInfo::from_probe(self.vendor_id, self.product_id, sys_cfg, chip_id);
        let _ = self.detected_family.set(chip.family);
        Ok(chip)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Initialize the adapter for OpenIPC monitor-mode receive.
    pub fn initialize_monitor(&self, radio: RadioConfig) -> Result<InitReport, DriverError> {
        block_on_ready(self.initialize_monitor_async(radio, false))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Initialize monitor mode with explicit bring-up options.
    pub fn initialize_monitor_with_options(
        &self,
        radio: RadioConfig,
        options: MonitorOptions,
    ) -> Result<InitReport, DriverError> {
        block_on_ready(self.initialize_monitor_with_options_async(radio, options))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Initialize monitor mode while controlling bad-FCS acceptance.
    pub fn initialize_monitor_accept_bad_fcs(
        &self,
        radio: RadioConfig,
        accept_bad_fcs: bool,
    ) -> Result<InitReport, DriverError> {
        block_on_ready(self.initialize_monitor_async(radio, accept_bad_fcs))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Best-effort monitor-mode shutdown for chips that need explicit deinit.
    pub fn shutdown_monitor(&self) -> Result<(), DriverError> {
        block_on_ready(self.shutdown_monitor_async())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Radiate an unmodulated carrier at the currently tuned channel center.
    ///
    /// `gain` uses the Realtek RF gain-index range; values are masked to 0..31
    /// to match devourer's MP single-tone interface.
    pub fn start_cw_tone(&self, channel: u8, gain: u8) -> Result<(), DriverError> {
        block_on_ready(self.start_cw_tone_async(channel, gain))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Stop an active CW carrier and restore the snapshotted RF/BB state.
    pub fn stop_cw_tone(&self) -> Result<(), DriverError> {
        block_on_ready(self.stop_cw_tone_async())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Retune an initialized monitor-mode adapter without repeating firmware bring-up.
    pub fn retune(&self, radio: RadioConfig) -> Result<(), DriverError> {
        block_on_ready(self.retune_async(radio))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Select the active Jaguar1 receive chains for diversity diagnostics.
    pub fn set_rx_path_mask(&self, mask: u8) -> Result<(), DriverError> {
        block_on_ready(self.set_rx_path_mask_async(mask))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Open and clear the selected bulk-IN endpoint.
    pub fn bulk_in_endpoint(&self) -> Result<nusb::Endpoint<Bulk, In>, DriverError> {
        let mut ep = self.open_bulk_in_endpoint()?;
        ep.clear_halt()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk IN failed: {err}")))?;
        Ok(ep)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Open and clear the selected bulk-OUT endpoint.
    pub fn bulk_out_endpoint(&self) -> Result<nusb::Endpoint<Bulk, Out>, DriverError> {
        let mut ep = self.open_bulk_out_endpoint()?;
        ep.clear_halt()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk OUT failed: {err}")))?;
        Ok(ep)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Build a Realtek TX descriptor for a radiotap+802.11 packet and send it.
    pub fn send_packet(
        &self,
        radiotap_packet: &[u8],
        current_channel: u8,
    ) -> Result<usize, DriverError> {
        self.send_packet_for_radio(
            radiotap_packet,
            crate::types::RadioConfig {
                channel: current_channel,
                ..crate::types::RadioConfig::default()
            },
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Build and send a packet using the adapter's full RF channel configuration.
    ///
    /// Passing the configured width is required for Jaguar3 40-in-80 subchannel
    /// placement; [`Self::send_packet`] remains the 20 MHz compatibility API.
    pub fn send_packet_for_radio(
        &self,
        radiotap_packet: &[u8],
        radio: crate::types::RadioConfig,
    ) -> Result<usize, DriverError> {
        let chip = self.probe_chip()?;
        let usb_frame = build_usb_tx_frame(
            radiotap_packet,
            RealtekTxOptions {
                current_channel: radio.channel,
                configured_channel_width: radio.channel_width,
                descriptor: RealtekTxDescriptor::for_chip_family(chip.family),
                legacy_8812_descriptor: std::env::var_os("DEVOURER_TX_LEGACY_8812_DESC").is_some(),
                ..RealtekTxOptions::default()
            },
        )
        .map_err(DriverError::TxBuild)?;
        let mut ep = self.bulk_out_endpoint()?;
        Self::send_usb_tx_frame_on(&mut ep, &usb_frame)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Override the Realtek TXAGC index used for adaptive-link uplink packets.
    pub fn set_tx_power_override(&self, current_channel: u8, power: u8) -> Result<(), DriverError> {
        block_on_ready(self.set_tx_power_override_async(current_channel, power))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Re-assert Jaguar3 coex state and firmware keepalives.
    pub fn run_jaguar3_coex_keepalive(&self) -> Result<(), DriverError> {
        block_on_ready(self.run_jaguar3_coex_keepalive_async())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Prepare the initialized adapter for a transmit-only workload.
    pub fn prepare_transmit_only(&self) -> Result<(), DriverError> {
        block_on_ready(self.prepare_transmit_only_async())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Read current thermal status.
    pub fn read_thermal_status(&self) -> Result<ThermalStatus, DriverError> {
        block_on_ready(self.read_thermal_status_async())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Read RTL8814 queue-depth diagnostics.
    pub fn read_queue_depth_8814(&self) -> Result<[u32; 5], DriverError> {
        block_on_ready(self.read_queue_depth_8814_async())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Read several bulk-IN transfers with multiple URBs in flight.
    pub fn read_rx_transfers(
        &self,
        length: usize,
        in_flight: usize,
    ) -> Result<Vec<Vec<u8>>, DriverError> {
        block_on_ready(self.read_rx_transfers_async(length, in_flight))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Read a baseband register through the Realtek PHY helpers.
    pub fn read_bb_reg(&self, register: u16, mask: u32) -> Result<u32, DriverError> {
        block_on_ready(self.read_bb_reg_async(register, mask))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Read a value through the baseband debug port.
    pub fn read_bb_dbgport(&self, selector: u32) -> Result<BbDbgportRead, DriverError> {
        block_on_ready(self.read_bb_dbgport_async(selector))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Read PHY false-alarm counters.
    pub fn read_false_alarm_counters(&self) -> Result<FalseAlarmCounters, DriverError> {
        block_on_ready(self.read_false_alarm_counters_async())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Run IQK calibration for the selected chip/channel.
    pub fn run_iqk(&self, chip: ChipInfo, channel: u8) -> Result<IqkReport, DriverError> {
        block_on_ready(self.run_iqk_async(chip, channel))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Run one PHYDM watchdog/DIG update tick.
    pub fn run_phydm_watchdog_tick(
        &self,
        state: &mut PhydmDigState,
    ) -> Result<PhydmWatchdogReport, DriverError> {
        block_on_ready(self.run_phydm_watchdog_tick_async(state))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Initialize RTL8812 thermal power tracking state.
    pub fn init_power_tracking_8812(
        &self,
        state: &mut PowerTrackingState,
    ) -> Result<(), DriverError> {
        block_on_ready(self.init_power_tracking_8812_async(state))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Clear RTL8812 thermal power tracking state.
    pub fn clear_power_tracking_8812(
        &self,
        state: &mut PowerTrackingState,
    ) -> Result<(), DriverError> {
        block_on_ready(self.clear_power_tracking_8812_async(state))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Run one RTL8812 thermal power tracking update tick.
    pub fn tick_power_tracking_8812(
        &self,
        state: &mut PowerTrackingState,
        channel: u8,
        width: crate::types::ChannelWidth,
    ) -> Result<PowerTrackingReport, DriverError> {
        block_on_ready(self.tick_power_tracking_8812_async(state, channel, width))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Run one Jaguar3 thermal power tracking update tick.
    pub fn tick_jaguar3_power_tracking(
        &self,
        state: &mut Jaguar3PowerTrackingState,
    ) -> Result<Jaguar3PowerTrackingReport, DriverError> {
        block_on_ready(self.tick_jaguar3_power_tracking_async(state))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Compatibility alias for [`Self::tick_jaguar3_power_tracking`].
    pub fn tick_power_tracking_8822c(
        &self,
        state: &mut Jaguar3PowerTrackingState,
    ) -> Result<Jaguar3PowerTrackingReport, DriverError> {
        self.tick_jaguar3_power_tracking(state)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Send a radiotap+802.11 packet using an already-open bulk-OUT endpoint.
    pub fn send_packet_on(
        ep: &mut nusb::Endpoint<Bulk, Out>,
        radiotap_packet: &[u8],
        options: RealtekTxOptions,
    ) -> Result<usize, DriverError> {
        let usb_frame =
            build_usb_tx_frame(radiotap_packet, options).map_err(DriverError::TxBuild)?;
        Self::send_usb_tx_frame_on(ep, &usb_frame)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Send a fully-built Realtek USB TX frame on an existing endpoint.
    pub fn send_usb_tx_frame_on(
        ep: &mut nusb::Endpoint<Bulk, Out>,
        usb_frame: &[u8],
    ) -> Result<usize, DriverError> {
        for attempt in 0..BULK_RETRY_ATTEMPTS {
            let completion = ep.transfer_blocking(Buffer::from(usb_frame), tx_timeout());
            match completion.status {
                Ok(()) => return Ok(completion.actual_len),
                Err(err) if should_retry_transfer_error(err, attempt, BULK_RETRY_ATTEMPTS) => {
                    // A timed-out blocking nusb transfer is surfaced as
                    // Cancelled. Devourer marks every non-OK async completion
                    // as potentially wedged and re-clears the endpoint before
                    // the next frame, so do the same for both transient and
                    // explicit stall completions here.
                    let _ = ep.clear_halt().wait();
                    std::thread::sleep(std::time::Duration::from_millis(
                        retry_delay_ms(attempt) as u64
                    ));
                }
                Err(err) => return Err(transfer_error("bulk OUT transfer failed", err)),
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
    }

    /// Parse a Realtek RX aggregate using the shared core parser.
    pub fn parse_rx_transfer<'a>(
        &self,
        transfer: &'a [u8],
    ) -> Result<Vec<RealtekRxPacket<'a>>, DriverError> {
        parse_rx_aggregate_with_kind(transfer, self.rx_descriptor_kind())
            .map_err(DriverError::InvalidTransfer)
    }

    /// Return the RX descriptor layout implied by this adapter's VID/PID.
    pub fn rx_descriptor_kind(&self) -> RxDescriptorKind {
        match self
            .detected_family
            .get()
            .copied()
            .or_else(|| supported_family_hint(self.vendor_id, self.product_id))
        {
            Some(ChipFamily::Rtl8822b | ChipFamily::Rtl8821c) => RxDescriptorKind::Jaguar2,
            Some(family) if family.is_jaguar3() => RxDescriptorKind::Jaguar3,
            _ => RxDescriptorKind::Jaguar1,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Perform a Realtek vendor control read.
    pub fn read_register(&self, register: u16, len: u16) -> Result<Vec<u8>, DriverError> {
        read_register_with_recovery(&self.interface, register, len)
    }

    #[cfg(target_arch = "wasm32")]
    /// Blocking vendor reads are unavailable on WASM; use async WebUSB APIs.
    pub fn read_register(&self, register: u16, _len: u16) -> Result<Vec<u8>, DriverError> {
        Err(DriverError::Nusb(format!(
            "blocking vendor read 0x{register:04x} is unavailable on wasm"
        )))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Perform a Realtek vendor control write.
    pub fn write_register(&self, register: u16, bytes: &[u8]) -> Result<(), DriverError> {
        write_register_with_recovery(&self.interface, register, bytes)
    }

    #[cfg(target_arch = "wasm32")]
    /// Blocking vendor writes are unavailable on WASM; use async WebUSB APIs.
    pub fn write_register(&self, register: u16, _bytes: &[u8]) -> Result<(), DriverError> {
        Err(DriverError::Nusb(format!(
            "blocking vendor write 0x{register:04x} is unavailable on wasm"
        )))
    }

    /// Read an 8-bit little-endian register value.
    pub fn read_u8(&self, register: u16) -> Result<u8, DriverError> {
        let bytes = self.read_register(register, 1)?;
        bytes.first().copied().ok_or(DriverError::RegisterReadSize {
            expected: 1,
            actual: bytes.len(),
        })
    }

    /// Read a 16-bit little-endian register value.
    pub fn read_u16(&self, register: u16) -> Result<u16, DriverError> {
        let bytes = self.read_register(register, 2)?;
        let array: [u8; 2] =
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| DriverError::RegisterReadSize {
                    expected: 2,
                    actual: bytes.len(),
                })?;
        Ok(u16::from_le_bytes(array))
    }

    /// Read a 32-bit little-endian register value.
    pub fn read_u32(&self, register: u16) -> Result<u32, DriverError> {
        let bytes = self.read_register(register, 4)?;
        let array: [u8; 4] =
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| DriverError::RegisterReadSize {
                    expected: 4,
                    actual: bytes.len(),
                })?;
        Ok(u32::from_le_bytes(array))
    }

    /// Write an 8-bit register value.
    pub fn write_u8(&self, register: u16, value: u8) -> Result<(), DriverError> {
        self.write_register(register, &[value])
    }

    /// Write a 16-bit little-endian register value.
    pub fn write_u16(&self, register: u16, value: u16) -> Result<(), DriverError> {
        self.write_register(register, &value.to_le_bytes())
    }

    /// Write a 32-bit little-endian register value.
    pub fn write_u32(&self, register: u16, value: u32) -> Result<(), DriverError> {
        self.write_register(register, &value.to_le_bytes())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for RealtekDevice {
    fn drop(&mut self) {
        let tone_active = self
            .cw_tone
            .get_mut()
            .map(|state| state.is_active())
            .unwrap_or(false);
        if tone_active {
            if let Err(err) = block_on_ready(self.stop_cw_tone_async()) {
                log::warn!(target: "openipc_rtl88xx::cw", "failed to restore CW tone state while dropping adapter: {err}");
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn tx_timeout() -> std::time::Duration {
    static TIMEOUT: OnceLock<std::time::Duration> = OnceLock::new();
    *TIMEOUT.get_or_init(|| {
        std::env::var("DEVOURER_TX_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(std::time::Duration::from_millis)
            .unwrap_or(USB_TIMEOUT)
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn block_on_ready<F: std::future::Future>(future: F) -> F::Output {
    use std::pin::pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn clone(_: *const ()) -> RawWaker {
        raw_waker()
    }
    fn wake(_: *const ()) {}
    fn wake_by_ref(_: *const ()) {}
    fn drop(_: *const ()) {}
    fn raw_waker() -> RawWaker {
        RawWaker::new(
            std::ptr::null(),
            &RawWakerVTable::new(clone, wake, wake_by_ref, drop),
        )
    }

    let waker = unsafe { Waker::from_raw(raw_waker()) };
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

pub(crate) fn discover_bulk_endpoints_with_override(
    interface: &nusb::Interface,
    bulk_out_override: Option<u8>,
) -> Result<(u8, u8, usize), DriverError> {
    let descriptor = interface
        .descriptor()
        .ok_or_else(|| DriverError::Nusb("interface 0 has no active descriptor".to_owned()))?;
    let mut bulk_in = None;
    let mut bulk_out = None;
    let mut override_found = false;
    let mut bulk_out_count = 0usize;
    for endpoint in descriptor.endpoints() {
        if endpoint.transfer_type() != TransferType::Bulk {
            continue;
        }
        let address = endpoint.address();
        if address & 0x80 != 0 {
            bulk_in.get_or_insert(address);
        } else {
            bulk_out_count += 1;
            if Some(address) == bulk_out_override {
                override_found = true;
                bulk_out = Some(address);
            }
            bulk_out.get_or_insert(address);
        }
    }
    if let Some(endpoint) = bulk_out_override {
        if !override_found {
            return Err(DriverError::EndpointOverrideNotFound(endpoint));
        }
    }
    Ok((
        bulk_in.ok_or(DriverError::EndpointNotFound("bulk IN"))?,
        bulk_out.ok_or(DriverError::EndpointNotFound("bulk OUT"))?,
        bulk_out_count,
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn device_matches_options(vendor_id: u16, product_id: u16, options: DriverOptions) -> bool {
    match (options.target_vendor_id, options.target_product_id) {
        (Some(vid), Some(pid)) => vendor_id == vid && product_id == pid,
        (Some(vid), None) => vendor_id == vid && is_supported_id(vendor_id, product_id),
        (None, Some(pid)) => product_id == pid && is_supported_id(vendor_id, product_id),
        (None, None) => is_supported_id(vendor_id, product_id),
    }
}
