#[cfg(not(target_arch = "wasm32"))]
use crate::tx::{build_usb_tx_frame, RealtekTxDescriptor, RealtekTxOptions};
use nusb::descriptors::TransferType;
#[cfg(not(target_arch = "wasm32"))]
use nusb::transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Out, Recipient};
#[cfg(not(target_arch = "wasm32"))]
use nusb::MaybeFuture;
use openipc_core::realtek::{parse_rx_aggregate_with_kind, RealtekRxPacket, RxDescriptorKind};

#[cfg(not(target_arch = "wasm32"))]
use crate::regs::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::types::{
    is_supported_id, ChipInfo, DriverOptions, InitReport, MonitorOptions, RadioConfig,
};
use crate::types::{supported_family_hint, ChipFamily, DriverError};
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
}

impl RealtekDevice {
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    /// Open the first visible adapter matching [`DriverOptions`].
    pub fn open_first(options: DriverOptions) -> Result<Self, DriverError> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("list_devices failed: {err}")))?
            .find(|dev| device_matches_options(dev.vendor_id(), dev.product_id(), options))
            .ok_or(DriverError::DeviceNotFound)?;

        let vendor_id = info.vendor_id();
        let product_id = info.product_id();
        let device = info.open().wait().map_err(|err| {
            DriverError::Nusb(format!(
                "open {vendor_id:04x}:{product_id:04x} failed: {err}"
            ))
        })?;

        Self::from_nusb_device(device, options)
    }

    #[cfg(target_os = "android")]
    /// Android does not support desktop enumeration; use `nusb::Device::from_fd`.
    pub fn open_first(_options: DriverOptions) -> Result<Self, DriverError> {
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
            device
                .reset()
                .wait()
                .map_err(|err| DriverError::Nusb(format!("device reset failed: {err}")))?;
        }

        let interface = device
            .detach_and_claim_interface(0)
            .wait()
            .map_err(|err| DriverError::Nusb(format!("claim interface 0 failed: {err}")))?;
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
        Ok(ChipInfo::from_probe(
            self.vendor_id,
            self.product_id,
            sys_cfg,
        ))
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
    /// Open and clear the selected bulk-IN endpoint.
    pub fn bulk_in_endpoint(&self) -> Result<nusb::Endpoint<Bulk, In>, DriverError> {
        let mut ep = self
            .interface
            .endpoint::<Bulk, In>(self.bulk_in_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk IN endpoint failed: {err}")))?;
        ep.clear_halt()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk IN failed: {err}")))?;
        Ok(ep)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Open and clear the selected bulk-OUT endpoint.
    pub fn bulk_out_endpoint(&self) -> Result<nusb::Endpoint<Bulk, Out>, DriverError> {
        let mut ep = self
            .interface
            .endpoint::<Bulk, Out>(self.bulk_out_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk OUT endpoint failed: {err}")))?;
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
        let chip = self.probe_chip()?;
        let usb_frame = build_usb_tx_frame(
            radiotap_packet,
            RealtekTxOptions {
                current_channel,
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
    /// Re-assert RTL8822C/RTL8812CU coex state and firmware keepalives.
    pub fn run_jaguar3_coex_keepalive(&self) -> Result<(), DriverError> {
        block_on_ready(self.run_jaguar3_coex_keepalive_async())
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
    /// Run one RTL8822C/RTL8812CU thermal power tracking update tick.
    pub fn tick_power_tracking_8822c(
        &self,
        state: &mut Jaguar3PowerTrackingState,
    ) -> Result<Jaguar3PowerTrackingReport, DriverError> {
        block_on_ready(self.tick_power_tracking_8822c_async(state))
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
        let completion = ep.transfer_blocking(Buffer::from(usb_frame), USB_TIMEOUT);
        completion
            .status
            .map_err(|err| DriverError::Nusb(format!("bulk OUT transfer failed: {err}")))?;
        Ok(completion.actual_len)
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
        match supported_family_hint(self.vendor_id, self.product_id) {
            Some(ChipFamily::Rtl8822c) => RxDescriptorKind::Jaguar3,
            _ => RxDescriptorKind::Jaguar1,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Perform a Realtek vendor control read.
    pub fn read_register(&self, register: u16, len: u16) -> Result<Vec<u8>, DriverError> {
        self.interface
            .control_in(
                ControlIn {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: REALTEK_VENDOR_READ_REQUEST,
                    value: register,
                    index: 0,
                    length: len,
                },
                USB_TIMEOUT,
            )
            .wait()
            .map_err(|err| DriverError::Nusb(format!("vendor read 0x{register:04x} failed: {err}")))
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
        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: REALTEK_VENDOR_WRITE_REQUEST,
                    value: register,
                    index: 0,
                    data: bytes,
                },
                USB_TIMEOUT,
            )
            .wait()
            .map_err(|err| {
                DriverError::Nusb(format!("vendor write 0x{register:04x} failed: {err}"))
            })
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
