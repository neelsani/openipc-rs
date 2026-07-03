#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn main() -> eframe::Result {
    nebulus::init_logging();
    let wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(
        eframe::egui_wgpu::SurfaceConfig {
            present_mode: eframe::wgpu::PresentMode::AutoNoVsync,
            desired_maximum_frame_latency: Some(1),
        },
    );
    eframe::run_native(
        "Nebulus",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_app_id("dev.neels.openipc.nebulus")
                .with_title("Nebulus")
                .with_inner_size([1360.0, 860.0])
                .with_min_inner_size([760.0, 540.0]),
            wgpu_options,
            dithering: false,
            ..Default::default()
        },
        Box::new(|context| Ok(Box::new(nebulus::create_app(context)))),
    )
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}
