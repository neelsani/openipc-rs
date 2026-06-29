use std::fmt;

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use nusb::MaybeFuture;

use crate::regs::{CHIP_VER_RTL_MASK, CHIP_VER_RTL_SHIFT, RF_TYPE_ID};

pub const SUPPORTED_DEVICES: &[SupportedDevice] = &[
    SupportedDevice::new(
        0x0bda,
        0x8812,
        ChipFamily::Rtl8812,
        "RTL8812AU / RTL8811AU reference PID",
    ),
    SupportedDevice::new(0x0bda, 0x881a, ChipFamily::Rtl8812, "RTL8812AU-VS"),
    SupportedDevice::new(0x0bda, 0x881b, ChipFamily::Rtl8812, "RTL8812AU-VL"),
    SupportedDevice::new(0x0bda, 0x0811, ChipFamily::Rtl8812, "RTL8811AU"),
    SupportedDevice::new(0x0bda, 0xa811, ChipFamily::Rtl8812, "RTL8811AU"),
    SupportedDevice::new(
        0x0bda,
        0xb811,
        ChipFamily::Rtl8812,
        "RTL8811AU / RTL8821AU variant",
    ),
    SupportedDevice::new(0x2357, 0x0101, ChipFamily::Rtl8812, "TP-Link Archer T4U"),
    SupportedDevice::new(0x2357, 0x0103, ChipFamily::Rtl8812, "TP-Link Archer T4UH"),
    SupportedDevice::new(0x2357, 0x010d, ChipFamily::Rtl8812, "TP-Link Archer T4U v2"),
    SupportedDevice::new(
        0x2357,
        0x010e,
        ChipFamily::Rtl8812,
        "TP-Link Archer T4UH v2",
    ),
    SupportedDevice::new(
        0x0b05,
        0x17d2,
        ChipFamily::Rtl8812,
        "ASUS USB-AC56 / RTL8812AU",
    ),
    SupportedDevice::new(0x2604, 0x0012, ChipFamily::Rtl8812, "Tenda U12 / RTL8812AU"),
    SupportedDevice::new(
        0x0409,
        0x0408,
        ChipFamily::Rtl8812,
        "NEC AtermWL900U / RTL8812AU",
    ),
    SupportedDevice::new(
        0x0586,
        0x3426,
        ChipFamily::Rtl8812,
        "ZyXEL NWD6605 / RTL8812AU",
    ),
    SupportedDevice::new(0x0bda, 0x8813, ChipFamily::Rtl8814, "RTL8814AU"),
    SupportedDevice::new(0x0bda, 0x0820, ChipFamily::Rtl8821, "RTL8821AU"),
    SupportedDevice::new(0x0bda, 0x0821, ChipFamily::Rtl8821, "RTL8821AU"),
    SupportedDevice::new(0x0bda, 0x0823, ChipFamily::Rtl8821, "RTL8821AU"),
    SupportedDevice::new(0x0bda, 0x8822, ChipFamily::Rtl8821, "RTL8821AU"),
    SupportedDevice::new(0x0411, 0x0242, ChipFamily::Rtl8821, "Buffalo RTL8821AU"),
    SupportedDevice::new(0x0411, 0x029b, ChipFamily::Rtl8821, "Buffalo RTL8821AU"),
    SupportedDevice::new(0x04bb, 0x0953, ChipFamily::Rtl8821, "I-O Data RTL8821AU"),
    SupportedDevice::new(0x056e, 0x4007, ChipFamily::Rtl8821, "Elecom RTL8821AU"),
    SupportedDevice::new(0x056e, 0x400e, ChipFamily::Rtl8821, "Elecom RTL8821AU"),
    SupportedDevice::new(0x056e, 0x400f, ChipFamily::Rtl8821, "Elecom RTL8821AU"),
    SupportedDevice::new(0x0846, 0x9052, ChipFamily::Rtl8821, "Netgear RTL8821AU"),
    SupportedDevice::new(0x0e66, 0x0023, ChipFamily::Rtl8821, "Hawking RTL8821AU"),
    SupportedDevice::new(0x2001, 0x3314, ChipFamily::Rtl8821, "D-Link RTL8821AU"),
    SupportedDevice::new(0x2001, 0x3318, ChipFamily::Rtl8821, "D-Link RTL8821AU"),
    SupportedDevice::new(0x2019, 0xab32, ChipFamily::Rtl8821, "Planex RTL8821AU"),
    SupportedDevice::new(0x20f4, 0x804b, ChipFamily::Rtl8821, "TRENDnet RTL8821AU"),
    SupportedDevice::new(0x2357, 0x011e, ChipFamily::Rtl8821, "TP-Link RTL8821AU"),
    SupportedDevice::new(
        0x2357,
        0x0120,
        ChipFamily::Rtl8821,
        "TP-Link Archer T2U Plus / RTL8821AU",
    ),
    SupportedDevice::new(0x2357, 0x0122, ChipFamily::Rtl8821, "TP-Link RTL8821AU"),
    SupportedDevice::new(0x3823, 0x6249, ChipFamily::Rtl8821, "Obihai RTL8821AU"),
    SupportedDevice::new(0x7392, 0xa811, ChipFamily::Rtl8821, "Edimax RTL8821AU"),
    SupportedDevice::new(0x7392, 0xa812, ChipFamily::Rtl8821, "Edimax RTL8821AU"),
    SupportedDevice::new(0x7392, 0xa813, ChipFamily::Rtl8821, "Edimax RTL8821AU"),
    SupportedDevice::new(0x7392, 0xb611, ChipFamily::Rtl8821, "Edimax RTL8821AU"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub family_hint: ChipFamily,
    pub label: &'static str,
}

impl SupportedDevice {
    pub const fn new(
        vendor_id: u16,
        product_id: u16,
        family_hint: ChipFamily,
        label: &'static str,
    ) -> Self {
        Self {
            vendor_id,
            product_id,
            family_hint,
            label,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipFamily {
    Rtl8812,
    Rtl8814,
    Rtl8821,
}

impl ChipFamily {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rtl8812 => "RTL8812/RTL8811",
            Self::Rtl8814 => "RTL8814",
            Self::Rtl8821 => "RTL8821",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RfType {
    OneTOneR,
    TwoTTwoR,
    FourTFourR,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipInfo {
    pub family: ChipFamily,
    pub rf_type: RfType,
    pub cut_version: u8,
    pub sys_cfg: u32,
}

impl ChipInfo {
    pub(crate) fn from_probe(vendor_id: u16, product_id: u16, sys_cfg: u32) -> Self {
        let family = if product_id == 0x8813 {
            ChipFamily::Rtl8814
        } else if is_rtl8821a_pid(vendor_id, product_id) {
            ChipFamily::Rtl8821
        } else {
            ChipFamily::Rtl8812
        };
        let rf_type = match family {
            ChipFamily::Rtl8814 => RfType::FourTFourR,
            ChipFamily::Rtl8821 => RfType::OneTOneR,
            ChipFamily::Rtl8812 => {
                if sys_cfg & RF_TYPE_ID != 0 {
                    RfType::OneTOneR
                } else {
                    RfType::TwoTTwoR
                }
            }
        };
        let raw_cut = ((sys_cfg & CHIP_VER_RTL_MASK) >> CHIP_VER_RTL_SHIFT) as u8;
        let cut_version = if family == ChipFamily::Rtl8814 {
            raw_cut
        } else {
            raw_cut.saturating_add(1)
        };
        Self {
            family,
            rf_type,
            cut_version,
            sys_cfg,
        }
    }

    pub const fn total_rf_paths(self) -> usize {
        match self.rf_type {
            RfType::OneTOneR => 1,
            RfType::TwoTTwoR => 2,
            RfType::FourTFourR => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelWidth {
    Mhz20,
    Mhz40,
    Mhz80,
}

impl ChannelWidth {
    pub(crate) const fn rf_bw_bits(self) -> u32 {
        match self {
            Self::Mhz20 => 3,
            Self::Mhz40 => 1,
            Self::Mhz80 => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioConfig {
    pub channel: u8,
    pub channel_offset: u8,
    pub channel_width: ChannelWidth,
}

impl Default for RadioConfig {
    fn default() -> Self {
        Self {
            channel: 36,
            channel_offset: 0,
            channel_width: ChannelWidth::Mhz20,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverOptions {
    pub skip_reset: bool,
    pub initialize_hardware: bool,
    pub target_vendor_id: Option<u16>,
    pub target_product_id: Option<u16>,
    pub tx_endpoint_override: Option<u8>,
}

impl Default for DriverOptions {
    fn default() -> Self {
        Self {
            skip_reset: false,
            initialize_hardware: true,
            target_vendor_id: None,
            target_product_id: None,
            tx_endpoint_override: None,
        }
    }
}

impl DriverOptions {
    pub fn from_env() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self::default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                skip_reset: std::env::var_os("OPENIPC_RS_SKIP_RESET").is_some()
                    || std::env::var_os("DEVOURER_SKIP_RESET").is_some(),
                target_vendor_id: read_env_u16("DEVOURER_VID")
                    .or_else(|| read_env_u16("OPENIPC_RS_USB_VID")),
                target_product_id: read_env_u16("DEVOURER_PID")
                    .or_else(|| read_env_u16("OPENIPC_RS_USB_PID")),
                tx_endpoint_override: read_env_u8("DEVOURER_TX_EP")
                    .or_else(|| read_env_u8("OPENIPC_RS_TX_EP")),
                ..Self::default()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Firmware8814Mode {
    #[default]
    Kernel,
    Rtw88,
}

impl Firmware8814Mode {
    pub fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "kernel" | "default" | "kernel-faithful" => Some(Self::Kernel),
            "rtw88" | "legacy" => Some(Self::Rtw88),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorOptions {
    pub accept_bad_fcs: bool,
    pub skip_tx_power: bool,
    pub force_iqk: bool,
    pub disable_iqk: bool,
    pub firmware_8814_mode: Firmware8814Mode,
    pub firmware_8814_chunk: Option<usize>,
}

impl Default for MonitorOptions {
    fn default() -> Self {
        Self {
            accept_bad_fcs: false,
            skip_tx_power: false,
            force_iqk: false,
            disable_iqk: false,
            firmware_8814_mode: Firmware8814Mode::Kernel,
            firmware_8814_chunk: None,
        }
    }
}

impl MonitorOptions {
    pub fn from_env() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self::default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                skip_tx_power: std::env::var_os("DEVOURER_SKIP_TXPWR").is_some(),
                force_iqk: std::env::var_os("DEVOURER_FORCE_IQK").is_some(),
                disable_iqk: std::env::var_os("DEVOURER_DISABLE_IQK").is_some(),
                firmware_8814_mode: std::env::var("DEVOURER_8814_FWDL")
                    .ok()
                    .and_then(|value| Firmware8814Mode::from_env_value(&value))
                    .unwrap_or_default(),
                firmware_8814_chunk: read_env_usize("DEVOURER_8814_FWDL_CHUNK")
                    .filter(|chunk| (64..=4096).contains(chunk)),
                ..Self::default()
            }
        }
    }

    pub const fn with_accept_bad_fcs(mut self, accept_bad_fcs: bool) -> Self {
        self.accept_bad_fcs = accept_bad_fcs;
        self
    }

    pub const fn should_run_iqk(self, family: ChipFamily) -> bool {
        if self.disable_iqk {
            return false;
        }
        match family {
            ChipFamily::Rtl8812 => true,
            ChipFamily::Rtl8814 => self.force_iqk,
            ChipFamily::Rtl8821 => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitStatus {
    AlreadyRunning,
    Initialized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    pub chip: ChipInfo,
    pub status: InitStatus,
    pub firmware_downloaded: bool,
}

#[derive(Debug)]
pub enum DriverError {
    Nusb(String),
    DeviceNotFound,
    EndpointNotFound(&'static str),
    EndpointOverrideNotFound(u8),
    RegisterReadSize { expected: usize, actual: usize },
    FirmwareChecksumTimeout,
    FirmwareReadyTimeout,
    UnsupportedFirmwarePath(ChipFamily),
    UnsupportedPowerTrackingPath(ChipFamily),
    UnsupportedIqkPath(ChipFamily),
    InvalidTxPower(u8),
    InvalidTransfer(openipc_core::realtek::AggregateError),
    TxBuild(openipc_core::realtek_tx::RealtekTxError),
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nusb(message) => write!(f, "{message}"),
            Self::DeviceNotFound => write!(f, "no supported Realtek rtl88xx adapter found"),
            Self::EndpointNotFound(kind) => write!(f, "no {kind} endpoint found on interface 0"),
            Self::EndpointOverrideNotFound(endpoint) => {
                write!(
                    f,
                    "requested bulk OUT endpoint 0x{endpoint:02x} was not found on interface 0"
                )
            }
            Self::RegisterReadSize { expected, actual } => {
                write!(
                    f,
                    "register read returned {actual} bytes, expected {expected}"
                )
            }
            Self::FirmwareChecksumTimeout => write!(f, "firmware checksum report did not arrive"),
            Self::FirmwareReadyTimeout => write!(f, "firmware did not report ready"),
            Self::UnsupportedFirmwarePath(chip) => {
                write!(f, "{} firmware download path is unsupported", chip.name())
            }
            Self::UnsupportedPowerTrackingPath(chip) => {
                write!(
                    f,
                    "{} thermal power tracking path is unsupported",
                    chip.name()
                )
            }
            Self::UnsupportedIqkPath(chip) => {
                write!(f, "{} IQK calibration path is unsupported", chip.name())
            }
            Self::InvalidTxPower(power) => {
                write!(
                    f,
                    "TX power override {power} is outside the 0..63 TXAGC range"
                )
            }
            Self::InvalidTransfer(err) => write!(f, "{err}"),
            Self::TxBuild(err) => write!(f, "{err}"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_env_integer(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse().ok()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_env_u16(name: &str) -> Option<u16> {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_env_integer(&value))
        .and_then(|value| u16::try_from(value).ok())
}

#[cfg(not(target_arch = "wasm32"))]
fn read_env_u8(name: &str) -> Option<u8> {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_env_integer(&value))
        .and_then(|value| u8::try_from(value).ok())
}

#[cfg(not(target_arch = "wasm32"))]
fn read_env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_env_integer(&value))
        .and_then(|value| usize::try_from(value).ok())
}

impl std::error::Error for DriverError {}

pub fn is_supported_id(vendor_id: u16, product_id: u16) -> bool {
    SUPPORTED_DEVICES
        .iter()
        .any(|dev| dev.vendor_id == vendor_id && dev.product_id == product_id)
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn list_devices() -> Result<Vec<UsbDeviceSummary>, DriverError> {
    let devices = nusb::list_devices()
        .wait()
        .map_err(|err| DriverError::Nusb(format!("list_devices failed: {err}")))?;

    Ok(devices
        .map(|dev| UsbDeviceSummary {
            vendor_id: dev.vendor_id(),
            product_id: dev.product_id(),
            product: dev.product_string().map(str::to_owned),
            manufacturer: dev.manufacturer_string().map(str::to_owned),
            bus_id: dev.bus_id().to_owned(),
            device_address: dev.device_address(),
            port_chain: dev.port_chain().to_vec(),
            supported: is_supported_id(dev.vendor_id(), dev.product_id()),
        })
        .collect())
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn list_devices() -> Result<Vec<UsbDeviceSummary>, DriverError> {
    let message = if cfg!(target_os = "android") {
        "USB enumeration is unavailable in the Android app sandbox; use UsbManager and nusb::Device::from_fd"
    } else {
        "blocking USB enumeration is unavailable on wasm; use nusb/WebUSB async enumeration"
    };
    Err(DriverError::Nusb(message.to_owned()))
}

pub fn list_supported_devices() -> Result<Vec<UsbDeviceSummary>, DriverError> {
    Ok(list_devices()?
        .into_iter()
        .filter(|dev| dev.supported)
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDeviceSummary {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    pub bus_id: String,
    pub device_address: u8,
    pub port_chain: Vec<u8>,
    pub supported: bool,
}

pub(crate) fn is_rtl8821a_pid(vid: u16, pid: u16) -> bool {
    matches!(
        ((vid as u32) << 16) | pid as u32,
        0x0BDA0820
            | 0x0BDA0821
            | 0x0BDA0823
            | 0x0BDA8822
            | 0x04110242
            | 0x0411029B
            | 0x04BB0953
            | 0x056E4007
            | 0x056E400E
            | 0x056E400F
            | 0x08469052
            | 0x0E660023
            | 0x20013314
            | 0x20013318
            | 0x2019AB32
            | 0x20F4804B
            | 0x2357011E
            | 0x23570120
            | 0x23570122
            | 0x38236249
            | 0x7392A811
            | 0x7392A812
            | 0x7392A813
            | 0x7392B611
    )
}
