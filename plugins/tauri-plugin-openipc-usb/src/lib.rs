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

pub trait OpenIpcUsbExt<R: Runtime> {
    fn openipc_usb(&self) -> &OpenIpcUsb<R>;
}

impl<R: Runtime, T: Manager<R>> OpenIpcUsbExt<R> for T {
    fn openipc_usb(&self) -> &OpenIpcUsb<R> {
        self.state::<OpenIpcUsb<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("openipc-usb")
        .invoke_handler(tauri::generate_handler![
            commands::list_devices,
            commands::open_device,
            commands::close_device,
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
