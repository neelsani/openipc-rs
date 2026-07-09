//! Process-wide `log` facade sink with bounded UI capture.

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Mutex, Once,
    },
};

use log::{Level, LevelFilter, Log, Metadata, Record};

const MAX_CAPTURED_RECORDS: usize = 4_000;
const TRIM_RECORDS: usize = 400;

#[derive(Debug)]
pub(crate) struct CapturedLog {
    pub(crate) level: Level,
    pub(crate) target: String,
    pub(crate) message: String,
}

struct NebulusLogger {
    level: AtomicU8,
    hot_trace_sequence: AtomicUsize,
    sampled_trace_records: AtomicU64,
    trimmed_records: AtomicU64,
    egui_records_excluded: AtomicU64,
    captured: Mutex<VecDeque<CapturedLog>>,
}

impl NebulusLogger {
    const fn new() -> Self {
        Self {
            level: AtomicU8::new(LevelFilter::Info as u8),
            hot_trace_sequence: AtomicUsize::new(0),
            sampled_trace_records: AtomicU64::new(0),
            trimmed_records: AtomicU64::new(0),
            egui_records_excluded: AtomicU64::new(0),
            captured: Mutex::new(VecDeque::new()),
        }
    }
}

impl Log for NebulusLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() as u8 <= self.level.load(Ordering::Relaxed)
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Packet, USB, and WFB trace sites can fire tens of thousands of times
        // per second. Preserve periodic samples for diagnosis without letting
        // string formatting, stderr, or the log panel preempt receive/decode.
        if record.level() == Level::Trace && high_rate_target(record.target()) {
            let sequence = self.hot_trace_sequence.fetch_add(1, Ordering::Relaxed);
            if !sequence.is_multiple_of(128) {
                self.sampled_trace_records.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        let message = record.args().to_string();
        platform_output(record.level(), record.target(), &message);
        // Capturing egui's own layout diagnostics in a widget that is being
        // laid out creates a feedback loop: the new row shifts the log list,
        // which produces another layout diagnostic. Keep those messages in
        // stderr/Logcat, but never feed them back into the in-app log view.
        if record.target().starts_with("egui") {
            self.egui_records_excluded.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut captured = self.captured.lock().expect("log capture poisoned");
        if captured.len() >= MAX_CAPTURED_RECORDS {
            captured.drain(..TRIM_RECORDS);
            self.trimmed_records
                .fetch_add(TRIM_RECORDS as u64, Ordering::Relaxed);
        }
        captured.push_back(CapturedLog {
            level: record.level(),
            target: record.target().to_owned(),
            message,
        });
    }

    fn flush(&self) {}
}

fn high_rate_target(target: &str) -> bool {
    matches!(
        target,
        "openipc_core::rtp"
            | "openipc_core::wfb"
            | "openipc_rtl88xx::usb"
            | "openipc_rtl88xx::register"
    )
}

static LOGGER: NebulusLogger = NebulusLogger::new();
static INIT: Once = Once::new();

/// Counters describing records intentionally omitted from the in-app capture.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CaptureStats {
    pub(crate) sampled_trace_records: u64,
    pub(crate) trimmed_records: u64,
    pub(crate) egui_records_excluded: u64,
}

pub(crate) fn init() {
    INIT.call_once(|| {
        if log::set_logger(&LOGGER).is_ok() {
            log::set_max_level(LevelFilter::Trace);
        }
    });
}

pub(crate) fn set_level(level: LevelFilter) {
    LOGGER.level.store(level as u8, Ordering::Relaxed);
}

pub(crate) fn drain() -> Vec<CapturedLog> {
    LOGGER
        .captured
        .lock()
        .expect("log capture poisoned")
        .drain(..)
        .collect()
}

pub(crate) fn capture_stats() -> CaptureStats {
    CaptureStats {
        sampled_trace_records: LOGGER.sampled_trace_records.load(Ordering::Relaxed),
        trimmed_records: LOGGER.trimmed_records.load(Ordering::Relaxed),
        egui_records_excluded: LOGGER.egui_records_excluded.load(Ordering::Relaxed),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn platform_output(level: Level, target: &str, message: &str) {
    eprintln!("{level:<5} {target}: {message}");
}

#[cfg(target_arch = "wasm32")]
fn platform_output(level: Level, target: &str, message: &str) {
    use wasm_bindgen::JsValue;

    let message = JsValue::from_str(&format!("{level:<5} {target}: {message}"));
    match level {
        Level::Error => web_sys::console::error_1(&message),
        Level::Warn => web_sys::console::warn_1(&message),
        Level::Info => web_sys::console::info_1(&message),
        Level::Debug | Level::Trace => web_sys::console::debug_1(&message),
    }
}
