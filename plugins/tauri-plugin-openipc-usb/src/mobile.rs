use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_openipc_usb);

/// Initialize the mobile USB bridge.
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<OpenIpcUsb<R>> {
    #[cfg(target_os = "android")]
    {
        let handle = api.register_android_plugin("dev.openipc.usb", "OpenIpcUsbPlugin")?;
        Ok(OpenIpcUsb {
            #[cfg(target_os = "android")]
            handle,
        })
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = api;
        Ok(OpenIpcUsb { _app: _app.clone() })
    }
}

/// Platform USB helper stored in Tauri state.
pub struct OpenIpcUsb<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: tauri::plugin::PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    _app: AppHandle<R>,
}

impl<R: Runtime> OpenIpcUsb<R> {
    /// List supported USB devices visible to Android's UsbManager.
    pub fn list_devices(&self) -> crate::Result<Vec<AndroidUsbDevice>> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("listDevices", ())
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        Err(crate::Error::Message(
            "Android USB discovery is only available in the Android Tauri runtime".to_owned(),
        ))
    }

    /// Request permission for and open a USB device, returning a file descriptor.
    pub fn open_device(
        &self,
        request: AndroidUsbOpenRequest,
    ) -> crate::Result<AndroidUsbOpenedDevice> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("openDevice", request)
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        {
            let _ = request;
            Err(crate::Error::Message(
                "Android USB open is only available in the Android Tauri runtime".to_owned(),
            ))
        }
    }

    /// Close a file descriptor opened by the Android USB bridge.
    pub fn close_device(&self, request: AndroidUsbCloseRequest) -> crate::Result<()> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("closeDevice", request)
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        {
            let _ = request;
            Ok(())
        }
    }

    /// Request Android VPN consent when needed and open a TUN fd.
    pub fn open_vpn(&self) -> crate::Result<AndroidVpnOpened> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("openVpn", ())
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        Err(crate::Error::Message(
            "Android VPN open is only available in the Android Tauri runtime".to_owned(),
        ))
    }

    /// Close an Android VPN descriptor held by the mobile plugin.
    pub fn close_vpn(&self, request: AndroidVpnCloseRequest) -> crate::Result<()> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("closeVpn", request)
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        {
            let _ = request;
            Ok(())
        }
    }
}
