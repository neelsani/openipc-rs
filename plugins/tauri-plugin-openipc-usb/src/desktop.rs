use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{models::*, Error};

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<OpenIpcUsb<R>> {
    Ok(OpenIpcUsb { _app: app.clone() })
}

pub struct OpenIpcUsb<R: Runtime> {
    _app: AppHandle<R>,
}

impl<R: Runtime> OpenIpcUsb<R> {
    pub fn list_devices(&self) -> crate::Result<Vec<AndroidUsbDevice>> {
        Err(Error::Message(
            "Android USB discovery is only available in the Android Tauri runtime".to_owned(),
        ))
    }

    pub fn open_device(
        &self,
        _request: AndroidUsbOpenRequest,
    ) -> crate::Result<AndroidUsbOpenedDevice> {
        Err(Error::Message(
            "Android USB open is only available in the Android Tauri runtime".to_owned(),
        ))
    }

    pub fn close_device(&self, _request: AndroidUsbCloseRequest) -> crate::Result<()> {
        Ok(())
    }
}
