//! WebAssembly bindings for the OpenIPC receiver pipeline and WebUSB driver.
//!
//! The Rust types in this crate wrap `openipc-core` and `openipc-rtl88xx` for
//! browser and Tauri-webview frontends. JavaScript receives typed video frames,
//! raw route payloads, diagnostics, and WebUSB helpers through wasm-bindgen.

mod adaptive;
mod js;
mod receiver;
mod video;
mod webusb;

pub use adaptive::OpenIpcAdaptiveLink;
pub use receiver::OpenIpcReceiver;
pub use webusb::supported_usb_filters;
#[cfg(target_arch = "wasm32")]
pub use webusb::{
    list_authorized_usb_devices, WebBbDbgportRead, WebFalseAlarmCounters, WebInitReport,
    WebIqkReport, WebPhydmWatchdogReport, WebPowerTrackingReport, WebQueueDepth8814,
    WebThermalStatus, WebUsbPhydmWatchdog, WebUsbPowerTracking8812, WebUsbRealtekDevice,
};

use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const OPENIPC_VIDEO_FRAME_TYPES: &'static str = r#"
export type OpenIpcVideoFrame = {
    data: Uint8Array;
    codec: "h264" | "h265";
    codecString: string;
    isKeyFrame: boolean;
    timestamp: number;
};

export type OpenIpcRawPayload = {
    data: Uint8Array;
    packetSeq: string;
    routeId: number;
    channelId: number;
};

export type OpenIpcRxTransferProfile = {
    frames: OpenIpcVideoFrame[];
    rawPayloads: OpenIpcRawPayload[];
    mavlinkPayloads: OpenIpcRawPayload[];
    rawPayloadCount: number;
    rawPayloadBytes: number;
    transferBytes: number;
    packets: number;
    acceptedPackets: number;
    droppedPackets: number;
    crcDropped: number;
    icvDropped: number;
    reportDropped: number;
    ignoredFrames: number;
    sessions: number;
    wfbPayloads: number;
    rtpPackets: number;
    videoFrames: number;
    mavlinkPayloadCount: number;
    mavlinkBytes: number;
    parseMs: number;
    pipelineMs: number;
    totalMs: number;
};
"#;
