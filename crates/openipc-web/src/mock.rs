use js_sys::{Array, Object, Reflect, Uint8Array};
use openipc_core::{
    ChannelId, FrameLayout, MockRtpPipeline, PayloadRouteId, ReceiverBatch, ReceiverBatchOptions,
    ReceiverRuntime,
};
use wasm_bindgen::prelude::*;

use crate::js::{elapsed_ms, now_ms, raw_payload_object, set_number};
use crate::video::{rtp_status_object, video_frame_object};

const MOCK_VIDEO_ROUTE_ID: PayloadRouteId = PayloadRouteId::new(1);
const MOCK_KEY_SLOT: u64 = 0;

#[wasm_bindgen]
/// No-hardware development source that generates RTP-backed RGBA test frames.
pub struct OpenIpcMockRtpPipeline {
    pipeline: MockRtpPipeline,
}

#[wasm_bindgen]
impl OpenIpcMockRtpPipeline {
    #[wasm_bindgen(constructor)]
    /// Create a mock RTP pipeline.
    pub fn new(width: u16, height: u16, fps: u16) -> OpenIpcMockRtpPipeline {
        OpenIpcMockRtpPipeline {
            pipeline: MockRtpPipeline::new(width, height, fps),
        }
    }

    #[wasm_bindgen(js_name = nextFrame, unchecked_return_type = "OpenIpcMockFrame")]
    /// Generate and recover the next mock RTP frame.
    pub fn next_frame(&mut self) -> Result<Object, JsValue> {
        let frame = self
            .pipeline
            .next_frame()
            .map_err(|err| JsValue::from_str(&format!("mock RTP failed: {err:?}")))?;
        let object = Object::new();
        Reflect::set(
            &object,
            &JsValue::from_str("width"),
            &JsValue::from_f64(f64::from(frame.width)),
        )?;
        Reflect::set(
            &object,
            &JsValue::from_str("height"),
            &JsValue::from_f64(f64::from(frame.height)),
        )?;
        Reflect::set(
            &object,
            &JsValue::from_str("frameIndex"),
            &JsValue::from_str(&frame.frame_index.to_string()),
        )?;
        Reflect::set(
            &object,
            &JsValue::from_str("timestamp"),
            &JsValue::from_f64(f64::from(frame.timestamp)),
        )?;
        Reflect::set(
            &object,
            &JsValue::from_str("rtpPackets"),
            &JsValue::from_f64(frame.rtp_packets as f64),
        )?;
        Reflect::set(
            &object,
            &JsValue::from_str("rtpBytes"),
            &JsValue::from_f64(frame.rtp_bytes as f64),
        )?;
        Reflect::set(
            &object,
            &JsValue::from_str("rgba"),
            &Uint8Array::from(frame.rgba.as_slice()),
        )?;
        Ok(object)
    }
}

#[wasm_bindgen]
/// No-hardware route runtime backed by the core mock payload pipeline.
///
/// JavaScript can feed synthetic recovered payload bytes, such as RTP packets,
/// and receive the same profiled batch shape used by real RX transfers.
pub struct OpenIpcMockPayloadRuntime {
    runtime: ReceiverRuntime,
    packet_seq: u64,
}

#[wasm_bindgen]
impl OpenIpcMockPayloadRuntime {
    #[wasm_bindgen(constructor)]
    /// Create a mock payload runtime for one channel id.
    pub fn new(channel_id: u32) -> OpenIpcMockPayloadRuntime {
        OpenIpcMockPayloadRuntime {
            runtime: ReceiverRuntime::with_mock_video_route(
                FrameLayout::WithFcs,
                MOCK_VIDEO_ROUTE_ID,
                ChannelId::new(channel_id),
                MOCK_KEY_SLOT,
            ),
            packet_seq: 0,
        }
    }

    #[wasm_bindgen(js_name = setRtpReorderEnabled)]
    /// Enable or disable the RTP reorder buffer used by the video route.
    pub fn set_rtp_reorder_enabled(&mut self, enabled: bool) {
        self.runtime.set_rtp_reorder_enabled(enabled);
    }

    #[wasm_bindgen(
        js_name = pushPayloadProfiled,
        unchecked_return_type = "OpenIpcRxTransferProfile"
    )]
    /// Push one synthetic recovered payload and return the standard RX profile.
    pub fn push_payload_profiled(&mut self, payload: &[u8]) -> Result<Object, JsValue> {
        let total_start = now_ms();
        let pipeline_start = now_ms();
        let batch = self
            .runtime
            .push_mock_payload(
                self.runtime.video_runtime(),
                self.packet_seq,
                payload,
                &ReceiverBatchOptions::default(),
            )
            .map_err(|err| JsValue::from_str(&format!("mock payload rejected: {err}")))?;
        self.packet_seq = self.packet_seq.wrapping_add(1);
        let pipeline_ms = elapsed_ms(pipeline_start);
        profiled_batch_object(
            batch,
            payload.len(),
            0.0,
            pipeline_ms,
            elapsed_ms(total_start),
        )
    }
}

fn profiled_batch_object(
    batch: ReceiverBatch,
    transfer_len: usize,
    parse_ms: f64,
    pipeline_ms: f64,
    total_ms: f64,
) -> Result<Object, JsValue> {
    let counters = batch.counters;
    let frames = frame_objects_array(batch.frames)?;
    let raw_payloads = raw_payload_array(batch.raw_payloads)?;
    let rtp_status = rtp_status_object(batch.rtp_status, batch.rtp_reorder_status)?;

    let object = Object::new();
    Reflect::set(&object, &JsValue::from_str("frames"), &frames)?;
    Reflect::set(&object, &JsValue::from_str("rawPayloads"), &raw_payloads)?;
    Reflect::set(
        &object,
        &JsValue::from_str("mavlinkPayloads"),
        &raw_payloads,
    )?;
    Reflect::set(&object, &JsValue::from_str("rtpStatus"), &rtp_status)?;
    set_number(
        &object,
        "rawPayloadCount",
        counters.raw_payload_count as f64,
    )?;
    set_number(
        &object,
        "rawPayloadBytes",
        counters.raw_payload_bytes as f64,
    )?;
    set_number(&object, "transferBytes", transfer_len as f64)?;
    set_number(&object, "packets", counters.packets.max(1) as f64)?;
    set_number(
        &object,
        "acceptedPackets",
        counters.accepted_packets.max(1) as f64,
    )?;
    set_number(&object, "droppedPackets", counters.dropped_packets as f64)?;
    set_number(&object, "crcDropped", counters.crc_dropped as f64)?;
    set_number(&object, "icvDropped", counters.icv_dropped as f64)?;
    set_number(&object, "reportDropped", counters.report_dropped as f64)?;
    set_number(&object, "ignoredFrames", counters.ignored_frames as f64)?;
    set_number(&object, "sessions", counters.sessions as f64)?;
    set_number(&object, "wfbPayloads", counters.wfb_payloads as f64)?;
    set_number(&object, "rtpPackets", counters.rtp_packets as f64)?;
    set_number(&object, "videoFrames", counters.video_frames as f64)?;
    set_number(
        &object,
        "mavlinkPayloadCount",
        counters.raw_payload_count as f64,
    )?;
    set_number(&object, "mavlinkBytes", counters.raw_payload_bytes as f64)?;
    set_number(&object, "parseMs", parse_ms)?;
    set_number(&object, "pipelineMs", pipeline_ms)?;
    set_number(&object, "totalMs", total_ms)?;
    Ok(object)
}

fn frame_objects_array(frames: Vec<openipc_core::DepacketizedFrame>) -> Result<Array, JsValue> {
    let out = Array::new();
    for frame in frames {
        out.push(&video_frame_object(frame)?.into());
    }
    Ok(out)
}

fn raw_payload_array(payloads: Vec<openipc_core::RoutePayload>) -> Result<Array, JsValue> {
    let out = Array::new();
    for payload in payloads {
        out.push(&raw_payload_object(payload)?.into());
    }
    Ok(out)
}
