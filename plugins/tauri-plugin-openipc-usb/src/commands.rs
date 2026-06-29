use tauri::{command, AppHandle, Runtime};

use crate::{models::*, OpenIpcUsbExt, Result};

#[command]
pub(crate) async fn list_devices<R: Runtime>(app: AppHandle<R>) -> Result<Vec<AndroidUsbDevice>> {
    app.openipc_usb().list_devices()
}

#[command]
pub(crate) async fn open_device<R: Runtime>(
    app: AppHandle<R>,
    request: AndroidUsbOpenRequest,
) -> Result<AndroidUsbOpenedDevice> {
    app.openipc_usb().open_device(request)
}

#[command]
pub(crate) async fn close_device<R: Runtime>(
    app: AppHandle<R>,
    request: AndroidUsbCloseRequest,
) -> Result<()> {
    app.openipc_usb().close_device(request)
}
