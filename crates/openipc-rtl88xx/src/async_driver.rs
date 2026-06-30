//! Async-shaped Realtek driver entry points.
//!
//! On wasm these methods are backed by WebUSB promises. On native targets they
//! wrap blocking `nusb` operations so the same HAL sequences can be shared
//! across targets; call them from a worker or other blocking context.

use nusb::transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Out, Recipient};
#[cfg(not(target_arch = "wasm32"))]
use nusb::MaybeFuture;

#[cfg(target_arch = "wasm32")]
use crate::device::discover_bulk_endpoints_with_override;
use crate::device::RealtekDevice;
use crate::regs::*;
use crate::tx::{build_usb_tx_frame, RealtekTxOptions};
#[cfg(target_arch = "wasm32")]
use crate::types::is_supported_id;
use crate::types::{
    ChipFamily, ChipInfo, DriverError, DriverOptions, InitReport, InitStatus, MonitorOptions,
    RadioConfig,
};
use crate::PowerTrackingState;

impl RealtekDevice {
    /// Open the first supported Realtek USB adapter using async-shaped API.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn open_first_async(options: DriverOptions) -> Result<Self, DriverError> {
        Self::open_first(options)
    }

    /// Build a Realtek device from a browser WebUSB device.
    #[cfg(target_arch = "wasm32")]
    pub async fn from_web_usb_device(device: web_sys::UsbDevice) -> Result<Self, DriverError> {
        Self::from_web_usb_device_with_options(device, DriverOptions::default()).await
    }

    /// Build a Realtek device from a browser WebUSB device with explicit options.
    #[cfg(target_arch = "wasm32")]
    pub async fn from_web_usb_device_with_options(
        device: web_sys::UsbDevice,
        options: DriverOptions,
    ) -> Result<Self, DriverError> {
        let device = nusb::Device::from_js(device)
            .await
            .map_err(|err| DriverError::Nusb(format!("nusb from_js failed: {err}")))?;
        let descriptor = device.device_descriptor();
        let vendor_id = descriptor.vendor_id();
        let product_id = descriptor.product_id();
        if !is_supported_id(vendor_id, product_id)
            && !web_usb_target_matches(vendor_id, product_id, options)
        {
            return Err(DriverError::Nusb(format!(
                "unsupported Realtek adapter {vendor_id:04x}:{product_id:04x}"
            )));
        }
        let interface = device
            .claim_interface(0)
            .await
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

    /// Probe the chip family and RF layout from the hardware IDs and SYS_CFG register.
    pub async fn probe_chip_async(&self) -> Result<ChipInfo, DriverError> {
        let sys_cfg = self.read_u32_async(REG_SYS_CFG).await?;
        Ok(ChipInfo::from_probe(
            self.vendor_id,
            self.product_id,
            sys_cfg,
        ))
    }

    /// Initialize the adapter for monitor-mode OpenIPC reception.
    pub async fn initialize_monitor_async(
        &self,
        radio: RadioConfig,
        accept_bad_fcs: bool,
    ) -> Result<InitReport, DriverError> {
        let options = MonitorOptions::from_env().with_accept_bad_fcs(accept_bad_fcs);
        self.initialize_monitor_with_options_async(radio, options)
            .await
    }

    /// Initialize the adapter for monitor-mode OpenIPC reception with full options.
    pub async fn initialize_monitor_with_options_async(
        &self,
        radio: RadioConfig,
        options: MonitorOptions,
    ) -> Result<InitReport, DriverError> {
        let chip = self.probe_chip_async().await?;
        let mut firmware_downloaded = false;
        let mut status = InitStatus::Initialized;
        let early_efuse_info = match chip.family {
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                Some(self.read_efuse_info_async(chip).await?)
            }
            ChipFamily::Rtl8814 | ChipFamily::Rtl8822c => None,
        };

        let fw_state = self.read_u32_async(REG_MCUFWDL).await.unwrap_or(0);
        let fw_already_running = match chip.family {
            ChipFamily::Rtl8814 => (fw_state & 0xff) == 0x78 || (fw_state & BIT15) != 0,
            ChipFamily::Rtl8822c => (fw_state & 0xffff) == 0xc078,
            _ => (fw_state & WINTINI_RDY) != 0,
        };

        if chip.family == ChipFamily::Rtl8822c {
            return self
                .initialize_monitor_jaguar3_async(chip, radio, options, fw_already_running)
                .await;
        }

        if fw_already_running {
            status = InitStatus::AlreadyRunning;
        }

        let should_run_boot_path =
            !fw_already_running || matches!(chip.family, ChipFamily::Rtl8812 | ChipFamily::Rtl8821);
        if should_run_boot_path {
            match chip.family {
                ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                    self.power_on_jaguar_async(chip).await?;
                    self.init_llt_table_async(chip).await?;
                    self.init_hardware_drop_incorrect_bulk_out_async().await?;
                    self.download_firmware_8812_family_async(chip).await?;
                    firmware_downloaded = true;
                }
                ChipFamily::Rtl8814 => {
                    self.download_firmware_8814_with_options_async(
                        options.firmware_8814_mode,
                        options.firmware_8814_chunk,
                    )
                    .await?;
                    firmware_downloaded = true;
                }
                ChipFamily::Rtl8822c => unreachable!("Jaguar3 is handled before generic init"),
            }
        }

        let efuse_info = if let Some(efuse_info) = early_efuse_info {
            efuse_info
        } else {
            self.read_efuse_info_async(chip).await?
        };

        self.load_mac_tables_async(chip, efuse_info).await?;
        self.init_queue_fifo_async(chip).await?;
        self.init_mac_rx_async(chip).await?;
        self.enable_bb_rf_domain_8814_async(chip).await?;
        self.load_phy_tables_async(chip, efuse_info).await?;
        self.load_rf_tables_async(chip, efuse_info).await?;
        self.configure_single_tx_path_async(chip).await?;
        self.finalize_mac_rx_async(chip, efuse_info).await?;
        self.enable_rx_bar_async().await?;
        self.set_channel_with_options_async(chip, radio, efuse_info, options.skip_tx_power)
            .await?;
        if chip.family == ChipFamily::Rtl8812 {
            let mut power_tracking = PowerTrackingState::default();
            self.init_power_tracking_8812_async(&mut power_tracking)
                .await?;
            self.clear_power_tracking_8812_async(&mut power_tracking)
                .await?;
            let _ = self
                .tick_power_tracking_8812_async(
                    &mut power_tracking,
                    radio.channel,
                    radio.channel_width,
                )
                .await?;
        }
        if options.should_run_iqk(chip.family) {
            let _ = self.run_iqk_async(chip, radio.channel).await?;
        }
        self.set_monitor_mode_async(options.accept_bad_fcs).await?;

        Ok(InitReport {
            chip,
            status,
            firmware_downloaded,
        })
    }

    /// Best-effort monitor-mode shutdown.
    ///
    /// This mirrors devourer's explicit Jaguar3 `Stop()` path: halt TRX, close
    /// the receive filter, and run the card-disable power sequence so the USB
    /// adapter can re-enumerate cleanly after sustained monitor/TX use. Older
    /// Jaguar1-family chips do not currently need extra shutdown writes here.
    pub async fn shutdown_monitor_async(&self) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        self.shutdown_monitor_for_chip_async(chip).await
    }

    pub(crate) async fn shutdown_monitor_for_chip_async(
        &self,
        chip: ChipInfo,
    ) -> Result<(), DriverError> {
        match chip.family {
            ChipFamily::Rtl8822c => self.shutdown_monitor_jaguar3_async().await,
            ChipFamily::Rtl8812 | ChipFamily::Rtl8814 | ChipFamily::Rtl8821 => Ok(()),
        }
    }

    /// Read one USB bulk-IN transfer from the receive endpoint.
    #[cfg(target_arch = "wasm32")]
    pub async fn read_rx_transfer_async(&self, length: usize) -> Result<Vec<u8>, DriverError> {
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, In>(self.bulk_in_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk IN endpoint failed: {err}")))?;
        endpoint
            .clear_halt()
            .await
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk IN failed: {err}")))?;
        let buffer = endpoint.allocate(length);
        endpoint.submit(buffer);
        let completion = endpoint.next_complete().await;
        completion
            .status
            .map_err(|err| DriverError::Nusb(format!("bulk IN transfer failed: {err}")))?;
        Ok(completion.buffer[..completion.actual_len].to_vec())
    }

    /// Read a small batch of USB bulk-IN transfers from the receive endpoint.
    #[cfg(target_arch = "wasm32")]
    pub async fn read_rx_transfers_async(
        &self,
        length: usize,
        in_flight: usize,
    ) -> Result<Vec<Vec<u8>>, DriverError> {
        let count = in_flight.clamp(1, 16);
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, In>(self.bulk_in_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk IN endpoint failed: {err}")))?;
        endpoint
            .clear_halt()
            .await
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk IN failed: {err}")))?;
        for _ in 0..count {
            let buffer = endpoint.allocate(length);
            endpoint.submit(buffer);
        }
        let mut transfers = Vec::with_capacity(count);
        for _ in 0..count {
            let completion = endpoint.next_complete().await;
            completion
                .status
                .map_err(|err| DriverError::Nusb(format!("bulk IN transfer failed: {err}")))?;
            transfers.push(completion.buffer[..completion.actual_len].to_vec());
        }
        Ok(transfers)
    }

    /// Read one USB bulk-IN transfer from the receive endpoint.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn read_rx_transfer_async(&self, length: usize) -> Result<Vec<u8>, DriverError> {
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, In>(self.bulk_in_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk IN endpoint failed: {err}")))?;
        endpoint
            .clear_halt()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk IN failed: {err}")))?;
        let completion = endpoint.transfer_blocking(endpoint.allocate(length), USB_TIMEOUT);
        completion
            .status
            .map_err(|err| DriverError::Nusb(format!("bulk IN transfer failed: {err}")))?;
        Ok(completion.buffer[..completion.actual_len].to_vec())
    }

    /// Read a small batch of USB bulk-IN transfers from the receive endpoint.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn read_rx_transfers_async(
        &self,
        length: usize,
        in_flight: usize,
    ) -> Result<Vec<Vec<u8>>, DriverError> {
        let count = in_flight.clamp(1, 16);
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, In>(self.bulk_in_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk IN endpoint failed: {err}")))?;
        endpoint
            .clear_halt()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk IN failed: {err}")))?;
        for _ in 0..count {
            let buffer = endpoint.allocate(length);
            endpoint.submit(buffer);
        }
        let mut transfers = Vec::with_capacity(count);
        for _ in 0..count {
            let Some(completion) = endpoint.wait_next_complete(USB_TIMEOUT) else {
                endpoint.cancel_all();
                return Err(DriverError::Nusb(
                    "bulk IN transfer timed out while reading in-flight batch".to_owned(),
                ));
            };
            completion
                .status
                .map_err(|err| DriverError::Nusb(format!("bulk IN transfer failed: {err}")))?;
            transfers.push(completion.buffer[..completion.actual_len].to_vec());
        }
        Ok(transfers)
    }

    /// Write one USB bulk-OUT transfer to the transmit endpoint.
    #[cfg(target_arch = "wasm32")]
    pub async fn write_tx_transfer_async(&self, transfer: &[u8]) -> Result<usize, DriverError> {
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, Out>(self.bulk_out_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk OUT endpoint failed: {err}")))?;
        endpoint
            .clear_halt()
            .await
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk OUT failed: {err}")))?;
        endpoint.submit(Buffer::from(transfer));
        let completion = endpoint.next_complete().await;
        completion
            .status
            .map_err(|err| DriverError::Nusb(format!("bulk OUT transfer failed: {err}")))?;
        Ok(completion.actual_len)
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn write_tx_transfer_raw_async(
        &self,
        transfer: &[u8],
    ) -> Result<usize, DriverError> {
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, Out>(self.bulk_out_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk OUT endpoint failed: {err}")))?;
        endpoint.submit(Buffer::from(transfer));
        let completion = endpoint.next_complete().await;
        completion
            .status
            .map_err(|err| DriverError::Nusb(format!("raw bulk OUT transfer failed: {err}")))?;
        Ok(completion.actual_len)
    }

    /// Write one USB bulk-OUT transfer to the transmit endpoint.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn write_tx_transfer_async(&self, transfer: &[u8]) -> Result<usize, DriverError> {
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, Out>(self.bulk_out_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk OUT endpoint failed: {err}")))?;
        endpoint
            .clear_halt()
            .wait()
            .map_err(|err| DriverError::Nusb(format!("clear halt on bulk OUT failed: {err}")))?;
        let completion = endpoint.transfer_blocking(Buffer::from(transfer), USB_TIMEOUT);
        completion
            .status
            .map_err(|err| DriverError::Nusb(format!("bulk OUT transfer failed: {err}")))?;
        Ok(completion.actual_len)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn write_tx_transfer_raw_async(
        &self,
        transfer: &[u8],
    ) -> Result<usize, DriverError> {
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, Out>(self.bulk_out_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk OUT endpoint failed: {err}")))?;
        let completion = endpoint.transfer_blocking(Buffer::from(transfer), USB_FIRMWARE_TIMEOUT);
        completion
            .status
            .map_err(|err| DriverError::Nusb(format!("raw bulk OUT transfer failed: {err}")))?;
        Ok(completion.actual_len)
    }

    /// Convert a radiotap + 802.11 packet into a Realtek TX frame and transmit it.
    pub async fn send_packet_async(
        &self,
        radiotap_packet: &[u8],
        options: RealtekTxOptions,
    ) -> Result<usize, DriverError> {
        let usb_frame =
            build_usb_tx_frame(radiotap_packet, options).map_err(DriverError::TxBuild)?;
        self.write_tx_transfer_async(&usb_frame).await
    }

    /// Read raw bytes from a Realtek vendor register.
    #[cfg(target_arch = "wasm32")]
    pub async fn read_register_async(
        &self,
        register: u16,
        len: u16,
    ) -> Result<Vec<u8>, DriverError> {
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
            .await
            .map_err(|err| DriverError::Nusb(format!("vendor read 0x{register:04x} failed: {err}")))
    }

    /// Read raw bytes from a Realtek vendor register.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn read_register_async(
        &self,
        register: u16,
        len: u16,
    ) -> Result<Vec<u8>, DriverError> {
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

    /// Write raw bytes to a Realtek vendor register.
    #[cfg(target_arch = "wasm32")]
    pub async fn write_register_async(
        &self,
        register: u16,
        bytes: &[u8],
    ) -> Result<(), DriverError> {
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
            .await
            .map_err(|err| {
                DriverError::Nusb(format!("vendor write 0x{register:04x} failed: {err}"))
            })
    }

    /// Write raw bytes to a Realtek vendor register.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn write_register_async(
        &self,
        register: u16,
        bytes: &[u8],
    ) -> Result<(), DriverError> {
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

    /// Read an 8-bit little-endian Realtek register value.
    pub async fn read_u8_async(&self, register: u16) -> Result<u8, DriverError> {
        let bytes = self.read_register_async(register, 1).await?;
        bytes.first().copied().ok_or(DriverError::RegisterReadSize {
            expected: 1,
            actual: bytes.len(),
        })
    }

    /// Read a 16-bit little-endian Realtek register value.
    pub async fn read_u16_async(&self, register: u16) -> Result<u16, DriverError> {
        let bytes = self.read_register_async(register, 2).await?;
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

    /// Read a 32-bit little-endian Realtek register value.
    pub async fn read_u32_async(&self, register: u16) -> Result<u32, DriverError> {
        let bytes = self.read_register_async(register, 4).await?;
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

    /// Write an 8-bit Realtek register value.
    pub async fn write_u8_async(&self, register: u16, value: u8) -> Result<(), DriverError> {
        self.write_register_async(register, &[value]).await
    }

    /// Write a 16-bit little-endian Realtek register value.
    pub async fn write_u16_async(&self, register: u16, value: u16) -> Result<(), DriverError> {
        self.write_register_async(register, &value.to_le_bytes())
            .await
    }

    /// Write a 32-bit little-endian Realtek register value.
    pub async fn write_u32_async(&self, register: u16, value: u32) -> Result<(), DriverError> {
        self.write_register_async(register, &value.to_le_bytes())
            .await
    }
}

#[cfg(target_arch = "wasm32")]
fn web_usb_target_matches(vendor_id: u16, product_id: u16, options: DriverOptions) -> bool {
    matches!(
        (options.target_vendor_id, options.target_product_id),
        (Some(vid), Some(pid)) if vendor_id == vid && product_id == pid
    )
}
