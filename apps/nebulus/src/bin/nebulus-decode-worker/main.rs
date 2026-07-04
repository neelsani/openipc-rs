#[cfg(any(all(target_arch = "wasm32", target_os = "unknown"), test))]
mod batch;
#[cfg(any(all(target_arch = "wasm32", target_os = "unknown"), test))]
mod low_latency_queue;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod web;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn main() {
    if let Err(error) = web::start() {
        wasm_bindgen::throw_val(error);
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn main() {}
