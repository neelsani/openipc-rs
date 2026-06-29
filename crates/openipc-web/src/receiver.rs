use js_sys::{Array, Object, Reflect, Uint8Array};
use openipc_core::realtek::{parse_rx_aggregate, RxPacketType};
use openipc_core::{
    ChannelId, FrameLayout, PayloadPipeline, PipelineEvent, RadioPort, ReceiverPipeline, WfbKeypair,
};
use wasm_bindgen::prelude::*;

use crate::js::{
    accept_rx_packet, append_payload_objects, append_video_frame_objects, append_video_frames,
    counters_json, elapsed_ms, now_ms, parse_hex_u64, set_number, video_frames_from_events,
};
use crate::video::video_frame_object;

#[wasm_bindgen]
pub struct OpenIpcReceiver {
    pub(crate) pipeline: ReceiverPipeline,
    mavlink_pipeline: Option<PayloadPipeline>,
}

#[wasm_bindgen]
impl OpenIpcReceiver {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<OpenIpcReceiver, JsValue> {
        Self::with_channel_id(openipc_core::channel::DEFAULT_LINK_ID << 8, 1, 5)
    }

    #[wasm_bindgen(js_name = withChannelId)]
    pub fn with_channel_id(
        channel_id: u32,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<OpenIpcReceiver, JsValue> {
        let pipeline = ReceiverPipeline::new(
            ChannelId::new(channel_id),
            FrameLayout::WithFcs,
            fec_k,
            fec_n,
        )
        .map_err(|err| JsValue::from_str(&format!("invalid receiver config: {err:?}")))?;
        Ok(Self {
            pipeline,
            mavlink_pipeline: None,
        })
    }

    #[wasm_bindgen(js_name = withKeypair)]
    pub fn with_keypair(
        channel_id: u32,
        keypair: &[u8],
        minimum_epoch: u64,
    ) -> Result<OpenIpcReceiver, JsValue> {
        let keypair = WfbKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid WFB keypair: {err}")))?;
        let mavlink_channel_id =
            ChannelId::from_link_port(channel_id >> 8, RadioPort::MavlinkRx).raw();
        openipc_receiver_with_keypair_and_mavlink_channel_inner(
            channel_id,
            mavlink_channel_id,
            keypair,
            minimum_epoch,
        )
    }

    #[wasm_bindgen(js_name = withKeypairAndMavlinkChannel)]
    pub fn with_keypair_and_mavlink_channel(
        channel_id: u32,
        mavlink_channel_id: u32,
        keypair: &[u8],
        minimum_epoch: u64,
    ) -> Result<OpenIpcReceiver, JsValue> {
        let keypair = WfbKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid WFB keypair: {err}")))?;
        openipc_receiver_with_keypair_and_mavlink_channel_inner(
            channel_id,
            mavlink_channel_id,
            keypair,
            minimum_epoch,
        )
    }

    #[wasm_bindgen(js_name = pushRtpPacket)]
    pub fn push_rtp_packet(&mut self, data: &[u8]) -> Option<Uint8Array> {
        self.pipeline
            .push_rtp(data)
            .map(|frame| Uint8Array::from(frame.data.as_slice()))
    }

    #[wasm_bindgen(
        js_name = pushRtpPacketDetailed,
        unchecked_return_type = "OpenIpcVideoFrame | null"
    )]
    pub fn push_rtp_packet_detailed(&mut self, data: &[u8]) -> Result<JsValue, JsValue> {
        match self.pipeline.push_rtp(data) {
            Some(frame) => Ok(video_frame_object(frame)?.into()),
            None => Ok(JsValue::NULL),
        }
    }

    #[wasm_bindgen(js_name = pushDecryptedFragment)]
    pub fn push_decrypted_fragment(
        &mut self,
        data_nonce_hex: &str,
        fragment: &[u8],
    ) -> Result<Array, JsValue> {
        let data_nonce = parse_hex_u64(data_nonce_hex)?;
        let events = self
            .pipeline
            .push_decrypted_fragment(data_nonce, fragment)
            .map_err(|err| JsValue::from_str(&format!("WFB fragment rejected: {err:?}")))?;

        let frames = Array::new();
        for event in events {
            if let PipelineEvent::VideoFrame(frame) = event {
                frames.push(&Uint8Array::from(frame.data.as_slice()));
            }
        }
        Ok(frames)
    }

    #[wasm_bindgen(js_name = pushDecrypted80211Frame)]
    pub fn push_decrypted_80211_frame(
        &mut self,
        frame: &[u8],
        fragment: &[u8],
    ) -> Result<Array, JsValue> {
        let events = self
            .pipeline
            .push_decrypted_80211_frame(frame, fragment)
            .map_err(|err| JsValue::from_str(&format!("802.11 frame rejected: {err:?}")))?;
        let frames = Array::new();
        for event in events {
            if let PipelineEvent::VideoFrame(frame) = event {
                frames.push(&Uint8Array::from(frame.data.as_slice()));
            }
        }
        Ok(frames)
    }

    #[wasm_bindgen(js_name = pushEncrypted80211Frame)]
    pub fn push_encrypted_80211_frame(&mut self, frame: &[u8]) -> Result<Array, JsValue> {
        let events = self
            .pipeline
            .push_80211_frame(frame)
            .map_err(|err| JsValue::from_str(&format!("802.11 frame rejected: {err}")))?;
        Ok(video_frames_from_events(events))
    }

    #[wasm_bindgen(js_name = pushRxTransfer)]
    pub fn push_rx_transfer(&mut self, transfer: &[u8]) -> Result<Array, JsValue> {
        let packets = parse_rx_aggregate(transfer)
            .map_err(|err| JsValue::from_str(&format!("Realtek RX aggregate rejected: {err}")))?;
        let frames = Array::new();
        for packet in packets {
            if packet.attrib.crc_err
                || packet.attrib.icv_err
                || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
            {
                continue;
            }
            let events = self
                .pipeline
                .push_80211_frame(packet.data)
                .map_err(|err| JsValue::from_str(&format!("OpenIPC frame rejected: {err}")))?;
            append_video_frames(&frames, events);
        }
        Ok(frames)
    }

    #[wasm_bindgen(
        js_name = pushRxTransferDetailed,
        unchecked_return_type = "OpenIpcVideoFrame[]"
    )]
    pub fn push_rx_transfer_detailed(&mut self, transfer: &[u8]) -> Result<Array, JsValue> {
        self.push_rx_transfer_detailed_with_options(transfer, false)
    }

    #[wasm_bindgen(
        js_name = pushRxTransferDetailedWithOptions,
        unchecked_return_type = "OpenIpcVideoFrame[]"
    )]
    pub fn push_rx_transfer_detailed_with_options(
        &mut self,
        transfer: &[u8],
        keep_corrupted: bool,
    ) -> Result<Array, JsValue> {
        let packets = parse_rx_aggregate(transfer)
            .map_err(|err| JsValue::from_str(&format!("Realtek RX aggregate rejected: {err}")))?;
        let frames = Array::new();
        for packet in packets {
            if !accept_rx_packet(packet.attrib, keep_corrupted) {
                continue;
            }
            let events = self
                .pipeline
                .push_80211_frame(packet.data)
                .map_err(|err| JsValue::from_str(&format!("OpenIPC frame rejected: {err}")))?;
            append_video_frame_objects(&frames, events)?;
        }
        Ok(frames)
    }

    #[wasm_bindgen(
        js_name = pushRxTransferProfiled,
        unchecked_return_type = "OpenIpcRxTransferProfile"
    )]
    pub fn push_rx_transfer_profiled(&mut self, transfer: &[u8]) -> Result<Object, JsValue> {
        self.push_rx_transfer_profiled_with_options(transfer, false)
    }

    #[wasm_bindgen(
        js_name = pushRxTransferProfiledWithOptions,
        unchecked_return_type = "OpenIpcRxTransferProfile"
    )]
    pub fn push_rx_transfer_profiled_with_options(
        &mut self,
        transfer: &[u8],
        keep_corrupted: bool,
    ) -> Result<Object, JsValue> {
        let total_start = now_ms();
        let parse_start = now_ms();
        let packets = parse_rx_aggregate(transfer)
            .map_err(|err| JsValue::from_str(&format!("Realtek RX aggregate rejected: {err}")))?;
        let parse_ms = elapsed_ms(parse_start);

        let frames = Array::new();
        let mavlink_payloads = Array::new();
        let mut accepted_packets = 0usize;
        let mut crc_dropped = 0usize;
        let mut icv_dropped = 0usize;
        let mut report_dropped = 0usize;
        let mut ignored_frames = 0usize;
        let mut sessions = 0usize;
        let mut wfb_payloads = 0usize;
        let mut rtp_packets = 0usize;
        let mut video_frames = 0usize;
        let mut mavlink_payload_count = 0usize;
        let mut mavlink_bytes = 0usize;

        let pipeline_start = now_ms();
        let packet_count = packets.len();
        for packet in packets {
            if packet.attrib.crc_err && !keep_corrupted {
                crc_dropped += 1;
                continue;
            }
            if packet.attrib.icv_err && !keep_corrupted {
                icv_dropped += 1;
                continue;
            }
            if packet.attrib.pkt_rpt_type != RxPacketType::NormalRx {
                report_dropped += 1;
                continue;
            }
            accepted_packets += 1;
            if let Some(mavlink_pipeline) = self.mavlink_pipeline.as_mut() {
                if let Ok(events) = mavlink_pipeline.push_80211_frame(packet.data) {
                    append_payload_objects(
                        &mavlink_payloads,
                        events,
                        mavlink_pipeline.channel_id(),
                        &mut mavlink_payload_count,
                        &mut mavlink_bytes,
                    )?;
                }
            }
            let events = self
                .pipeline
                .push_80211_frame(packet.data)
                .map_err(|err| JsValue::from_str(&format!("OpenIPC frame rejected: {err}")))?;
            for event in events {
                match event {
                    PipelineEvent::IgnoredFrame => ignored_frames += 1,
                    PipelineEvent::SessionEstablished { .. } => sessions += 1,
                    PipelineEvent::WfbPayload { .. } => wfb_payloads += 1,
                    PipelineEvent::RtpPacket { .. } => rtp_packets += 1,
                    PipelineEvent::VideoFrame(frame) => {
                        video_frames += 1;
                        frames.push(&video_frame_object(frame)?.into());
                    }
                }
            }
        }
        let pipeline_ms = elapsed_ms(pipeline_start);

        let object = Object::new();
        Reflect::set(&object, &JsValue::from_str("frames"), &frames)?;
        Reflect::set(
            &object,
            &JsValue::from_str("mavlinkPayloads"),
            &mavlink_payloads,
        )?;
        set_number(&object, "transferBytes", transfer.len() as f64)?;
        set_number(&object, "packets", packet_count as f64)?;
        set_number(&object, "acceptedPackets", accepted_packets as f64)?;
        set_number(
            &object,
            "droppedPackets",
            (crc_dropped + icv_dropped + report_dropped) as f64,
        )?;
        set_number(&object, "crcDropped", crc_dropped as f64)?;
        set_number(&object, "icvDropped", icv_dropped as f64)?;
        set_number(&object, "reportDropped", report_dropped as f64)?;
        set_number(&object, "ignoredFrames", ignored_frames as f64)?;
        set_number(&object, "sessions", sessions as f64)?;
        set_number(&object, "wfbPayloads", wfb_payloads as f64)?;
        set_number(&object, "rtpPackets", rtp_packets as f64)?;
        set_number(&object, "videoFrames", video_frames as f64)?;
        set_number(&object, "mavlinkPayloadCount", mavlink_payload_count as f64)?;
        set_number(&object, "mavlinkBytes", mavlink_bytes as f64)?;
        set_number(&object, "parseMs", parse_ms)?;
        set_number(&object, "pipelineMs", pipeline_ms)?;
        set_number(&object, "totalMs", elapsed_ms(total_start))?;
        Ok(object)
    }

    #[wasm_bindgen(js_name = fecCounters)]
    pub fn fec_counters(&self) -> String {
        counters_json(self.pipeline.fec_counters())
    }
}

fn openipc_receiver_with_keypair_and_mavlink_channel_inner(
    channel_id: u32,
    mavlink_channel_id: u32,
    keypair: WfbKeypair,
    minimum_epoch: u64,
) -> Result<OpenIpcReceiver, JsValue> {
    let pipeline = ReceiverPipeline::with_keypair(
        ChannelId::new(channel_id),
        FrameLayout::WithFcs,
        keypair,
        minimum_epoch,
    )
    .map_err(|err| JsValue::from_str(&format!("invalid encrypted receiver config: {err}")))?;
    let mavlink_pipeline = PayloadPipeline::with_keypair(
        ChannelId::new(mavlink_channel_id),
        FrameLayout::WithFcs,
        keypair,
        minimum_epoch,
    )
    .map_err(|err| JsValue::from_str(&format!("invalid MAVLink receiver config: {err}")))?;
    Ok(OpenIpcReceiver {
        pipeline,
        mavlink_pipeline: Some(mavlink_pipeline),
    })
}
