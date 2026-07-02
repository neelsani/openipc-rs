//! Nebulus egui ground-station application.

#[cfg(target_os = "android")]
mod android;
mod app;
mod audio;
mod build_info;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod desktop_tray;
mod logging;
mod model;
mod recording;
mod runtime;
mod settings;
#[cfg(not(target_arch = "wasm32"))]
mod tun_bridge;
mod ui;
mod video;

pub use app::NebulusApp;

/// Install Nebulus's process-wide logger and UI log capture sink.
pub fn init_logging() {
    logging::init();
}

/// Build Nebulus from an eframe creation context.
pub fn create_app(context: &eframe::CreationContext<'_>) -> NebulusApp {
    init_logging();
    NebulusApp::new(context)
}

/// Android NativeActivity entrypoint used by cargo-apk/xbuild packages.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: android_activity::AndroidApp) {
    init_logging();
    android::install(app.clone());
    let options = eframe::NativeOptions {
        android_app: Some(app),
        viewport: eframe::egui::ViewportBuilder::default().with_fullscreen(true),
        renderer: eframe::Renderer::Glow,
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
