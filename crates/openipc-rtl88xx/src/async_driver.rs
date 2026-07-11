//! Async-shaped Realtek driver entry points.
//!
//! On wasm these methods are backed by WebUSB promises. On native targets they
//! wrap blocking `nusb` operations so the same HAL sequences can be shared
//! across targets; call them from a worker or other blocking context.

use nusb::transfer::{Buffer, Bulk, In, Out, TransferError};
#[cfg(target_arch = "wasm32")]
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient};
#[cfg(not(target_arch = "wasm32"))]
use nusb::MaybeFuture;

#[cfg(target_arch = "wasm32")]
use crate::device::discover_bulk_endpoints_with_override;
use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::regs::*;
use crate::tx::{build_usb_tx_aggregate, build_usb_tx_frame, RealtekTxOptions};
#[cfg(target_arch = "wasm32")]
use crate::types::is_supported_id;
use crate::types::{
    ChannelWidth, ChipFamily, ChipInfo, DriverError, DriverOptions, InitReport, InitStatus,
    MonitorOptions, RadioConfig,
};
#[cfg(target_arch = "wasm32")]
use crate::usb_recovery::CONTROL_RETRY_ATTEMPTS;
use crate::usb_recovery::{
    retry_delay_ms, should_retry_transfer_error, transfer_error, BULK_RETRY_ATTEMPTS,
    FIRMWARE_BULK_RETRY_ATTEMPTS,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::usb_transport::{read_register_with_recovery, write_register_with_recovery};
use crate::PowerTrackingState;
#[cfg(target_arch = "wasm32")]
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
#[cfg(target_arch = "wasm32")]
use std::sync::OnceLock;

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
        Self::from_nusb_device_async(device, options).await
    }

    /// Build a browser driver from an exact device opened by `nusb` discovery.
    #[cfg(target_arch = "wasm32")]
    pub async fn from_nusb_device_async(
        device: nusb::Device,
        options: DriverOptions,
    ) -> Result<Self, DriverError> {
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
            detected_family: OnceLock::new(),
            efuse_logical_map: OnceLock::new(),
            efuse_info: OnceLock::new(),
            diagnostics: std::sync::Mutex::new(
                crate::diagnostics::DriverDiagnosticsState::default(),
            ),
            cck_filter_8821c: OnceLock::new(),
            jaguar2_kfree: OnceLock::new(),
            h2c_box: AtomicU8::new(0),
            cw_tone: std::sync::Mutex::new(crate::async_cw::CwToneState::default()),
            continuous_tx: std::sync::Mutex::new(
                crate::async_continuous_tx::ContinuousTxState::default(),
            ),
            retune_state: std::sync::Mutex::new(crate::retune_state::FastRetuneState::default()),
            rx_path_mask: std::sync::Mutex::new(None),
            tx_stats: std::sync::Mutex::new(crate::TxStats::default()),
            tx_power_control: std::sync::Mutex::new(crate::tx_control::TxPowerControl::default()),
            rx_quality: crate::RxQualityAccumulator::default(),
            rx_path_activity: crate::RxPathActivityAccumulator::default(),
            cfo_tracker: std::sync::Mutex::new(crate::CfoTracker::default()),
            cfo_tracking_enabled: std::sync::atomic::AtomicBool::new(false),
            crystal_cap: std::sync::atomic::AtomicU8::new(u8::MAX),
            crystal_cap_bases: std::sync::Mutex::new(None),
            beacon_interval_tu: std::sync::atomic::AtomicU16::new(0),
            beacon_mpdu: std::sync::Mutex::new(Vec::new()),
            beacon_tbtt_offset_us: std::sync::atomic::AtomicI64::new(0),
            cca_disabled: std::sync::Mutex::new(false),
            beamforming_peer: std::sync::Mutex::new(None),
            beamforming_report_ready: std::sync::atomic::AtomicBool::new(false),
            beamforming_apply_on: std::sync::atomic::AtomicBool::new(false),
            beamforming_report_count: std::sync::atomic::AtomicU32::new(0),
            firmware_boot_status: std::sync::Mutex::new(crate::FirmwareBootStatus::default()),
            tx_mode_default: std::sync::Mutex::new(None),
            tx_packet_power_step: std::sync::atomic::AtomicU8::new(0),
            ndpa_period: std::sync::atomic::AtomicU32::new(0),
            ndpa_counter: std::sync::atomic::AtomicU64::new(0),
            jaguar3_tx_rf_bw: std::sync::atomic::AtomicU8::new(u8::MAX),
            narrowband_adc: std::sync::atomic::AtomicU8::new(u8::MAX),
            narrowband_dac: std::sync::atomic::AtomicU8::new(u8::MAX),
            jaguar2_dig_enabled: std::sync::atomic::AtomicBool::new(true),
            jaguar2_thermal_tracking_enabled: std::sync::atomic::AtomicBool::new(true),
            tx_endpoint_prepared: std::sync::atomic::AtomicBool::new(false),
            tx_wedged: std::sync::atomic::AtomicBool::new(false),
            jaguar3_rx_wanted: std::sync::atomic::AtomicBool::new(true),
            ampdu_mode: std::sync::Mutex::new(crate::AmpduMode::disabled()),
            tx_reports_enabled: std::sync::atomic::AtomicBool::new(false),
            tx_report_tag: std::sync::atomic::AtomicU8::new(0),
            usb_tx_aggregate_max: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Probe the chip family and RF layout from the hardware IDs and SYS_CFG register.
    pub async fn probe_chip_async(&self) -> Result<ChipInfo, DriverError> {
        let sys_cfg = self.read_u32_async(REG_SYS_CFG).await?;
        let chip_id = self.read_u8_async(0x00fc).await.unwrap_or(0);
        let chip = ChipInfo::from_probe(self.vendor_id, self.product_id, sys_cfg, chip_id);
        let _ = self.detected_family.set(chip.family);
        self.record_probe_diagnostics(sys_cfg, chip_id, chip);
        Ok(chip)
    }

    /// Probe the adapter and return its static USB/radio capability report.
    pub async fn adapter_capabilities_async(
        &self,
    ) -> Result<crate::AdapterCapabilities, DriverError> {
        let mut capabilities = crate::AdapterCapabilities::for_chip(self.probe_chip_async().await?);
        if let Some(efuse) = self.efuse_info.get() {
            capabilities.crystal_cap_default = efuse.crystal_cap.min(capabilities.crystal_cap_max);
        }
        Ok(capabilities)
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
        self.begin_diagnostics(radio, options);
        let result = self
            .initialize_monitor_with_options_inner_async(radio, options)
            .await;
        if result.is_ok() {
            self.capture_post_init_registers().await;
        }
        match &result {
            Ok(_) => self.finish_diagnostics(Ok(())),
            Err(error) => self.finish_diagnostics(Err(error)),
        }
        result
    }

    async fn initialize_monitor_with_options_inner_async(
        &self,
        radio: RadioConfig,
        options: MonitorOptions,
    ) -> Result<InitReport, DriverError> {
        log::info!(
            target: "openipc_rtl88xx::init",
            "starting Realtek monitor initialization vid={:04x} pid={:04x} channel={} width={:?}",
            self.vendor_id,
            self.product_id,
            radio.channel,
            radio.channel_width
        );
        let chip = self
            .diagnostic_stage("probe", self.probe_chip_async())
            .await?;
        self.jaguar3_tx_rf_bw.store(
            options.jaguar3_tx_rf_bw.unwrap_or(u8::MAX),
            std::sync::atomic::Ordering::Release,
        );
        self.narrowband_adc.store(
            options.narrowband_adc.unwrap_or(u8::MAX),
            std::sync::atomic::Ordering::Release,
        );
        self.narrowband_dac.store(
            options
                .narrowband_dac
                .or(options.jaguar3_nb_dac)
                .unwrap_or(u8::MAX),
            std::sync::atomic::Ordering::Release,
        );
        self.jaguar2_dig_enabled
            .store(!options.skip_dig, std::sync::atomic::Ordering::Release);
        self.jaguar2_thermal_tracking_enabled.store(
            options.thermal_tracking,
            std::sync::atomic::Ordering::Release,
        );
        self.cfo_tracking_enabled
            .store(options.cfo_tracking, std::sync::atomic::Ordering::Release);
        self.jaguar3_rx_wanted.store(
            options.jaguar3_enable_rx,
            std::sync::atomic::Ordering::Release,
        );
        self.set_ndpa_period(options.ndpa_period);
        if let Some(index) = options.tx_power_index {
            self.tx_power_control
                .lock()
                .map_err(|_| DriverError::DriverStatePoisoned)?
                .flat_index = Some(index);
        }
        self.prepare_firmware_boot_status(false)?;
        log::debug!(target: "openipc_rtl88xx::init", "probed Realtek adapter: {chip:?}");
        if chip.family.is_jaguar2() {
            self.prepare_firmware_boot_status(true)?;
            let report = self
                .initialize_monitor_jaguar2_async(chip, radio, options)
                .await?;
            self.diagnostic_stage(
                "interference_mitigation",
                self.apply_interference_mitigation_async(chip, radio, options),
            )
            .await?;
            self.note_full_tune(radio)?;
            self.note_firmware_boot_status(report.firmware_downloaded, true)?;
            self.diagnostic_stage(
                "post_init_options",
                self.apply_post_init_options_async(chip, options),
            )
            .await?;
            return Ok(report);
        }
        let mut firmware_downloaded = false;
        let mut status = InitStatus::Initialized;
        let early_efuse_info = match chip.family {
            ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => Some(
                self.diagnostic_stage("efuse", self.read_efuse_info_async(chip))
                    .await?,
            ),
            ChipFamily::Rtl8814
            | ChipFamily::Rtl8822b
            | ChipFamily::Rtl8821c
            | ChipFamily::Rtl8822c
            | ChipFamily::Rtl8822e => None,
        };

        let fw_state = self.read_u32_async(REG_MCUFWDL).await.unwrap_or(0);
        let fw_already_running = match chip.family {
            ChipFamily::Rtl8814 => (fw_state & 0xff) == 0x78 || (fw_state & BIT15) != 0,
            ChipFamily::Rtl8822b | ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                (fw_state & 0xffff) == 0xc078
            }
            _ => (fw_state & WINTINI_RDY) != 0,
        };
        log::debug!(
            target: "openipc_rtl88xx::firmware",
            "read firmware state register=0x{fw_state:08x} running={fw_already_running}"
        );

        if chip.family.is_jaguar3() {
            self.prepare_firmware_boot_status(!fw_already_running)?;
            let report = self
                .initialize_monitor_jaguar3_async(chip, radio, options, fw_already_running)
                .await?;
            if options.disable_cca {
                self.set_cca_disabled_async(true).await?;
            }
            self.diagnostic_stage(
                "interference_mitigation",
                self.apply_interference_mitigation_async(chip, radio, options),
            )
            .await?;
            self.note_full_tune(radio)?;
            self.note_firmware_boot_status(report.firmware_downloaded, true)?;
            self.diagnostic_stage(
                "post_init_options",
                self.apply_post_init_options_async(chip, options),
            )
            .await?;
            return Ok(report);
        }

        if fw_already_running {
            status = InitStatus::AlreadyRunning;
        }

        let should_run_boot_path =
            !fw_already_running || matches!(chip.family, ChipFamily::Rtl8812 | ChipFamily::Rtl8821);
        if should_run_boot_path {
            self.prepare_firmware_boot_status(true)?;
            match chip.family {
                ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                    self.diagnostic_stage("power_on", self.power_on_jaguar_async(chip))
                        .await?;
                    self.diagnostic_stage("llt", self.init_llt_table_async(chip))
                        .await?;
                    self.diagnostic_stage(
                        "bulk_out_hardware",
                        self.init_hardware_drop_incorrect_bulk_out_async(),
                    )
                    .await?;
                    self.diagnostic_stage(
                        "firmware_download",
                        self.download_firmware_8812_family_async(chip),
                    )
                    .await?;
                    firmware_downloaded = true;
                }
                ChipFamily::Rtl8814 => {
                    self.diagnostic_stage(
                        "firmware_download",
                        self.download_firmware_8814_with_options_async(
                            options.firmware_8814_mode,
                            options.firmware_8814_chunk,
                        ),
                    )
                    .await?;
                    firmware_downloaded = true;
                }
                ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                    unreachable!("Jaguar3 is handled before generic init")
                }
                ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => {
                    unreachable!("Jaguar2 is handled before generic init")
                }
            }
        }

        let efuse_info = if let Some(efuse_info) = early_efuse_info {
            efuse_info
        } else {
            self.diagnostic_stage("efuse", self.read_efuse_info_async(chip))
                .await?
        };
        let _ = self.efuse_info.set(efuse_info);

        self.diagnostic_stage("mac_tables", self.load_mac_tables_async(chip, efuse_info))
            .await?;
        self.diagnostic_stage("queue_fifo", self.init_queue_fifo_async(chip))
            .await?;
        self.diagnostic_stage("mac_rx", self.init_mac_rx_async(chip))
            .await?;
        self.diagnostic_stage("bb_rf_domain", self.enable_bb_rf_domain_async(chip))
            .await?;
        self.diagnostic_stage("phy_tables", self.load_phy_tables_async(chip, efuse_info))
            .await?;
        self.diagnostic_stage("rf_tables", self.load_rf_tables_async(chip, efuse_info))
            .await?;
        self.diagnostic_stage("igi_floor", self.set_igi_floor_jaguar1_async(chip))
            .await?;
        let mut power_tracking = if chip.family == ChipFamily::Rtl8812 {
            let mut state = PowerTrackingState::default();
            self.diagnostic_stage(
                "power_tracking_init",
                self.init_power_tracking_8812_async(&mut state),
            )
            .await?;
            Some(state)
        } else {
            None
        };
        self.diagnostic_stage("tx_path", self.configure_single_tx_path_async(chip))
            .await?;
        self.diagnostic_stage(
            "channel_and_tx_power",
            self.set_channel_with_options_async(chip, radio, efuse_info, options.skip_tx_power),
        )
        .await?;
        if let Some(state) = power_tracking.as_mut() {
            let _ = self
                .diagnostic_stage(
                    "power_tracking_tick",
                    self.tick_power_tracking_8812_async(state, radio.channel, radio.channel_width),
                )
                .await?;
        }
        if options.should_run_iqk(chip.family) {
            let _ = self
                .diagnostic_stage("iqk", self.run_iqk_async(chip, radio.channel))
                .await?;
        }
        // Match Devourer's post-channel MAC tail so tuning and IQK cannot
        // overwrite the final queue, RX-filter, NAV, and RTL8814 trace state.
        self.diagnostic_stage(
            "mac_rx_finalize",
            self.finalize_mac_rx_async(chip, efuse_info),
        )
        .await?;
        self.diagnostic_stage("rx_bar", self.enable_rx_bar_async())
            .await?;
        if options.disable_cca && chip.family.is_jaguar3() {
            self.set_cca_disabled_async(true).await?;
        }
        if let Some(mask) = options.rx_path_mask {
            self.diagnostic_stage(
                "rx_path_mask",
                self.set_rx_path_mask_for_chip_async(chip, mask),
            )
            .await?;
            *self
                .rx_path_mask
                .lock()
                .map_err(|_| DriverError::DriverStatePoisoned)? = Some(mask);
        }
        self.diagnostic_stage(
            "monitor_filters",
            self.set_monitor_mode_async(options.accept_bad_fcs),
        )
        .await?;
        self.diagnostic_stage(
            "interference_mitigation",
            self.apply_interference_mitigation_async(chip, radio, options),
        )
        .await?;
        if let Some(gain) = options.cw_tone_gain {
            if options.beamforming_sounder || options.beamforming_sounder_mac.is_some() {
                self.arm_beamforming_sounder_async(options.beamforming_sounder_mac)
                    .await?;
            }
            self.diagnostic_stage("cw_tone", self.start_cw_tone_async(radio.channel, gain))
                .await?;
        }

        let report = InitReport {
            chip,
            status,
            firmware_downloaded,
        };
        log::info!(
            target: "openipc_rtl88xx::init",
            "Realtek monitor initialization complete chip={:?} status={:?} firmware_downloaded={}",
            report.chip.family,
            report.status,
            report.firmware_downloaded
        );
        self.note_full_tune(radio)?;
        self.note_firmware_boot_status(report.firmware_downloaded, true)?;
        self.diagnostic_stage(
            "post_init_options",
            self.apply_post_init_options_async(chip, options),
        )
        .await?;
        Ok(report)
    }

    async fn apply_post_init_options_async(
        &self,
        chip: ChipInfo,
        options: MonitorOptions,
    ) -> Result<(), DriverError> {
        if options.cw_tone_gain.is_some() {
            return Ok(());
        }
        self.set_tx_reports(options.tx_reports);
        if options.usb_tx_aggregate_max != 0 {
            self.set_usb_tx_aggregation_async(options.usb_tx_aggregate_max)
                .await?;
        }
        if let Some(mac) = options.ack_responder {
            self.set_ack_responder_async(mac).await?;
        }
        if let Some(mode) = options.ampdu_mode {
            self.set_ampdu_mode_async(mode).await?;
        }
        if options.disable_cca && !chip.family.is_jaguar1() {
            self.set_cca_disabled_async(true).await?;
        }
        if options.cfo_tracking || options.crystal_cap.is_some() {
            self.prepare_crystal_cap_async(chip).await?;
        }
        if let Some(cap) = options.crystal_cap {
            self.set_crystal_cap_async(Some(cap)).await?;
        }
        if options.beamforming_sounder || options.beamforming_sounder_mac.is_some() {
            self.arm_beamforming_sounder_async(options.beamforming_sounder_mac)
                .await?;
        }
        if let Some(peer) = options.beamformee_of {
            self.arm_beamformee_async(
                peer,
                None,
                if options.beamformee_mu {
                    crate::BeamformingFeedback::Mu
                } else {
                    crate::BeamformingFeedback::Su
                },
            )
            .await?;
        }
        if let Some(peer) = options.transmit_beamforming_peer {
            if chip.family.is_jaguar3() {
                self.arm_transmit_beamforming_async(peer, options.beamforming_sounder_mac)
                    .await?;
            }
        }
        Ok(())
    }

    /// Best-effort monitor-mode shutdown.
    ///
    /// This mirrors devourer's explicit Jaguar3 `Stop()` path: halt TRX, close
    /// the receive filter, and run the card-disable power sequence so the USB
    /// adapter can re-enumerate cleanly after sustained monitor/TX use. Older
    /// Jaguar1-family chips do not currently need extra shutdown writes here.
    pub async fn shutdown_monitor_async(&self) -> Result<(), DriverError> {
        self.stop_cw_tone_async().await?;
        let chip = self.probe_chip_async().await?;
        self.shutdown_monitor_for_chip_async(chip).await
    }

    /// Retune an initialized monitor-mode adapter without reloading firmware.
    ///
    /// This is intended for idle channel surveys and deliberate channel
    /// changes. Callers must stop normal RX/TX processing while retuning.
    pub async fn retune_async(&self, radio: RadioConfig) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family.is_jaguar3() {
            self.set_channel_bwmode_8822c_async(
                chip,
                radio.channel,
                radio.channel_offset,
                radio.channel_width,
            )
            .await?;
            self.set_default_tx_power_jaguar3_async(chip, radio.channel)
                .await?;
        } else if chip.family.is_jaguar2() {
            let efuse = if let Some(efuse) = self.efuse_info.get().copied() {
                efuse
            } else {
                let efuse = self.read_efuse_info_async(chip).await?;
                let _ = self.efuse_info.set(efuse);
                efuse
            };
            if chip.family == ChipFamily::Rtl8821c {
                self.set_channel_bw_8821c_async(chip, radio, efuse.rfe_type)
                    .await?;
                self.apply_tx_power_8821c_async(radio).await?;
            } else {
                self.set_channel_bw_8822b_async(chip, radio, efuse.rfe_type)
                    .await?;
                self.apply_tx_power_8822b_async(chip, radio, efuse.rfe_type)
                    .await?;
            }
        } else {
            let efuse = if let Some(efuse) = self.efuse_info.get().copied() {
                efuse
            } else {
                let efuse = self.read_efuse_info_async(chip).await?;
                let _ = self.efuse_info.set(efuse);
                efuse
            };
            self.set_channel_with_options_async(chip, radio, efuse, false)
                .await?;
        }
        if chip.family.is_jaguar3() {
            let disabled = *self
                .cca_disabled
                .lock()
                .map_err(|_| DriverError::DriverStatePoisoned)?;
            if disabled {
                self.apply_cca_disabled_async(true).await?;
            }
        }
        if !chip.family.uses_halmac_descriptor() {
            let mask = *self
                .rx_path_mask
                .lock()
                .map_err(|_| DriverError::DriverStatePoisoned)?;
            if let Some(mask) = mask {
                self.write_u8_async(0x0808, mask).await?;
            }
        }
        self.note_full_tune(radio)
    }

    /// Return the radio configuration tracked after initialization or retuning.
    pub fn current_radio_config(&self) -> Result<Option<RadioConfig>, DriverError> {
        Ok(self
            .retune_state
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .radio)
    }

    /// Lean same-band retune using the current width and primary-channel offset.
    ///
    /// Band changes automatically use the full retune path. `cache_rf` controls
    /// whether supported chips reuse their RF18 snapshots between hops.
    pub async fn fast_retune_async(
        &self,
        channel: u8,
        cache_rf: bool,
    ) -> Result<crate::types::RetuneReport, DriverError> {
        let current = self
            .current_radio_config()?
            .ok_or(DriverError::RadioNotInitialized)?;
        let next = RadioConfig { channel, ..current };
        if channel == current.channel {
            return Ok(crate::types::RetuneReport {
                radio: current,
                used_fast_path: true,
            });
        }
        let chip = self.probe_chip_async().await?;
        let same_band = (current.channel <= 14) == (channel <= 14);
        let used_fast_path = if !same_band {
            false
        } else {
            match chip.family {
                ChipFamily::Rtl8812 | ChipFamily::Rtl8821 => {
                    self.fast_retune_jaguar1_async(chip, current, channel, cache_rf)
                        .await?
                }
                ChipFamily::Rtl8814 => false,
                ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => {
                    self.fast_retune_jaguar2_async(chip, current, channel, cache_rf)
                        .await?
                }
                ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                    self.fast_retune_jaguar3_async(chip, current, channel, cache_rf)
                        .await?
                }
            }
        };
        if used_fast_path {
            self.retune_state
                .lock()
                .map_err(|_| DriverError::DriverStatePoisoned)?
                .radio = Some(next);
        } else {
            self.retune_async(next).await?;
        }
        Ok(crate::types::RetuneReport {
            radio: next,
            used_fast_path,
        })
    }

    /// Switch bandwidth at the current channel, using the lean 20/10/5 MHz path.
    pub async fn fast_set_bandwidth_async(
        &self,
        width: ChannelWidth,
    ) -> Result<crate::types::RetuneReport, DriverError> {
        let current = self
            .current_radio_config()?
            .ok_or(DriverError::RadioNotInitialized)?;
        let next = RadioConfig {
            channel_width: width,
            ..current
        };
        if width == current.channel_width {
            return Ok(crate::types::RetuneReport {
                radio: current,
                used_fast_path: true,
            });
        }
        let chip = self.probe_chip_async().await?;
        if !crate::AdapterCapabilities::for_chip(chip).supports_width(width) {
            return Err(DriverError::UnsupportedChannelWidth {
                family: chip.family,
                width,
            });
        }
        let is_fast_width = |candidate: ChannelWidth| {
            matches!(
                candidate,
                ChannelWidth::Mhz20 | ChannelWidth::Mhz10 | ChannelWidth::Mhz5
            )
        };
        let used_fast_path = if is_fast_width(current.channel_width) && is_fast_width(width) {
            match chip.family {
                ChipFamily::Rtl8812 | ChipFamily::Rtl8814 => {
                    self.fast_set_bandwidth_jaguar1_async(chip, current, width)
                        .await?
                }
                ChipFamily::Rtl8821 => false,
                ChipFamily::Rtl8822b => {
                    self.fast_set_bandwidth_jaguar2_async(chip, current, width)
                        .await?
                }
                ChipFamily::Rtl8821c => false,
                ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                    self.set_bandwidth_dividers_8822c_async(chip, width, current.channel)
                        .await?;
                    true
                }
            }
        } else {
            false
        };
        if used_fast_path {
            self.retune_state
                .lock()
                .map_err(|_| DriverError::DriverStatePoisoned)?
                .radio = Some(next);
        } else {
            self.retune_async(next).await?;
        }
        Ok(crate::types::RetuneReport {
            radio: next,
            used_fast_path,
        })
    }

    async fn fast_set_bandwidth_jaguar1_async(
        &self,
        chip: ChipInfo,
        current: RadioConfig,
        width: ChannelWidth,
    ) -> Result<bool, DriverError> {
        let mut state = self
            .retune_state
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .jaguar1
            .clone();
        if state.bandwidth_20_8ac.is_none() {
            if current.channel_width != ChannelWidth::Mhz20 {
                return Ok(false);
            }
            state.bandwidth_20_8ac = Some(self.read_u32_async(0x08ac).await?);
        }
        let base = state.bandwidth_20_8ac.expect("20 MHz cache was primed");
        if width == ChannelWidth::Mhz20 {
            self.write_u32_async(0x08ac, base).await?;
        } else {
            let is_5mhz = width == ChannelWidth::Mhz5;
            let (mask, mut adc, mut dac) = if chip.family == ChipFamily::Rtl8814 {
                (
                    0x1031_03c3,
                    if is_5mhz { 2 } else { 3 },
                    if is_5mhz { 2 } else { 3 },
                )
            } else {
                (
                    0x0030_03c3,
                    if is_5mhz { 0 } else { 1 },
                    if is_5mhz { 1 } else { 2 },
                )
            };
            let adc_override = self.narrowband_adc.load(Ordering::Acquire);
            let dac_override = self.narrowband_dac.load(Ordering::Acquire);
            if adc_override != u8::MAX {
                adc = u32::from(adc_override & 0x03);
            }
            if dac_override != u8::MAX {
                dac = u32::from(dac_override & 0x03);
            }
            let fields = (dac << 20) | (adc << 8) | ((if is_5mhz { 1 } else { 2 }) << 6);
            self.write_u32_async(0x08ac, (base & !mask) | (fields & mask))
                .await?;
        }
        self.retune_state
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .jaguar1 = state;
        Ok(true)
    }

    async fn fast_set_bandwidth_jaguar2_async(
        &self,
        chip: ChipInfo,
        current: RadioConfig,
        width: ChannelWidth,
    ) -> Result<bool, DriverError> {
        let mut state = self
            .retune_state
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .jaguar2
            .clone();
        if state.bandwidth_20_8ac.is_none() {
            if current.channel_width != ChannelWidth::Mhz20 {
                return Ok(false);
            }
            state.bandwidth_20_8ac = Some(self.read_u32_async(0x08ac).await?);
            state.bandwidth_rf18 = Some(self.query_rf_reg_async(chip, RfPath::A, 0x18).await?);
            state.bandwidth_center = current.channel;
            state.bandwidth_is_2g = current.channel <= 14;
            state.bandwidth_two_paths = chip.total_rf_paths() >= 2;
        }
        let base = state.bandwidth_20_8ac.expect("20 MHz cache was primed");
        let rf18 = state.bandwidth_rf18.expect("RF18 cache was primed");
        if width == ChannelWidth::Mhz20 {
            self.write_u32_async(0x08ac, base).await?;
            self.set_bb_reg_async(0x08c4, BIT30, 1).await?;
            self.set_rf_reg_async(chip, RfPath::A, 0x18, 0x000f_ffff, rf18)
                .await?;
            if state.bandwidth_two_paths {
                self.set_rf_reg_async(chip, RfPath::B, 0x18, 0x000f_ffff, rf18)
                    .await?;
            }
        } else {
            let is_5mhz = width == ChannelWidth::Mhz5;
            let mut value = base & if is_5mhz { 0xefee_fe00 } else { 0xeffe_ff00 };
            value |= if is_5mhz { BIT6 } else { BIT7 };
            let adc = self.narrowband_adc.load(Ordering::Acquire);
            if adc != u8::MAX {
                value &= !((0x03 << 8) | BIT16);
                value |= u32::from(adc & 0x03) << 8;
                value |= u32::from(adc & 0x04 != 0) << 16;
            }
            let dac = self.narrowband_dac.load(Ordering::Acquire);
            if dac != u8::MAX {
                value &= !((0x03 << 20) | BIT28);
                value |= u32::from(dac & 0x03) << 20;
                value |= u32::from(dac & 0x04 != 0) << 28;
            }
            self.write_u32_async(0x08ac, value).await?;
            self.set_bb_reg_async(0x08c4, BIT30, 0).await?;
            self.set_bb_reg_async(0x08c8, BIT31, 1).await?;
            if state.bandwidth_is_2g {
                self.set_bb_reg_async(0x0808, BIT28, 1).await?;
                self.set_bb_reg_async(0x0454, BIT7, 0).await?;
                self.set_bb_reg_async(0x0a80, BIT18, 0).await?;
            } else {
                self.set_bb_reg_async(0x0a80, BIT18, 1).await?;
                self.set_bb_reg_async(0x0454, BIT7, 1).await?;
                self.set_bb_reg_async(0x0808, BIT28, 0).await?;
                self.set_bb_reg_async(0x0814, 0x0000_fc00, 34).await?;
            }
            let alternate = (rf18 & !0xff) | if state.bandwidth_center == 1 { 2 } else { 1 };
            self.set_rf_reg_async(chip, RfPath::A, 0x18, 0x000f_ffff, alternate)
                .await?;
            if state.bandwidth_two_paths {
                self.set_rf_reg_async(chip, RfPath::B, 0x18, 0x000f_ffff, alternate)
                    .await?;
            }
            self.set_rf_reg_async(chip, RfPath::A, 0x18, 0x000f_ffff, rf18)
                .await?;
            if state.bandwidth_two_paths {
                self.set_rf_reg_async(chip, RfPath::B, 0x18, 0x000f_ffff, rf18)
                    .await?;
            }
            self.set_rf_reg_async(chip, RfPath::A, 0xb8, BIT19, 0)
                .await?;
            self.set_rf_reg_async(chip, RfPath::A, 0xb8, BIT19, 1)
                .await?;
            self.bb_reset_jaguar2_async().await?;
        }
        self.retune_state
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .jaguar2 = state;
        Ok(true)
    }

    fn note_full_tune(&self, radio: RadioConfig) -> Result<(), DriverError> {
        self.retune_state
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .note_full_tune(radio);
        Ok(())
    }

    fn note_firmware_boot_status(&self, attempted: bool, ready: bool) -> Result<(), DriverError> {
        *self
            .firmware_boot_status
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)? = crate::FirmwareBootStatus {
            supported: true,
            attempted,
            checksum_ok: attempted && ready,
            ready: attempted && ready,
        };
        Ok(())
    }

    fn prepare_firmware_boot_status(&self, attempted: bool) -> Result<(), DriverError> {
        *self
            .firmware_boot_status
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)? = crate::FirmwareBootStatus {
            supported: true,
            attempted,
            checksum_ok: false,
            ready: false,
        };
        Ok(())
    }

    /// Return the retained outcome of the latest successful initialization.
    pub fn firmware_boot_status(&self) -> crate::FirmwareBootStatus {
        self.firmware_boot_status
            .lock()
            .map_or_else(|_| crate::FirmwareBootStatus::default(), |status| *status)
    }

    pub(crate) fn invalidate_jaguar3_fast_cache(&self) -> Result<(), DriverError> {
        self.retune_state
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .invalidate_jaguar3();
        Ok(())
    }

    /// Select the active Jaguar1 receive chains.
    ///
    /// This is primarily a diversity/combining diagnostic. Channel changes and
    /// IQK may restore register `0x808`, so call it after those operations.
    pub async fn set_rx_path_mask_async(&self, mask: u8) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        self.set_rx_path_mask_for_chip_async(chip, mask).await?;
        *self
            .rx_path_mask
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)? = Some(mask);
        Ok(())
    }

    /// Read the active Jaguar1 RX-chain mask from register `0x808`.
    pub async fn rx_path_mask_async(&self) -> Result<u8, DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family.uses_halmac_descriptor() {
            return Err(DriverError::UnsupportedRxPathMask(chip.family));
        }
        self.read_u8_async(0x0808).await
    }

    /// Disable or restore Jaguar2/3's safe MAC EDCCA transmit-deferral gate.
    pub async fn set_cca_disabled_async(&self, disabled: bool) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family.is_jaguar1() {
            return Err(DriverError::UnsupportedCcaControl(chip.family));
        }
        self.apply_cca_disabled_async(disabled).await?;
        *self
            .cca_disabled
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)? = disabled;
        Ok(())
    }

    pub(crate) async fn apply_cca_disabled_async(&self, disabled: bool) -> Result<(), DriverError> {
        let mut reg_520 = self.read_u32_async(0x0520).await?;
        let mut reg_524 = self.read_u32_async(0x0524).await?;
        if disabled {
            reg_520 |= 1 << 15;
            reg_524 &= !(1 << 11);
        } else {
            reg_520 &= !(1 << 15);
            reg_524 |= 1 << 11;
        }
        self.write_u32_async(0x0520, reg_520).await?;
        self.write_u32_async(0x0524, reg_524).await
    }

    /// Read the adapter's free-running 64-bit hardware TSF in microseconds.
    pub async fn read_hardware_tsf_async(&self) -> Result<u64, DriverError> {
        let mut high = self.read_u32_async(0x0564).await?;
        let mut low = self.read_u32_async(0x0560).await?;
        if self.read_u32_async(0x0564).await? != high {
            high = self.read_u32_async(0x0564).await?;
            low = self.read_u32_async(0x0560).await?;
        }
        Ok((u64::from(high) << 32) | u64::from(low))
    }

    /// Set the Jaguar2/3 free-running hardware TSF.
    pub async fn write_hardware_tsf_async(&self, tsf: u64) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family.is_jaguar1() {
            return Err(DriverError::UnsupportedTsfWrite(chip.family));
        }
        self.write_u32_async(0x0560, tsf as u32).await?;
        self.write_u32_async(0x0564, (tsf >> 32) as u32).await
    }

    /// Load and enable a hardware-timed beacon on Jaguar2/3.
    pub async fn start_beacon_async(
        &self,
        beacon: &[u8],
        interval_tu: u16,
    ) -> Result<(), DriverError> {
        let chip = self.probe_chip_async().await?;
        if chip.family.is_jaguar1() {
            return Err(DriverError::UnsupportedBeacon(chip.family));
        }
        let radiotap_len = if beacon.len() >= 8 && beacon[0] == 0 {
            usize::from(u16::from_le_bytes([beacon[2], beacon[3]]))
        } else {
            0
        };
        let radiotap_len = if (8..=beacon.len()).contains(&radiotap_len) {
            radiotap_len
        } else {
            0
        };
        let mpdu = &beacon[radiotap_len..];
        if mpdu.len() < 24 || mpdu[0] != 0x80 {
            return Err(DriverError::InvalidBeacon);
        }
        if chip.family.is_jaguar2() {
            self.download_beacon_page_jaguar2_async(chip.family, mpdu)
                .await?;
        } else {
            self.download_beacon_page_jaguar3_async(mpdu).await?;
        }

        let source = &mpdu[10..16];
        let bssid = &mpdu[16..22];
        self.write_u32_async(
            0x0610,
            u32::from_le_bytes(source[..4].try_into().expect("source slice is four bytes")),
        )
        .await?;
        self.write_u16_async(
            0x0614,
            u16::from_le_bytes(source[4..6].try_into().expect("source tail is two bytes")),
        )
        .await?;
        self.write_u32_async(
            0x0618,
            u32::from_le_bytes(bssid[..4].try_into().expect("BSSID slice is four bytes")),
        )
        .await?;
        self.write_u16_async(
            0x061c,
            u16::from_le_bytes(bssid[4..6].try_into().expect("BSSID tail is two bytes")),
        )
        .await?;
        let network_type = self.read_u8_async(REG_CR + 2).await.unwrap_or(0);
        self.write_u8_async(REG_CR + 2, (network_type & !0x03) | 0x03)
            .await?;
        let interval_tu = interval_tu.max(1);
        self.write_u16_async(0x0554, interval_tu).await?;
        self.write_u8_async(0x0550, BIT3 as u8 | BIT4 as u8).await?;
        let txq = self.read_u32_async(REG_FWHW_TXQ_CTRL).await.unwrap_or(0);
        self.write_u32_async(REG_FWHW_TXQ_CTRL, txq | BIT22).await?;
        if chip.family.is_jaguar3() {
            self.send_h2c_raw_8822c_async(0x690c_0100, 0).await?;
        }
        self.beacon_interval_tu
            .store(interval_tu, Ordering::Release);
        *self
            .beacon_mpdu
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)? = mpdu.to_vec();
        self.beacon_tbtt_offset_us.store(0, Ordering::Release);
        Ok(())
    }

    /// Shift a Jaguar2/3 hardware beacon by a whole-TU one-shot interval change.
    pub async fn adjust_beacon_timing_async(&self, microseconds: i32) -> Result<i32, DriverError> {
        let chip = self.probe_chip_async().await?;
        let nominal = self.beacon_interval_tu.load(Ordering::Acquire);
        if nominal == 0 {
            return Ok(0);
        }
        if chip.family.is_jaguar1() {
            return Err(DriverError::UnsupportedBeacon(chip.family));
        }
        let mut delta_tu = if microseconds >= 0 {
            (microseconds + 512) / 1024
        } else {
            (microseconds - 512) / 1024
        };
        if delta_tu == 0 {
            return Ok(0);
        }
        let one_shot = (i32::from(nominal) + delta_tu).max(1);
        delta_tu = one_shot - i32::from(nominal);
        let period_us = i64::from(nominal) * 1024;
        let mut position = self.beacon_grid_position_async(period_us).await?;
        if position > period_us - 20_000 {
            crate::time::sleep_micros((period_us - position + 5_000).max(0) as u32).await;
        }
        position = self.beacon_grid_position_async(period_us).await?;
        self.write_u16_async(0x0554, one_shot as u16).await?;
        let restore_delay = (period_us - position) + i64::from(one_shot) * 512;
        crate::time::sleep_micros(restore_delay.max(0) as u32).await;
        self.write_u16_async(0x0554, nominal).await?;
        let previous = self.beacon_tbtt_offset_us.load(Ordering::Acquire);
        self.beacon_tbtt_offset_us.store(
            (previous + i64::from(delta_tu) * 1024).rem_euclid(period_us),
            Ordering::Release,
        );
        if chip.family.is_jaguar2() {
            self.redownload_jaguar2_beacon_async(chip.family).await?;
        }
        Ok(delta_tu * 1024)
    }

    /// Shift a Jaguar2/3 hardware beacon at microsecond resolution.
    pub async fn adjust_beacon_timing_fine_async(
        &self,
        microseconds: i32,
    ) -> Result<i32, DriverError> {
        let chip = self.probe_chip_async().await?;
        if self.beacon_interval_tu.load(Ordering::Acquire) == 0 {
            return Ok(0);
        }
        if chip.family.is_jaguar1() {
            return Err(DriverError::UnsupportedBeacon(chip.family));
        }
        let tsf = self.read_hardware_tsf_async().await?;
        let shifted = if microseconds >= 0 {
            tsf.wrapping_sub(microseconds as u64)
        } else {
            tsf.wrapping_add(u64::from(microseconds.unsigned_abs()))
        };
        let control = self.read_u8_async(0x0550).await?;
        self.write_u8_async(0x0550, control & !(BIT3 as u8)).await?;
        self.write_u32_async(0x0560, shifted as u32).await?;
        self.write_u32_async(0x0564, (shifted >> 32) as u32).await?;
        self.write_u8_async(0x0550, control | BIT3 as u8).await?;
        self.beacon_tbtt_offset_us.store(0, Ordering::Release);
        if chip.family.is_jaguar2() {
            self.redownload_jaguar2_beacon_async(chip.family).await?;
        }
        Ok(microseconds)
    }

    async fn beacon_grid_position_async(&self, period_us: i64) -> Result<i64, DriverError> {
        let tsf = self.read_hardware_tsf_async().await?;
        let position = (tsf % period_us as u64) as i64;
        let offset = self.beacon_tbtt_offset_us.load(Ordering::Acquire);
        Ok((position - offset).rem_euclid(period_us))
    }

    async fn redownload_jaguar2_beacon_async(&self, family: ChipFamily) -> Result<(), DriverError> {
        let mpdu = self
            .beacon_mpdu
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .clone();
        if mpdu.is_empty() {
            return Err(DriverError::InvalidBeacon);
        }
        self.download_beacon_page_jaguar2_async(family, &mpdu).await
    }

    /// Set both crystal load-capacitance legs to a raw trim code.
    ///
    /// `None` restores the EFUSE/default code. Call after hardware bring-up.
    pub async fn set_crystal_cap_async(&self, cap: Option<u8>) -> Result<u8, DriverError> {
        let family = match self.detected_family.get().copied() {
            Some(family) => family,
            None => self.probe_chip_async().await?.family,
        };
        let default = self
            .efuse_info
            .get()
            .map_or(0x20, |efuse| efuse.crystal_cap);
        let max = if family.is_jaguar3() { 0x7f } else { 0x3f };
        let cap = cap.unwrap_or(default).min(max);
        if self
            .crystal_cap_bases
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .is_none()
        {
            let chip = self.probe_chip_async().await?;
            self.prepare_crystal_cap_async(chip).await?;
        }
        let bases = self
            .crystal_cap_bases
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .ok_or(DriverError::RadioNotInitialized)?;
        match family {
            ChipFamily::Rtl8812 => {
                let data = u32::from(cap) | (u32::from(cap) << 6);
                self.write_u32_async(0x002c, bases[0] | (data << 19))
                    .await?;
            }
            ChipFamily::Rtl8814 => {
                let data = u32::from(cap) | (u32::from(cap) << 6);
                self.write_u32_async(0x002c, bases[0] | (data << 15))
                    .await?;
            }
            ChipFamily::Rtl8821 => {
                let data = u32::from(cap) | (u32::from(cap) << 6);
                self.write_u32_async(0x002c, bases[0] | (data << 12))
                    .await?;
            }
            ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => {
                self.write_u32_async(0x0024, bases[0] | (u32::from(cap) << 25))
                    .await?;
                self.write_u32_async(0x0028, bases[1] | (u32::from(cap) << 1))
                    .await?;
            }
            ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                let data = u32::from(cap) | (u32::from(cap) << 7);
                self.write_u32_async(0x1040, bases[0] | (data << 10))
                    .await?;
            }
        }
        self.crystal_cap.store(cap, Ordering::Release);
        Ok(cap)
    }

    /// Return the last explicitly applied crystal-cap code, when known.
    pub fn crystal_cap(&self) -> Option<u8> {
        let cap = self.crystal_cap.load(Ordering::Acquire);
        (cap != u8::MAX).then_some(cap)
    }

    /// Enable or disable accumulation for periodic closed-loop CFO tracking.
    pub fn set_cfo_tracking_enabled(&self, enabled: bool) {
        self.cfo_tracking_enabled.store(enabled, Ordering::Release);
    }

    /// Return whether periodic closed-loop CFO correction is enabled.
    pub fn cfo_tracking_enabled(&self) -> bool {
        self.cfo_tracking_enabled.load(Ordering::Acquire)
    }

    /// Return whether periodic Jaguar2 thermal compensation is enabled.
    pub fn jaguar2_thermal_tracking_enabled(&self) -> bool {
        self.jaguar2_thermal_tracking_enabled
            .load(Ordering::Acquire)
    }

    /// Drain CFO samples and apply at most one crystal-cap correction step.
    pub async fn cfo_tracking_tick_async(&self) -> Result<Option<crate::CfoStep>, DriverError> {
        if !self.cfo_tracking_enabled.load(Ordering::Acquire) {
            return Ok(None);
        }
        let family = match self.detected_family.get().copied() {
            Some(family) => family,
            None => self.probe_chip_async().await?.family,
        };
        let cap_max = if family.is_jaguar3() { 0x7f } else { 0x3f };
        let current = self
            .crystal_cap()
            .or_else(|| self.efuse_info.get().map(|efuse| efuse.crystal_cap))
            .unwrap_or(0x20)
            .min(cap_max);
        let step = self
            .cfo_tracker
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .step(current, cap_max);
        if let Some(next) = step.and_then(|result| result.crystal_cap) {
            self.set_crystal_cap_async(Some(next)).await?;
        }
        Ok(step)
    }

    async fn prepare_crystal_cap_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        if self
            .crystal_cap_bases
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)?
            .is_some()
        {
            return Ok(());
        }
        let bases = match chip.family {
            ChipFamily::Rtl8812 => [self.read_u32_async(0x002c).await? & !0x7ff8_0000, 0],
            ChipFamily::Rtl8814 => [self.read_u32_async(0x002c).await? & !0x07ff_8000, 0],
            ChipFamily::Rtl8821 => [self.read_u32_async(0x002c).await? & !0x00ff_f000, 0],
            ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => [
                self.read_u32_async(0x0024).await? & !0x7e00_0000,
                self.read_u32_async(0x0028).await? & !0x0000_007e,
            ],
            ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                [self.read_u32_async(0x1040).await? & !0x00ff_fc00, 0]
            }
        };
        *self
            .crystal_cap_bases
            .lock()
            .map_err(|_| DriverError::DriverStatePoisoned)? = Some(bases);
        Ok(())
    }

    async fn set_rx_path_mask_for_chip_async(
        &self,
        chip: ChipInfo,
        mask: u8,
    ) -> Result<(), DriverError> {
        if chip.family.uses_halmac_descriptor() {
            return Err(DriverError::UnsupportedRxPathMask(chip.family));
        }
        self.write_u8_async(0x0808, mask).await
    }

    pub(crate) async fn shutdown_monitor_for_chip_async(
        &self,
        chip: ChipInfo,
    ) -> Result<(), DriverError> {
        match chip.family {
            ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => {
                self.shutdown_monitor_jaguar2_async(chip.family).await
            }
            ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => {
                self.shutdown_monitor_jaguar3_async().await
            }
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
        for attempt in 0..BULK_RETRY_ATTEMPTS {
            let buffer = endpoint.allocate(length);
            endpoint.submit(buffer);
            let completion = endpoint.next_complete().await;
            match completion.status {
                Ok(()) => {
                    log::trace!(target: "openipc_rtl88xx::usb", "bulk IN complete endpoint=0x{:02x} bytes={}", self.bulk_in_ep, completion.actual_len);
                    return Ok(completion.buffer[..completion.actual_len].to_vec());
                }
                Err(err) if should_retry_transfer_error(err, attempt, BULK_RETRY_ATTEMPTS) => {
                    // Devourer treats any failed TX completion as a possibly
                    // wedged endpoint. WebUSB has no separate timeout knob,
                    // but it can recover the endpoint before retrying.
                    let _ = endpoint.clear_halt().await;
                    crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                }
                Err(err) => return Err(transfer_error("bulk IN transfer failed", err)),
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
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
        log::trace!(target: "openipc_rtl88xx::usb", "bulk IN batch complete endpoint=0x{:02x} transfers={} bytes={}", self.bulk_in_ep, transfers.len(), transfers.iter().map(Vec::len).sum::<usize>());
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
        for attempt in 0..BULK_RETRY_ATTEMPTS {
            endpoint.submit(endpoint.allocate(length));
            let completion = loop {
                if let Some(completion) =
                    endpoint.wait_next_complete(std::time::Duration::from_secs(60))
                {
                    break completion;
                }
            };
            match completion.status {
                Ok(()) => {
                    log::trace!(target: "openipc_rtl88xx::usb", "bulk IN complete endpoint=0x{:02x} bytes={}", self.bulk_in_ep, completion.actual_len);
                    return Ok(completion.buffer[..completion.actual_len].to_vec());
                }
                Err(err) if should_retry_transfer_error(err, attempt, BULK_RETRY_ATTEMPTS) => {
                    if err == TransferError::Stall {
                        let _ = endpoint.clear_halt().wait();
                    }
                    crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                }
                Err(err) => return Err(transfer_error("bulk IN transfer failed", err)),
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
    }

    /// Read a small batch of USB bulk-IN transfers from the receive endpoint.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn read_rx_transfers_async(
        &self,
        length: usize,
        in_flight: usize,
    ) -> Result<Vec<Vec<u8>>, DriverError> {
        let count = in_flight.clamp(1, 16);
        for attempt in 0..BULK_RETRY_ATTEMPTS {
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
            let mut retry = false;
            for _ in 0..count {
                let completion = loop {
                    if let Some(completion) =
                        endpoint.wait_next_complete(std::time::Duration::from_secs(60))
                    {
                        break completion;
                    }
                };
                match completion.status {
                    Ok(()) => transfers.push(completion.buffer[..completion.actual_len].to_vec()),
                    Err(err) if should_retry_transfer_error(err, attempt, BULK_RETRY_ATTEMPTS) => {
                        endpoint.cancel_all();
                        if err == TransferError::Stall {
                            let _ = endpoint.clear_halt().wait();
                        }
                        retry = true;
                        break;
                    }
                    Err(err) => return Err(transfer_error("bulk IN transfer failed", err)),
                }
            }
            if retry {
                crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                continue;
            }
            log::trace!(target: "openipc_rtl88xx::usb", "bulk IN batch complete endpoint=0x{:02x} transfers={} bytes={}", self.bulk_in_ep, transfers.len(), transfers.iter().map(Vec::len).sum::<usize>());
            return Ok(transfers);
        }
        unreachable!("retry loop either returns or reports the final USB error")
    }

    /// Write one USB bulk-OUT transfer to the transmit endpoint.
    #[cfg(target_arch = "wasm32")]
    pub async fn write_tx_transfer_async(&self, transfer: &[u8]) -> Result<usize, DriverError> {
        self.write_tx_transfer_terminated_async(transfer, false)
            .await
    }

    #[cfg(target_arch = "wasm32")]
    async fn write_tx_transfer_terminated_async(
        &self,
        transfer: &[u8],
        terminate: bool,
    ) -> Result<usize, DriverError> {
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, Out>(self.bulk_out_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk OUT endpoint failed: {err}")))?;
        if terminate
            && (!self
                .tx_endpoint_prepared
                .swap(true, std::sync::atomic::Ordering::AcqRel)
                || self
                    .tx_wedged
                    .swap(false, std::sync::atomic::Ordering::AcqRel))
        {
            endpoint.clear_halt().await.map_err(|err| {
                DriverError::Nusb(format!("clear halt on Jaguar1 bulk OUT failed: {err}"))
            })?;
        }
        self.record_tx_submission();
        let attempts = BULK_RETRY_ATTEMPTS;
        for attempt in 0..attempts {
            endpoint.submit(Buffer::from(transfer));
            let completion = endpoint.next_complete().await;
            match completion.status {
                Ok(()) => {
                    log::trace!(target: "openipc_rtl88xx::usb", "bulk OUT complete endpoint=0x{:02x} bytes={}", self.bulk_out_ep, completion.actual_len);
                    if completion.actual_len != transfer.len() {
                        if attempt + 1 < attempts {
                            let _ = endpoint.clear_halt().await;
                            crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                            continue;
                        }
                        if terminate {
                            self.tx_wedged
                                .store(true, std::sync::atomic::Ordering::Release);
                        }
                        self.record_tx_failure(crate::TxErrorKind::ShortWrite);
                        return Err(DriverError::BulkOutShort {
                            expected: transfer.len(),
                            actual: completion.actual_len,
                        });
                    }
                    if terminate
                        && !transfer.is_empty()
                        && transfer.len().is_multiple_of(endpoint.max_packet_size())
                    {
                        endpoint.submit(Buffer::new(0));
                        let zlp = endpoint.next_complete().await;
                        if let Err(err) = zlp.status {
                            self.tx_wedged
                                .store(true, std::sync::atomic::Ordering::Release);
                            self.record_tx_failure(tx_error_kind(err));
                            return Err(transfer_error("bulk OUT terminating ZLP failed", err));
                        }
                    }
                    return Ok(completion.actual_len);
                }
                Err(err) if should_retry_transfer_error(err, attempt, attempts) => {
                    if err == TransferError::Stall {
                        let _ = endpoint.clear_halt().await;
                    }
                    crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                }
                Err(err) => {
                    if terminate {
                        self.tx_wedged
                            .store(true, std::sync::atomic::Ordering::Release);
                    }
                    self.record_tx_failure(tx_error_kind(err));
                    return Err(transfer_error("bulk OUT transfer failed", err));
                }
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
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
        for attempt in 0..FIRMWARE_BULK_RETRY_ATTEMPTS {
            endpoint.submit(Buffer::from(transfer));
            let completion = endpoint.next_complete().await;
            match completion.status {
                Ok(()) => {
                    log::trace!(target: "openipc_rtl88xx::usb", "raw bulk OUT complete endpoint=0x{:02x} bytes={}", self.bulk_out_ep, completion.actual_len);
                    return Ok(completion.actual_len);
                }
                Err(err)
                    if should_retry_transfer_error(err, attempt, FIRMWARE_BULK_RETRY_ATTEMPTS) =>
                {
                    let _ = endpoint.clear_halt().await;
                    crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                }
                Err(err) => return Err(transfer_error("raw bulk OUT transfer failed", err)),
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
    }

    /// Write one USB bulk-OUT transfer to the transmit endpoint.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn write_tx_transfer_async(&self, transfer: &[u8]) -> Result<usize, DriverError> {
        self.write_tx_transfer_terminated_async(transfer, false)
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn write_tx_transfer_terminated_async(
        &self,
        transfer: &[u8],
        terminate: bool,
    ) -> Result<usize, DriverError> {
        let mut endpoint = self
            .interface
            .endpoint::<Bulk, Out>(self.bulk_out_ep)
            .map_err(|err| DriverError::Nusb(format!("open bulk OUT endpoint failed: {err}")))?;
        if terminate
            && (!self
                .tx_endpoint_prepared
                .swap(true, std::sync::atomic::Ordering::AcqRel)
                || self
                    .tx_wedged
                    .swap(false, std::sync::atomic::Ordering::AcqRel))
        {
            endpoint.clear_halt().wait().map_err(|err| {
                DriverError::Nusb(format!("clear halt on Jaguar1 bulk OUT failed: {err}"))
            })?;
        }
        self.record_tx_submission();
        let attempts = BULK_RETRY_ATTEMPTS;
        for attempt in 0..attempts {
            let completion =
                endpoint.transfer_blocking(Buffer::from(transfer), crate::device::tx_timeout());
            match completion.status {
                Ok(()) => {
                    log::trace!(target: "openipc_rtl88xx::usb", "bulk OUT complete endpoint=0x{:02x} bytes={}", self.bulk_out_ep, completion.actual_len);
                    if completion.actual_len != transfer.len() {
                        if attempt + 1 < attempts {
                            let _ = endpoint.clear_halt().wait();
                            crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                            continue;
                        }
                        if terminate {
                            self.tx_wedged
                                .store(true, std::sync::atomic::Ordering::Release);
                        }
                        self.record_tx_failure(crate::TxErrorKind::ShortWrite);
                        return Err(DriverError::BulkOutShort {
                            expected: transfer.len(),
                            actual: completion.actual_len,
                        });
                    }
                    if terminate
                        && !transfer.is_empty()
                        && transfer.len().is_multiple_of(endpoint.max_packet_size())
                    {
                        let zlp =
                            endpoint.transfer_blocking(Buffer::new(0), crate::device::tx_timeout());
                        if let Err(err) = zlp.status {
                            self.tx_wedged
                                .store(true, std::sync::atomic::Ordering::Release);
                            self.record_tx_failure(tx_error_kind(err));
                            return Err(transfer_error("bulk OUT terminating ZLP failed", err));
                        }
                    }
                    return Ok(completion.actual_len);
                }
                Err(err) if should_retry_transfer_error(err, attempt, attempts) => {
                    let _ = endpoint.clear_halt().wait();
                    crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                }
                Err(err) => {
                    if terminate {
                        self.tx_wedged
                            .store(true, std::sync::atomic::Ordering::Release);
                    }
                    self.record_tx_failure(tx_error_kind(err));
                    return Err(transfer_error("bulk OUT transfer failed", err));
                }
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
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
        for attempt in 0..FIRMWARE_BULK_RETRY_ATTEMPTS {
            let completion =
                endpoint.transfer_blocking(Buffer::from(transfer), USB_FIRMWARE_TIMEOUT);
            match completion.status {
                Ok(()) => {
                    log::trace!(target: "openipc_rtl88xx::usb", "raw bulk OUT complete endpoint=0x{:02x} bytes={}", self.bulk_out_ep, completion.actual_len);
                    return Ok(completion.actual_len);
                }
                Err(err)
                    if should_retry_transfer_error(err, attempt, FIRMWARE_BULK_RETRY_ATTEMPTS) =>
                {
                    let _ = endpoint.clear_halt().wait();
                    crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                }
                Err(err) => return Err(transfer_error("raw bulk OUT transfer failed", err)),
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
    }

    /// Convert a radiotap + 802.11 packet into a Realtek TX frame and transmit it.
    pub async fn send_packet_async(
        &self,
        radiotap_packet: &[u8],
        mut options: RealtekTxOptions,
    ) -> Result<usize, DriverError> {
        self.apply_pending_beamforming_async().await?;
        let chip = self.probe_chip_async().await?;
        options.capabilities = Some(crate::TxCapabilities::for_chip(chip));
        if let Some(channel) = openipc_core::parse_radiotap_tx_channel(radiotap_packet)
            .map_err(|error| DriverError::TxBuild(error.into()))?
        {
            let report = self.fast_retune_async(channel, true).await?;
            options.current_channel = report.radio.channel;
            options.configured_channel_width = report.radio.channel_width;
            options.configured_channel_offset = report.radio.channel_offset;
        }
        options = self.apply_persistent_tx_options(chip.family, options)?;
        let usb_frame =
            build_usb_tx_frame(radiotap_packet, options).map_err(DriverError::TxBuild)?;
        self.write_tx_transfer_terminated_async(
            &usb_frame,
            options.descriptor.uses_terminated_bulk_out(),
        )
        .await
    }

    /// Pack and submit one accepted prefix of radiotap packets asynchronously.
    ///
    /// The return value is the number of packets represented by the transfer;
    /// this API is shared by native executors and browser WebUSB callers.
    pub async fn send_packet_batch_async(
        &self,
        packets: &[&[u8]],
        mut options: RealtekTxOptions,
    ) -> Result<usize, DriverError> {
        if packets.is_empty() {
            return Ok(0);
        }
        self.apply_pending_beamforming_async().await?;
        let chip = self.probe_chip_async().await?;
        options.capabilities = Some(crate::TxCapabilities::for_chip(chip));
        if let Some(channel) = openipc_core::parse_radiotap_tx_channel(packets[0])
            .map_err(|error| DriverError::TxBuild(error.into()))?
        {
            let report = self.fast_retune_async(channel, true).await?;
            options.current_channel = report.radio.channel;
            options.configured_channel_width = report.radio.channel_width;
            options.configured_channel_offset = report.radio.channel_offset;
        }
        options =
            self.apply_persistent_tx_options_for_batch(chip.family, options, packets.len())?;
        let aggregate = build_usb_tx_aggregate(
            packets,
            options,
            usb_bulk_size(self.device.speed()),
            self.usb_tx_aggregation().max(1),
        )
        .map_err(DriverError::TxBuild)?;
        log::trace!(target: "openipc_rtl88xx::tx", "USB TX aggregate frames={} bytes={} shim={}", aggregate.frame_count, aggregate.bytes.len(), aggregate.first_shim);
        self.write_tx_transfer_terminated_async(
            &aggregate.bytes,
            options.descriptor.uses_terminated_bulk_out(),
        )
        .await?;
        Ok(aggregate.frame_count)
    }

    /// Read raw bytes from a Realtek vendor register.
    #[cfg(target_arch = "wasm32")]
    pub async fn read_register_async(
        &self,
        register: u16,
        len: u16,
    ) -> Result<Vec<u8>, DriverError> {
        for attempt in 0..CONTROL_RETRY_ATTEMPTS {
            match self
                .interface
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
            {
                Ok(bytes) => {
                    self.record_register_read(register, &bytes);
                    log::trace!(target: "openipc_rtl88xx::register", "read register=0x{register:04x} len={} value={}", bytes.len(), hex_bytes(&bytes));
                    return Ok(bytes);
                }
                Err(err) if should_retry_transfer_error(err, attempt, CONTROL_RETRY_ATTEMPTS) => {
                    self.record_register_failure(
                        b'R',
                        register,
                        format!("attempt {}: {err}", attempt + 1),
                    );
                    log::warn!(target: "openipc_rtl88xx::usb", "retrying failed register read register=0x{register:04x} attempt={}: {err}", attempt + 1);
                    crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                }
                Err(err) => {
                    self.record_register_failure(b'R', register, err.to_string());
                    return Err(transfer_error(
                        format!("vendor read 0x{register:04x} failed"),
                        err,
                    ));
                }
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
    }

    /// Read raw bytes from a Realtek vendor register.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn read_register_async(
        &self,
        register: u16,
        len: u16,
    ) -> Result<Vec<u8>, DriverError> {
        match read_register_with_recovery(&self.interface, register, len) {
            Ok(bytes) => {
                self.record_register_read(register, &bytes);
                log::trace!(target: "openipc_rtl88xx::register", "read register=0x{register:04x} len={} value={}", bytes.len(), hex_bytes(&bytes));
                Ok(bytes)
            }
            Err(error) => {
                self.record_register_failure(b'R', register, error.to_string());
                Err(error)
            }
        }
    }

    /// Write raw bytes to a Realtek vendor register.
    #[cfg(target_arch = "wasm32")]
    pub async fn write_register_async(
        &self,
        register: u16,
        bytes: &[u8],
    ) -> Result<(), DriverError> {
        for attempt in 0..CONTROL_RETRY_ATTEMPTS {
            match self
                .interface
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
            {
                Ok(()) => {
                    self.record_register_write(register, bytes);
                    log::trace!(target: "openipc_rtl88xx::register", "write register=0x{register:04x} len={} value={}", bytes.len(), hex_bytes(bytes));
                    return Ok(());
                }
                Err(err) if should_retry_transfer_error(err, attempt, CONTROL_RETRY_ATTEMPTS) => {
                    self.record_register_failure(
                        b'W',
                        register,
                        format!("attempt {}: {err}", attempt + 1),
                    );
                    log::warn!(target: "openipc_rtl88xx::usb", "retrying failed register write register=0x{register:04x} attempt={}: {err}", attempt + 1);
                    crate::time::sleep_ms(retry_delay_ms(attempt)).await;
                }
                Err(err) => {
                    self.record_register_failure(b'W', register, err.to_string());
                    return Err(transfer_error(
                        format!("vendor write 0x{register:04x} failed"),
                        err,
                    ));
                }
            }
        }
        unreachable!("retry loop either returns or reports the final USB error")
    }

    /// Write raw bytes to a Realtek vendor register.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn write_register_async(
        &self,
        register: u16,
        bytes: &[u8],
    ) -> Result<(), DriverError> {
        match write_register_with_recovery(&self.interface, register, bytes) {
            Ok(()) => {
                self.record_register_write(register, bytes);
                log::trace!(target: "openipc_rtl88xx::register", "write register=0x{register:04x} len={} value={}", bytes.len(), hex_bytes(bytes));
                Ok(())
            }
            Err(error) => {
                self.record_register_failure(b'W', register, error.to_string());
                Err(error)
            }
        }
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

fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

const fn usb_bulk_size(speed: Option<nusb::Speed>) -> usize {
    match speed {
        Some(nusb::Speed::Super | nusb::Speed::SuperPlus) => 1024,
        Some(nusb::Speed::High) => 512,
        Some(nusb::Speed::Low | nusb::Speed::Full) | None => 64,
        _ => 64,
    }
}

fn tx_error_kind(error: TransferError) -> crate::TxErrorKind {
    match error {
        TransferError::Cancelled => crate::TxErrorKind::Timeout,
        TransferError::Stall => crate::TxErrorKind::Stall,
        TransferError::Disconnected => crate::TxErrorKind::Disconnected,
        _ => crate::TxErrorKind::Other,
    }
}

#[cfg(target_arch = "wasm32")]
fn web_usb_target_matches(vendor_id: u16, product_id: u16, options: DriverOptions) -> bool {
    matches!(
        (options.target_vendor_id, options.target_product_id),
        (Some(vid), Some(pid)) if vendor_id == vid && product_id == pid
    )
}
