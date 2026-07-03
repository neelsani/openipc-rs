//! Best-effort operating-system scheduling hints for latency-critical threads.

/// Raise the egui/render thread to an interactive priority where supported.
pub(crate) fn tune_render_thread() {
    tune_current_thread(ThreadRole::Render);
}

/// Raise the USB, protocol, and decoder-submit worker above background work.
pub(crate) fn tune_receiver_thread() {
    tune_current_thread(ThreadRole::Receiver);
}

#[derive(Clone, Copy)]
enum ThreadRole {
    Render,
    Receiver,
}

#[cfg(target_os = "android")]
fn tune_current_thread(role: ThreadRole) {
    // Matches Android's urgent-display class for render and PixelPilot's
    // receive-thread priority for the radio/decode worker.
    let priority = match role {
        ThreadRole::Render => -8,
        ThreadRole::Receiver => -16,
    };
    if let Err(error) = crate::android::set_current_thread_priority(priority) {
        log::debug!(target: "nebulus::latency", "Android thread priority unchanged: {error}");
    }
}

#[cfg(target_os = "macos")]
fn tune_current_thread(_role: ThreadRole) {
    // SAFETY: This changes only the calling pthread's QoS class.
    let status = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
    };
    if status != 0 {
        log::debug!(target: "nebulus::latency", "macOS thread QoS unchanged: {status}");
    }
}

#[cfg(target_os = "windows")]
fn tune_current_thread(_role: ThreadRole) {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
    };

    // SAFETY: GetCurrentThread returns a valid pseudo-handle for this call.
    if let Err(error) = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) } {
        log::debug!(target: "nebulus::latency", "Windows thread priority unchanged: {error}");
    }
}

#[cfg(target_os = "linux")]
fn tune_current_thread(role: ThreadRole) {
    let priority = match role {
        ThreadRole::Render => -5,
        ThreadRole::Receiver => -10,
    };
    // Linux nice values are per-thread. Unprivileged systems may reject a
    // negative value, in which case normal scheduling remains in effect.
    // SAFETY: setpriority has no memory-safety preconditions.
    let status = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, priority) };
    if status != 0 {
        log::debug!(target: "nebulus::latency", "Linux thread priority unchanged");
    }
}
