use super::*;

pub(crate) fn emit_log(app: &AppHandle, level: &'static str, message: String) {
    let _ = app.emit(LOG_EVENT, LogPayload { level, message });
}

pub(crate) fn emit_vpn_status(app: &AppHandle, status: VpnStatusPayload) {
    let _ = app.emit(VPN_STATUS_EVENT, status);
}

pub(crate) fn emit_stopped(app: &AppHandle, reason: &'static str, message: String) {
    let _ = app.emit(
        STOPPED_EVENT,
        StoppedPayload {
            reason,
            message: message.clone(),
        },
    );
    let level = if reason == "error" { "error" } else { "info" };
    emit_log(app, level, message);
}
