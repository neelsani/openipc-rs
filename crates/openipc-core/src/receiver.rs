use crate::channel::ChannelId;
use crate::ieee80211::{FrameLayout, WifiFrame};
use crate::realtek::{
    parse_rx_aggregate, parse_rx_aggregate_with_kind, AggregateError, RealtekRxPacket,
    RxDescriptorKind, RxPacketType,
};
use crate::routes::{
    PayloadRouteError, PayloadRouteEvent, PayloadRouteId, PayloadRouteManager, PayloadRuntimeKey,
};
use crate::rtp::{
    DepacketizedFrame, RtpDepacketizer, RtpDepacketizerStatus, RtpHeader, RtpReorderBuffer,
    RtpReorderStatus,
};
use crate::wfb::{FecCounters, WfbKeypair};

/// Shared receive runtime for OpenIPC video plus optional raw payload taps.
///
/// This is the easiest core entry point for apps. It accepts Realtek RX
/// transfers, 802.11 frames, or already-decrypted fragments; routes recovered
/// WFB payloads by route id; and depacketizes the configured video route from
/// RTP into Annex-B H.264/H.265 frames.
#[derive(Debug, Clone)]
pub struct ReceiverRuntime {
    routes: PayloadRouteManager,
    video_runtime: PayloadRuntimeKey,
    video_route_id: PayloadRouteId,
    rtp: RtpDepacketizer,
    rtp_reorder: Option<RtpReorderBuffer>,
}

/// Options that control how one receive batch is processed.
#[derive(Debug, Clone)]
pub struct ReceiverBatchOptions {
    /// Keep CRC/ICV-marked packets instead of dropping them before WFB parsing.
    pub accept_corrupted: bool,
    /// Route ids whose recovered payload bytes should be copied into the batch.
    pub raw_payload_routes: Vec<PayloadRouteId>,
    /// RTP payload-type filters whose matching packets should be copied.
    pub rtp_payload_taps: Vec<RtpPayloadTap>,
    /// Depacketize the configured video route into Annex-B access units.
    ///
    /// Disable this when an application transfers recovered RTP to another
    /// execution context, such as a browser decode worker. Raw route taps are
    /// still produced while this is disabled.
    pub depacketize_video: bool,
}

impl Default for ReceiverBatchOptions {
    fn default() -> Self {
        Self {
            accept_corrupted: false,
            raw_payload_routes: Vec::new(),
            rtp_payload_taps: Vec::new(),
            depacketize_video: true,
        }
    }
}

/// Filter for copying RTP packets from a recovered route.
///
/// This is useful for mixed media routes. For example, OpenIPC can carry Opus
/// audio as RTP payload type 98 on the same WFB route as video. A tap lets an
/// app copy only those audio RTP packets while the built-in video depacketizer
/// continues to consume the same route for H.264/H.265.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpPayloadTap {
    /// Application route id whose recovered payloads should be inspected.
    pub route_id: PayloadRouteId,
    /// RTP payload type to copy.
    pub payload_type: u8,
}

/// Recovered WFB payload bytes copied for an application route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePayload {
    /// Application-defined route id that requested this payload tap.
    pub route_id: PayloadRouteId,
    /// WFB channel id that carried the payload.
    pub channel_id: ChannelId,
    /// Recovered WFB packet sequence number.
    pub packet_seq: u64,
    /// Raw recovered payload bytes.
    pub data: Vec<u8>,
}

/// Counters collected while processing one receive batch.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverBatchCounters {
    /// Realtek RX packets seen in the batch.
    pub packets: usize,
    /// Packets accepted after Realtek descriptor filtering.
    pub accepted_packets: usize,
    /// Structurally valid IEEE 802.11 frames parsed from accepted packets.
    pub wifi_frames: usize,
    /// Parsed frames whose channel id matched at least one configured runtime.
    pub matched_frames: usize,
    /// Accepted packets whose IEEE 802.11 frame could not be parsed.
    pub wifi_parse_dropped: usize,
    /// Packets dropped by descriptor filtering.
    pub dropped_packets: usize,
    /// Packets dropped because the Realtek descriptor reported a CRC error.
    pub crc_dropped: usize,
    /// Packets dropped because the Realtek descriptor reported an ICV error.
    pub icv_dropped: usize,
    /// Packets dropped because they were not normal RX packets.
    pub report_dropped: usize,
    /// 802.11 frames that did not match any configured route or payload shape.
    pub ignored_frames: usize,
    /// WFB session packets accepted by configured routes.
    pub sessions: usize,
    /// Recovered payloads on the configured video route.
    pub wfb_payloads: usize,
    /// RTP packets observed on the configured video route.
    pub rtp_packets: usize,
    /// Annex-B frames emitted by the RTP depacketizer.
    pub video_frames: usize,
    /// Raw payload copies emitted for routes in [`ReceiverBatchOptions`].
    pub raw_payload_count: usize,
    /// Total bytes copied into raw payload outputs.
    pub raw_payload_bytes: usize,
    /// Route-manager errors treated as dropped/ignored frames.
    pub route_errors: usize,
}

/// Output produced from one transfer, packet list, frame, or fragment push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverBatch {
    /// Encoded Annex-B video frames from the configured video route.
    pub frames: Vec<DepacketizedFrame>,
    /// Raw payload bytes for requested route taps.
    pub raw_payloads: Vec<RoutePayload>,
    /// Per-batch parser and routing counters.
    pub counters: ReceiverBatchCounters,
    /// Current cumulative FEC counters for the video runtime.
    pub fec_counters: FecCounters,
    /// Current cumulative RTP depacketizer diagnostics.
    pub rtp_status: RtpDepacketizerStatus,
    /// Current RTP reorder-buffer diagnostics.
    pub rtp_reorder_status: RtpReorderStatus,
    /// First WFB/route error observed in this batch, if any.
    pub route_error: Option<String>,
}

impl ReceiverRuntime {
    /// Build a runtime around an existing route manager.
    ///
    /// `video_runtime` and `video_route_id` identify the route whose recovered
    /// payloads are RTP video and should be depacketized into frames.
    pub fn from_routes(
        routes: PayloadRouteManager,
        video_runtime: PayloadRuntimeKey,
        video_route_id: PayloadRouteId,
    ) -> Self {
        Self {
            routes,
            video_runtime,
            video_route_id,
            rtp: RtpDepacketizer::new(),
            rtp_reorder: None,
        }
    }

    /// Create a runtime with an unencrypted/plain video route.
    ///
    /// This is mainly useful for tests and pre-decrypted captures.
    pub fn with_plain_video_route(
        frame_layout: FrameLayout,
        video_route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<Self, PayloadRouteError> {
        let mut routes = PayloadRouteManager::new(frame_layout);
        let video_runtime =
            routes.add_plain_route(video_route_id, channel_id, key_slot, fec_k, fec_n)?;
        Ok(Self::from_routes(routes, video_runtime, video_route_id))
    }

    /// Create a runtime with an encrypted WFB video route.
    pub fn with_keyed_video_route(
        frame_layout: FrameLayout,
        video_route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
        keypair: WfbKeypair,
        minimum_epoch: u64,
    ) -> Result<Self, PayloadRouteError> {
        let mut routes = PayloadRouteManager::new(frame_layout);
        let video_runtime =
            routes.add_keyed_route(video_route_id, channel_id, key_slot, keypair, minimum_epoch)?;
        Ok(Self::from_routes(routes, video_runtime, video_route_id))
    }

    /// Create a runtime whose video route accepts already-recovered payloads.
    ///
    /// Use [`Self::push_direct_payload`] to inject RTP or other recovered
    /// payload bytes. They still pass through route fanout and the built-in RTP
    /// depacketizer, making this suitable for UDP input and no-hardware tests.
    pub fn with_direct_video_route(
        frame_layout: FrameLayout,
        video_route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
    ) -> Self {
        let mut routes = PayloadRouteManager::new(frame_layout);
        let video_runtime = routes.add_direct_route(video_route_id, channel_id, key_slot);
        Self::from_routes(routes, video_runtime, video_route_id)
    }

    /// Create a synthetic video payload route for tests and development.
    ///
    /// This is an alias for [`Self::with_direct_video_route`].
    pub fn with_mock_video_route(
        frame_layout: FrameLayout,
        video_route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
    ) -> Self {
        Self::with_direct_video_route(frame_layout, video_route_id, channel_id, key_slot)
    }

    /// Return the route-manager runtime key used for video.
    pub const fn video_runtime(&self) -> PayloadRuntimeKey {
        self.video_runtime
    }

    /// Return the application route id used for video.
    pub const fn video_route_id(&self) -> PayloadRouteId {
        self.video_route_id
    }

    /// Borrow the underlying route manager.
    pub fn routes(&self) -> &PayloadRouteManager {
        &self.routes
    }

    /// Mutably borrow the underlying route manager.
    pub fn routes_mut(&mut self) -> &mut PayloadRouteManager {
        &mut self.routes
    }

    /// Mutably borrow the RTP depacketizer for advanced video handling.
    pub fn rtp_mut(&mut self) -> &mut RtpDepacketizer {
        &mut self.rtp
    }

    /// Return cumulative RTP depacketizer diagnostics for the video route.
    pub fn rtp_status(&self) -> RtpDepacketizerStatus {
        self.rtp.status()
    }

    /// Return cumulative RTP reorder-buffer diagnostics for the video route.
    pub fn rtp_reorder_status(&self) -> RtpReorderStatus {
        self.rtp_reorder
            .as_ref()
            .map(RtpReorderBuffer::status)
            .unwrap_or_default()
    }

    /// Enable or disable the small RTP sequence reorder buffer.
    ///
    /// Reordering can improve startup and fragmented-frame recovery on jittery
    /// links, but it may add a tiny amount of latency when packets arrive out
    /// of order. It is disabled by default for the lowest-latency path.
    pub fn set_rtp_reorder_enabled(&mut self, enabled: bool) {
        if enabled {
            self.rtp_reorder
                .get_or_insert_with(RtpReorderBuffer::default);
        } else {
            self.rtp_reorder = None;
        }
    }

    /// Return true when RTP packets pass through the reorder buffer.
    pub const fn rtp_reorder_enabled(&self) -> bool {
        self.rtp_reorder.is_some()
    }

    /// Process one raw RTP packet on the configured video route.
    pub fn push_rtp_packet(
        &mut self,
        packet: &[u8],
    ) -> Result<Vec<DepacketizedFrame>, crate::rtp::RtpError> {
        let mut frames = Vec::new();
        self.push_video_payload_into(packet, &mut frames)?;
        Ok(frames)
    }

    fn push_video_payload_into(
        &mut self,
        payload: &[u8],
        frames: &mut Vec<DepacketizedFrame>,
    ) -> Result<usize, crate::rtp::RtpError> {
        let before = frames.len();
        if let Some(reorder) = self.rtp_reorder.as_mut() {
            for ordered in reorder.push(payload)? {
                if let Some(frame) = self.rtp.push(&ordered)? {
                    frames.push(frame);
                }
            }
        } else if let Some(frame) = self.rtp.push(payload)? {
            frames.push(frame);
        }
        Ok(frames.len() - before)
    }

    /// Add an unencrypted/plain raw-payload route.
    pub fn add_plain_route(
        &mut self,
        route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<PayloadRuntimeKey, PayloadRouteError> {
        self.routes
            .add_plain_route(route_id, channel_id, key_slot, fec_k, fec_n)
    }

    /// Add an encrypted WFB raw-payload route.
    pub fn add_keyed_route(
        &mut self,
        route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
        keypair: WfbKeypair,
        minimum_epoch: u64,
    ) -> Result<PayloadRuntimeKey, PayloadRouteError> {
        self.routes
            .add_keyed_route(route_id, channel_id, key_slot, keypair, minimum_epoch)
    }

    /// Add a route that accepts already-recovered payloads directly.
    pub fn add_direct_route(
        &mut self,
        route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
    ) -> PayloadRuntimeKey {
        self.routes.add_direct_route(route_id, channel_id, key_slot)
    }

    /// Add a synthetic raw-payload route for tests and development.
    ///
    /// This is an alias for [`Self::add_direct_route`].
    pub fn add_mock_route(
        &mut self,
        route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
    ) -> PayloadRuntimeKey {
        self.add_direct_route(route_id, channel_id, key_slot)
    }

    /// Return cumulative FEC counters for the video runtime.
    pub fn video_fec_counters(&self) -> FecCounters {
        self.routes
            .fec_counters(self.video_runtime)
            .unwrap_or_default()
    }

    /// Return true if an 802.11 frame belongs to the configured video runtime.
    pub fn accepts_video_frame(&self, frame: &[u8]) -> bool {
        self.routes.accepts_80211_frame(self.video_runtime, frame)
    }

    /// Parse and process one Realtek USB RX transfer.
    pub fn push_rx_transfer(
        &mut self,
        transfer: &[u8],
        options: &ReceiverBatchOptions,
    ) -> Result<ReceiverBatch, AggregateError> {
        let packets = parse_rx_aggregate(transfer)?;
        Ok(self.push_rx_packets(packets, options))
    }

    /// Parse and process one Realtek USB RX transfer with an explicit descriptor layout.
    ///
    /// Use the layout reported by the hardware driver for Jaguar3 adapters.
    /// [`Self::push_rx_transfer`] remains the Jaguar1-compatible convenience method.
    pub fn push_rx_transfer_with_kind(
        &mut self,
        transfer: &[u8],
        descriptor_kind: RxDescriptorKind,
        options: &ReceiverBatchOptions,
    ) -> Result<ReceiverBatch, AggregateError> {
        let packets = parse_rx_aggregate_with_kind(transfer, descriptor_kind)?;
        Ok(self.push_rx_packets(packets, options))
    }

    /// Process already parsed Realtek RX packets.
    pub fn push_rx_packets<'a>(
        &mut self,
        packets: impl IntoIterator<Item = RealtekRxPacket<'a>>,
        options: &ReceiverBatchOptions,
    ) -> ReceiverBatch {
        let mut batch = self.empty_batch();
        let mut route_events = Vec::new();

        for packet in packets {
            let parsed = if packet.attrib.crc_err
                || packet.attrib.icv_err
                || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
            {
                None
            } else {
                WifiFrame::parse(packet.data, self.routes.frame_layout()).ok()
            };
            self.push_rx_packet(packet, parsed, options, &mut route_events, &mut batch);
        }

        self.finish_rx_batch(batch)
    }

    /// Process RX packets accompanied by an already-validated WiFi view.
    ///
    /// This avoids reparsing frames that an application already inspected for
    /// diversity selection or signal accounting.
    pub fn push_parsed_rx_packets<'a>(
        &mut self,
        packets: impl IntoIterator<Item = (RealtekRxPacket<'a>, Option<WifiFrame<'a>>)>,
        options: &ReceiverBatchOptions,
    ) -> ReceiverBatch {
        let mut batch = self.empty_batch();
        let mut route_events = Vec::new();
        for (packet, parsed) in packets {
            self.push_rx_packet(packet, parsed, options, &mut route_events, &mut batch);
        }
        self.finish_rx_batch(batch)
    }

    fn push_rx_packet(
        &mut self,
        packet: RealtekRxPacket<'_>,
        parsed: Option<WifiFrame<'_>>,
        options: &ReceiverBatchOptions,
        route_events: &mut Vec<PayloadRouteEvent>,
        batch: &mut ReceiverBatch,
    ) {
        batch.counters.packets += 1;
        if packet.attrib.crc_err && !options.accept_corrupted {
            batch.counters.crc_dropped += 1;
            return;
        }
        if packet.attrib.icv_err && !options.accept_corrupted {
            batch.counters.icv_dropped += 1;
            return;
        }
        if packet.attrib.pkt_rpt_type != RxPacketType::NormalRx {
            batch.counters.report_dropped += 1;
            return;
        }

        batch.counters.accepted_packets += 1;
        let Some(parsed) = parsed else {
            batch.counters.wifi_parse_dropped += 1;
            batch.counters.ignored_frames += 1;
            return;
        };
        batch.counters.wifi_frames += 1;
        if parsed
            .channel_id()
            .is_some_and(|channel_id| self.routes.accepts_channel_id(channel_id))
        {
            batch.counters.matched_frames += 1;
        }
        match self.routes.push_wifi_frame_into(parsed, route_events) {
            Ok(()) => self.apply_route_events(route_events.drain(..), options, batch),
            Err(error) => {
                batch.counters.ignored_frames += 1;
                batch.counters.route_errors += 1;
                if batch.route_error.is_none() {
                    batch.route_error = Some(error.to_string());
                }
            }
        }
    }

    fn finish_rx_batch(&self, mut batch: ReceiverBatch) -> ReceiverBatch {
        batch.counters.dropped_packets =
            batch.counters.crc_dropped + batch.counters.icv_dropped + batch.counters.report_dropped;
        batch.fec_counters = self.video_fec_counters();
        batch
    }

    /// Process one OpenIPC/WFB 802.11 frame.
    pub fn push_80211_frame(
        &mut self,
        frame: &[u8],
        options: &ReceiverBatchOptions,
    ) -> Result<ReceiverBatch, PayloadRouteError> {
        let mut batch = self.empty_batch();
        let events = self.routes.push_80211_frame(frame)?;
        self.apply_route_events(events, options, &mut batch);
        batch.fec_counters = self.video_fec_counters();
        Ok(batch)
    }

    /// Process one 802.11 frame when the caller already decrypted the WFB fragment.
    pub fn push_decrypted_80211_frame(
        &mut self,
        runtime: PayloadRuntimeKey,
        frame: &[u8],
        decrypted_fragment: &[u8],
        options: &ReceiverBatchOptions,
    ) -> Result<ReceiverBatch, PayloadRouteError> {
        let mut batch = self.empty_batch();
        let events = self
            .routes
            .push_decrypted_80211_frame(runtime, frame, decrypted_fragment)?;
        self.apply_route_events(events, options, &mut batch);
        batch.fec_counters = self.video_fec_counters();
        Ok(batch)
    }

    /// Process one already-decrypted WFB fragment.
    pub fn push_decrypted_fragment(
        &mut self,
        runtime: PayloadRuntimeKey,
        data_nonce: u64,
        decrypted_fragment: &[u8],
        options: &ReceiverBatchOptions,
    ) -> Result<ReceiverBatch, PayloadRouteError> {
        let mut batch = self.empty_batch();
        let events =
            self.routes
                .push_decrypted_fragment(runtime, data_nonce, decrypted_fragment)?;
        self.apply_route_events(events, options, &mut batch);
        batch.fec_counters = self.video_fec_counters();
        Ok(batch)
    }

    /// Process one already-recovered payload through routes and RTP handling.
    pub fn push_direct_payload(
        &mut self,
        runtime: PayloadRuntimeKey,
        packet_seq: u64,
        payload: &[u8],
        options: &ReceiverBatchOptions,
    ) -> Result<ReceiverBatch, PayloadRouteError> {
        let mut batch = self.empty_batch();
        let (route_ids, direct) = self
            .routes
            .route_membership(runtime)
            .ok_or(PayloadRouteError::UnknownRuntime(runtime))?;
        if direct {
            self.apply_recovered_payload(
                &route_ids,
                runtime.channel_id(),
                packet_seq,
                payload,
                options,
                &mut batch,
            );
        } else {
            batch.counters.ignored_frames += 1;
        }
        batch.fec_counters = self.video_fec_counters();
        batch.rtp_status = self.rtp_status();
        batch.rtp_reorder_status = self.rtp_reorder_status();
        Ok(batch)
    }

    /// Process one synthetic recovered payload for tests and development.
    ///
    /// This is an alias for [`Self::push_direct_payload`].
    pub fn push_mock_payload(
        &mut self,
        runtime: PayloadRuntimeKey,
        packet_seq: u64,
        payload: &[u8],
        options: &ReceiverBatchOptions,
    ) -> Result<ReceiverBatch, PayloadRouteError> {
        self.push_direct_payload(runtime, packet_seq, payload, options)
    }

    fn empty_batch(&self) -> ReceiverBatch {
        ReceiverBatch {
            frames: Vec::new(),
            raw_payloads: Vec::new(),
            counters: ReceiverBatchCounters::default(),
            fec_counters: self.video_fec_counters(),
            rtp_status: self.rtp_status(),
            rtp_reorder_status: self.rtp_reorder_status(),
            route_error: None,
        }
    }

    fn apply_route_events(
        &mut self,
        events: impl IntoIterator<Item = PayloadRouteEvent>,
        options: &ReceiverBatchOptions,
        batch: &mut ReceiverBatch,
    ) {
        for event in events {
            match event {
                PayloadRouteEvent::IgnoredFrame => batch.counters.ignored_frames += 1,
                PayloadRouteEvent::SessionEstablished { .. } => batch.counters.sessions += 1,
                PayloadRouteEvent::Payload {
                    route_ids, payload, ..
                } => {
                    self.apply_owned_recovered_payload(
                        &route_ids,
                        payload.channel_id,
                        payload.packet_seq,
                        payload.data,
                        options,
                        batch,
                    );
                }
            }
        }
        batch.rtp_status = self.rtp_status();
        batch.rtp_reorder_status = self.rtp_reorder_status();
    }

    fn apply_recovered_payload(
        &mut self,
        route_ids: &[PayloadRouteId],
        channel_id: ChannelId,
        packet_seq: u64,
        data: &[u8],
        options: &ReceiverBatchOptions,
        batch: &mut ReceiverBatch,
    ) {
        self.apply_video_payload(route_ids, data, options, batch);

        for &route_id in route_ids {
            if options.raw_payload_routes.contains(&route_id) {
                copy_raw_payload(route_id, channel_id, packet_seq, data, batch);
            }
        }

        if options.rtp_payload_taps.is_empty() {
            return;
        }
        let Ok(header) = RtpHeader::parse(data) else {
            return;
        };
        for tap in &options.rtp_payload_taps {
            if header.payload_type == tap.payload_type && route_ids.contains(&tap.route_id) {
                copy_raw_payload(tap.route_id, channel_id, packet_seq, data, batch);
            }
        }
    }

    fn apply_owned_recovered_payload(
        &mut self,
        route_ids: &[PayloadRouteId],
        channel_id: ChannelId,
        packet_seq: u64,
        data: Vec<u8>,
        options: &ReceiverBatchOptions,
        batch: &mut ReceiverBatch,
    ) {
        self.apply_video_payload(route_ids, &data, options, batch);

        let payload_type = (!options.rtp_payload_taps.is_empty())
            .then(|| {
                RtpHeader::parse(&data)
                    .ok()
                    .map(|header| header.payload_type)
            })
            .flatten();
        let mut remaining = route_ids
            .iter()
            .filter(|route_id| options.raw_payload_routes.contains(route_id))
            .count()
            + options
                .rtp_payload_taps
                .iter()
                .filter(|tap| {
                    payload_type == Some(tap.payload_type) && route_ids.contains(&tap.route_id)
                })
                .count();
        if remaining == 0 {
            return;
        }

        let mut data = Some(data);
        for &route_id in route_ids {
            if options.raw_payload_routes.contains(&route_id) {
                remaining -= 1;
                push_raw_payload(
                    route_id,
                    channel_id,
                    packet_seq,
                    take_or_clone_payload(&mut data, remaining),
                    batch,
                );
            }
        }
        for tap in &options.rtp_payload_taps {
            if payload_type == Some(tap.payload_type) && route_ids.contains(&tap.route_id) {
                remaining -= 1;
                push_raw_payload(
                    tap.route_id,
                    channel_id,
                    packet_seq,
                    take_or_clone_payload(&mut data, remaining),
                    batch,
                );
            }
        }
    }

    fn apply_video_payload(
        &mut self,
        route_ids: &[PayloadRouteId],
        data: &[u8],
        options: &ReceiverBatchOptions,
        batch: &mut ReceiverBatch,
    ) {
        if route_ids.contains(&self.video_route_id) {
            batch.counters.wfb_payloads += 1;
            batch.counters.rtp_packets += 1;
            if options.depacketize_video {
                if let Ok(frames) = self.push_video_payload_into(data, &mut batch.frames) {
                    batch.counters.video_frames += frames;
                }
            }
        }
    }
}

fn take_or_clone_payload(data: &mut Option<Vec<u8>>, remaining: usize) -> Vec<u8> {
    if remaining == 0 {
        data.take()
            .expect("the final raw route must own the recovered payload")
    } else {
        data.as_ref()
            .expect("raw route payload must remain available")
            .clone()
    }
}

fn copy_raw_payload(
    route_id: PayloadRouteId,
    channel_id: ChannelId,
    packet_seq: u64,
    data: &[u8],
    batch: &mut ReceiverBatch,
) {
    push_raw_payload(route_id, channel_id, packet_seq, data.to_vec(), batch);
}

fn push_raw_payload(
    route_id: PayloadRouteId,
    channel_id: ChannelId,
    packet_seq: u64,
    data: Vec<u8>,
    batch: &mut ReceiverBatch,
) {
    batch.counters.raw_payload_count += 1;
    batch.counters.raw_payload_bytes += data.len();
    batch.raw_payloads.push(RoutePayload {
        route_id,
        channel_id,
        packet_seq,
        data,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RadioPort;

    fn plain(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn rtp(payload_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0x80, payload_type & 0x7f];
        packet.extend_from_slice(&7u16.to_be_bytes());
        packet.extend_from_slice(&48_000u32.to_be_bytes());
        packet.extend_from_slice(&0x1122_3344u32.to_be_bytes());
        packet.extend_from_slice(payload);
        packet
    }

    fn h264_stap_a_rtp() -> Vec<u8> {
        let sps = [0x67, 0x42, 0x00, 0x1e, 0xab];
        let pps = [0x68, 0xce, 0x06, 0xe2];
        let idr = [0x65, 0x88, 0x84, 0x21];
        let mut payload = vec![24];
        for nalu in [&sps[..], &pps[..], &idr[..]] {
            payload.extend_from_slice(&(nalu.len() as u16).to_be_bytes());
            payload.extend_from_slice(nalu);
        }
        let mut packet = rtp(crate::rtp::RTP_PAYLOAD_TYPE_H264, &payload);
        packet[1] |= 0x80;
        packet
    }

    #[test]
    fn decrypted_fragment_can_fan_out_to_raw_route() {
        let route = PayloadRouteId::new(7);
        let mut runtime = ReceiverRuntime::with_plain_video_route(
            FrameLayout::WithFcs,
            route,
            ChannelId::default_video(),
            0,
            1,
            1,
        )
        .unwrap();
        let batch = runtime
            .push_decrypted_fragment(
                runtime.video_runtime(),
                0,
                &plain(b"payload bytes"),
                &ReceiverBatchOptions {
                    raw_payload_routes: vec![route],
                    ..ReceiverBatchOptions::default()
                },
            )
            .unwrap();

        assert_eq!(batch.counters.wfb_payloads, 1);
        assert_eq!(batch.counters.raw_payload_count, 1);
        assert_eq!(batch.raw_payloads[0].data, b"payload bytes");
    }

    #[test]
    fn rtp_payload_tap_copies_only_matching_payload_type() {
        let video_route = PayloadRouteId::new(1);
        let audio_route = PayloadRouteId::new(3);
        let mut runtime = ReceiverRuntime::with_plain_video_route(
            FrameLayout::WithFcs,
            video_route,
            ChannelId::default_video(),
            0,
            1,
            1,
        )
        .unwrap();
        runtime
            .add_plain_route(audio_route, ChannelId::default_video(), 0, 1, 1)
            .unwrap();

        let ignored = runtime
            .push_decrypted_fragment(
                runtime.video_runtime(),
                0,
                &plain(&rtp(crate::rtp::RTP_PAYLOAD_TYPE_H264, b"video")),
                &ReceiverBatchOptions {
                    rtp_payload_taps: vec![RtpPayloadTap {
                        route_id: audio_route,
                        payload_type: crate::rtp::RTP_PAYLOAD_TYPE_OPUS,
                    }],
                    ..ReceiverBatchOptions::default()
                },
            )
            .unwrap();
        assert_eq!(ignored.counters.raw_payload_count, 0);

        let packet = rtp(crate::rtp::RTP_PAYLOAD_TYPE_OPUS, b"opus");
        let batch = runtime
            .push_decrypted_fragment(
                runtime.video_runtime(),
                1 << 8,
                &plain(&packet),
                &ReceiverBatchOptions {
                    rtp_payload_taps: vec![RtpPayloadTap {
                        route_id: audio_route,
                        payload_type: crate::rtp::RTP_PAYLOAD_TYPE_OPUS,
                    }],
                    ..ReceiverBatchOptions::default()
                },
            )
            .unwrap();

        assert_eq!(batch.counters.raw_payload_count, 1);
        assert_eq!(batch.raw_payloads[0].route_id, audio_route);
        assert_eq!(batch.raw_payloads[0].data, packet);
    }

    #[test]
    fn rtp_reorder_is_opt_in() {
        let mut runtime = ReceiverRuntime::with_plain_video_route(
            FrameLayout::WithFcs,
            PayloadRouteId::new(1),
            ChannelId::default_video(),
            0,
            1,
            1,
        )
        .unwrap();

        assert!(!runtime.rtp_reorder_enabled());
        assert_eq!(runtime.rtp_reorder_status(), RtpReorderStatus::default());

        runtime.set_rtp_reorder_enabled(true);
        assert!(runtime.rtp_reorder_enabled());

        runtime.set_rtp_reorder_enabled(false);
        assert!(!runtime.rtp_reorder_enabled());
        assert_eq!(runtime.rtp_reorder_status(), RtpReorderStatus::default());
    }

    #[test]
    fn auxiliary_route_does_not_count_as_video_payload() {
        let video_route = PayloadRouteId::new(1);
        let data_route = PayloadRouteId::new(2);
        let mut runtime = ReceiverRuntime::with_plain_video_route(
            FrameLayout::WithFcs,
            video_route,
            ChannelId::default_video(),
            0,
            1,
            1,
        )
        .unwrap();
        let data_runtime = runtime
            .add_plain_route(
                data_route,
                ChannelId::from_link_port(crate::channel::DEFAULT_LINK_ID, RadioPort::TunnelRx),
                0,
                1,
                1,
            )
            .unwrap();
        let batch = runtime
            .push_decrypted_fragment(
                data_runtime,
                0,
                &plain(b"data bytes"),
                &ReceiverBatchOptions {
                    raw_payload_routes: vec![data_route],
                    ..ReceiverBatchOptions::default()
                },
            )
            .unwrap();

        assert_eq!(batch.counters.wfb_payloads, 0);
        assert_eq!(batch.counters.rtp_packets, 0);
        assert_eq!(batch.counters.raw_payload_count, 1);
        assert_eq!(batch.raw_payloads[0].data, b"data bytes");
    }

    #[test]
    fn direct_payload_runtime_uses_same_video_route_and_rtp_depacketizer() {
        let video_route = PayloadRouteId::new(1);
        let mut runtime = ReceiverRuntime::with_direct_video_route(
            FrameLayout::WithFcs,
            video_route,
            ChannelId::default_video(),
            0,
        );

        let packet = h264_stap_a_rtp();
        let batch = runtime
            .push_direct_payload(
                runtime.video_runtime(),
                123,
                &packet,
                &ReceiverBatchOptions {
                    raw_payload_routes: vec![video_route],
                    ..ReceiverBatchOptions::default()
                },
            )
            .unwrap();

        assert_eq!(batch.counters.wfb_payloads, 1);
        assert_eq!(batch.counters.rtp_packets, 1);
        assert_eq!(batch.counters.video_frames, 1);
        assert_eq!(batch.frames.len(), 1);
        assert_eq!(batch.frames[0].codec, crate::rtp::Codec::H264);
        assert!(batch.frames[0].is_keyframe);
        assert_eq!(batch.raw_payloads[0].data, packet);
        assert_eq!(batch.fec_counters, FecCounters::default());
    }

    #[test]
    fn video_depacketization_can_be_delegated_without_losing_raw_rtp() {
        let video_route = PayloadRouteId::new(1);
        let mut runtime = ReceiverRuntime::with_direct_video_route(
            FrameLayout::WithFcs,
            video_route,
            ChannelId::default_video(),
            0,
        );
        let packet = h264_stap_a_rtp();

        let batch = runtime
            .push_direct_payload(
                runtime.video_runtime(),
                123,
                &packet,
                &ReceiverBatchOptions {
                    raw_payload_routes: vec![video_route],
                    depacketize_video: false,
                    ..ReceiverBatchOptions::default()
                },
            )
            .unwrap();

        assert_eq!(batch.counters.wfb_payloads, 1);
        assert_eq!(batch.counters.rtp_packets, 1);
        assert_eq!(batch.counters.video_frames, 0);
        assert!(batch.frames.is_empty());
        assert_eq!(batch.raw_payloads[0].data, packet);
        assert_eq!(batch.rtp_status, RtpDepacketizerStatus::default());
    }

    #[test]
    fn rx_transfer_accepts_explicit_jaguar3_descriptor_layout() {
        let mut runtime = ReceiverRuntime::with_plain_video_route(
            FrameLayout::WithFcs,
            PayloadRouteId::new(1),
            ChannelId::default_video(),
            0,
            1,
            1,
        )
        .unwrap();
        let mut transfer = vec![0u8; 32];
        transfer[..4].copy_from_slice(&8u32.to_le_bytes());
        transfer[24..32].copy_from_slice(&[0x08, 0, 0, 0, 0, 0, 0, 0]);

        let batch = runtime
            .push_rx_transfer_with_kind(
                &transfer,
                RxDescriptorKind::Jaguar3,
                &ReceiverBatchOptions::default(),
            )
            .unwrap();

        assert_eq!(batch.counters.packets, 1);
        assert_eq!(batch.counters.accepted_packets, 1);
        assert_eq!(batch.counters.ignored_frames, 1);
    }
}
