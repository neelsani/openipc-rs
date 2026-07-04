mod decoder;
mod rtp;

use std::cell::Cell;

use js_sys::{Object, Reflect};
use wasm_bindgen::{closure::Closure, JsCast as _, JsValue};

#[derive(Clone, Copy)]
enum WorkerRole {
    Rtp,
    Decoder,
}

thread_local! {
    static ROLE: Cell<Option<WorkerRole>> = const { Cell::new(None) };
}

pub(crate) fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let role = match worker_scope().name().as_str() {
        "nebulus-rtp" => {
            rtp::start();
            WorkerRole::Rtp
        }
        "nebulus-video-decode" => {
            decoder::start().map_err(|error| JsValue::from_str(&error))?;
            WorkerRole::Decoder
        }
        name => return Err(JsValue::from_str(&format!("unknown worker role {name}"))),
    };
    ROLE.with(|slot| slot.set(Some(role)));

    let onmessage =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let result = ROLE.with(|slot| match slot.get() {
                Some(WorkerRole::Rtp) => rtp::handle_message(event.data()),
                Some(WorkerRole::Decoder) => decoder::handle_message(event.data()),
                None => Err("worker role was not initialized".to_owned()),
            });
            if let Err(error) = result {
                post_error(error);
            }
        });
    worker_scope().set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let object = Object::new();
    set_string(&object, "kind", "ready");
    set_string(
        &object,
        "role",
        match role {
            WorkerRole::Rtp => "rtp",
            WorkerRole::Decoder => "decoder",
        },
    );
    let _ = worker_scope().post_message(&object);
    Ok(())
}

pub(super) fn worker_scope() -> web_sys::DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

pub(super) fn post_error(message: String) {
    let object = Object::new();
    set_string(&object, "kind", "error");
    set_string(&object, "message", &message);
    let _ = worker_scope().post_message(&object);
}

pub(super) fn post_kind(kind: &str) {
    let object = Object::new();
    set_string(&object, "kind", kind);
    let _ = worker_scope().post_message(&object);
}

pub(super) fn set_value(object: &Object, name: &str, value: &JsValue) {
    let _ = Reflect::set(object, &JsValue::from_str(name), value);
}

pub(super) fn set_string(object: &Object, name: &str, value: &str) {
    set_value(object, name, &JsValue::from_str(value));
}

pub(super) fn set_number(object: &Object, name: &str, value: f64) {
    set_value(object, name, &JsValue::from_f64(value));
}

pub(super) fn set_bool(object: &Object, name: &str, value: bool) {
    set_value(object, name, &JsValue::from_bool(value));
}

pub(super) fn set_optional_number(object: &Object, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        set_number(object, name, value);
    }
}

pub(super) fn string_field(value: &JsValue, name: &str) -> Option<String> {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_string())
}

pub(super) fn bool_field(value: &JsValue, name: &str) -> bool {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

pub(super) fn number_field(value: &JsValue, name: &str) -> Option<f64> {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_f64())
}
