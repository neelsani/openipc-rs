#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::AudioPlayer;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use web::AudioPlayer;
