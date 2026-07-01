use js_sys::Object;
use nusb::transfer::{Bulk, In, TransferError};
use openipc_core::realtek::{parse_rx_aggregate_with_kind, RxPacketType};
use openipc_core::{PayloadRouteId, ReceiverBatchOptions, RtpPayloadTap};
use wasm_bindgen::prelude::*;

use crate::adaptive::OpenIpcAdaptiveLink;
use crate::js::{elapsed_ms, ms_from_js, now_ms, set_number};
use crate::receiver::{receiver_profile_object, OpenIpcReceiver};
use crate::webusb::WebUsbRealtekDevice;

const MAX_RX_TRANSFERS_IN_FLIGHT: usize = 16;

/// Persistent WebUSB receive queue coupled directly to the Rust protocol stack.
///
/// This keeps USB buffers inside WASM, continuously recycles every completed
/// transfer, and only crosses into JavaScript for recovered application data.
#[wasm_bindgen]
pub struct WebUsbReceiverSession {
    endpoint: nusb::Endpoint<Bulk, In>,
    receiver: OpenIpcReceiver,
    options: ReceiverBatchOptions,
    transfer_size: usize,
    in_flight: usize,
    signal_samples: Vec<([u8; 4], [i8; 4])>,
}

#[wasm_bindgen]
impl WebUsbReceiverSession {
    #[wasm_bindgen(js_name = create)]
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        device: &WebUsbRealtekDevice,
        receiver: OpenIpcReceiver,
        transfer_size: usize,
        in_flight: usize,
        keep_corrupted: bool,
        raw_route_ids: &[u32],
        rtp_tap_route_ids: &[u32],
        rtp_tap_payload_types: &[u8],
    ) -> Result<WebUsbReceiverSession, JsValue> {
        if rtp_tap_route_ids.len() != rtp_tap_payload_types.len() {
            return Err(JsValue::from_str(
                "RTP tap route id and payload type arrays must have the same length",
            ));
        }

        let mut endpoint = device
            .driver
            .open_bulk_in_endpoint()
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
        let max_packet = endpoint.max_packet_size().max(1);
        let transfer_size = transfer_size
            .max(max_packet)
            .div_ceil(max_packet)
            .saturating_mul(max_packet);
        let in_flight = in_flight.clamp(1, MAX_RX_TRANSFERS_IN_FLIGHT);
        while endpoint.pending() < in_flight {
            endpoint.submit(endpoint.allocate(transfer_size));
        }

        let options = ReceiverBatchOptions {
            accept_corrupted: keep_corrupted,
            raw_payload_routes: raw_route_ids
                .iter()
                .map(|id| PayloadRouteId::new(u64::from(*id)))
                .collect(),
            rtp_payload_taps: rtp_tap_route_ids
                .iter()
                .zip(rtp_tap_payload_types.iter())
                .map(|(route_id, payload_type)| RtpPayloadTap {
                    route_id: PayloadRouteId::new(u64::from(*route_id)),
                    payload_type: *payload_type,
                })
                .collect(),
        };

        Ok(Self {
            endpoint,
            receiver,
            options,
            transfer_size,
            in_flight,
            signal_samples: Vec::new(),
        })
    }

    #[wasm_bindgen(getter, js_name = pendingTransfers)]
    pub fn pending_transfers(&self) -> usize {
        self.endpoint.pending()
    }

    #[wasm_bindgen(getter, js_name = transferSize)]
    pub fn transfer_size(&self) -> usize {
        self.transfer_size
    }

    #[wasm_bindgen(
        js_name = nextProfile,
        unchecked_return_type = "OpenIpcRxTransferProfile"
    )]
    pub async fn next_profile(&mut self) -> Result<Object, JsValue> {
        self.refill();
        let total_start = now_ms();
        let read_start = now_ms();
        let completion = self.endpoint.next_complete().await;
        let usb_read_ms = elapsed_ms(read_start);
        let actual_len = completion.actual_len;
        let buffer = completion.buffer;

        if let Err(error) = completion.status {
            if error == TransferError::Stall {
                self.endpoint.clear_halt().await.map_err(|err| {
                    JsValue::from_str(&format!("clear bulk-IN halt failed: {err}"))
                })?;
            }
            self.endpoint.submit(buffer);
            return Err(JsValue::from_str(&format!(
                "bulk-IN transfer failed: {error}"
            )));
        }

        let bytes = &buffer[..actual_len];
        let parse_start = now_ms();
        let packets = match parse_rx_aggregate_with_kind(bytes, self.receiver.rx_descriptor_kind) {
            Ok(packets) => packets,
            Err(error) => {
                self.endpoint.submit(buffer);
                return Err(JsValue::from_str(&format!(
                    "Realtek RX aggregate rejected: {error}"
                )));
            }
        };
        let parse_ms = elapsed_ms(parse_start);

        self.signal_samples.clear();
        for packet in &packets {
            if packet.attrib.crc_err
                || packet.attrib.icv_err
                || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
                || !self.receiver.runtime.accepts_video_frame(packet.data)
            {
                continue;
            }
            self.signal_samples
                .push((packet.attrib.rssi, packet.attrib.snr));
        }

        let pipeline_start = now_ms();
        let batch = self
            .receiver
            .runtime
            .push_rx_packets(packets, &self.options);
        let pipeline_ms = elapsed_ms(pipeline_start);

        // No borrowed transfer data remains after push_rx_packets. Recycle the
        // USB buffer before allocating JavaScript frame and telemetry objects.
        self.endpoint.submit(buffer);
        self.refill();

        let profile = receiver_profile_object(
            batch,
            actual_len,
            parse_ms,
            pipeline_ms,
            elapsed_ms(total_start),
        )?;
        set_number(&profile, "usbReadMs", usb_read_ms)?;
        set_number(
            &profile,
            "pendingUsbTransfers",
            self.endpoint.pending() as f64,
        )?;
        Ok(profile)
    }

    #[wasm_bindgen(js_name = recordAdaptive)]
    pub fn record_adaptive(&mut self, adaptive: &mut OpenIpcAdaptiveLink, now_ms: f64) {
        let now_ms = ms_from_js(now_ms);
        for (rssi, snr) in self.signal_samples.drain(..) {
            adaptive.record_rx_paths(now_ms, rssi, snr);
        }
        adaptive.record_counters(now_ms, self.receiver.video_fec_counters());
    }

    fn refill(&mut self) {
        while self.endpoint.pending() < self.in_flight {
            self.endpoint
                .submit(self.endpoint.allocate(self.transfer_size));
        }
    }
}
