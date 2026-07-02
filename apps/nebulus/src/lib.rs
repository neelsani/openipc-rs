//! Nebulus egui ground-station application.

#[cfg(target_os = "android")]
mod android;
mod app;
mod audio;
mod build_info;
mod model;
mod runtime;
mod settings;
#[cfg(not(target_arch = "wasm32"))]
mod tun_bridge;
mod ui;
mod video;

pub use app::NebulusApp;

/// Build Nebulus from an eframe creation context.
pub fn create_app(context: &eframe::CreationContext<'_>) -> NebulusApp {
    NebulusApp::new(context)
}

/// Android NativeActivity entrypoint used by cargo-apk/xbuild packages.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: android_activity::AndroidApp) {
    let filter = android_logger::FilterBuilder::new()
        .parse("info,nebulus=debug,openipc_core=debug,openipc_rtl88xx=debug,openipc_video=debug")
        .build();
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("Nebulus")
            .with_max_level(log::LevelFilter::Debug)
            .with_filter(filter),
    );
    android::install(app.clone());
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
        // Android exposes one ANativeWindow. Creating Vulkan and EGL surfaces
        // for it at the same time can leave the selected backend with an
        // already-connected, invalid surface. GLES is also the accelerated
        // backend provided by the standard Android emulator.
        setup.instance_descriptor.backends = eframe::wgpu::Backends::GL;
    }
    let options = eframe::NativeOptions {
        android_app: Some(app),
        viewport: eframe::egui::ViewportBuilder::default().with_fullscreen(true),
        wgpu_options,
        ..Default::default()
    };
    let _ = eframe::run_native(
        "Nebulus",
        options,
        Box::new(|context| Ok(Box::new(create_app(context)))),
    );
}

/// Start Nebulus inside a browser canvas.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub async fn start_web() -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();
    let window = web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("no document"))?;
    let canvas = document
        .get_element_by_id("nebulus-canvas")
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("missing #nebulus-canvas"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|context| Ok(Box::new(create_app(context)))),
        )
        .await
}
