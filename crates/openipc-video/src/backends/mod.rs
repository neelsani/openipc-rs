//! Target decoder backends for desktop, Android, and WebAssembly.

#[cfg(test)]
pub(crate) mod test_fixtures;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub mod web;
