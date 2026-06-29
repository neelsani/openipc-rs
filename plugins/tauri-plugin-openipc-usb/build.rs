const COMMANDS: &[&str] = &["list_devices", "open_device", "close_device"];

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        tauri_plugin::mobile::update_android_manifest(
            "openipc-usb-device-attached",
            "activity",
            r#"<intent-filter>
    <action android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED" />
</intent-filter>
<meta-data
    android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED"
    android:resource="@xml/openipc_usb_device_filter" />"#
                .to_owned(),
        )
        .expect("failed to update generated Android manifest for OpenIPC USB attach handling");
    }

    println!("cargo:rerun-if-changed=android/src/main/res/xml/openipc_usb_device_filter.xml");

    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
