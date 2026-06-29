const COMMANDS: &[&str] = &["list_devices", "open_device", "close_device"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
