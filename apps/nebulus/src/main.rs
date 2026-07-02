#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    eframe::run_native(
        "Nebulus",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_app_id("dev.neels.openipc.nebulus")
                .with_title("Nebulus")
                .with_inner_size([1360.0, 860.0])
                .with_min_inner_size([760.0, 540.0]),
            ..Default::default()
        },
        Box::new(|context| Ok(Box::new(nebulus::create_app(context)))),
    )
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}
