use js_sys::{Array, Uint8Array};
use openipc_core::ieee80211::WifiFrame;
use openipc_core::realtek::{parse_rx_aggregate, RxPacketType};
use openipc_core::{
    AdaptiveLinkSender, ChannelId, FecCounters, FrameLayout, RadioPort, WfbTxKeypair,
};
use wasm_bindgen::prelude::*;

use crate::js::{counters_json, escape_json_str, ms_from_js};
use crate::receiver::OpenIpcReceiver;
#[cfg(target_arch = "wasm32")]
use crate::webusb::WebUsbRealtekDevice;

#[wasm_bindgen]
pub struct OpenIpcAdaptiveLink {
    sender: AdaptiveLinkSender,
    last_counters: FecCounters,
    rx_channel_id: ChannelId,
}

#[wasm_bindgen]
impl OpenIpcAdaptiveLink {
    #[wasm_bindgen(constructor)]
    pub fn new(
        link_id: u32,
        keypair: &[u8],
        epoch: u64,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<OpenIpcAdaptiveLink, JsValue> {
        let keypair = WfbTxKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid adaptive-link keypair: {err}")))?;
        let sender = AdaptiveLinkSender::new(link_id, keypair, epoch, fec_k, fec_n)
            .map_err(|err| JsValue::from_str(&format!("invalid adaptive-link config: {err}")))?;
        Ok(Self {
            sender,
            last_counters: FecCounters::default(),
            rx_channel_id: ChannelId::from_link_port(link_id, RadioPort::Video),
        })
    }

    #[wasm_bindgen(js_name = recordRx)]
    pub fn record_rx(&mut self, now_ms: f64, rssi0: u8, rssi1: u8, snr0: i8, snr1: i8) {
        self.sender
            .link_mut()
            .record_rx(ms_from_js(now_ms), rssi0, rssi1, snr0, snr1);
    }

    #[wasm_bindgen(js_name = recordRxTransfer)]
    pub fn record_rx_transfer(&mut self, transfer: &[u8], now_ms: f64) -> Result<(), JsValue> {
        let packets = parse_rx_aggregate(transfer)
            .map_err(|err| JsValue::from_str(&format!("Realtek RX aggregate rejected: {err}")))?;
        let now_ms = ms_from_js(now_ms);
        for packet in packets {
            if packet.attrib.crc_err
                || packet.attrib.icv_err
                || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
            {
                continue;
            }
            if !WifiFrame::parse(packet.data, FrameLayout::WithFcs)
                .map(|frame| frame.matches_channel_id(self.rx_channel_id))
                .unwrap_or(false)
            {
                continue;
            }
            self.sender
                .record_rx_paths(now_ms, packet.attrib.rssi, packet.attrib.snr);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = recordReceiverCounters)]
    pub fn record_receiver_counters(&mut self, receiver: &OpenIpcReceiver, now_ms: f64) {
        self.record_counter_delta(ms_from_js(now_ms), receiver.pipeline.fec_counters());
    }

    #[wasm_bindgen(js_name = recordFec)]
    pub fn record_fec(&mut self, now_ms: f64, total: u32, recovered: u32, lost: u32) {
        self.sender
            .record_fec(ms_from_js(now_ms), total, recovered, lost);
    }

    #[wasm_bindgen(js_name = requestKeyframe)]
    pub fn request_keyframe(&mut self) {
        self.sender.link_mut().request_keyframe();
    }

    #[wasm_bindgen(js_name = setKeyframeRequestMessages)]
    pub fn set_keyframe_request_messages(&mut self, messages: u32) {
        self.sender
            .link_mut()
            .set_keyframe_request_messages(messages);
    }

    #[wasm_bindgen(js_name = setVideoStartIdleMs)]
    pub fn set_video_start_idle_ms(&mut self, idle_ms: u32) {
        self.sender
            .link_mut()
            .set_video_start_idle_ms(idle_ms as u64);
    }

    #[wasm_bindgen(js_name = tick)]
    pub fn tick(&mut self, now_ms: f64) -> Result<Array, JsValue> {
        let frames = self
            .sender
            .tick(ms_from_js(now_ms))
            .map_err(|err| JsValue::from_str(&format!("adaptive-link tick failed: {err}")))?;
        let out = Array::new();
        for frame in frames {
            out.push(&Uint8Array::from(frame.as_slice()));
        }
        Ok(out)
    }

    #[wasm_bindgen(js_name = counters)]
    pub fn counters(&self) -> String {
        counters_json(self.last_counters)
    }

    #[wasm_bindgen(js_name = quality)]
    pub fn quality(&mut self, now_ms: f64) -> String {
        let quality = self.sender.link_mut().quality(ms_from_js(now_ms));
        format!(
            r#"{{"lostLastSecond":{},"recoveredLastSecond":{},"totalLastSecond":{},"rssi":[{},{}],"snr":[{},{}],"linkScore":[{},{}],"idrCode":"{}"}}"#,
            quality.lost_last_second,
            quality.recovered_last_second,
            quality.total_last_second,
            quality.rssi[0],
            quality.rssi[1],
            quality.snr[0],
            quality.snr[1],
            quality.link_score[0],
            quality.link_score[1],
            escape_json_str(&quality.idr_code),
        )
    }

    fn record_counter_delta(&mut self, now_ms: u64, counters: FecCounters) {
        let total = counters
            .total_packets
            .saturating_sub(self.last_counters.total_packets);
        let recovered = counters
            .recovered_packets
            .saturating_sub(self.last_counters.recovered_packets);
        let lost = counters
            .lost_packets
            .saturating_sub(self.last_counters.lost_packets);
        self.last_counters = counters;
        self.sender.record_fec(
            now_ms,
            total.min(u32::MAX as u64) as u32,
            recovered.min(u32::MAX as u64) as u32,
            lost.min(u32::MAX as u64) as u32,
        );
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl OpenIpcAdaptiveLink {
    #[wasm_bindgen(js_name = tickAndSend)]
    pub async fn tick_and_send(
        &mut self,
        device: &WebUsbRealtekDevice,
        now_ms: f64,
        current_channel: u8,
    ) -> Result<usize, JsValue> {
        let frames = self
            .sender
            .tick(ms_from_js(now_ms))
            .map_err(|err| JsValue::from_str(&format!("adaptive-link tick failed: {err}")))?;
        let count = frames.len();
        for frame in frames {
            device.send_packet(&frame, current_channel).await?;
        }
        Ok(count)
    }
}
