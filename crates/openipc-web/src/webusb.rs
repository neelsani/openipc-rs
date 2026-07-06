#[cfg(target_arch = "wasm32")]
use js_sys::{Array, Int8Array, Object, Reflect, Uint32Array, Uint8Array};
#[cfg(target_arch = "wasm32")]
use openipc_rtl88xx::is_supported_id;
use openipc_rtl88xx::SUPPORTED_DEVICES;
#[cfg(target_arch = "wasm32")]
use openipc_rtl88xx::{
    BbDbgportRead, BeamformingFeedback, ChannelWidth, CsiMaskSpec, DriverOptions,
    FalseAlarmCounters, Firmware8814Mode, InitReport, InitStatus, IqkReport,
    Jaguar3PowerTrackingReport, Jaguar3PowerTrackingState, MonitorOptions, PhydmDigState,
    PhydmWatchdogReport, PowerTrackingReport, PowerTrackingState, RadioConfig, RealtekDevice,
    RealtekTxDescriptor, RealtekTxOptions, RetuneReport, RxEnergy, ThermalBucket, ThermalStatus,
};
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// WebUSB-backed Realtek rtl88xx device.
pub struct WebUsbRealtekDevice {
    pub(crate) driver: RealtekDevice,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Monitor-mode initialization report returned to JavaScript.
pub struct WebInitReport {
    report: InitReport,
}

#[cfg(target_arch = "wasm32")]
impl From<InitReport> for WebInitReport {
    fn from(report: InitReport) -> Self {
        Self { report }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebInitReport {
    #[wasm_bindgen(getter)]
    pub fn chip(&self) -> String {
        self.report.chip.family.name().to_owned()
    }

    #[wasm_bindgen(getter, js_name = rfPaths)]
    pub fn rf_paths(&self) -> usize {
        self.report.chip.total_rf_paths()
    }

    #[wasm_bindgen(getter, js_name = cutVersion)]
    pub fn cut_version(&self) -> u8 {
        self.report.chip.cut_version
    }

    #[wasm_bindgen(getter)]
    pub fn status(&self) -> String {
        init_status_name(&self.report.status).to_owned()
    }

    #[wasm_bindgen(getter, js_name = firmwareDownloaded)]
    pub fn firmware_downloaded(&self) -> bool {
        self.report.firmware_downloaded
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Channel-retune result returned to JavaScript.
pub struct WebRetuneReport {
    report: RetuneReport,
}

#[cfg(target_arch = "wasm32")]
impl From<RetuneReport> for WebRetuneReport {
    fn from(report: RetuneReport) -> Self {
        Self { report }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebRetuneReport {
    #[wasm_bindgen(getter)]
    pub fn channel(&self) -> u8 {
        self.report.radio.channel
    }

    #[wasm_bindgen(getter, js_name = usedFastPath)]
    pub fn used_fast_path(&self) -> bool {
        self.report.used_fast_path
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Thermal status returned to JavaScript.
#[derive(Clone, Copy)]
pub struct WebThermalStatus {
    status: ThermalStatus,
}

#[cfg(target_arch = "wasm32")]
impl From<ThermalStatus> for WebThermalStatus {
    fn from(status: ThermalStatus) -> Self {
        Self { status }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebThermalStatus {
    #[wasm_bindgen(getter)]
    pub fn raw(&self) -> u8 {
        self.status.raw
    }

    #[wasm_bindgen(getter)]
    pub fn baseline(&self) -> u8 {
        self.status.baseline
    }

    #[wasm_bindgen(getter)]
    pub fn delta(&self) -> i16 {
        self.status.delta
    }

    #[wasm_bindgen(getter)]
    pub fn valid(&self) -> bool {
        self.status.valid
    }

    #[wasm_bindgen(getter)]
    pub fn bucket(&self) -> String {
        thermal_bucket_name(self.status.bucket()).to_owned()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Frame-free receive-energy snapshot returned to JavaScript.
pub struct WebRxEnergy {
    energy: RxEnergy,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebRxEnergy {
    #[wasm_bindgen(getter, js_name = faOfdm)]
    pub fn fa_ofdm(&self) -> u32 {
        self.energy.fa_ofdm
    }
    #[wasm_bindgen(getter, js_name = faCck)]
    pub fn fa_cck(&self) -> u32 {
        self.energy.fa_cck
    }
    #[wasm_bindgen(getter, js_name = ccaOfdm)]
    pub fn cca_ofdm(&self) -> u32 {
        self.energy.cca_ofdm
    }
    #[wasm_bindgen(getter, js_name = ccaCck)]
    pub fn cca_cck(&self) -> u32 {
        self.energy.cca_cck
    }
    #[wasm_bindgen(getter)]
    pub fn igi(&self) -> u8 {
        self.energy.igi
    }
    #[wasm_bindgen(getter, js_name = nhmDuration)]
    pub fn nhm_duration(&self) -> u16 {
        self.energy.nhm_duration
    }
    #[wasm_bindgen(getter, js_name = nhmValid)]
    pub fn nhm_valid(&self) -> bool {
        self.energy.nhm_valid
    }
    pub fn nhm(&self) -> Uint8Array {
        Uint8Array::from(self.energy.nhm.as_slice())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// RTL8814 queue-depth diagnostic values.
#[derive(Clone, Copy)]
pub struct WebQueueDepth8814 {
    values: [u32; 5],
}

#[cfg(target_arch = "wasm32")]
impl From<[u32; 5]> for WebQueueDepth8814 {
    fn from(values: [u32; 5]) -> Self {
        Self { values }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebQueueDepth8814 {
    #[wasm_bindgen(getter)]
    pub fn q0(&self) -> u32 {
        self.values[0]
    }

    #[wasm_bindgen(getter)]
    pub fn q1(&self) -> u32 {
        self.values[1]
    }

    #[wasm_bindgen(getter)]
    pub fn q2(&self) -> u32 {
        self.values[2]
    }

    #[wasm_bindgen(getter)]
    pub fn q3(&self) -> u32 {
        self.values[3]
    }

    #[wasm_bindgen(getter)]
    pub fn q4(&self) -> u32 {
        self.values[4]
    }

    pub fn values(&self) -> Uint32Array {
        Uint32Array::from(self.values.as_slice())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Baseband debug-port read returned to JavaScript.
#[derive(Clone, Copy)]
pub struct WebBbDbgportRead {
    read: BbDbgportRead,
}

#[cfg(target_arch = "wasm32")]
impl From<BbDbgportRead> for WebBbDbgportRead {
    fn from(read: BbDbgportRead) -> Self {
        Self { read }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebBbDbgportRead {
    #[wasm_bindgen(getter)]
    pub fn selector(&self) -> u32 {
        self.read.selector
    }

    #[wasm_bindgen(getter)]
    pub fn value(&self) -> u32 {
        self.read.value
    }

    #[wasm_bindgen(getter, js_name = savedSelector)]
    pub fn saved_selector(&self) -> u32 {
        self.read.saved_selector
    }

    #[wasm_bindgen(getter, js_name = chipAlive)]
    pub fn chip_alive(&self) -> bool {
        self.read.chip_alive
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// PHY false-alarm counters returned to JavaScript.
#[derive(Clone, Copy)]
pub struct WebFalseAlarmCounters {
    counters: FalseAlarmCounters,
}

#[cfg(target_arch = "wasm32")]
impl From<FalseAlarmCounters> for WebFalseAlarmCounters {
    fn from(counters: FalseAlarmCounters) -> Self {
        Self { counters }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebFalseAlarmCounters {
    #[wasm_bindgen(getter, js_name = ofdmFail)]
    pub fn ofdm_fail(&self) -> u32 {
        self.counters.cnt_ofdm_fail
    }

    #[wasm_bindgen(getter, js_name = cckFail)]
    pub fn cck_fail(&self) -> u32 {
        self.counters.cnt_cck_fail
    }

    #[wasm_bindgen(getter, js_name = ofdmCca)]
    pub fn ofdm_cca(&self) -> u32 {
        self.counters.cnt_ofdm_cca
    }

    #[wasm_bindgen(getter, js_name = cckCca)]
    pub fn cck_cca(&self) -> u32 {
        self.counters.cnt_cck_cca
    }

    #[wasm_bindgen(getter, js_name = cckCrcOk)]
    pub fn cck_crc_ok(&self) -> u32 {
        self.counters.cnt_cck_crc32_ok
    }

    #[wasm_bindgen(getter, js_name = cckCrcError)]
    pub fn cck_crc_error(&self) -> u32 {
        self.counters.cnt_cck_crc32_error
    }

    #[wasm_bindgen(getter, js_name = ofdmCrcOk)]
    pub fn ofdm_crc_ok(&self) -> u32 {
        self.counters.cnt_ofdm_crc32_ok
    }

    #[wasm_bindgen(getter, js_name = ofdmCrcError)]
    pub fn ofdm_crc_error(&self) -> u32 {
        self.counters.cnt_ofdm_crc32_error
    }

    #[wasm_bindgen(getter, js_name = htCrcOk)]
    pub fn ht_crc_ok(&self) -> u32 {
        self.counters.cnt_ht_crc32_ok
    }

    #[wasm_bindgen(getter, js_name = htCrcError)]
    pub fn ht_crc_error(&self) -> u32 {
        self.counters.cnt_ht_crc32_error
    }

    #[wasm_bindgen(getter, js_name = vhtCrcOk)]
    pub fn vht_crc_ok(&self) -> u32 {
        self.counters.cnt_vht_crc32_ok
    }

    #[wasm_bindgen(getter, js_name = vhtCrcError)]
    pub fn vht_crc_error(&self) -> u32 {
        self.counters.cnt_vht_crc32_error
    }

    #[wasm_bindgen(getter)]
    pub fn all(&self) -> u32 {
        self.counters.cnt_all
    }

    #[wasm_bindgen(getter, js_name = ccaAll)]
    pub fn cca_all(&self) -> u32 {
        self.counters.cnt_cca_all
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Result from one PHYDM watchdog tick.
#[derive(Clone, Copy)]
pub struct WebPhydmWatchdogReport {
    report: PhydmWatchdogReport,
}

#[cfg(target_arch = "wasm32")]
impl From<PhydmWatchdogReport> for WebPhydmWatchdogReport {
    fn from(report: PhydmWatchdogReport) -> Self {
        Self { report }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebPhydmWatchdogReport {
    #[wasm_bindgen(getter, js_name = previousIgi)]
    pub fn previous_igi(&self) -> u8 {
        self.report.previous_igi
    }

    #[wasm_bindgen(getter, js_name = currentIgi)]
    pub fn current_igi(&self) -> u8 {
        self.report.current_igi
    }

    #[wasm_bindgen(getter)]
    pub fn counters(&self) -> WebFalseAlarmCounters {
        self.report.counters.into()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// RTL8812 thermal power-tracking report.
#[derive(Clone, Copy)]
pub struct WebPowerTrackingReport {
    report: PowerTrackingReport,
}

#[cfg(target_arch = "wasm32")]
impl From<PowerTrackingReport> for WebPowerTrackingReport {
    fn from(report: PowerTrackingReport) -> Self {
        Self { report }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebPowerTrackingReport {
    #[wasm_bindgen(getter)]
    pub fn enabled(&self) -> bool {
        self.report.enabled
    }

    #[wasm_bindgen(getter, js_name = thermalRaw)]
    pub fn thermal_raw(&self) -> u8 {
        self.report.thermal_raw
    }

    #[wasm_bindgen(getter, js_name = thermalAverage)]
    pub fn thermal_average(&self) -> u8 {
        self.report.thermal_average
    }

    #[wasm_bindgen(getter, js_name = eepromThermal)]
    pub fn eeprom_thermal(&self) -> u8 {
        self.report.eeprom_thermal
    }

    #[wasm_bindgen(getter)]
    pub fn delta(&self) -> u8 {
        self.report.delta
    }

    #[wasm_bindgen(getter, js_name = defaultOfdmIndex)]
    pub fn default_ofdm_index(&self) -> u8 {
        self.report.default_ofdm_index
    }

    #[wasm_bindgen(getter, js_name = finalOfdmIndex0)]
    pub fn final_ofdm_index_0(&self) -> u8 {
        self.report.final_ofdm_index[0]
    }

    #[wasm_bindgen(getter, js_name = finalOfdmIndex1)]
    pub fn final_ofdm_index_1(&self) -> u8 {
        self.report.final_ofdm_index[1]
    }

    #[wasm_bindgen(js_name = finalOfdmIndex)]
    pub fn final_ofdm_index(&self) -> Uint8Array {
        Uint8Array::from(self.report.final_ofdm_index.as_slice())
    }

    #[wasm_bindgen(getter, js_name = swingDelta0)]
    pub fn swing_delta_0(&self) -> i8 {
        self.report.swing_delta[0]
    }

    #[wasm_bindgen(getter, js_name = swingDelta1)]
    pub fn swing_delta_1(&self) -> i8 {
        self.report.swing_delta[1]
    }

    #[wasm_bindgen(js_name = swingDelta)]
    pub fn swing_delta(&self) -> Int8Array {
        Int8Array::from(self.report.swing_delta.as_slice())
    }

    #[wasm_bindgen(getter)]
    pub fn applied(&self) -> bool {
        self.report.applied
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Jaguar3 thermal power-tracking report.
#[derive(Clone, Copy)]
pub struct WebJaguar3PowerTrackingReport {
    report: Jaguar3PowerTrackingReport,
}

#[cfg(target_arch = "wasm32")]
impl From<Jaguar3PowerTrackingReport> for WebJaguar3PowerTrackingReport {
    fn from(report: Jaguar3PowerTrackingReport) -> Self {
        Self { report }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebJaguar3PowerTrackingReport {
    #[wasm_bindgen(getter, js_name = thermalA)]
    pub fn thermal_a(&self) -> u8 {
        self.report.thermal_raw[0]
    }

    #[wasm_bindgen(getter, js_name = thermalB)]
    pub fn thermal_b(&self) -> u8 {
        self.report.thermal_raw[1]
    }

    #[wasm_bindgen(getter, js_name = referenceA)]
    pub fn reference_a(&self) -> i16 {
        self.report.thermal_ref[0]
    }

    #[wasm_bindgen(getter, js_name = referenceB)]
    pub fn reference_b(&self) -> i16 {
        self.report.thermal_ref[1]
    }

    #[wasm_bindgen(getter, js_name = compensationA)]
    pub fn compensation_a(&self) -> i8 {
        self.report.compensation_index[0]
    }

    #[wasm_bindgen(getter, js_name = compensationB)]
    pub fn compensation_b(&self) -> i8 {
        self.report.compensation_index[1]
    }

    #[wasm_bindgen(getter, js_name = lckRan)]
    pub fn lck_ran(&self) -> bool {
        self.report.lck_ran
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// IQK calibration report returned to JavaScript.
#[derive(Clone, Copy)]
pub struct WebIqkReport {
    report: IqkReport,
}

#[cfg(target_arch = "wasm32")]
impl From<IqkReport> for WebIqkReport {
    fn from(report: IqkReport) -> Self {
        Self { report }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebIqkReport {
    #[wasm_bindgen(getter)]
    pub fn chip(&self) -> String {
        self.report.chip.family.name().to_owned()
    }

    #[wasm_bindgen(getter)]
    pub fn channel(&self) -> u8 {
        self.report.channel
    }

    #[wasm_bindgen(getter)]
    pub fn ran(&self) -> bool {
        self.report.ran
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbRealtekDevice {
    #[wasm_bindgen(js_name = fromWebUsbDevice)]
    /// Create a driver from a user-granted WebUSB `USBDevice`.
    pub async fn from_web_usb_device(
        device: web_sys::UsbDevice,
    ) -> Result<WebUsbRealtekDevice, JsValue> {
        let driver = RealtekDevice::from_web_usb_device(device)
            .await
            .map_err(driver_error)?;
        Ok(Self { driver })
    }

    #[wasm_bindgen(js_name = fromWebUsbDeviceWithOptions)]
    /// Create a driver with an optional bulk-OUT endpoint override.
    pub async fn from_web_usb_device_with_options(
        device: web_sys::UsbDevice,
        tx_endpoint_override: i32,
    ) -> Result<WebUsbRealtekDevice, JsValue> {
        Self::from_web_usb_device_advanced(device, tx_endpoint_override, -1, -1).await
    }

    #[wasm_bindgen(js_name = fromWebUsbDeviceAdvanced)]
    /// Create a driver with endpoint and VID/PID overrides.
    pub async fn from_web_usb_device_advanced(
        device: web_sys::UsbDevice,
        tx_endpoint_override: i32,
        target_vendor_id: i32,
        target_product_id: i32,
    ) -> Result<WebUsbRealtekDevice, JsValue> {
        let driver = RealtekDevice::from_web_usb_device_with_options(
            device,
            DriverOptions {
                tx_endpoint_override: optional_u8(tx_endpoint_override, "txEndpointOverride")?,
                target_vendor_id: optional_u16(target_vendor_id, "targetVendorId")?,
                target_product_id: optional_u16(target_product_id, "targetProductId")?,
                ..DriverOptions::default()
            },
        )
        .await
        .map_err(driver_error)?;
        Ok(Self { driver })
    }

    #[wasm_bindgen(js_name = bulkInEndpoint)]
    /// Return the selected bulk-IN endpoint address.
    pub fn bulk_in_endpoint(&self) -> u8 {
        self.driver.bulk_in_ep
    }

    #[wasm_bindgen(js_name = bulkOutEndpoint)]
    /// Return the selected bulk-OUT endpoint address.
    pub fn bulk_out_endpoint(&self) -> u8 {
        self.driver.bulk_out_ep
    }

    #[wasm_bindgen(js_name = rxDescriptorKind)]
    /// Return the Realtek RX descriptor layout needed by `OpenIpcReceiver`.
    pub fn rx_descriptor_kind(&self) -> String {
        match self.driver.rx_descriptor_kind() {
            openipc_core::realtek::RxDescriptorKind::Jaguar3 => "jaguar3",
            openipc_core::realtek::RxDescriptorKind::Jaguar2 => "jaguar2",
            openipc_core::realtek::RxDescriptorKind::Jaguar1 => "jaguar1",
        }
        .to_owned()
    }

    #[wasm_bindgen(js_name = initializeMonitor)]
    /// Initialize the adapter for OpenIPC monitor-mode receive.
    pub async fn initialize_monitor(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
    ) -> Result<WebInitReport, JsValue> {
        self.initialize_monitor_with_options(channel, channel_width_mhz, channel_offset, false)
            .await
    }

    #[wasm_bindgen(js_name = initializeMonitorWithOptions)]
    /// Initialize monitor mode with bad-FCS handling control.
    pub async fn initialize_monitor_with_options(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
        accept_bad_fcs: bool,
    ) -> Result<WebInitReport, JsValue> {
        let radio = RadioConfig {
            channel,
            channel_offset,
            channel_width: parse_channel_width(channel_width_mhz)?,
        };
        let report = self
            .driver
            .initialize_monitor_async(radio, accept_bad_fcs)
            .await
            .map_err(driver_error)?;
        Ok(report.into())
    }

    #[wasm_bindgen(js_name = initializeMonitorAdvanced)]
    /// Initialize monitor mode with advanced bring-up options.
    pub async fn initialize_monitor_advanced(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
        accept_bad_fcs: bool,
        skip_tx_power: bool,
        force_iqk: bool,
        disable_iqk: bool,
        firmware_8814_mode: String,
        firmware_8814_chunk: i32,
    ) -> Result<WebInitReport, JsValue> {
        self.initialize_monitor_advanced_with_txgapk(
            channel,
            channel_width_mhz,
            channel_offset,
            accept_bad_fcs,
            skip_tx_power,
            force_iqk,
            disable_iqk,
            false,
            firmware_8814_mode,
            firmware_8814_chunk,
        )
        .await
    }

    #[wasm_bindgen(js_name = initializeMonitorAdvancedWithTxgapk)]
    /// Initialize monitor mode with all calibration and bring-up options.
    pub async fn initialize_monitor_advanced_with_txgapk(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
        accept_bad_fcs: bool,
        skip_tx_power: bool,
        force_iqk: bool,
        disable_iqk: bool,
        skip_txgapk: bool,
        firmware_8814_mode: String,
        firmware_8814_chunk: i32,
    ) -> Result<WebInitReport, JsValue> {
        let radio = RadioConfig {
            channel,
            channel_offset,
            channel_width: parse_channel_width(channel_width_mhz)?,
        };
        let mode = if firmware_8814_mode.trim().is_empty() {
            Firmware8814Mode::Kernel
        } else {
            Firmware8814Mode::from_env_value(&firmware_8814_mode).ok_or_else(|| {
                JsValue::from_str("firmware8814Mode must be \"kernel\" or \"rtw88\"")
            })?
        };
        let options = MonitorOptions {
            accept_bad_fcs,
            skip_tx_power,
            force_iqk,
            disable_iqk,
            skip_txgapk,
            firmware_8814_mode: mode,
            firmware_8814_chunk: optional_usize(firmware_8814_chunk, "firmware8814Chunk")?,
            rx_path_mask: None,
            ..MonitorOptions::default()
        };
        let report = self
            .driver
            .initialize_monitor_with_options_async(radio, options)
            .await
            .map_err(driver_error)?;
        Ok(report.into())
    }

    #[wasm_bindgen(js_name = retune)]
    /// Fully retune an initialized adapter while preserving firmware state.
    pub async fn retune(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
    ) -> Result<(), JsValue> {
        self.driver
            .retune_async(RadioConfig {
                channel,
                channel_offset,
                channel_width: parse_channel_width(channel_width_mhz)?,
            })
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = fastRetune)]
    /// Lean same-band retune with automatic full-retune fallback.
    pub async fn fast_retune(
        &self,
        channel: u8,
        cache_rf: bool,
    ) -> Result<WebRetuneReport, JsValue> {
        self.driver
            .fast_retune_async(channel, cache_rf)
            .await
            .map(WebRetuneReport::from)
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = shutdownMonitor)]
    /// Best-effort monitor-mode shutdown for chips that need explicit deinit.
    pub async fn shutdown_monitor(&self) -> Result<(), JsValue> {
        self.driver
            .shutdown_monitor_async()
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = startCwTone)]
    /// Start a Devourer-compatible bare RF carrier for test equipment use.
    pub async fn start_cw_tone(&self, channel: u8, gain: u8) -> Result<(), JsValue> {
        self.driver
            .start_cw_tone_async(channel, gain)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = stopCwTone)]
    /// Stop a CW carrier and restore the adapter's RF/baseband state.
    pub async fn stop_cw_tone(&self) -> Result<(), JsValue> {
        self.driver.stop_cw_tone_async().await.map_err(driver_error)
    }

    #[wasm_bindgen(js_name = startContinuousTx)]
    /// Start a hardware-generated modulated continuous carrier.
    pub async fn start_continuous_tx(&self) -> Result<(), JsValue> {
        self.driver
            .start_continuous_tx_async()
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = stopContinuousTx)]
    /// Stop modulated continuous TX and restore normal RF state.
    pub async fn stop_continuous_tx(&self) -> Result<(), JsValue> {
        self.driver
            .stop_continuous_tx_async()
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = setRxPathMask)]
    /// Select active Jaguar1 receive chains for diversity diagnostics.
    pub async fn set_rx_path_mask(&self, mask: u8) -> Result<(), JsValue> {
        self.driver
            .set_rx_path_mask_async(mask)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readRxTransfer)]
    /// Read one Realtek bulk-IN transfer.
    pub async fn read_rx_transfer(&self, length: usize) -> Result<Uint8Array, JsValue> {
        let bytes = self
            .driver
            .read_rx_transfer_async(length)
            .await
            .map_err(driver_error)?;
        Ok(Uint8Array::from(bytes.as_slice()))
    }

    #[wasm_bindgen(js_name = readRxTransfers)]
    /// Read several Realtek bulk-IN transfers with multiple reads in flight.
    pub async fn read_rx_transfers(
        &self,
        length: usize,
        in_flight: usize,
    ) -> Result<Array, JsValue> {
        let transfers = self
            .driver
            .read_rx_transfers_async(length, in_flight)
            .await
            .map_err(driver_error)?;
        let out = Array::new();
        for transfer in transfers {
            out.push(&Uint8Array::from(transfer.as_slice()));
        }
        Ok(out)
    }

    #[wasm_bindgen(js_name = writeTxTransfer)]
    /// Write one fully-built Realtek USB TX transfer.
    pub async fn write_tx_transfer(&self, transfer: &[u8]) -> Result<usize, JsValue> {
        self.driver
            .write_tx_transfer_async(transfer)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = sendPacket)]
    /// Send one radiotap+802.11 packet through the Realtek adapter.
    pub async fn send_packet(
        &self,
        radiotap_packet: &[u8],
        current_channel: u8,
    ) -> Result<usize, JsValue> {
        self.send_packet_with_options(radiotap_packet, current_channel, false)
            .await
    }

    #[wasm_bindgen(js_name = sendPacketWithOptions)]
    /// Send one radiotap+802.11 packet with descriptor compatibility options.
    pub async fn send_packet_with_options(
        &self,
        radiotap_packet: &[u8],
        current_channel: u8,
        legacy_8812_descriptor: bool,
    ) -> Result<usize, JsValue> {
        let chip = self.driver.probe_chip_async().await.map_err(driver_error)?;
        self.driver
            .send_packet_async(
                radiotap_packet,
                RealtekTxOptions {
                    current_channel,
                    descriptor: RealtekTxDescriptor::for_chip_family(chip.family),
                    legacy_8812_descriptor,
                    ..RealtekTxOptions::default()
                },
            )
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = sendPacketForRadio)]
    /// Send a packet with the full RF width needed for 40-in-80 placement.
    pub async fn send_packet_for_radio(
        &self,
        radiotap_packet: &[u8],
        current_channel: u8,
        channel_width_mhz: u16,
        legacy_8812_descriptor: bool,
    ) -> Result<usize, JsValue> {
        let chip = self.driver.probe_chip_async().await.map_err(driver_error)?;
        self.driver
            .send_packet_async(
                radiotap_packet,
                RealtekTxOptions {
                    current_channel,
                    configured_channel_width: parse_channel_width(channel_width_mhz)?,
                    descriptor: RealtekTxDescriptor::for_chip_family(chip.family),
                    legacy_8812_descriptor,
                    ..RealtekTxOptions::default()
                },
            )
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = armBeamformingSounder)]
    /// Arm the hardware sounding engine; `ownMac` is empty or exactly six bytes.
    pub async fn arm_beamforming_sounder(&self, own_mac: &[u8]) -> Result<(), JsValue> {
        self.driver
            .arm_beamforming_sounder_async(optional_mac(own_mac, "ownMac")?)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = armBeamformee)]
    /// Arm an unassociated SU/MU beamformee responder.
    pub async fn arm_beamformee(
        &self,
        beamformer_mac: &[u8],
        own_mac: &[u8],
        mu_feedback: bool,
    ) -> Result<(), JsValue> {
        self.driver
            .arm_beamformee_async(
                required_mac(beamformer_mac, "beamformerMac")?,
                optional_mac(own_mac, "ownMac")?,
                if mu_feedback {
                    BeamformingFeedback::Mu
                } else {
                    BeamformingFeedback::Su
                },
            )
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = applyCsiMask)]
    /// Apply an RX CSI mask over an inclusive MHz range.
    pub async fn apply_csi_mask(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
        low_mhz: u32,
        high_mhz: u32,
        weight: u8,
    ) -> Result<usize, JsValue> {
        let spec = CsiMaskSpec::new(
            low_mhz
                .checked_mul(1000)
                .ok_or_else(|| JsValue::from_str("lowMhz is too large"))?,
            high_mhz
                .checked_mul(1000)
                .ok_or_else(|| JsValue::from_str("highMhz is too large"))?,
            weight,
        )
        .ok_or_else(|| JsValue::from_str("invalid CSI mask range or weight"))?;
        self.driver
            .apply_csi_mask_async(
                RadioConfig {
                    channel,
                    channel_offset,
                    channel_width: parse_channel_width(channel_width_mhz)?,
                },
                spec,
            )
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = clearCsiMask)]
    /// Clear and disable the RX CSI mask.
    pub async fn clear_csi_mask(&self) -> Result<(), JsValue> {
        self.driver
            .clear_csi_mask_async()
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = applyNbiNotch)]
    /// Enable one NBI notch at an absolute frequency in MHz.
    pub async fn apply_nbi_notch(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
        frequency_mhz: u32,
    ) -> Result<bool, JsValue> {
        self.driver
            .apply_nbi_notch_async(
                RadioConfig {
                    channel,
                    channel_offset,
                    channel_width: parse_channel_width(channel_width_mhz)?,
                },
                frequency_mhz
                    .checked_mul(1000)
                    .ok_or_else(|| JsValue::from_str("frequencyMhz is too large"))?,
            )
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = disableNbiNotch)]
    /// Disable the generation-specific NBI filter.
    pub async fn disable_nbi_notch(&self) -> Result<(), JsValue> {
        self.driver
            .disable_nbi_notch_async()
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = setTxPowerOverride)]
    /// Override Realtek TX power for adaptive-link uplink packets.
    pub async fn set_tx_power_override(
        &self,
        current_channel: u8,
        power: u8,
    ) -> Result<(), JsValue> {
        self.driver
            .set_tx_power_override_async(current_channel, power)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = runJaguar3CoexKeepalive)]
    /// Re-assert Jaguar3 coex state and firmware keepalives.
    pub async fn run_jaguar3_coex_keepalive(&self) -> Result<(), JsValue> {
        self.driver
            .run_jaguar3_coex_keepalive_async()
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readThermalStatus)]
    /// Read thermal status diagnostics.
    pub async fn read_thermal_status(&self) -> Result<WebThermalStatus, JsValue> {
        let status = self
            .driver
            .read_thermal_status_async()
            .await
            .map_err(driver_error)?;
        Ok(status.into())
    }

    #[wasm_bindgen(js_name = readRxEnergy)]
    /// Read frame-free FA/CCA/IGI and NHM diagnostics.
    pub async fn read_rx_energy(&self) -> Result<WebRxEnergy, JsValue> {
        let energy = self
            .driver
            .read_rx_energy_async()
            .await
            .map_err(driver_error)?;
        Ok(WebRxEnergy { energy })
    }

    #[wasm_bindgen(js_name = readQueueDepth8814)]
    /// Read RTL8814 queue-depth diagnostics.
    pub async fn read_queue_depth_8814(&self) -> Result<WebQueueDepth8814, JsValue> {
        let regs = self
            .driver
            .read_queue_depth_8814_async()
            .await
            .map_err(driver_error)?;
        Ok(regs.into())
    }

    #[wasm_bindgen(js_name = readBbReg)]
    /// Read a baseband register with a mask.
    pub async fn read_bb_reg(&self, register: u16, mask: u32) -> Result<u32, JsValue> {
        self.driver
            .read_bb_reg_async(register, mask)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readBbDbgport)]
    /// Read the baseband debug port.
    pub async fn read_bb_dbgport(&self, selector: u32) -> Result<WebBbDbgportRead, JsValue> {
        let read = self
            .driver
            .read_bb_dbgport_async(selector)
            .await
            .map_err(driver_error)?;
        Ok(read.into())
    }

    #[wasm_bindgen(js_name = readFalseAlarmCounters)]
    /// Read PHY false-alarm counters.
    pub async fn read_false_alarm_counters(&self) -> Result<WebFalseAlarmCounters, JsValue> {
        let counters = self
            .driver
            .read_false_alarm_counters_async()
            .await
            .map_err(driver_error)?;
        Ok(counters.into())
    }

    #[wasm_bindgen(js_name = runIqk)]
    /// Run IQK calibration for the current channel.
    pub async fn run_iqk(&self, channel: u8) -> Result<WebIqkReport, JsValue> {
        let chip = self.driver.probe_chip_async().await.map_err(driver_error)?;
        let report = self
            .driver
            .run_iqk_async(chip, channel)
            .await
            .map_err(driver_error)?;
        Ok(report.into())
    }

    #[wasm_bindgen(js_name = readRegisterU8)]
    /// Read an 8-bit Realtek register.
    pub async fn read_register_u8(&self, register: u16) -> Result<u8, JsValue> {
        self.driver
            .read_u8_async(register)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readRegisterU32)]
    /// Read a 32-bit Realtek register.
    pub async fn read_register_u32(&self, register: u16) -> Result<u32, JsValue> {
        self.driver
            .read_u32_async(register)
            .await
            .map_err(driver_error)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Stateful PHYDM watchdog helper for WebUSB apps.
pub struct WebUsbPhydmWatchdog {
    state: PhydmDigState,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbPhydmWatchdog {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: PhydmDigState::default(),
        }
    }

    #[wasm_bindgen(js_name = tick)]
    pub async fn tick(
        &mut self,
        device: &WebUsbRealtekDevice,
    ) -> Result<WebPhydmWatchdogReport, JsValue> {
        let report = device
            .driver
            .run_phydm_watchdog_tick_async(&mut self.state)
            .await
            .map_err(driver_error)?;
        Ok(report.into())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Stateful RTL8812 power-tracking helper for WebUSB apps.
pub struct WebUsbPowerTracking8812 {
    state: PowerTrackingState,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbPowerTracking8812 {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: PowerTrackingState::default(),
        }
    }

    #[wasm_bindgen(js_name = init)]
    pub async fn init(&mut self, device: &WebUsbRealtekDevice) -> Result<(), JsValue> {
        device
            .driver
            .init_power_tracking_8812_async(&mut self.state)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = clear)]
    pub async fn clear(&mut self, device: &WebUsbRealtekDevice) -> Result<(), JsValue> {
        device
            .driver
            .clear_power_tracking_8812_async(&mut self.state)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = tick)]
    pub async fn tick(
        &mut self,
        device: &WebUsbRealtekDevice,
        channel: u8,
        channel_width_mhz: u16,
    ) -> Result<WebPowerTrackingReport, JsValue> {
        let report = device
            .driver
            .tick_power_tracking_8812_async(
                &mut self.state,
                channel,
                parse_channel_width(channel_width_mhz)?,
            )
            .await
            .map_err(driver_error)?;
        Ok(report.into())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Stateful Jaguar3 power-tracking helper for WebUSB apps.
pub struct WebUsbJaguar3PowerTracking {
    state: Jaguar3PowerTrackingState,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbJaguar3PowerTracking {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: Jaguar3PowerTrackingState::default(),
        }
    }

    #[wasm_bindgen(js_name = tick)]
    pub async fn tick(
        &mut self,
        device: &WebUsbRealtekDevice,
    ) -> Result<WebJaguar3PowerTrackingReport, JsValue> {
        let report = device
            .driver
            .tick_jaguar3_power_tracking_async(&mut self.state)
            .await
            .map_err(driver_error)?;
        Ok(report.into())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
/// Compatibility wrapper for the former RTL8822C-specific class name.
pub struct WebUsbPowerTracking8822c {
    inner: WebUsbJaguar3PowerTracking,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbPowerTracking8822c {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: WebUsbJaguar3PowerTracking::new(),
        }
    }

    #[wasm_bindgen(js_name = tick)]
    pub async fn tick(
        &mut self,
        device: &WebUsbRealtekDevice,
    ) -> Result<WebJaguar3PowerTrackingReport, JsValue> {
        self.inner.tick(device).await
    }
}

#[cfg(target_arch = "wasm32")]
fn thermal_bucket_name(bucket: ThermalBucket) -> &'static str {
    match bucket {
        ThermalBucket::Unknown => "unknown",
        ThermalBucket::Cool => "cool",
        ThermalBucket::Warm => "warm",
        ThermalBucket::Hot => "hot",
        ThermalBucket::Critical => "critical",
    }
}

#[cfg(target_arch = "wasm32")]
fn parse_channel_width(width_mhz: u16) -> Result<ChannelWidth, JsValue> {
    match width_mhz {
        5 => Ok(ChannelWidth::Mhz5),
        10 => Ok(ChannelWidth::Mhz10),
        20 => Ok(ChannelWidth::Mhz20),
        40 => Ok(ChannelWidth::Mhz40),
        80 => Ok(ChannelWidth::Mhz80),
        _ => Err(JsValue::from_str(
            "unsupported channel width; expected 5, 10, 20, 40, or 80 MHz",
        )),
    }
}

#[cfg(target_arch = "wasm32")]
fn required_mac(value: &[u8], name: &str) -> Result<[u8; 6], JsValue> {
    value
        .try_into()
        .map_err(|_| JsValue::from_str(&format!("{name} must contain exactly 6 bytes")))
}

#[cfg(target_arch = "wasm32")]
fn optional_mac(value: &[u8], name: &str) -> Result<Option<[u8; 6]>, JsValue> {
    if value.is_empty() {
        Ok(None)
    } else {
        required_mac(value, name).map(Some)
    }
}

#[cfg(target_arch = "wasm32")]
fn optional_u8(value: i32, name: &str) -> Result<Option<u8>, JsValue> {
    if value < 0 {
        return Ok(None);
    }
    u8::try_from(value)
        .map(Some)
        .map_err(|_| JsValue::from_str(&format!("{name} is outside 0..255")))
}

#[cfg(target_arch = "wasm32")]
fn optional_u16(value: i32, name: &str) -> Result<Option<u16>, JsValue> {
    if value < 0 {
        return Ok(None);
    }
    u16::try_from(value)
        .map(Some)
        .map_err(|_| JsValue::from_str(&format!("{name} is outside 0..65535")))
}

#[cfg(target_arch = "wasm32")]
fn optional_usize(value: i32, name: &str) -> Result<Option<usize>, JsValue> {
    if value < 0 {
        return Ok(None);
    }
    usize::try_from(value)
        .map(Some)
        .map_err(|_| JsValue::from_str(&format!("{name} is invalid")))
}

#[cfg(target_arch = "wasm32")]
fn init_status_name(status: &InitStatus) -> &'static str {
    match status {
        InitStatus::AlreadyRunning => "already_running",
        InitStatus::Initialized => "initialized",
    }
}

#[cfg(target_arch = "wasm32")]
fn driver_error(err: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&err.to_string())
}

#[wasm_bindgen(js_name = supportedUsbFilters)]
/// Return WebUSB request-device filters as a JSON string.
pub fn supported_usb_filters() -> String {
    // Kept as JSON to avoid forcing web-sys types into the Rust API.
    let mut json = String::from("[");
    for (index, device) in SUPPORTED_DEVICES.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"vendorId":{},"productId":{}}}"#,
            device.vendor_id, device.product_id
        ));
    }
    json.push(']');
    json
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = listAuthorizedUsbDevices)]
/// List already-authorized supported WebUSB devices.
pub async fn list_authorized_usb_devices() -> Result<Array, JsValue> {
    let devices = nusb::list_devices()
        .await
        .map_err(|err| JsValue::from_str(&format!("nusb list_devices failed: {err}")))?;

    let out = Array::new();
    for device in devices {
        if !is_supported_id(device.vendor_id(), device.product_id()) {
            continue;
        }
        let obj = Object::new();
        Reflect::set(
            &obj,
            &JsValue::from_str("vendorId"),
            &JsValue::from_f64(device.vendor_id() as f64),
        )?;
        Reflect::set(
            &obj,
            &JsValue::from_str("productId"),
            &JsValue::from_f64(device.product_id() as f64),
        )?;
        if let Some(product) = device.product_string() {
            Reflect::set(
                &obj,
                &JsValue::from_str("product"),
                &JsValue::from_str(product),
            )?;
        }
        if let Some(manufacturer) = device.manufacturer_string() {
            Reflect::set(
                &obj,
                &JsValue::from_str("manufacturer"),
                &JsValue::from_str(manufacturer),
            )?;
        }
        out.push(&obj);
    }
    Ok(out)
}
