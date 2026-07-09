use js_sys::{Array, Uint8Array};
use openipc_core::ieee80211::WifiFrame;
use openipc_core::realtek::{parse_rx_aggregate_with_kind, RxDescriptorKind, RxPacketType};
use openipc_core::{AdaptiveLink, ChannelId, FecCounters, FrameLayout, RadioPort, WfbTxKeypair};
#[cfg(target_arch = "wasm32")]
use openipc_uplink::TxFailureKind;
use openipc_uplink::{TxOutcome, UplinkEngine};
use wasm_bindgen::prelude::*;

use crate::js::{counters_json, escape_json_str, ms_from_js};
use crate::receiver::{parse_rx_descriptor_kind, OpenIpcReceiver};
#[cfg(target_arch = "wasm32")]
use crate::webusb::WebUsbRealtekDevice;

#[wasm_bindgen]
/// Browser/WASM adaptive-link feedback sender.
///
/// The app records RX quality and FEC counters, then calls `tick()` or
/// `tickAndSend()` to produce/send encrypted WFB feedback packets.
pub struct OpenIpcAdaptiveLink {
    link: AdaptiveLink,
    engine: UplinkEngine,
    last_feedback_ms: Option<u64>,
    last_counters: FecCounters,
    rx_channel_id: ChannelId,
    rx_descriptor_kind: RxDescriptorKind,
}

impl OpenIpcAdaptiveLink {
    fn prepare_batch(
        &mut self,
        schedule_ms: u64,
        payload_now_ms: u64,
    ) -> Result<Option<openipc_uplink::TxBatch>, String> {
        if self
            .last_feedback_ms
            .is_none_or(|last| schedule_ms.saturating_sub(last) >= 100)
        {
            let feedback = self.link.feedback_udp_payload(payload_now_ms);
            self.engine
                .send_udp(
                    openipc_core::ADAPTIVE_LINK_GS_PORT,
                    openipc_core::ADAPTIVE_LINK_VTX_PORT,
                    &feedback,
                )
                .map_err(|error| error.to_string())?;
            self.last_feedback_ms = Some(schedule_ms);
        }
        self.engine
            .ready_batch(schedule_ms, usize::MAX)
            .map_err(|error| error.to_string())
    }

    fn frames_due(
        &mut self,
        schedule_ms: u64,
        payload_now_ms: u64,
    ) -> Result<Vec<Vec<u8>>, String> {
        let Some(batch) = self.prepare_batch(schedule_ms, payload_now_ms)? else {
            return Ok(Vec::new());
        };
        self.engine
            .mark_submitted(&batch)
            .map_err(|error| error.to_string())?;
        let frames = batch
            .frames()
            .iter()
            .map(|frame| frame.bytes().to_vec())
            .collect::<Vec<_>>();
        for frame in batch.frames() {
            self.engine
                .report_completion(frame.ticket(), TxOutcome::Completed, schedule_ms)
                .map_err(|error| error.to_string())?;
        }
        Ok(frames)
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn record_rx_paths(&mut self, now_ms: u64, rssi: [u8; 4], snr: [i8; 4]) {
        self.link.record_rx_paths(now_ms, rssi, snr);
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn record_counters(&mut self, now_ms: u64, counters: FecCounters) {
        self.record_counter_delta(now_ms, counters);
    }
}

#[wasm_bindgen]
impl OpenIpcAdaptiveLink {
    #[wasm_bindgen(constructor)]
    /// Create a new adaptive-link sender for a link id and WFB TX keypair.
    pub fn new(
        link_id: u32,
        keypair: &[u8],
        epoch: u64,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<OpenIpcAdaptiveLink, JsValue> {
        let keypair = WfbTxKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid adaptive-link keypair: {err}")))?;
        let engine = UplinkEngine::new(link_id, keypair, epoch, fec_k, fec_n)
            .map_err(|err| JsValue::from_str(&format!("invalid adaptive-link config: {err}")))?;
        Ok(Self {
            link: AdaptiveLink::new(),
            engine,
            last_feedback_ms: None,
            last_counters: FecCounters::default(),
            rx_channel_id: ChannelId::from_link_port(link_id, RadioPort::Video),
            rx_descriptor_kind: RxDescriptorKind::Jaguar1,
        })
    }

    #[wasm_bindgen(js_name = setRxDescriptorKind)]
    /// Select the Realtek USB RX descriptor layout for future RSSI/SNR sampling.
    pub fn set_rx_descriptor_kind(&mut self, kind: &str) -> Result<(), JsValue> {
        self.rx_descriptor_kind = parse_rx_descriptor_kind(kind)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = recordRx)]
    /// Record one pair of RSSI/SNR samples for link-quality estimation.
    pub fn record_rx(&mut self, now_ms: f64, rssi0: u8, rssi1: u8, snr0: i8, snr1: i8) {
        self.link
            .record_rx(ms_from_js(now_ms), rssi0, rssi1, snr0, snr1);
    }

    #[wasm_bindgen(js_name = recordRxTransfer)]
    /// Parse one RX transfer and record RSSI/SNR for matching video frames.
    pub fn record_rx_transfer(&mut self, transfer: &[u8], now_ms: f64) -> Result<(), JsValue> {
        let packets = parse_rx_aggregate_with_kind(transfer, self.rx_descriptor_kind)
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
            self.link
                .record_rx_paths(now_ms, packet.attrib.rssi, packet.attrib.snr);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = recordReceiverCounters)]
    /// Record FEC counter deltas from an [`OpenIpcReceiver`].
    pub fn record_receiver_counters(&mut self, receiver: &OpenIpcReceiver, now_ms: f64) {
        self.record_counter_delta(ms_from_js(now_ms), receiver.video_fec_counters());
    }

    #[wasm_bindgen(js_name = recordFec)]
    /// Record explicit FEC totals for the current time window.
    pub fn record_fec(&mut self, now_ms: f64, total: u32, recovered: u32, lost: u32) {
        self.link
            .record_fec(ms_from_js(now_ms), total, recovered, lost);
    }

    #[wasm_bindgen(js_name = requestKeyframe)]
    /// Force keyframe-request messages in upcoming feedback packets.
    pub fn request_keyframe(&mut self) {
        self.link.request_keyframe();
    }

    #[wasm_bindgen(js_name = setKeyframeRequestMessages)]
    /// Configure how many feedback packets carry a keyframe request.
    pub fn set_keyframe_request_messages(&mut self, messages: u32) {
        self.link.set_keyframe_request_messages(messages);
    }

    #[wasm_bindgen(js_name = setVideoStartIdleMs)]
    /// Configure how long a quiet video stream is considered idle.
    pub fn set_video_start_idle_ms(&mut self, idle_ms: u32) {
        self.link.set_video_start_idle_ms(idle_ms as u64);
    }

    #[wasm_bindgen(js_name = tick)]
    /// Return feedback frames that should be sent at `now_ms`.
    pub fn tick(&mut self, now_ms: f64) -> Result<Array, JsValue> {
        let frames = self
            .frames_due(ms_from_js(crate::js::now_ms()), ms_from_js(now_ms))
            .map_err(|err| JsValue::from_str(&format!("adaptive-link tick failed: {err}")))?;
        let out = Array::new();
        for frame in frames {
            out.push(&Uint8Array::from(frame.as_slice()));
        }
        Ok(out)
    }

    #[wasm_bindgen(js_name = counters)]
    /// Return the last recorded FEC counters as JSON.
    pub fn counters(&self) -> String {
        counters_json(self.last_counters)
    }

    #[wasm_bindgen(js_name = quality)]
    /// Return the current link-quality report as JSON.
    pub fn quality(&mut self, now_ms: f64) -> String {
        let quality = self.link.quality(ms_from_js(now_ms));
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
        self.link.record_fec(
            now_ms,
            total.min(u32::MAX as u64) as u32,
            recovered.min(u32::MAX as u64) as u32,
            lost.min(u32::MAX as u64) as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::OpenIpcAdaptiveLink;

    #[test]
    fn facade_routes_feedback_through_userspace_udp() {
        let mut keypair = vec![3; 32];
        keypair.extend_from_slice(&[9; 32]);
        let mut adaptive = OpenIpcAdaptiveLink::new(0x7505d6, &keypair, 0, 1, 5).unwrap();
        adaptive.set_keyframe_request_messages(0);

        assert_eq!(
            adaptive.frames_due(1_000, 1_000).unwrap().len(),
            6,
            "one WFB session plus five UDP feedback shards"
        );
        assert!(adaptive.frames_due(1_050, 1_050).unwrap().is_empty());
        assert_eq!(adaptive.frames_due(1_100, 1_100).unwrap().len(), 5);
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl OpenIpcAdaptiveLink {
    #[wasm_bindgen(js_name = tickAndSend)]
    /// Produce due feedback frames and send them through a WebUSB Realtek device.
    pub async fn tick_and_send(
        &mut self,
        device: &WebUsbRealtekDevice,
        now_ms: f64,
        current_channel: u8,
    ) -> Result<usize, JsValue> {
        self.tick_and_send_for_radio(device, now_ms, current_channel, 20)
            .await
    }

    #[wasm_bindgen(js_name = tickAndSendForRadio)]
    /// Produce due feedback frames with the adapter's configured RF width.
    pub async fn tick_and_send_for_radio(
        &mut self,
        device: &WebUsbRealtekDevice,
        now_ms: f64,
        current_channel: u8,
        channel_width_mhz: u16,
    ) -> Result<usize, JsValue> {
        let schedule_ms = ms_from_js(crate::js::now_ms());
        let Some(batch) = self
            .prepare_batch(schedule_ms, ms_from_js(now_ms))
            .map_err(|err| JsValue::from_str(&format!("adaptive-link tick failed: {err}")))?
        else {
            return Ok(0);
        };
        self.engine
            .mark_submitted(&batch)
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
        let count = batch.frames().len();
        for (index, frame) in batch.frames().iter().enumerate() {
            let result = device
                .send_packet_for_radio(frame.bytes(), current_channel, channel_width_mhz, false)
                .await;
            let outcome = if result.is_ok() {
                TxOutcome::Completed
            } else {
                TxOutcome::Retryable(TxFailureKind::Other)
            };
            self.engine
                .report_completion(frame.ticket(), outcome, schedule_ms)
                .map_err(|err| JsValue::from_str(&err.to_string()))?;
            if let Err(error) = result {
                for unsent in &batch.frames()[index + 1..] {
                    self.engine
                        .report_completion(
                            unsent.ticket(),
                            TxOutcome::Retryable(TxFailureKind::Other),
                            schedule_ms,
                        )
                        .map_err(|err| JsValue::from_str(&err.to_string()))?;
                }
                return Err(error);
            }
        }
        Ok(count)
    }
}
