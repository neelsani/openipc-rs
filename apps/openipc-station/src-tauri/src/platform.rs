use super::*;

pub(crate) fn radio_config(
    channel: u8,
    channel_width_mhz: u16,
    channel_offset: u8,
) -> Result<RadioConfig, String> {
    let channel_width = match channel_width_mhz {
        20 => ChannelWidth::Mhz20,
        40 => ChannelWidth::Mhz40,
        80 => ChannelWidth::Mhz80,
        _ => return Err(format!("unsupported channel width {channel_width_mhz}")),
    };
    Ok(RadioConfig {
        channel,
        channel_offset,
        channel_width,
    })
}

pub(crate) fn usb_id(vendor_id: u16, product_id: u16) -> String {
    format!("{vendor_id:04x}:{product_id:04x}")
}

pub(crate) fn parse_usb_id(value: &str) -> Result<(u16, u16), String> {
    let (vendor, product) = value
        .split_once(':')
        .ok_or_else(|| format!("invalid USB device id {value}; expected vvvv:pppp"))?;
    let vendor_id =
        u16::from_str_radix(vendor, 16).map_err(|_| format!("invalid USB vendor id {vendor}"))?;
    let product_id = u16::from_str_radix(product, 16)
        .map_err(|_| format!("invalid USB product id {product}"))?;
    Ok((vendor_id, product_id))
}

#[cfg(not(target_os = "android"))]
pub(crate) fn station_device_from_summary(device: UsbDeviceSummary) -> StationUsbDevice {
    StationUsbDevice {
        id: None,
        vendor_id: device.vendor_id,
        product_id: device.product_id,
        product: device.product,
        manufacturer: device.manufacturer,
    }
}

pub(crate) fn device_label(
    manufacturer: Option<&str>,
    product: Option<&str>,
    device_id: &str,
) -> String {
    let name = [manufacturer, product]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        device_id.to_owned()
    } else {
        format!("{name} ({device_id})")
    }
}

#[cfg(target_os = "android")]
pub(crate) fn duplicate_fd(fd: i32) -> Result<OwnedFd, String> {
    if fd < 0 {
        return Err(format!("invalid USB file descriptor {fd}"));
    }

    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(format!(
            "duplicate USB file descriptor failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    // SAFETY: `dup` returned a fresh descriptor that this function now owns.
    Ok(unsafe { OwnedFd::from_raw_fd(dup_fd) })
}

pub(crate) fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

pub(crate) fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
