//! Cross-process exclusive ownership for one physical desktop USB adapter.

use crate::types::{nusb_device_id, DriverError};
use std::collections::hash_map::DefaultHasher;
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;

pub(crate) struct UsbDeviceLock {
    _file: File,
}

impl UsbDeviceLock {
    pub(crate) fn acquire(info: &nusb::DeviceInfo) -> Result<Self, DriverError> {
        let key = nusb_device_id(info);
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let path =
            std::env::temp_dir().join(format!("openipc-rs-usb-{:016x}.lock", hasher.finish()));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| {
                DriverError::Nusb(format!(
                    "cannot open USB ownership lock {}: {err}",
                    path.display()
                ))
            })?;
        file.try_lock().map_err(|err| match err {
            std::fs::TryLockError::WouldBlock => DriverError::DeviceBusy(key.clone()),
            std::fs::TryLockError::Error(err) => {
                DriverError::Nusb(format!("cannot lock USB adapter {key}: {err}"))
            }
        })?;

        let _ = file.set_len(0);
        let _ = writeln!(file, "pid={} device={key}", std::process::id());
        log::info!(target: "openipc_rtl88xx::usb", "locked USB adapter {key} for exclusive access");
        Ok(Self { _file: file })
    }
}
