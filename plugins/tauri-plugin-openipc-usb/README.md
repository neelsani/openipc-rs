# tauri-plugin-openipc-usb

Local Tauri plugin used by OpenIPC Station on Android.

Android apps cannot rely on desktop-style USB enumeration from the app sandbox.
This plugin keeps the Android-specific part small:

- list supported Realtek USB WiFi adapters through Android `UsbManager`,
- request user permission for the selected adapter,
- open a `UsbDeviceConnection`,
- return the connection file descriptor to Station's Rust backend,
- close the original Android handle after Rust has duplicated the descriptor.

The Realtek driver still lives in `openipc-rtl88xx`. This plugin only bridges
Android permission and file-descriptor handoff.

The crate is part of the workspace and is versioned with the rest of the repo,
but it has `publish = false` and is not uploaded to crates.io.
