//! Local Tauri plugin for Android USB discovery and permission handoff.
//!
//! Desktop builds use nusb directly. Android builds need a small platform bridge
//! to list UsbManager devices, request permission, and pass an opened file
//! descriptor back to Rust so nusb can own the actual transfers.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::*;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::OpenIpcUsb;
#[cfg(mobile)]
use mobile::OpenIpcUsb;

/// Extension trait that exposes the OpenIPC USB plugin state from a Tauri app.
pub trait OpenIpcUsbExt<R: Runtime> {
    /// Return the platform USB helper managed by the plugin.
    fn openipc_usb(&self) -> &OpenIpcUsb<R>;
}

impl<R: Runtime, T: Manager<R>> OpenIpcUsbExt<R> for T {
    fn openipc_usb(&self) -> &OpenIpcUsb<R> {
        self.state::<OpenIpcUsb<R>>().inner()
    }
}

/// Initialize the local OpenIPC USB Tauri plugin.
///
/// On Android this registers the Kotlin UsbManager bridge. On desktop it
/// installs a no-op implementation because desktop discovery uses nusb
/// directly in the station backend.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("openipc-usb")
        .invoke_handler(tauri::generate_handler![
            commands::list_devices,
            commands::open_device,
            commands::close_device,
            commands::open_vpn,
            commands::close_vpn,
        ])
        .setup(|app, api| {
            #[cfg(mobile)]
            let openipc_usb = mobile::init(app, api)?;
            #[cfg(desktop)]
            let openipc_usb = desktop::init(app, api)?;
            app.manage(openipc_usb);
            Ok(())
        })
        .build()
}
