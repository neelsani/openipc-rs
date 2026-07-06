#[cfg(target_arch = "wasm32")]
use js_sys::{Function, Promise};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;

pub(crate) struct DateNow;

#[cfg(target_arch = "wasm32")]
pub(crate) fn monotonic_micros() -> u64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0, |performance| (performance.now() * 1_000.0) as u64)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn monotonic_micros() -> u64 {
    static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    STARTED
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros()
        .min(u128::from(u64::MAX)) as u64
}

impl DateNow {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn now() -> f64 {
        js_sys::Date::now()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn now() -> f64 {
        use std::time::{SystemTime, UNIX_EPOCH};

        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }

    pub(crate) fn deadline_ms(delta_ms: f64) -> f64 {
        Self::now() + delta_ms
    }

    pub(crate) fn expired(deadline_ms: f64) -> bool {
        Self::now() >= deadline_ms
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn sleep_micros(micros: u32) {
    if micros >= 1_000 {
        sleep_ms(micros.div_ceil(1_000)).await;
    } else {
        yield_now().await;
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn sleep_micros(micros: u32) {
    std::thread::sleep(std::time::Duration::from_micros(micros as u64));
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn sleep_ms(ms: u32) {
    let Some(window) = web_sys::window() else {
        yield_now().await;
        return;
    };
    let promise = Promise::new(&mut |resolve: Function, _reject: Function| {
        let callback = Closure::once_into_js(move || {
            let _ = resolve.call0(&JsValue::UNDEFINED);
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.unchecked_ref(),
            ms.min(i32::MAX as u32) as i32,
        );
    });
    let _ = JsFuture::from(promise).await;
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn sleep_ms(ms: u32) {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn yield_now() {
    let _ = JsFuture::from(Promise::resolve(&JsValue::UNDEFINED)).await;
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn yield_now() {
    std::thread::yield_now();
}
