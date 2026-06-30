use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// USB device summary returned by the Android UsbManager plugin.
pub struct AndroidUsbDevice {
    /// Stable Android device name used when requesting permission or opening.
    pub device_id: String,
    /// USB vendor ID.
    pub vendor_id: u16,
    /// USB product ID.
    pub product_id: u16,
    /// Product string when Android exposes it.
    pub product: Option<String>,
    /// Manufacturer string when Android exposes it.
    pub manufacturer: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Request used to ask Android for permission and open a USB device.
pub struct AndroidUsbOpenRequest {
    /// Preferred Android device name.
    pub device_id: Option<String>,
    /// Optional vendor ID fallback when no device name is selected.
    pub vendor_id: Option<u16>,
    /// Optional product ID fallback when no device name is selected.
    pub product_id: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Result of opening an Android USB device through UsbManager.
pub struct AndroidUsbOpenedDevice {
    /// File descriptor duplicated from Android and passed into nusb.
    pub fd: i32,
    /// Android device name that was opened.
    pub device_id: String,
    /// USB vendor ID.
    pub vendor_id: u16,
    /// USB product ID.
    pub product_id: u16,
    /// Product string when Android exposes it.
    pub product: Option<String>,
    /// Manufacturer string when Android exposes it.
    pub manufacturer: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Request used to close a descriptor opened by the Android USB plugin.
pub struct AndroidUsbCloseRequest {
    /// File descriptor returned by `AndroidUsbOpenedDevice`.
    pub fd: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Result of opening an Android VpnService TUN interface.
pub struct AndroidVpnOpened {
    /// File descriptor for the Android VPN/TUN interface.
    pub fd: i32,
    /// Human-readable interface/session name.
    pub interface_name: String,
    /// IPv4 address configured on the interface.
    pub address: String,
    /// CIDR prefix length configured on the interface.
    pub prefix_length: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Request used to close a descriptor opened by the Android VPN bridge.
pub struct AndroidVpnCloseRequest {
    /// File descriptor returned by `AndroidVpnOpened`.
    pub fd: i32,
}
