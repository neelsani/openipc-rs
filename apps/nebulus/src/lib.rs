//! Nebulus egui ground-station application.

#[cfg(target_os = "android")]
mod android;
mod app;
mod audio;
mod build_info;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod desktop_tray;
mod logging;
#[cfg(not(target_arch = "wasm32"))]
mod low_latency;
mod model;
mod recording;
mod runtime;
mod settings;
#[cfg(not(target_arch = "wasm32"))]
mod tun_bridge;
mod ui;
mod video;
#[cfg(target_os = "windows")]
mod wintun;

pub use app::NebulusApp;

/// Install Nebulus's process-wide logger and UI log capture sink.
pub fn init_logging() {
    logging::init();
}

/// Build Nebulus from an eframe creation context.
pub fn create_app(context: &eframe::CreationContext<'_>) -> NebulusApp {
    init_logging();
    #[cfg(not(target_arch = "wasm32"))]
    low_latency::tune_render_thread();
    NebulusApp::new(context)
}

/// Android NativeActivity entrypoint used by cargo-apk/xbuild packages.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: android_activity::AndroidApp) {
    init_logging();
    android::install(app.clone());
    let glow_options = egui_glow::GlowConfiguration {
        vsync: false,
        hardware_acceleration: egui_glow::HardwareAcceleration::Required,
        ..Default::default()
    };
    let options = eframe::NativeOptions {
        android_app: Some(app),
        viewport: eframe::egui::ViewportBuilder::default().with_fullscreen(true),
        renderer: eframe::Renderer::Glow,
        glow_options,
        dithering: false,
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
    prime_low_latency_webgl(&canvas)?;
    let options = eframe::WebOptions {
        dithering: false,
        ..Default::default()
    };
    eframe::WebRunner::new()
        .start(
            canvas,
            options,
            Box::new(|context| Ok(Box::new(create_app(context)))),
        )
        .await
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn prime_low_latency_webgl(
    canvas: &web_sys::HtmlCanvasElement,
) -> Result<(), wasm_bindgen::JsValue> {
    let options = js_sys::Object::new();
    for (name, value) in [
        ("alpha", false),
        ("antialias", false),
        ("depth", false),
        ("desynchronized", true),
        ("preserveDrawingBuffer", false),
        ("stencil", false),
    ] {
        js_sys::Reflect::set(
            &options,
            &wasm_bindgen::JsValue::from_str(name),
            &wasm_bindgen::JsValue::from_bool(value),
        )?;
    }
    js_sys::Reflect::set(
        &options,
        &wasm_bindgen::JsValue::from_str("powerPreference"),
        &wasm_bindgen::JsValue::from_str("high-performance"),
    )?;
    // Creating the context here makes eframe reuse it when WebRunner starts.
    // Browsers that do not implement a hint simply ignore that property.
    let _ = canvas.get_context_with_context_options("webgl2", &options)?;
    Ok(())
}
