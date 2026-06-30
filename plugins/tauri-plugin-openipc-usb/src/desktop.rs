use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{models::*, Error};

/// Initialize the desktop placeholder implementation.
pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<OpenIpcUsb<R>> {
    Ok(OpenIpcUsb { _app: app.clone() })
}

/// Desktop placeholder for the Android-only USB permission bridge.
pub struct OpenIpcUsb<R: Runtime> {
    _app: AppHandle<R>,
}

impl<R: Runtime> OpenIpcUsb<R> {
    /// Return an error because desktop listing is handled directly with nusb.
    pub fn list_devices(&self) -> crate::Result<Vec<AndroidUsbDevice>> {
        Err(Error::Message(
            "Android USB discovery is only available in the Android Tauri runtime".to_owned(),
        ))
    }

    /// Return an error because desktop opening is handled directly with nusb.
    pub fn open_device(
        &self,
        _request: AndroidUsbOpenRequest,
    ) -> crate::Result<AndroidUsbOpenedDevice> {
        Err(Error::Message(
            "Android USB open is only available in the Android Tauri runtime".to_owned(),
        ))
    }

    /// No-op close for desktop, where the plugin does not own file descriptors.
    pub fn close_device(&self, _request: AndroidUsbCloseRequest) -> crate::Result<()> {
        Ok(())
    }

    /// Return an error because desktop VPN is opened directly by the station backend.
    pub fn open_vpn(&self) -> crate::Result<AndroidVpnOpened> {
        Err(Error::Message(
            "Android VPN open is only available in the Android Tauri runtime".to_owned(),
        ))
    }

    /// No-op close for desktop, where the plugin does not own VPN descriptors.
    pub fn close_vpn(&self, _request: AndroidVpnCloseRequest) -> crate::Result<()> {
        Ok(())
    }
}
