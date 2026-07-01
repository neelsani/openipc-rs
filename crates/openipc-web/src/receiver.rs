use js_sys::{Array, Object, Reflect, Uint8Array};
use openipc_core::realtek::{parse_rx_aggregate_with_kind, RxDescriptorKind};
use openipc_core::{
    ChannelId, FrameLayout, PayloadRouteId, RadioPort, ReceiverBatch, ReceiverBatchOptions,
    ReceiverRuntime, RtpPayloadTap, WfbKeypair,
};
use wasm_bindgen::prelude::*;

use crate::js::{
    counters_json, counters_object, elapsed_ms, now_ms, parse_hex_u64, raw_payload_object,
    set_number,
};
use crate::video::{rtp_status_object, video_frame_object};

const VIDEO_ROUTE_ID: PayloadRouteId = PayloadRouteId::new(1);
const TELEMETRY_ROUTE_ID: PayloadRouteId = PayloadRouteId::new(2);
const DEFAULT_KEY_SLOT: u64 = 0;

#[wasm_bindgen]
/// Browser/WASM receiver for OpenIPC RX transfers and RTP packets.
pub struct OpenIpcReceiver {
    pub(crate) runtime: ReceiverRuntime,
    pub(crate) rx_descriptor_kind: RxDescriptorKind,
}

impl OpenIpcReceiver {
    pub(crate) fn video_fec_counters(&self) -> openipc_core::FecCounters {
        self.runtime.video_fec_counters()
    }
}

#[wasm_bindgen]
impl OpenIpcReceiver {
    #[wasm_bindgen(constructor)]
    /// Create a plain/FEC-only receiver for the default OpenIPC video channel.
    pub fn new() -> Result<OpenIpcReceiver, JsValue> {
        Self::with_channel_id(openipc_core::channel::DEFAULT_LINK_ID << 8, 1, 5)
    }

    #[wasm_bindgen(js_name = withChannelId)]
    /// Create a plain/FEC-only receiver for a specific channel id.
    pub fn with_channel_id(
        channel_id: u32,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<OpenIpcReceiver, JsValue> {
        let runtime = ReceiverRuntime::with_plain_video_route(
            FrameLayout::WithFcs,
            VIDEO_ROUTE_ID,
            ChannelId::new(channel_id),
            DEFAULT_KEY_SLOT,
            fec_k,
            fec_n,
        )
        .map_err(|err| JsValue::from_str(&format!("invalid receiver config: {err}")))?;
        Ok(Self {
            runtime,
            rx_descriptor_kind: RxDescriptorKind::Jaguar1,
        })
    }

    #[wasm_bindgen(js_name = withKeypair)]
    /// Create an encrypted WFB receiver and default telemetry downlink tap.
    pub fn with_keypair(
        channel_id: u32,
        keypair: &[u8],
        minimum_epoch: u64,
    ) -> Result<OpenIpcReceiver, JsValue> {
        let keypair = WfbKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid WFB keypair: {err}")))?;
        let telemetry_channel_id =
            ChannelId::from_link_port(channel_id >> 8, RadioPort::TelemetryRx).raw();
        openipc_receiver_with_keypair_and_telemetry_channel_inner(
            channel_id,
            telemetry_channel_id,
            keypair,
            minimum_epoch,
        )
    }

    #[wasm_bindgen(js_name = withKeypairOnly)]
    /// Create an encrypted WFB receiver with only the video route.
    pub fn with_keypair_only(
        channel_id: u32,
        keypair: &[u8],
        minimum_epoch: u64,
    ) -> Result<OpenIpcReceiver, JsValue> {
        let keypair = WfbKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid WFB keypair: {err}")))?;
        let runtime = ReceiverRuntime::with_keyed_video_route(
            FrameLayout::WithFcs,
            VIDEO_ROUTE_ID,
            ChannelId::new(channel_id),
            DEFAULT_KEY_SLOT,
            keypair,
            minimum_epoch,
        )
        .map_err(|err| JsValue::from_str(&format!("invalid encrypted receiver config: {err}")))?;
        Ok(OpenIpcReceiver {
            runtime,
            rx_descriptor_kind: RxDescriptorKind::Jaguar1,
        })
    }

    #[wasm_bindgen(js_name = setRxDescriptorKind)]
    /// Select the Realtek USB RX descriptor layout for future bulk-IN transfers.
    pub fn set_rx_descriptor_kind(&mut self, kind: &str) -> Result<(), JsValue> {
        self.rx_descriptor_kind = parse_rx_descriptor_kind(kind)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = setRtpReorderEnabled)]
    /// Enable or disable the small RTP sequence reorder buffer.
    pub fn set_rtp_reorder_enabled(&mut self, enabled: bool) {
        self.runtime.set_rtp_reorder_enabled(enabled);
    }

    #[wasm_bindgen(js_name = withKeypairAndMavlinkChannel)]
    /// Create an encrypted WFB receiver with an explicit raw telemetry channel.
    ///
    /// This is the historical JS name. New applications should call
    /// `withKeypairAndTelemetryChannel`.
    pub fn with_keypair_and_mavlink_channel(
        channel_id: u32,
        mavlink_channel_id: u32,
        keypair: &[u8],
        minimum_epoch: u64,
    ) -> Result<OpenIpcReceiver, JsValue> {
        Self::with_keypair_and_telemetry_channel(
            channel_id,
            mavlink_channel_id,
            keypair,
            minimum_epoch,
        )
    }

    #[wasm_bindgen(js_name = withKeypairAndTelemetryChannel)]
    /// Create an encrypted WFB receiver with an explicit raw telemetry channel.
    pub fn with_keypair_and_telemetry_channel(
        channel_id: u32,
        telemetry_channel_id: u32,
        keypair: &[u8],
        minimum_epoch: u64,
    ) -> Result<OpenIpcReceiver, JsValue> {
        let keypair = WfbKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid WFB keypair: {err}")))?;
        openipc_receiver_with_keypair_and_telemetry_channel_inner(
            channel_id,
            telemetry_channel_id,
            keypair,
            minimum_epoch,
        )
    }

    #[wasm_bindgen(js_name = pushRtpPacket)]
    /// Push one raw RTP packet and return Annex-B bytes when a frame completes.
    pub fn push_rtp_packet(&mut self, data: &[u8]) -> Option<Uint8Array> {
        self.runtime
            .push_rtp_packet(data)
            .ok()
            .and_then(|mut frames| frames.drain(..).next())
            .map(|frame| Uint8Array::from(frame.data.as_slice()))
    }

    #[wasm_bindgen(
        js_name = pushRtpPacketDetailed,
        unchecked_return_type = "OpenIpcVideoFrame | null"
    )]
    /// Push one RTP packet and return a typed frame object when one completes.
    pub fn push_rtp_packet_detailed(&mut self, data: &[u8]) -> Result<JsValue, JsValue> {
        match self
            .runtime
            .push_rtp_packet(data)
            .ok()
            .and_then(|mut frames| frames.drain(..).next())
        {
            Some(frame) => Ok(video_frame_object(frame)?.into()),
            None => Ok(JsValue::NULL),
        }
    }

    #[wasm_bindgen(js_name = pushDecryptedFragment)]
    /// Push an already-decrypted WFB fragment into the video runtime.
    pub fn push_decrypted_fragment(
        &mut self,
        data_nonce_hex: &str,
        fragment: &[u8],
    ) -> Result<Array, JsValue> {
        let data_nonce = parse_hex_u64(data_nonce_hex)?;
        let batch = self
            .runtime
            .push_decrypted_fragment(
                self.runtime.video_runtime(),
                data_nonce,
                fragment,
                &ReceiverBatchOptions::default(),
            )
            .map_err(|err| JsValue::from_str(&format!("WFB fragment rejected: {err}")))?;
        Ok(frame_bytes_array(batch.frames))
    }

    #[wasm_bindgen(js_name = pushDecrypted80211Frame)]
    /// Push an 802.11 frame with a caller-supplied decrypted WFB fragment.
    pub fn push_decrypted_80211_frame(
        &mut self,
        frame: &[u8],
        fragment: &[u8],
    ) -> Result<Array, JsValue> {
        let batch = self
            .runtime
            .push_decrypted_80211_frame(
                self.runtime.video_runtime(),
                frame,
                fragment,
                &ReceiverBatchOptions::default(),
            )
            .map_err(|err| JsValue::from_str(&format!("802.11 frame rejected: {err}")))?;
        Ok(frame_bytes_array(batch.frames))
    }

    #[wasm_bindgen(js_name = pushEncrypted80211Frame)]
    /// Push one encrypted OpenIPC/WFB 802.11 frame.
    pub fn push_encrypted_80211_frame(&mut self, frame: &[u8]) -> Result<Array, JsValue> {
        let batch = self
            .runtime
            .push_80211_frame(frame, &ReceiverBatchOptions::default())
            .map_err(|err| JsValue::from_str(&format!("802.11 frame rejected: {err}")))?;
        Ok(frame_bytes_array(batch.frames))
    }

    #[wasm_bindgen(js_name = pushRxTransfer)]
    /// Push one Realtek RX USB transfer and return completed Annex-B frames.
    pub fn push_rx_transfer(&mut self, transfer: &[u8]) -> Result<Array, JsValue> {
        let batch = self
            .runtime
            .push_rx_transfer_with_kind(
                transfer,
                self.rx_descriptor_kind,
                &ReceiverBatchOptions::default(),
            )
            .map_err(|err| JsValue::from_str(&format!("Realtek RX aggregate rejected: {err}")))?;
        Ok(frame_bytes_array(batch.frames))
    }

    #[wasm_bindgen(
        js_name = pushRxTransferDetailed,
        unchecked_return_type = "OpenIpcVideoFrame[]"
    )]
    /// Push one RX transfer and return typed frame objects.
    pub fn push_rx_transfer_detailed(&mut self, transfer: &[u8]) -> Result<Array, JsValue> {
        self.push_rx_transfer_detailed_with_options(transfer, false)
    }

    #[wasm_bindgen(
        js_name = pushRxTransferDetailedWithOptions,
        unchecked_return_type = "OpenIpcVideoFrame[]"
    )]
    /// Push one RX transfer with control over CRC/ICV-marked packets.
    pub fn push_rx_transfer_detailed_with_options(
        &mut self,
        transfer: &[u8],
        keep_corrupted: bool,
    ) -> Result<Array, JsValue> {
        let batch = self
            .runtime
            .push_rx_transfer_with_kind(
                transfer,
                self.rx_descriptor_kind,
                &ReceiverBatchOptions {
                    accept_corrupted: keep_corrupted,
                    ..ReceiverBatchOptions::default()
                },
            )
            .map_err(|err| JsValue::from_str(&format!("Realtek RX aggregate rejected: {err}")))?;
        frame_objects_array(batch.frames)
    }

    #[wasm_bindgen(
        js_name = pushRxTransferProfiled,
        unchecked_return_type = "OpenIpcRxTransferProfile"
    )]
    /// Push one RX transfer and return frames plus parser/latency counters.
    pub fn push_rx_transfer_profiled(&mut self, transfer: &[u8]) -> Result<Object, JsValue> {
        self.push_rx_transfer_profiled_with_options(transfer, false)
    }

    #[wasm_bindgen(
        js_name = pushRxTransferProfiledWithOptions,
        unchecked_return_type = "OpenIpcRxTransferProfile"
    )]
    /// Push one RX transfer with profiling and bad-FCS handling options.
    pub fn push_rx_transfer_profiled_with_options(
        &mut self,
        transfer: &[u8],
        keep_corrupted: bool,
    ) -> Result<Object, JsValue> {
        self.push_rx_transfer_profiled_inner(
            transfer,
            keep_corrupted,
            &[TELEMETRY_ROUTE_ID.raw() as u32],
            &[],
        )
    }

    #[wasm_bindgen(
        js_name = pushRxTransferProfiledWithRouteIds,
        unchecked_return_type = "OpenIpcRxTransferProfile"
    )]
    /// Push one RX transfer and copy raw payloads for caller-selected route IDs.
    pub fn push_rx_transfer_profiled_with_route_ids(
        &mut self,
        transfer: &[u8],
        keep_corrupted: bool,
        raw_route_ids: &[u32],
    ) -> Result<Object, JsValue> {
        self.push_rx_transfer_profiled_inner(transfer, keep_corrupted, raw_route_ids, &[])
    }

    #[wasm_bindgen(
        js_name = pushRxTransferProfiledWithRouteIdsAndRtpTaps,
        unchecked_return_type = "OpenIpcRxTransferProfile"
    )]
    /// Push one RX transfer and copy raw payloads plus filtered RTP payload taps.
    pub fn push_rx_transfer_profiled_with_route_ids_and_rtp_taps(
        &mut self,
        transfer: &[u8],
        keep_corrupted: bool,
        raw_route_ids: &[u32],
        rtp_tap_route_ids: &[u32],
        rtp_tap_payload_types: &[u8],
    ) -> Result<Object, JsValue> {
        if rtp_tap_route_ids.len() != rtp_tap_payload_types.len() {
            return Err(JsValue::from_str(
                "RTP tap route id and payload type arrays must have the same length",
            ));
        }
        let rtp_payload_taps = rtp_tap_route_ids
            .iter()
            .zip(rtp_tap_payload_types.iter())
            .map(|(route_id, payload_type)| RtpPayloadTap {
                route_id: PayloadRouteId::new(*route_id as u64),
                payload_type: *payload_type,
            })
            .collect::<Vec<_>>();
        self.push_rx_transfer_profiled_inner(
            transfer,
            keep_corrupted,
            raw_route_ids,
            &rtp_payload_taps,
        )
    }

    #[wasm_bindgen(js_name = addKeyedRoute)]
    /// Add an encrypted raw-payload route to the receiver.
    pub fn add_keyed_route(
        &mut self,
        route_id: u32,
        channel_id: u32,
        keypair: &[u8],
        minimum_epoch: u64,
    ) -> Result<(), JsValue> {
        let keypair = WfbKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid WFB keypair: {err}")))?;
        self.runtime
            .add_keyed_route(
                PayloadRouteId::new(route_id as u64),
                ChannelId::new(channel_id),
                DEFAULT_KEY_SLOT,
                keypair,
                minimum_epoch,
            )
            .map_err(|err| JsValue::from_str(&format!("invalid route config: {err}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = fecCounters)]
    /// Return cumulative video FEC counters as JSON.
    pub fn fec_counters(&self) -> String {
        counters_json(self.video_fec_counters())
    }
}

impl OpenIpcReceiver {
    fn push_rx_transfer_profiled_inner(
        &mut self,
        transfer: &[u8],
        keep_corrupted: bool,
        raw_route_ids: &[u32],
        rtp_payload_taps: &[RtpPayloadTap],
    ) -> Result<Object, JsValue> {
        let total_start = now_ms();
        let parse_start = now_ms();
        let packets = parse_rx_aggregate_with_kind(transfer, self.rx_descriptor_kind)
            .map_err(|err| JsValue::from_str(&format!("Realtek RX aggregate rejected: {err}")))?;
        let parse_ms = elapsed_ms(parse_start);

        let raw_payload_routes = raw_route_ids
            .iter()
            .map(|id| PayloadRouteId::new(*id as u64))
            .collect();
        let pipeline_start = now_ms();
        let batch = self.runtime.push_rx_packets(
            packets,
            &ReceiverBatchOptions {
                accept_corrupted: keep_corrupted,
                raw_payload_routes,
                rtp_payload_taps: rtp_payload_taps.to_vec(),
            },
        );
        let pipeline_ms = elapsed_ms(pipeline_start);
        receiver_profile_object(
            batch,
            transfer.len(),
            parse_ms,
            pipeline_ms,
            elapsed_ms(total_start),
        )
    }
}

pub(crate) fn receiver_profile_object(
    batch: ReceiverBatch,
    transfer_bytes: usize,
    parse_ms: f64,
    pipeline_ms: f64,
    total_ms: f64,
) -> Result<Object, JsValue> {
    let counters = batch.counters;
    let fec_counters = batch.fec_counters;
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
    Reflect::set(
        &object,
        &JsValue::from_str("fecCounters"),
        &counters_object(fec_counters)?.into(),
    )?;
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
    set_number(&object, "transferBytes", transfer_bytes as f64)?;
    set_number(&object, "packets", counters.packets as f64)?;
    set_number(&object, "acceptedPackets", counters.accepted_packets as f64)?;
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

fn openipc_receiver_with_keypair_and_telemetry_channel_inner(
    channel_id: u32,
    telemetry_channel_id: u32,
    keypair: WfbKeypair,
    minimum_epoch: u64,
) -> Result<OpenIpcReceiver, JsValue> {
    let mut runtime = ReceiverRuntime::with_keyed_video_route(
        FrameLayout::WithFcs,
        VIDEO_ROUTE_ID,
        ChannelId::new(channel_id),
        DEFAULT_KEY_SLOT,
        keypair,
        minimum_epoch,
    )
    .map_err(|err| JsValue::from_str(&format!("invalid encrypted receiver config: {err}")))?;
    runtime
        .add_keyed_route(
            TELEMETRY_ROUTE_ID,
            ChannelId::new(telemetry_channel_id),
            DEFAULT_KEY_SLOT,
            keypair,
            minimum_epoch,
        )
        .map_err(|err| JsValue::from_str(&format!("invalid MAVLink receiver config: {err}")))?;
    Ok(OpenIpcReceiver {
        runtime,
        rx_descriptor_kind: RxDescriptorKind::Jaguar1,
    })
}

pub(crate) fn parse_rx_descriptor_kind(kind: &str) -> Result<RxDescriptorKind, JsValue> {
    match kind {
        "jaguar1" | "rtl8812" | "rtl8821" | "rtl8814" => Ok(RxDescriptorKind::Jaguar1),
        "jaguar3" | "rtl8812cu" | "rtl8812eu" | "rtl8822c" | "rtl8822cu" | "rtl8822e"
        | "rtl8822eu" => Ok(RxDescriptorKind::Jaguar3),
        _ => Err(JsValue::from_str(
            "unsupported RX descriptor kind; expected jaguar1 or jaguar3",
        )),
    }
}

fn frame_bytes_array(frames: Vec<openipc_core::DepacketizedFrame>) -> Array {
    let out = Array::new();
    for frame in frames {
        out.push(&Uint8Array::from(frame.data.as_slice()));
    }
    out
}

pub(crate) fn frame_objects_array(
    frames: Vec<openipc_core::DepacketizedFrame>,
) -> Result<Array, JsValue> {
    let out = Array::new();
    for frame in frames {
        out.push(&video_frame_object(frame)?.into());
    }
    Ok(out)
}

pub(crate) fn raw_payload_array(
    payloads: Vec<openipc_core::RoutePayload>,
) -> Result<Array, JsValue> {
    let out = Array::new();
    for payload in payloads {
        out.push(&raw_payload_object(payload)?.into());
    }
    Ok(out)
}
