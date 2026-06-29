#[cfg(target_arch = "wasm32")]
use js_sys::{Array, Object, Reflect, Uint8Array};
#[cfg(target_arch = "wasm32")]
use openipc_core::realtek_tx::RealtekTxOptions;
#[cfg(target_arch = "wasm32")]
use openipc_rtl88xx::is_supported_id;
use openipc_rtl88xx::SUPPORTED_DEVICES;
#[cfg(target_arch = "wasm32")]
use openipc_rtl88xx::{
    ChannelWidth, DriverOptions, FalseAlarmCounters, Firmware8814Mode, InitReport, InitStatus,
    IqkReport, MonitorOptions, PhydmDigState, PhydmWatchdogReport, PowerTrackingReport,
    PowerTrackingState, RadioConfig, RealtekDevice, ThermalBucket,
};
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WebUsbRealtekDevice {
    driver: RealtekDevice,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbRealtekDevice {
    #[wasm_bindgen(js_name = fromWebUsbDevice)]
    pub async fn from_web_usb_device(
        device: web_sys::UsbDevice,
    ) -> Result<WebUsbRealtekDevice, JsValue> {
        let driver = RealtekDevice::from_web_usb_device(device)
            .await
            .map_err(driver_error)?;
        Ok(Self { driver })
    }

    #[wasm_bindgen(js_name = fromWebUsbDeviceWithOptions)]
    pub async fn from_web_usb_device_with_options(
        device: web_sys::UsbDevice,
        tx_endpoint_override: i32,
    ) -> Result<WebUsbRealtekDevice, JsValue> {
        Self::from_web_usb_device_advanced(device, tx_endpoint_override, -1, -1).await
    }

    #[wasm_bindgen(js_name = fromWebUsbDeviceAdvanced)]
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
    pub fn bulk_in_endpoint(&self) -> u8 {
        self.driver.bulk_in_ep
    }

    #[wasm_bindgen(js_name = bulkOutEndpoint)]
    pub fn bulk_out_endpoint(&self) -> u8 {
        self.driver.bulk_out_ep
    }

    #[wasm_bindgen(js_name = initializeMonitor)]
    pub async fn initialize_monitor(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
    ) -> Result<String, JsValue> {
        self.initialize_monitor_with_options(channel, channel_width_mhz, channel_offset, false)
            .await
    }

    #[wasm_bindgen(js_name = initializeMonitorWithOptions)]
    pub async fn initialize_monitor_with_options(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
        accept_bad_fcs: bool,
    ) -> Result<String, JsValue> {
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
        Ok(init_report_json(&report))
    }

    #[wasm_bindgen(js_name = initializeMonitorAdvanced)]
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
    ) -> Result<String, JsValue> {
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
            firmware_8814_mode: mode,
            firmware_8814_chunk: optional_usize(firmware_8814_chunk, "firmware8814Chunk")?,
        };
        let report = self
            .driver
            .initialize_monitor_with_options_async(radio, options)
            .await
            .map_err(driver_error)?;
        Ok(init_report_json(&report))
    }

    #[wasm_bindgen(js_name = readRxTransfer)]
    pub async fn read_rx_transfer(&self, length: usize) -> Result<Uint8Array, JsValue> {
        let bytes = self
            .driver
            .read_rx_transfer_async(length)
            .await
            .map_err(driver_error)?;
        Ok(Uint8Array::from(bytes.as_slice()))
    }

    #[wasm_bindgen(js_name = readRxTransfers)]
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
    pub async fn write_tx_transfer(&self, transfer: &[u8]) -> Result<usize, JsValue> {
        self.driver
            .write_tx_transfer_async(transfer)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = sendPacket)]
    pub async fn send_packet(
        &self,
        radiotap_packet: &[u8],
        current_channel: u8,
    ) -> Result<usize, JsValue> {
        self.send_packet_with_options(radiotap_packet, current_channel, false)
            .await
    }

    #[wasm_bindgen(js_name = sendPacketWithOptions)]
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
                    is_8814a: chip.family == openipc_rtl88xx::ChipFamily::Rtl8814,
                    legacy_8812_descriptor,
                    ..RealtekTxOptions::default()
                },
            )
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = setTxPowerOverride)]
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

    #[wasm_bindgen(js_name = readThermalStatus)]
    pub async fn read_thermal_status(&self) -> Result<String, JsValue> {
        let status = self
            .driver
            .read_thermal_status_async()
            .await
            .map_err(driver_error)?;
        Ok(format!(
            r#"{{"raw":{},"baseline":{},"delta":{},"valid":{},"bucket":"{}"}}"#,
            status.raw,
            status.baseline,
            status.delta,
            status.valid,
            thermal_bucket_name(status.bucket())
        ))
    }

    #[wasm_bindgen(js_name = readQueueDepth8814)]
    pub async fn read_queue_depth_8814(&self) -> Result<String, JsValue> {
        let regs = self
            .driver
            .read_queue_depth_8814_async()
            .await
            .map_err(driver_error)?;
        Ok(format!(
            r#"[{},{},{},{},{}]"#,
            regs[0], regs[1], regs[2], regs[3], regs[4]
        ))
    }

    #[wasm_bindgen(js_name = readBbReg)]
    pub async fn read_bb_reg(&self, register: u16, mask: u32) -> Result<u32, JsValue> {
        self.driver
            .read_bb_reg_async(register, mask)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readBbDbgport)]
    pub async fn read_bb_dbgport(&self, selector: u32) -> Result<String, JsValue> {
        let read = self
            .driver
            .read_bb_dbgport_async(selector)
            .await
            .map_err(driver_error)?;
        Ok(format!(
            r#"{{"selector":{},"value":{},"savedSelector":{},"chipAlive":{}}}"#,
            read.selector, read.value, read.saved_selector, read.chip_alive
        ))
    }

    #[wasm_bindgen(js_name = readFalseAlarmCounters)]
    pub async fn read_false_alarm_counters(&self) -> Result<String, JsValue> {
        let counters = self
            .driver
            .read_false_alarm_counters_async()
            .await
            .map_err(driver_error)?;
        Ok(false_alarm_counters_json(counters))
    }

    #[wasm_bindgen(js_name = runIqk)]
    pub async fn run_iqk(&self, channel: u8) -> Result<String, JsValue> {
        let chip = self.driver.probe_chip_async().await.map_err(driver_error)?;
        let report = self
            .driver
            .run_iqk_async(chip, channel)
            .await
            .map_err(driver_error)?;
        Ok(iqk_report_json(report))
    }

    #[wasm_bindgen(js_name = readRegisterU8)]
    pub async fn read_register_u8(&self, register: u16) -> Result<u8, JsValue> {
        self.driver
            .read_u8_async(register)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readRegisterU32)]
    pub async fn read_register_u32(&self, register: u16) -> Result<u32, JsValue> {
        self.driver
            .read_u32_async(register)
            .await
            .map_err(driver_error)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
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
    pub async fn tick(&mut self, device: &WebUsbRealtekDevice) -> Result<String, JsValue> {
        let report = device
            .driver
            .run_phydm_watchdog_tick_async(&mut self.state)
            .await
            .map_err(driver_error)?;
        Ok(phydm_watchdog_report_json(report))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
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
    ) -> Result<String, JsValue> {
        let report = device
            .driver
            .tick_power_tracking_8812_async(
                &mut self.state,
                channel,
                parse_channel_width(channel_width_mhz)?,
            )
            .await
            .map_err(driver_error)?;
        Ok(power_tracking_report_json(report))
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
fn false_alarm_counters_json(counters: FalseAlarmCounters) -> String {
    format!(
        r#"{{"ofdmFail":{},"cckFail":{},"ofdmCca":{},"cckCca":{},"cckCrcOk":{},"cckCrcError":{},"ofdmCrcOk":{},"ofdmCrcError":{},"htCrcOk":{},"htCrcError":{},"vhtCrcOk":{},"vhtCrcError":{},"all":{},"ccaAll":{}}}"#,
        counters.cnt_ofdm_fail,
        counters.cnt_cck_fail,
        counters.cnt_ofdm_cca,
        counters.cnt_cck_cca,
        counters.cnt_cck_crc32_ok,
        counters.cnt_cck_crc32_error,
        counters.cnt_ofdm_crc32_ok,
        counters.cnt_ofdm_crc32_error,
        counters.cnt_ht_crc32_ok,
        counters.cnt_ht_crc32_error,
        counters.cnt_vht_crc32_ok,
        counters.cnt_vht_crc32_error,
        counters.cnt_all,
        counters.cnt_cca_all
    )
}

#[cfg(target_arch = "wasm32")]
fn phydm_watchdog_report_json(report: PhydmWatchdogReport) -> String {
    format!(
        r#"{{"previousIgi":{},"currentIgi":{},"counters":{}}}"#,
        report.previous_igi,
        report.current_igi,
        false_alarm_counters_json(report.counters)
    )
}

#[cfg(target_arch = "wasm32")]
fn power_tracking_report_json(report: PowerTrackingReport) -> String {
    format!(
        r#"{{"enabled":{},"thermalRaw":{},"thermalAverage":{},"eepromThermal":{},"delta":{},"defaultOfdmIndex":{},"finalOfdmIndex":[{},{}],"swingDelta":[{},{}],"applied":{}}}"#,
        report.enabled,
        report.thermal_raw,
        report.thermal_average,
        report.eeprom_thermal,
        report.delta,
        report.default_ofdm_index,
        report.final_ofdm_index[0],
        report.final_ofdm_index[1],
        report.swing_delta[0],
        report.swing_delta[1],
        report.applied
    )
}

#[cfg(target_arch = "wasm32")]
fn iqk_report_json(report: IqkReport) -> String {
    format!(
        r#"{{"chip":"{}","channel":{},"ran":{}}}"#,
        report.chip.family.name(),
        report.channel,
        report.ran
    )
}

#[cfg(target_arch = "wasm32")]
fn parse_channel_width(width_mhz: u16) -> Result<ChannelWidth, JsValue> {
    match width_mhz {
        20 => Ok(ChannelWidth::Mhz20),
        40 => Ok(ChannelWidth::Mhz40),
        80 => Ok(ChannelWidth::Mhz80),
        _ => Err(JsValue::from_str(
            "unsupported channel width; expected 20, 40, or 80 MHz",
        )),
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
fn init_report_json(report: &InitReport) -> String {
    let status = match report.status {
        InitStatus::AlreadyRunning => "already_running",
        InitStatus::Initialized => "initialized",
    };
    format!(
        r#"{{"chip":"{}","rfPaths":{},"cutVersion":{},"status":"{}","firmwareDownloaded":{}}}"#,
        report.chip.family.name(),
        report.chip.total_rf_paths(),
        report.chip.cut_version,
        status,
        report.firmware_downloaded
    )
}

#[cfg(target_arch = "wasm32")]
fn driver_error(err: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&err.to_string())
}

#[wasm_bindgen(js_name = supportedUsbFilters)]
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
