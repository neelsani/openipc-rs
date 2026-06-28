use js_sys::{Array, Object, Reflect, Uint8Array};
use openipc_core::ieee80211::WifiFrame;
use openipc_core::realtek::{parse_rx_aggregate, RxPacketType};
#[cfg(target_arch = "wasm32")]
use openipc_core::realtek_tx::RealtekTxOptions;
use openipc_core::{
    AdaptiveLinkSender, ChannelId, Codec, DepacketizedFrame, FecCounters, FrameLayout,
    PipelineEvent, RadioPort, ReceiverPipeline, WfbKeypair, WfbTxKeypair,
};
#[cfg(target_arch = "wasm32")]
use openipc_rtl88xx::{
    ChannelWidth, DriverOptions, FalseAlarmCounters, Firmware8814Mode, InitReport, InitStatus,
    IqkReport, MonitorOptions, PhydmDigState, PhydmWatchdogReport, PowerTrackingReport,
    PowerTrackingState, RadioConfig, RealtekDevice, ThermalBucket,
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

export type OpenIpcRxTransferProfile = {
    frames: OpenIpcVideoFrame[];
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
    parseMs: number;
    pipelineMs: number;
    totalMs: number;
};
"#;

#[wasm_bindgen]
pub struct OpenIpcReceiver {
    pipeline: ReceiverPipeline,
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
        Ok(Self { pipeline })
    }

    #[wasm_bindgen(js_name = withKeypair)]
    pub fn with_keypair(
        channel_id: u32,
        keypair: &[u8],
        minimum_epoch: u64,
    ) -> Result<OpenIpcReceiver, JsValue> {
        let keypair = WfbKeypair::from_bytes(keypair)
            .map_err(|err| JsValue::from_str(&format!("invalid WFB keypair: {err}")))?;
        let pipeline = ReceiverPipeline::with_keypair(
            ChannelId::new(channel_id),
            FrameLayout::WithFcs,
            keypair,
            minimum_epoch,
        )
        .map_err(|err| JsValue::from_str(&format!("invalid encrypted receiver config: {err}")))?;
        Ok(Self { pipeline })
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
        let mut accepted_packets = 0usize;
        let mut crc_dropped = 0usize;
        let mut icv_dropped = 0usize;
        let mut report_dropped = 0usize;
        let mut ignored_frames = 0usize;
        let mut sessions = 0usize;
        let mut wfb_payloads = 0usize;
        let mut rtp_packets = 0usize;
        let mut video_frames = 0usize;

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

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WebUsbRealtekDevice {
    driver: RealtekDevice,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbRealtekDevice {
    #[wasm_bindgen(js_name = fromWebUsbDevice)]
    pub async fn from_web_usb_device(
        device: web_sys::UsbDevice,
    ) -> Result<WebUsbRealtekDevice, JsValue> {
        let driver = RealtekDevice::from_web_usb_device(device)
            .await
            .map_err(driver_error)?;
        Ok(Self { driver })
    }

    #[wasm_bindgen(js_name = fromWebUsbDeviceWithOptions)]
    pub async fn from_web_usb_device_with_options(
        device: web_sys::UsbDevice,
        tx_endpoint_override: i32,
    ) -> Result<WebUsbRealtekDevice, JsValue> {
        Self::from_web_usb_device_advanced(device, tx_endpoint_override, -1, -1).await
    }

    #[wasm_bindgen(js_name = fromWebUsbDeviceAdvanced)]
    pub async fn from_web_usb_device_advanced(
        device: web_sys::UsbDevice,
        tx_endpoint_override: i32,
        target_vendor_id: i32,
        target_product_id: i32,
    ) -> Result<WebUsbRealtekDevice, JsValue> {
        let driver = RealtekDevice::from_web_usb_device_with_options(
            device,
            DriverOptions {
                tx_endpoint_override: optional_u8(tx_endpoint_override, "txEndpointOverride")?,
                target_vendor_id: optional_u16(target_vendor_id, "targetVendorId")?,
                target_product_id: optional_u16(target_product_id, "targetProductId")?,
                ..DriverOptions::default()
            },
        )
        .await
        .map_err(driver_error)?;
        Ok(Self { driver })
    }

    #[wasm_bindgen(js_name = bulkInEndpoint)]
    pub fn bulk_in_endpoint(&self) -> u8 {
        self.driver.bulk_in_ep
    }

    #[wasm_bindgen(js_name = bulkOutEndpoint)]
    pub fn bulk_out_endpoint(&self) -> u8 {
        self.driver.bulk_out_ep
    }

    #[wasm_bindgen(js_name = initializeMonitor)]
    pub async fn initialize_monitor(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
    ) -> Result<String, JsValue> {
        self.initialize_monitor_with_options(channel, channel_width_mhz, channel_offset, false)
            .await
    }

    #[wasm_bindgen(js_name = initializeMonitorWithOptions)]
    pub async fn initialize_monitor_with_options(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
        accept_bad_fcs: bool,
    ) -> Result<String, JsValue> {
        let radio = RadioConfig {
            channel,
            channel_offset,
            channel_width: parse_channel_width(channel_width_mhz)?,
        };
        let report = self
            .driver
            .initialize_monitor_async(radio, accept_bad_fcs)
            .await
            .map_err(driver_error)?;
        Ok(init_report_json(&report))
    }

    #[wasm_bindgen(js_name = initializeMonitorAdvanced)]
    pub async fn initialize_monitor_advanced(
        &self,
        channel: u8,
        channel_width_mhz: u16,
        channel_offset: u8,
        accept_bad_fcs: bool,
        skip_tx_power: bool,
        force_iqk: bool,
        disable_iqk: bool,
        firmware_8814_mode: String,
        firmware_8814_chunk: i32,
    ) -> Result<String, JsValue> {
        let radio = RadioConfig {
            channel,
            channel_offset,
            channel_width: parse_channel_width(channel_width_mhz)?,
        };
        let mode = if firmware_8814_mode.trim().is_empty() {
            Firmware8814Mode::Kernel
        } else {
            Firmware8814Mode::from_env_value(&firmware_8814_mode).ok_or_else(|| {
                JsValue::from_str("firmware8814Mode must be \"kernel\" or \"rtw88\"")
            })?
        };
        let options = MonitorOptions {
            accept_bad_fcs,
            skip_tx_power,
            force_iqk,
            disable_iqk,
            firmware_8814_mode: mode,
            firmware_8814_chunk: optional_usize(firmware_8814_chunk, "firmware8814Chunk")?,
        };
        let report = self
            .driver
            .initialize_monitor_with_options_async(radio, options)
            .await
            .map_err(driver_error)?;
        Ok(init_report_json(&report))
    }

    #[wasm_bindgen(js_name = readRxTransfer)]
    pub async fn read_rx_transfer(&self, length: usize) -> Result<Uint8Array, JsValue> {
        let bytes = self
            .driver
            .read_rx_transfer_async(length)
            .await
            .map_err(driver_error)?;
        Ok(Uint8Array::from(bytes.as_slice()))
    }

    #[wasm_bindgen(js_name = readRxTransfers)]
    pub async fn read_rx_transfers(
        &self,
        length: usize,
        in_flight: usize,
    ) -> Result<Array, JsValue> {
        let transfers = self
            .driver
            .read_rx_transfers_async(length, in_flight)
            .await
            .map_err(driver_error)?;
        let out = Array::new();
        for transfer in transfers {
            out.push(&Uint8Array::from(transfer.as_slice()));
        }
        Ok(out)
    }

    #[wasm_bindgen(js_name = writeTxTransfer)]
    pub async fn write_tx_transfer(&self, transfer: &[u8]) -> Result<usize, JsValue> {
        self.driver
            .write_tx_transfer_async(transfer)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = sendPacket)]
    pub async fn send_packet(
        &self,
        radiotap_packet: &[u8],
        current_channel: u8,
    ) -> Result<usize, JsValue> {
        self.send_packet_with_options(radiotap_packet, current_channel, false)
            .await
    }

    #[wasm_bindgen(js_name = sendPacketWithOptions)]
    pub async fn send_packet_with_options(
        &self,
        radiotap_packet: &[u8],
        current_channel: u8,
        legacy_8812_descriptor: bool,
    ) -> Result<usize, JsValue> {
        let chip = self.driver.probe_chip_async().await.map_err(driver_error)?;
        self.driver
            .send_packet_async(
                radiotap_packet,
                RealtekTxOptions {
                    current_channel,
                    is_8814a: chip.family == openipc_rtl88xx::ChipFamily::Rtl8814,
                    legacy_8812_descriptor,
                    ..RealtekTxOptions::default()
                },
            )
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = setTxPowerOverride)]
    pub async fn set_tx_power_override(
        &self,
        current_channel: u8,
        power: u8,
    ) -> Result<(), JsValue> {
        self.driver
            .set_tx_power_override_async(current_channel, power)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readThermalStatus)]
    pub async fn read_thermal_status(&self) -> Result<String, JsValue> {
        let status = self
            .driver
            .read_thermal_status_async()
            .await
            .map_err(driver_error)?;
        Ok(format!(
            r#"{{"raw":{},"baseline":{},"delta":{},"valid":{},"bucket":"{}"}}"#,
            status.raw,
            status.baseline,
            status.delta,
            status.valid,
            thermal_bucket_name(status.bucket())
        ))
    }

    #[wasm_bindgen(js_name = readQueueDepth8814)]
    pub async fn read_queue_depth_8814(&self) -> Result<String, JsValue> {
        let regs = self
            .driver
            .read_queue_depth_8814_async()
            .await
            .map_err(driver_error)?;
        Ok(format!(
            r#"[{},{},{},{},{}]"#,
            regs[0], regs[1], regs[2], regs[3], regs[4]
        ))
    }

    #[wasm_bindgen(js_name = readBbReg)]
    pub async fn read_bb_reg(&self, register: u16, mask: u32) -> Result<u32, JsValue> {
        self.driver
            .read_bb_reg_async(register, mask)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readBbDbgport)]
    pub async fn read_bb_dbgport(&self, selector: u32) -> Result<String, JsValue> {
        let read = self
            .driver
            .read_bb_dbgport_async(selector)
            .await
            .map_err(driver_error)?;
        Ok(format!(
            r#"{{"selector":{},"value":{},"savedSelector":{},"chipAlive":{}}}"#,
            read.selector, read.value, read.saved_selector, read.chip_alive
        ))
    }

    #[wasm_bindgen(js_name = readFalseAlarmCounters)]
    pub async fn read_false_alarm_counters(&self) -> Result<String, JsValue> {
        let counters = self
            .driver
            .read_false_alarm_counters_async()
            .await
            .map_err(driver_error)?;
        Ok(false_alarm_counters_json(counters))
    }

    #[wasm_bindgen(js_name = runIqk)]
    pub async fn run_iqk(&self, channel: u8) -> Result<String, JsValue> {
        let chip = self.driver.probe_chip_async().await.map_err(driver_error)?;
        let report = self
            .driver
            .run_iqk_async(chip, channel)
            .await
            .map_err(driver_error)?;
        Ok(iqk_report_json(report))
    }

    #[wasm_bindgen(js_name = readRegisterU8)]
    pub async fn read_register_u8(&self, register: u16) -> Result<u8, JsValue> {
        self.driver
            .read_u8_async(register)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = readRegisterU32)]
    pub async fn read_register_u32(&self, register: u16) -> Result<u32, JsValue> {
        self.driver
            .read_u32_async(register)
            .await
            .map_err(driver_error)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WebUsbPhydmWatchdog {
    state: PhydmDigState,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbPhydmWatchdog {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: PhydmDigState::default(),
        }
    }

    #[wasm_bindgen(js_name = tick)]
    pub async fn tick(&mut self, device: &WebUsbRealtekDevice) -> Result<String, JsValue> {
        let report = device
            .driver
            .run_phydm_watchdog_tick_async(&mut self.state)
            .await
            .map_err(driver_error)?;
        Ok(phydm_watchdog_report_json(report))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WebUsbPowerTracking8812 {
    state: PowerTrackingState,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebUsbPowerTracking8812 {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: PowerTrackingState::default(),
        }
    }

    #[wasm_bindgen(js_name = init)]
    pub async fn init(&mut self, device: &WebUsbRealtekDevice) -> Result<(), JsValue> {
        device
            .driver
            .init_power_tracking_8812_async(&mut self.state)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = clear)]
    pub async fn clear(&mut self, device: &WebUsbRealtekDevice) -> Result<(), JsValue> {
        device
            .driver
            .clear_power_tracking_8812_async(&mut self.state)
            .await
            .map_err(driver_error)
    }

    #[wasm_bindgen(js_name = tick)]
    pub async fn tick(
        &mut self,
        device: &WebUsbRealtekDevice,
        channel: u8,
        channel_width_mhz: u16,
    ) -> Result<String, JsValue> {
        let report = device
            .driver
            .tick_power_tracking_8812_async(
                &mut self.state,
                channel,
                parse_channel_width(channel_width_mhz)?,
            )
            .await
            .map_err(driver_error)?;
        Ok(power_tracking_report_json(report))
    }
}

#[cfg(target_arch = "wasm32")]
fn thermal_bucket_name(bucket: ThermalBucket) -> &'static str {
    match bucket {
        ThermalBucket::Unknown => "unknown",
        ThermalBucket::Cool => "cool",
        ThermalBucket::Warm => "warm",
        ThermalBucket::Hot => "hot",
        ThermalBucket::Critical => "critical",
    }
}

#[cfg(target_arch = "wasm32")]
fn false_alarm_counters_json(counters: FalseAlarmCounters) -> String {
    format!(
        r#"{{"ofdmFail":{},"cckFail":{},"ofdmCca":{},"cckCca":{},"cckCrcOk":{},"cckCrcError":{},"ofdmCrcOk":{},"ofdmCrcError":{},"htCrcOk":{},"htCrcError":{},"vhtCrcOk":{},"vhtCrcError":{},"all":{},"ccaAll":{}}}"#,
        counters.cnt_ofdm_fail,
        counters.cnt_cck_fail,
        counters.cnt_ofdm_cca,
        counters.cnt_cck_cca,
        counters.cnt_cck_crc32_ok,
        counters.cnt_cck_crc32_error,
        counters.cnt_ofdm_crc32_ok,
        counters.cnt_ofdm_crc32_error,
        counters.cnt_ht_crc32_ok,
        counters.cnt_ht_crc32_error,
        counters.cnt_vht_crc32_ok,
        counters.cnt_vht_crc32_error,
        counters.cnt_all,
        counters.cnt_cca_all
    )
}

#[cfg(target_arch = "wasm32")]
fn phydm_watchdog_report_json(report: PhydmWatchdogReport) -> String {
    format!(
        r#"{{"previousIgi":{},"currentIgi":{},"counters":{}}}"#,
        report.previous_igi,
        report.current_igi,
        false_alarm_counters_json(report.counters)
    )
}

#[cfg(target_arch = "wasm32")]
fn power_tracking_report_json(report: PowerTrackingReport) -> String {
    format!(
        r#"{{"enabled":{},"thermalRaw":{},"thermalAverage":{},"eepromThermal":{},"delta":{},"defaultOfdmIndex":{},"finalOfdmIndex":[{},{}],"swingDelta":[{},{}],"applied":{}}}"#,
        report.enabled,
        report.thermal_raw,
        report.thermal_average,
        report.eeprom_thermal,
        report.delta,
        report.default_ofdm_index,
        report.final_ofdm_index[0],
        report.final_ofdm_index[1],
        report.swing_delta[0],
        report.swing_delta[1],
        report.applied
    )
}

#[cfg(target_arch = "wasm32")]
fn iqk_report_json(report: IqkReport) -> String {
    format!(
        r#"{{"chip":"{}","channel":{},"ran":{}}}"#,
        report.chip.family.name(),
        report.channel,
        report.ran
    )
}

#[cfg(target_arch = "wasm32")]
fn parse_channel_width(width_mhz: u16) -> Result<ChannelWidth, JsValue> {
    match width_mhz {
        20 => Ok(ChannelWidth::Mhz20),
        40 => Ok(ChannelWidth::Mhz40),
        80 => Ok(ChannelWidth::Mhz80),
        _ => Err(JsValue::from_str(
            "unsupported channel width; expected 20, 40, or 80 MHz",
        )),
    }
}

#[cfg(target_arch = "wasm32")]
fn optional_u8(value: i32, name: &str) -> Result<Option<u8>, JsValue> {
    if value < 0 {
        return Ok(None);
    }
    u8::try_from(value)
        .map(Some)
        .map_err(|_| JsValue::from_str(&format!("{name} is outside 0..255")))
}

#[cfg(target_arch = "wasm32")]
fn optional_u16(value: i32, name: &str) -> Result<Option<u16>, JsValue> {
    if value < 0 {
        return Ok(None);
    }
    u16::try_from(value)
        .map(Some)
        .map_err(|_| JsValue::from_str(&format!("{name} is outside 0..65535")))
}

#[cfg(target_arch = "wasm32")]
fn optional_usize(value: i32, name: &str) -> Result<Option<usize>, JsValue> {
    if value < 0 {
        return Ok(None);
    }
    usize::try_from(value)
        .map(Some)
        .map_err(|_| JsValue::from_str(&format!("{name} is invalid")))
}

#[cfg(target_arch = "wasm32")]
fn init_report_json(report: &InitReport) -> String {
    let status = match report.status {
        InitStatus::AlreadyRunning => "already_running",
        InitStatus::Initialized => "initialized",
    };
    format!(
        r#"{{"chip":"{}","rfPaths":{},"cutVersion":{},"status":"{}","firmwareDownloaded":{}}}"#,
        report.chip.family.name(),
        report.chip.total_rf_paths(),
        report.chip.cut_version,
        status,
        report.firmware_downloaded
    )
}

#[cfg(target_arch = "wasm32")]
fn driver_error(err: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&err.to_string())
}

fn video_frames_from_events(events: Vec<PipelineEvent>) -> Array {
    let frames = Array::new();
    append_video_frames(&frames, events);
    frames
}

fn append_video_frames(frames: &Array, events: Vec<PipelineEvent>) {
    for event in events {
        if let PipelineEvent::VideoFrame(frame) = event {
            frames.push(&Uint8Array::from(frame.data.as_slice()));
        }
    }
}

fn append_video_frame_objects(frames: &Array, events: Vec<PipelineEvent>) -> Result<(), JsValue> {
    for event in events {
        if let PipelineEvent::VideoFrame(frame) = event {
            frames.push(&video_frame_object(frame)?.into());
        }
    }
    Ok(())
}

fn accept_rx_packet(attrib: openipc_core::realtek::RxPacketAttrib, keep_corrupted: bool) -> bool {
    attrib.pkt_rpt_type == RxPacketType::NormalRx
        && (keep_corrupted || (!attrib.crc_err && !attrib.icv_err))
}

fn video_frame_object(frame: DepacketizedFrame) -> Result<Object, JsValue> {
    let object = Object::new();
    let codec_string = codec_string(&frame);
    Reflect::set(
        &object,
        &JsValue::from_str("data"),
        &Uint8Array::from(frame.data.as_slice()),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("codec"),
        &JsValue::from_str(codec_name(frame.codec)),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("codecString"),
        &JsValue::from_str(&codec_string),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("isKeyFrame"),
        &JsValue::from_bool(frame.is_keyframe),
    )?;
    Reflect::set(
        &object,
        &JsValue::from_str("timestamp"),
        &JsValue::from_f64(f64::from(frame.timestamp)),
    )?;
    Ok(object)
}

fn codec_name(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "h264",
        Codec::H265 => "h265",
    }
}

fn codec_string(frame: &DepacketizedFrame) -> String {
    match frame.codec {
        Codec::H264 => h264_codec_string(&frame.data).unwrap_or_else(|| "avc1.42E01E".to_owned()),
        Codec::H265 => "hev1.1.6.L93.B0".to_owned(),
    }
}

fn h264_codec_string(frame: &[u8]) -> Option<String> {
    for unit in annex_b_units(frame) {
        let nalu = &frame[unit.start..unit.end];
        if nalu.len() >= 4 && nalu[0] & 0x1f == 7 {
            return Some(format!(
                "avc1.{}{}{}",
                hex_byte(nalu[1]),
                hex_byte(nalu[2]),
                hex_byte(nalu[3])
            ));
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct AnnexBUnit {
    start: usize,
    end: usize,
}

fn annex_b_units(frame: &[u8]) -> Vec<AnnexBUnit> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 < frame.len() {
        let len = start_code_len(frame, index);
        if len > 0 {
            starts.push(index);
            index += len;
        } else {
            index += 1;
        }
    }
    if starts.is_empty() && !frame.is_empty() {
        return vec![AnnexBUnit {
            start: 0,
            end: frame.len(),
        }];
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| AnnexBUnit {
            start: start + start_code_len(frame, *start),
            end: starts.get(index + 1).copied().unwrap_or(frame.len()),
        })
        .collect()
}

fn start_code_len(frame: &[u8], offset: usize) -> usize {
    if frame.get(offset) != Some(&0) || frame.get(offset + 1) != Some(&0) {
        return 0;
    }
    if frame.get(offset + 2) == Some(&1) {
        return 3;
    }
    if frame.get(offset + 2) == Some(&0) && frame.get(offset + 3) == Some(&1) {
        return 4;
    }
    0
}

fn hex_byte(value: u8) -> String {
    format!("{value:02X}")
}

fn set_number(object: &Object, key: &str, value: f64) -> Result<(), JsValue> {
    Reflect::set(object, &JsValue::from_str(key), &JsValue::from_f64(value))?;
    Ok(())
}

fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.performance())
            .map(|performance| performance.now())
            .unwrap_or_else(js_sys::Date::now)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}

fn elapsed_ms(start_ms: f64) -> f64 {
    let elapsed = now_ms() - start_ms;
    if elapsed.is_finite() && elapsed >= 0.0 {
        elapsed
    } else {
        0.0
    }
}

fn counters_json(counters: FecCounters) -> String {
    format!(
        r#"{{"totalPackets":{},"recoveredPackets":{},"lostPackets":{},"badPackets":{}}}"#,
        counters.total_packets,
        counters.recovered_packets,
        counters.lost_packets,
        counters.bad_packets
    )
}

fn escape_json_str(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn ms_from_js(now_ms: f64) -> u64 {
    if now_ms.is_finite() && now_ms > 0.0 {
        now_ms.min(u64::MAX as f64) as u64
    } else {
        0
    }
}

#[wasm_bindgen(js_name = supportedUsbFilters)]
pub fn supported_usb_filters() -> String {
    // Kept as JSON to avoid forcing web-sys types into the Rust API.
    r#"[{"vendorId":3034,"productId":34834},{"vendorId":3034,"productId":2065},{"vendorId":3034,"productId":43025},{"vendorId":3034,"productId":47121},{"vendorId":3034,"productId":34835},{"vendorId":9047,"productId":288}]"#.to_owned()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = listAuthorizedUsbDevices)]
pub async fn list_authorized_usb_devices() -> Result<Array, JsValue> {
    let devices = nusb::list_devices()
        .await
        .map_err(|err| JsValue::from_str(&format!("nusb list_devices failed: {err}")))?;

    let out = Array::new();
    for device in devices {
        let obj = Object::new();
        Reflect::set(
            &obj,
            &JsValue::from_str("vendorId"),
            &JsValue::from_f64(device.vendor_id() as f64),
        )?;
        Reflect::set(
            &obj,
            &JsValue::from_str("productId"),
            &JsValue::from_f64(device.product_id() as f64),
        )?;
        if let Some(product) = device.product_string() {
            Reflect::set(
                &obj,
                &JsValue::from_str("product"),
                &JsValue::from_str(product),
            )?;
        }
        if let Some(manufacturer) = device.manufacturer_string() {
            Reflect::set(
                &obj,
                &JsValue::from_str("manufacturer"),
                &JsValue::from_str(manufacturer),
            )?;
        }
        out.push(&obj);
    }
    Ok(out)
}

fn parse_hex_u64(input: &str) -> Result<u64, JsValue> {
    let trimmed = input.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u64::from_str_radix(hex, 16)
        .map_err(|err| JsValue::from_str(&format!("invalid nonce hex: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h264_codec_string_comes_from_annex_b_sps() {
        let frame = [0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f, 0xac, 0xd9];
        assert_eq!(h264_codec_string(&frame).as_deref(), Some("avc1.64001F"));
    }
}
