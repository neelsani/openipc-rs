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
    list_authorized_usb_devices, WebUsbPhydmWatchdog, WebUsbPowerTracking8812, WebUsbRealtekDevice,
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
    channelId: number;
};

export type OpenIpcRxTransferProfile = {
    frames: OpenIpcVideoFrame[];
    mavlinkPayloads: OpenIpcRawPayload[];
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
