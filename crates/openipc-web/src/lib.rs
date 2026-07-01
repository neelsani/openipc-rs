//! WebAssembly bindings for the OpenIPC receiver pipeline and WebUSB driver.
//!
//! The Rust types in this crate wrap `openipc-core` and `openipc-rtl88xx` for
//! browser and Tauri-webview frontends. JavaScript receives typed video frames,
//! raw route payloads, diagnostics, and WebUSB helpers through wasm-bindgen.

mod adaptive;
mod js;
mod mock;
mod receiver;
mod video;
mod webusb;

pub use adaptive::OpenIpcAdaptiveLink;
pub use mock::{OpenIpcMockPayloadRuntime, OpenIpcMockRtpPipeline};
pub use receiver::OpenIpcReceiver;
pub use webusb::supported_usb_filters;
#[cfg(target_arch = "wasm32")]
pub use webusb::{
    list_authorized_usb_devices, WebBbDbgportRead, WebFalseAlarmCounters, WebInitReport,
    WebIqkReport, WebJaguar3PowerTrackingReport, WebPhydmWatchdogReport, WebPowerTrackingReport,
    WebQueueDepth8814, WebThermalStatus, WebUsbJaguar3PowerTracking, WebUsbPhydmWatchdog,
    WebUsbPowerTracking8812, WebUsbPowerTracking8822c, WebUsbRealtekDevice,
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
    payloadType: number;
    sequenceNumber: number;
    nalType: number;
    decoderConfigComplete: boolean;
    codecConfig: OpenIpcCodecConfigState;
};

export type OpenIpcCodecConfigState = {
    h264Sps: boolean;
    h264Pps: boolean;
    h265Vps: boolean;
    h265Sps: boolean;
    h265Pps: boolean;
};

export type OpenIpcRtpStatus = {
    packets: number;
    framesEmitted: number;
    configWaitDrops: number;
    keyframesWithPrependedConfig: number;
    parameterSetsPrepended: number;
    fragmentSequenceGaps: number;
    fragmentOverflows: number;
    unsupportedPayloads: number;
    malformedPackets: number;
    lastPayloadType: number | null;
    lastSequenceNumber: number | null;
    lastTimestamp: number | null;
    lastCodec: "h264" | "h265" | null;
    lastNalType: number | null;
    codecConfig: OpenIpcCodecConfigState;
    h264ConfigComplete: boolean;
    h265ConfigComplete: boolean;
    reorderBufferedPackets: number;
    reorderedPackets: number;
    latePackets: number;
    forcedFlushes: number;
};

export type OpenIpcMockFrame = {
    width: number;
    height: number;
    frameIndex: string;
    timestamp: number;
    rtpPackets: number;
    rtpBytes: number;
    rgba: Uint8Array;
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
    rtpStatus: OpenIpcRtpStatus;
};
"#;
