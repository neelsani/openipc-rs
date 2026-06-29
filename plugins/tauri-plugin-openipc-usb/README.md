# tauri-plugin-openipc-usb

Local Tauri plugin used by OpenIPC Station on Android.

Android apps cannot rely on desktop-style USB enumeration from the app sandbox.
This plugin keeps the Android-specific part small:

- list supported Realtek USB WiFi adapters through Android `UsbManager`,
- request user permission for the selected adapter,
- open a `UsbDeviceConnection`,
- return the connection file descriptor to Station's Rust backend,
- close the original Android handle after Rust has duplicated the descriptor.

The plugin Android library manifest declares `android.hardware.usb.host`, and
the build script injects a `USB_DEVICE_ATTACHED` intent filter plus
`@xml/openipc_usb_device_filter` metadata into Tauri's generated app activity.
That mirrors the useful PixelPilot pieces: Android can recognize compatible
Realtek adapters, while runtime access still goes through
`UsbManager.requestPermission`.

The Realtek driver still lives in `openipc-rtl88xx`. This plugin only bridges
Android permission and file-descriptor handoff.

The crate is part of the workspace and is versioned with the rest of the repo,
but it has `publish = false` and is not uploaded to crates.io.
