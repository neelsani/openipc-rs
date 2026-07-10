#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn main() -> Result<(), String> {
    nebulus_app::run_desktop()
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}
