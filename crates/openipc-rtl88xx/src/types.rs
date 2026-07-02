use std::fmt;

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use nusb::MaybeFuture;

use crate::regs::{CHIP_VER_RTL_MASK, CHIP_VER_RTL_SHIFT, RF_TYPE_ID};

/// Realtek USB IDs supported by the driver.
///
/// This table is the source of truth for desktop filtering, WebUSB filters,
/// and the Android Tauri plugin's generated USB filter resources.
pub const SUPPORTED_DEVICES: &[SupportedDevice] = &[
    SupportedDevice::new(
        0x0bda,
        0x8812,
        ChipFamily::Rtl8812,
        "RTL8812AU / RTL8811AU / RTL8812EU reference PID",
    ),
    SupportedDevice::new(
        0x0bda,
        0x881a,
        ChipFamily::Rtl8812,
        "RTL8812AU-VS / RTL8812EU variant",
    ),
    SupportedDevice::new(
        0x0bda,
        0x881b,
        ChipFamily::Rtl8812,
        "RTL8812AU-VL / RTL8812EU variant",
    ),
    SupportedDevice::new(0x0bda, 0x881c, ChipFamily::Rtl8822e, "RTL8812EU variant"),
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
    SupportedDevice::new(
        0x0bda,
        0xc812,
        ChipFamily::Rtl8822c,
        "RTL8812CU / RTL8822CU WiFi-only default PID",
    ),
    SupportedDevice::new(
        0x0bda,
        0xc82c,
        ChipFamily::Rtl8822c,
        "RTL8822CU multi-function default PID",
    ),
    SupportedDevice::new(
        0x0bda,
        0xc82e,
        ChipFamily::Rtl8822c,
        "RTL8822CU multi-function default PID",
    ),
    SupportedDevice::new(
        0x0bda,
        0xa81a,
        ChipFamily::Rtl8822e,
        "RTL8812EU / LB-LINK BL-M8812EU2",
    ),
    SupportedDevice::new(0x0bda, 0xe822, ChipFamily::Rtl8822e, "RTL8822EU"),
    SupportedDevice::new(0x0bda, 0xa82a, ChipFamily::Rtl8822e, "RTL8822EU"),
];

/// Static metadata for one supported USB VID/PID pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedDevice {
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
    /// Expected Realtek chip family before hardware probing.
    pub family_hint: ChipFamily,
    /// Human-readable adapter or chipset label.
    pub label: &'static str,
}

impl SupportedDevice {
    /// Create a static supported-device entry.
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

/// Realtek chip family supported by this driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipFamily {
    /// RTL8812AU / RTL8811AU class.
    Rtl8812,
    /// RTL8814AU class.
    Rtl8814,
    /// RTL8821AU class.
    Rtl8821,
    /// RTL8812CU / RTL8822CU Jaguar3 class.
    Rtl8822c,
    /// RTL8812EU / RTL8822EU Jaguar3 class.
    Rtl8822e,
}

impl ChipFamily {
    /// Return a human-readable chip-family name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rtl8812 => "RTL8812/RTL8811",
            Self::Rtl8814 => "RTL8814",
            Self::Rtl8821 => "RTL8821",
            Self::Rtl8822c => "RTL8812CU/RTL8822CU",
            Self::Rtl8822e => "RTL8812EU/RTL8822EU",
        }
    }

    /// Return true for Jaguar3 devices with the shared 8822C/8822E descriptor layout.
    pub const fn is_jaguar3(self) -> bool {
        matches!(self, Self::Rtl8822c | Self::Rtl8822e)
    }
}

/// Number of transmit/receive RF paths reported by the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RfType {
    /// One transmit and one receive path.
    OneTOneR,
    /// Two transmit and two receive paths.
    TwoTTwoR,
    /// Four transmit and four receive paths.
    FourTFourR,
}

/// Chip information read from USB id and hardware registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipInfo {
    /// Detected chip family.
    pub family: ChipFamily,
    /// Detected RF path count.
    pub rf_type: RfType,
    /// Realtek cut/revision value.
    pub cut_version: u8,
    /// Raw SYS_CFG register value used during probing.
    pub sys_cfg: u32,
}

impl ChipInfo {
    pub(crate) fn from_probe(
        vendor_id: u16,
        product_id: u16,
        sys_cfg: u32,
        sys_cfg2_chip_id: u8,
    ) -> Self {
        // SYS_CFG2 is authoritative for Jaguar3. RTL8812EU can use the same
        // USB PID as RTL8812AU, so PID-only dispatch silently selects the wrong
        // firmware, PHY tables, and RX descriptor layout.
        let family = if sys_cfg2_chip_id == 0x17 {
            ChipFamily::Rtl8822e
        } else if sys_cfg2_chip_id == 0x13 {
            ChipFamily::Rtl8822c
        } else if product_id == 0x8813 {
            ChipFamily::Rtl8814
        } else if is_rtl8822e_pid(vendor_id, product_id) {
            ChipFamily::Rtl8822e
        } else if is_rtl8822c_pid(vendor_id, product_id) {
            ChipFamily::Rtl8822c
        } else if is_rtl8821a_pid(vendor_id, product_id) {
            ChipFamily::Rtl8821
        } else {
            ChipFamily::Rtl8812
        };
        let rf_type = match family {
            ChipFamily::Rtl8814 => RfType::FourTFourR,
            ChipFamily::Rtl8821 => RfType::OneTOneR,
            ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => RfType::TwoTTwoR,
            ChipFamily::Rtl8812 => {
                if sys_cfg & RF_TYPE_ID != 0 {
                    RfType::OneTOneR
                } else {
                    RfType::TwoTTwoR
                }
            }
        };
        let raw_cut = ((sys_cfg & CHIP_VER_RTL_MASK) >> CHIP_VER_RTL_SHIFT) as u8;
        let cut_version = if matches!(
            family,
            ChipFamily::Rtl8814 | ChipFamily::Rtl8822c | ChipFamily::Rtl8822e
        ) {
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

    /// Return the number of RF paths implied by [`Self::rf_type`].
    pub const fn total_rf_paths(self) -> usize {
        match self.rf_type {
            RfType::OneTOneR => 1,
            RfType::TwoTTwoR => 2,
            RfType::FourTFourR => 4,
        }
    }
}

/// Configured WiFi channel width for monitor mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelWidth {
    /// 5 MHz narrowband mode on Jaguar3 devices.
    Mhz5,
    /// 10 MHz narrowband mode on Jaguar3 devices.
    Mhz10,
    /// 20 MHz channel.
    Mhz20,
    /// 40 MHz channel.
    Mhz40,
    /// 80 MHz channel.
    Mhz80,
}

impl ChannelWidth {
    pub(crate) const fn rf_bw_bits(self) -> u32 {
        match self {
            Self::Mhz5 | Self::Mhz10 => 3,
            Self::Mhz20 => 3,
            Self::Mhz40 => 1,
            Self::Mhz80 => 0,
        }
    }
}

/// Radio channel configuration for monitor-mode bring-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioConfig {
    /// Primary WiFi channel number.
    pub channel: u8,
    /// Secondary-channel offset used for 40/80 MHz operation.
    pub channel_offset: u8,
    /// Channel width.
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

/// Device open and endpoint selection options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverOptions {
    /// Skip USB reset before claiming the adapter.
    pub skip_reset: bool,
    /// Run hardware initialization after opening when used by high-level open helpers.
    pub initialize_hardware: bool,
    /// Optional USB vendor-id filter.
    pub target_vendor_id: Option<u16>,
    /// Optional USB product-id filter.
    pub target_product_id: Option<u16>,
    /// Optional bulk-OUT endpoint override.
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
    /// Read driver options from `OPENIPC_RS_*` and devourer-compatible env vars.
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

/// RTL8814 firmware download strategy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Firmware8814Mode {
    /// Kernel-faithful reserved-page/DDMA path.
    #[default]
    Kernel,
    /// rtw88-style firmware path retained for bring-up experiments.
    Rtw88,
}

impl Firmware8814Mode {
    /// Parse an environment/configuration string.
    pub fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "kernel" | "default" | "kernel-faithful" => Some(Self::Kernel),
            "rtw88" | "legacy" => Some(Self::Rtw88),
            _ => None,
        }
    }
}

/// Monitor-mode hardware initialization options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorOptions {
    /// Ask hardware to pass packets even when CRC/ICV is marked bad.
    pub accept_bad_fcs: bool,
    /// Skip TX power table programming during channel setup.
    pub skip_tx_power: bool,
    /// Run IQK on chips where it is normally skipped.
    pub force_iqk: bool,
    /// Disable IQK even where it is normally run.
    pub disable_iqk: bool,
    /// Skip RTL8822E TX gain calibration after IQK.
    pub skip_txgapk: bool,
    /// RTL8814 firmware download path.
    pub firmware_8814_mode: Firmware8814Mode,
    /// Optional RTL8814 firmware chunk size override.
    pub firmware_8814_chunk: Option<usize>,
    /// Optional Jaguar1 RX-chain mask written to register `0x808` after IQK.
    ///
    /// Bits 0/4 select CCK/OFDM path A, 1/5 path B, 2/6 path C, and 3/7 path D.
    pub rx_path_mask: Option<u8>,
}

impl Default for MonitorOptions {
    fn default() -> Self {
        Self {
            accept_bad_fcs: false,
            skip_tx_power: false,
            force_iqk: false,
            disable_iqk: false,
            skip_txgapk: false,
            firmware_8814_mode: Firmware8814Mode::Kernel,
            firmware_8814_chunk: None,
            rx_path_mask: None,
        }
    }
}

impl MonitorOptions {
    /// Read monitor options from devourer-compatible env vars.
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
                disable_iqk: std::env::var_os("DEVOURER_DISABLE_IQK").is_some()
                    || std::env::var_os("DEVOURER_SKIP_IQK").is_some(),
                skip_txgapk: std::env::var_os("DEVOURER_SKIP_TXGAPK").is_some(),
                firmware_8814_mode: std::env::var("DEVOURER_8814_FWDL")
                    .ok()
                    .and_then(|value| Firmware8814Mode::from_env_value(&value))
                    .unwrap_or_default(),
                firmware_8814_chunk: read_env_usize("DEVOURER_8814_FWDL_CHUNK")
                    .filter(|chunk| (64..=4096).contains(chunk)),
                rx_path_mask: read_env_u8("DEVOURER_RX_PATHS"),
                ..Self::default()
            }
        }
    }

    /// Return a copy with `accept_bad_fcs` changed.
    pub const fn with_accept_bad_fcs(mut self, accept_bad_fcs: bool) -> Self {
        self.accept_bad_fcs = accept_bad_fcs;
        self
    }

    /// Return a copy with a Jaguar1 RX-chain mask applied after channel setup and IQK.
    pub const fn with_rx_path_mask(mut self, mask: u8) -> Self {
        self.rx_path_mask = Some(mask);
        self
    }

    /// Return whether IQK should run for the selected chip family.
    pub const fn should_run_iqk(self, family: ChipFamily) -> bool {
        if self.disable_iqk {
            return false;
        }
        match family {
            ChipFamily::Rtl8812 => true,
            ChipFamily::Rtl8814 => self.force_iqk,
            ChipFamily::Rtl8821 => false,
            ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => true,
        }
    }
}

/// Result status from monitor-mode initialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitStatus {
    /// The adapter already appeared to be initialized.
    AlreadyRunning,
    /// Initialization ran during this call.
    Initialized,
}

/// Report returned after monitor-mode initialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// Chip information used for initialization.
    pub chip: ChipInfo,
    /// Whether initialization was skipped or performed.
    pub status: InitStatus,
    /// True if firmware was downloaded during initialization.
    pub firmware_downloaded: bool,
}

/// Error type returned by the Realtek USB driver.
#[derive(Debug)]
pub enum DriverError {
    /// Error from nusb or platform USB access.
    Nusb(String),
    /// No matching supported adapter was found.
    DeviceNotFound,
    /// Required endpoint was missing.
    EndpointNotFound(&'static str),
    /// Requested bulk-OUT endpoint override was not present.
    EndpointOverrideNotFound(u8),
    /// A control read returned an unexpected byte count.
    RegisterReadSize {
        /// Number of bytes requested.
        expected: usize,
        /// Number of bytes returned by the device/backend.
        actual: usize,
    },
    /// Firmware checksum C2H message did not arrive.
    FirmwareChecksumTimeout,
    /// Firmware ready state did not arrive.
    FirmwareReadyTimeout,
    /// Requested firmware path is not implemented for this chip.
    UnsupportedFirmwarePath(ChipFamily),
    /// Thermal power tracking is not implemented for this chip.
    UnsupportedPowerTrackingPath(ChipFamily),
    /// IQK calibration path is not implemented for this chip.
    UnsupportedIqkPath(ChipFamily),
    /// RX-chain masking is only supported by the Jaguar1 register layout.
    UnsupportedRxPathMask(ChipFamily),
    /// TX power override was outside the Realtek TXAGC range.
    InvalidTxPower(u8),
    /// Realtek RX aggregate parse error.
    InvalidTransfer(openipc_core::realtek::AggregateError),
    /// Realtek TX descriptor/frame build error.
    TxBuild(crate::tx::RealtekTxError),
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
            Self::UnsupportedRxPathMask(chip) => {
                write!(f, "{} does not use the Jaguar1 RX-path mask", chip.name())
            }
            Self::InvalidTxPower(power) => {
                write!(
                    f,
                    "TX power override {power} is outside the selected chipset's TXAGC range"
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

/// Return true if a USB VID/PID pair exists in [`SUPPORTED_DEVICES`].
pub fn is_supported_id(vendor_id: u16, product_id: u16) -> bool {
    supported_device(vendor_id, product_id).is_some()
}

/// Return static metadata for a supported USB VID/PID pair.
pub fn supported_device(vendor_id: u16, product_id: u16) -> Option<&'static SupportedDevice> {
    SUPPORTED_DEVICES
        .iter()
        .find(|dev| dev.vendor_id == vendor_id && dev.product_id == product_id)
}

/// Return the chip-family hint associated with a supported USB VID/PID pair.
pub fn supported_family_hint(vendor_id: u16, product_id: u16) -> Option<ChipFamily> {
    supported_device(vendor_id, product_id).map(|device| device.family_hint)
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
/// List all USB devices visible to desktop `nusb`.
///
/// The returned summaries include unsupported devices with `supported = false`
/// so diagnostic tools can show what the OS sees.
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
/// Return an explanatory error on platforms without blocking desktop USB listing.
pub fn list_devices() -> Result<Vec<UsbDeviceSummary>, DriverError> {
    let message = if cfg!(target_os = "android") {
        "USB enumeration is unavailable in the Android app sandbox; use UsbManager and nusb::Device::from_fd"
    } else {
        "blocking USB enumeration is unavailable on wasm; use nusb/WebUSB async enumeration"
    };
    Err(DriverError::Nusb(message.to_owned()))
}

/// List only supported Realtek rtl88xx adapters visible to desktop `nusb`.
pub fn list_supported_devices() -> Result<Vec<UsbDeviceSummary>, DriverError> {
    Ok(list_devices()?
        .into_iter()
        .filter(|dev| dev.supported)
        .collect())
}

/// User-facing summary of one USB device observed by `nusb`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDeviceSummary {
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
    /// Optional USB product string.
    pub product: Option<String>,
    /// Optional USB manufacturer string.
    pub manufacturer: Option<String>,
    /// Platform bus id reported by `nusb`.
    pub bus_id: String,
    /// USB device address on the bus.
    pub device_address: u8,
    /// USB hub port path.
    pub port_chain: Vec<u8>,
    /// True if this VID/PID is in [`SUPPORTED_DEVICES`].
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

pub(crate) fn is_rtl8822c_pid(vid: u16, pid: u16) -> bool {
    matches!(
        ((vid as u32) << 16) | pid as u32,
        0x0BDAC812 | 0x0BDAC82C | 0x0BDAC82E
    )
}

pub(crate) fn is_rtl8822e_pid(vid: u16, pid: u16) -> bool {
    matches!(
        ((vid as u32) << 16) | pid as u32,
        0x0BDA881C | 0x0BDAA81A | 0x0BDAE822 | 0x0BDAA82A
    )
}
