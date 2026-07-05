use super::*;
use openipc_core::realtek::RxDescriptorKind;

const VIDEO_ROUTE_ID: PayloadRouteId = PayloadRouteId::new(1);
// Internal only: keep the fixed VPN bridge out of user-defined route ids.
const VPN_ROUTE_ID: PayloadRouteId = PayloadRouteId::new(u64::MAX);
const DEFAULT_KEY_SLOT: u64 = 0;
const TUN_TX_SESSION_INTERVAL_MS: u64 = 1000;
const TUN_TX_DRAIN_LIMIT: usize = 32;
const JAGUAR3_COEX_TICK_MS: u64 = 2000;

pub(crate) struct UdpRouteSink {
    route_id: PayloadRouteId,
    dest: SocketAddr,
    socket: UdpSocket,
}

pub(crate) struct TunRuntime {
    route_id: PayloadRouteId,
    bridge: crate::tun_bridge::TunBridge,
    tx: WfbTransmitter,
    tx_options: RealtekTxOptions,
    tx_params: TxRadioParams,
    last_session_ms: Option<u64>,
}

impl AdaptiveRuntime {
    fn record_rx(&mut self, now_ms: u64, attrib: &RxPacketAttrib) {
        self.sender.record_rx_paths(now_ms, attrib.rssi, attrib.snr);
    }

    fn record_pipeline(&mut self, now_ms: u64, counters: FecCounters) {
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

    fn quality(&mut self, now_ms: u64) -> LinkQualityPayload {
        let quality = self.sender.link_mut().quality(now_ms);
        LinkQualityPayload {
            lost_last_second: quality.lost_last_second,
            recovered_last_second: quality.recovered_last_second,
            total_last_second: quality.total_last_second,
            rssi: quality.rssi,
            snr: quality.snr,
            link_score: quality.link_score,
            idr_code: quality.idr_code,
        }
    }

    fn tick(
        &mut self,
        now_ms: u64,
        ep_out: &mut nusb::Endpoint<Bulk, Out>,
    ) -> Result<usize, String> {
        let frames = self.sender.tick(now_ms).map_err(|err| err.to_string())?;
        let count = frames.len();
        for frame in frames {
            RealtekDevice::send_packet_on(ep_out, &frame, self.tx_options)
                .map_err(|err| err.to_string())?;
        }
        Ok(count)
    }
}

impl TunRuntime {
    fn new(
        route_id: PayloadRouteId,
        link_id: u32,
        keypair_bytes: &[u8],
        chip_family: ChipFamily,
        rf_channel: u8,
        raw_fd: Option<i32>,
    ) -> Result<Self, String> {
        let bridge = crate::tun_bridge::TunBridge::open_default(raw_fd)?;
        let tx_keypair = WfbTxKeypair::from_bytes(keypair_bytes).map_err(|err| err.to_string())?;
        let tx = WfbTransmitter::new(
            ChannelId::from_link_port(link_id, RadioPort::TunnelTx),
            tx_keypair,
            0,
            1,
            5,
        )
        .map_err(|err| err.to_string())?;
        Ok(Self {
            route_id,
            bridge,
            tx,
            tx_options: realtek_tx_options(chip_family, rf_channel),
            tx_params: TxRadioParams::openipc_uplink_default(),
            last_session_ms: None,
        })
    }

    fn name(&self) -> &str {
        self.bridge.name()
    }

    fn handles_route(&self, route_id: PayloadRouteId) -> bool {
        self.route_id == route_id
    }

    fn write_downlink_payload(&mut self, payload: &[u8]) -> Result<usize, String> {
        self.bridge
            .write_downlink_payload(payload)
            .map_err(|err| format!("write TUN downlink failed: {err}"))
    }

    fn tick(
        &mut self,
        now_ms: u64,
        ep_out: &mut nusb::Endpoint<Bulk, Out>,
    ) -> Result<usize, String> {
        let mut sent = 0usize;
        let session_due = match self.last_session_ms {
            Some(last) => now_ms.saturating_sub(last) >= TUN_TX_SESSION_INTERVAL_MS,
            None => true,
        };
        if session_due {
            let frame = self.tx.session_radio_packet(self.tx_params);
            RealtekDevice::send_packet_on(ep_out, &frame, self.tx_options)
                .map_err(|err| err.to_string())?;
            self.last_session_ms = Some(now_ms);
            sent += 1;
        }

        for _ in 0..TUN_TX_DRAIN_LIMIT {
            let Some(payload) = self
                .bridge
                .read_uplink_payload()
                .map_err(|err| format!("read TUN uplink failed: {err}"))?
            else {
                break;
            };
            let frames = self
                .tx
                .radio_packets_for_payload(&payload, self.tx_params)
                .map_err(|err| err.to_string())?;
            for frame in frames {
                RealtekDevice::send_packet_on(ep_out, &frame, self.tx_options)
                    .map_err(|err| err.to_string())?;
                sent += 1;
            }
        }
        Ok(sent)
    }
}

fn udp_route_sinks(routes: &[PayloadRouteRequest]) -> Result<Vec<UdpRouteSink>, String> {
    let mut sinks = Vec::new();
    for route in routes
        .iter()
        .filter(|route| route.action == PayloadRouteAction::Udp)
    {
        let host = route
            .udp_host
            .as_deref()
            .filter(|host| !host.trim().is_empty())
            .unwrap_or("127.0.0.1");
        let port = route.udp_port.unwrap_or(5600);
        let dest = format!("{host}:{port}")
            .to_socket_addrs()
            .map_err(|err| format!("invalid UDP destination for {}: {err}", route.name))?;
        let dest = dest
            .into_iter()
            .next()
            .ok_or_else(|| format!("invalid UDP destination for {}", route.name))?;
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|err| format!("bind UDP sink for {} failed: {err}", route.name))?;
        sinks.push(UdpRouteSink {
            route_id: PayloadRouteId::new(route.route_id),
            dest,
            socket,
        });
    }
    Ok(sinks)
}

fn realtek_tx_options(chip_family: ChipFamily, rf_channel: u8) -> RealtekTxOptions {
    RealtekTxOptions {
        current_channel: rf_channel,
        descriptor: openipc_rtl88xx::RealtekTxDescriptor::for_chip_family(chip_family),
        legacy_8812_descriptor: std::env::var_os("DEVOURER_TX_LEGACY_8812_DESC").is_some(),
        ..RealtekTxOptions::default()
    }
}

fn rx_descriptor_kind(chip_family: ChipFamily) -> RxDescriptorKind {
    match chip_family {
        ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => RxDescriptorKind::Jaguar2,
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => RxDescriptorKind::Jaguar3,
        ChipFamily::Rtl8812 | ChipFamily::Rtl8814 | ChipFamily::Rtl8821 => {
            RxDescriptorKind::Jaguar1
        }
    }
}

pub(crate) fn run_rx_worker(
    app: AppHandle,
    device: Arc<RealtekDevice>,
    chip_family: ChipFamily,
    request: StartRxRequest,
    stop: Arc<AtomicBool>,
    video_channel: Option<tauri::ipc::Channel<tauri::ipc::Response>>,
) -> Result<(), String> {
    let keypair_bytes = BASE64
        .decode(request.keypair_base64.as_bytes())
        .map_err(|err| format!("invalid keypair base64: {err}"))?;
    let minimum_epoch = request
        .minimum_epoch
        .parse::<u64>()
        .map_err(|err| format!("invalid minimum epoch: {err}"))?;
    let keypair = WfbKeypair::from_bytes(&keypair_bytes).map_err(|err| err.to_string())?;
    let mut receiver = ReceiverRuntime::with_keyed_video_route(
        FrameLayout::WithFcs,
        VIDEO_ROUTE_ID,
        ChannelId::new(request.channel_id),
        DEFAULT_KEY_SLOT,
        keypair,
        minimum_epoch,
    )
    .map_err(|err| err.to_string())?;
    receiver.set_rtp_reorder_enabled(request.rtp_reorder_enabled);
    let enabled_routes: Vec<_> = request
        .payload_routes
        .iter()
        .filter(|route| route.enabled)
        .cloned()
        .collect();
    for route in &enabled_routes {
        let route_id = PayloadRouteId::new(route.route_id);
        if route_id == VIDEO_ROUTE_ID {
            continue;
        }
        receiver
            .add_keyed_route(
                route_id,
                ChannelId::new(route.channel_id),
                DEFAULT_KEY_SLOT,
                keypair,
                minimum_epoch,
            )
            .map_err(|err| format!("invalid route {}: {err}", route.name))?;
    }
    let link_id = request.channel_id >> 8;
    if request.vpn_enabled {
        receiver
            .add_keyed_route(
                VPN_ROUTE_ID,
                ChannelId::from_link_port(link_id, RadioPort::TunnelRx),
                DEFAULT_KEY_SLOT,
                keypair,
                minimum_epoch,
            )
            .map_err(|err| format!("invalid VPN tunnel route: {err}"))?;
    }
    let raw_payload_routes: Vec<PayloadRouteId> = enabled_routes
        .iter()
        .filter(|route| route.action != PayloadRouteAction::Audio)
        .map(|route| PayloadRouteId::new(route.route_id))
        .chain(request.vpn_enabled.then_some(VPN_ROUTE_ID))
        .collect();
    let rtp_payload_taps: Vec<RtpPayloadTap> = enabled_routes
        .iter()
        .filter(|route| route.action == PayloadRouteAction::Audio)
        .map(|route| RtpPayloadTap {
            route_id: PayloadRouteId::new(route.route_id),
            payload_type: route.payload_type.unwrap_or(RTP_PAYLOAD_TYPE_OPUS),
        })
        .collect();
    let receiver_options = ReceiverBatchOptions {
        raw_payload_routes,
        rtp_payload_taps,
        ..ReceiverBatchOptions::default()
    };
    let udp_sinks = udp_route_sinks(&enabled_routes)?;
    let mut ep_in = device.bulk_in_endpoint().map_err(|err| err.to_string())?;
    let mut ep_out = if request.adaptive_enabled || request.vpn_enabled {
        Some(device.bulk_out_endpoint().map_err(|err| err.to_string())?)
    } else {
        None
    };
    let mut tun = if request.vpn_enabled {
        let tun = TunRuntime::new(
            VPN_ROUTE_ID,
            link_id,
            &keypair_bytes,
            chip_family,
            request.rf_channel,
            request.vpn_tun_fd,
        )?;
        emit_vpn_status(
            &app,
            VpnStatusPayload {
                interface_name: tun.name().to_owned(),
                local_ip: crate::tun_bridge::OPENIPC_VPN_ADDRESS,
                prefix_length: crate::tun_bridge::OPENIPC_VPN_PREFIX_LEN,
                rx_port: RadioPort::TunnelRx.as_u8(),
                tx_port: RadioPort::TunnelTx.as_u8(),
            },
        );
        emit_log(
            &app,
            "info",
            format!(
                "VPN tunnel active on {} as {}/{} (RX port 0x20, TX port 0xa0)",
                tun.name(),
                crate::tun_bridge::OPENIPC_VPN_ADDRESS,
                crate::tun_bridge::OPENIPC_VPN_PREFIX_LEN
            ),
        );
        Some(tun)
    } else {
        None
    };
    let mut adaptive = if request.adaptive_enabled {
        let tx_power_start = Instant::now();
        device
            .set_tx_power_override(request.rf_channel, request.alink_tx_power)
            .map_err(|err| err.to_string())?;
        emit_log(
            &app,
            "info",
            format!(
                "Adaptive uplink TX power set to {} ({:.1} ms)",
                request.alink_tx_power,
                elapsed_ms(tx_power_start)
            ),
        );
        let tx_keypair = WfbTxKeypair::from_bytes(&keypair_bytes).map_err(|err| err.to_string())?;
        Some(AdaptiveRuntime {
            sender: AdaptiveLinkSender::new(link_id, tx_keypair, 0, 1, 5)
                .map_err(|err| err.to_string())?,
            last_counters: receiver.video_fec_counters(),
            tx_options: realtek_tx_options(chip_family, request.rf_channel),
        })
    } else {
        None
    };

    while ep_in.pending() < RX_TRANSFERS_IN_FLIGHT {
        ep_in.submit(ep_in.allocate(request.transfer_size));
    }

    emit_log(
        &app,
        "info",
        format!("Native RX loop started ({RX_TRANSFERS_IN_FLIGHT} bulk-IN transfers in flight)"),
    );

    let mut last_jaguar3_coex_tick_ms = 0u64;
    let mut jaguar3_power_tracking = Jaguar3PowerTrackingState::default();
    while !stop.load(Ordering::Relaxed) {
        let loop_start = Instant::now();
        let read_start = Instant::now();
        let Some(completion) = ep_in.wait_next_complete(Duration::from_millis(1000)) else {
            let now_ms = unix_time_ms();
            tick_jaguar3_coex(
                &app,
                &device,
                chip_family,
                &mut last_jaguar3_coex_tick_ms,
                &mut jaguar3_power_tracking,
                now_ms,
            );
            tick_tx_idle(&mut adaptive, &mut tun, ep_out.as_mut(), now_ms);
            continue;
        };
        let usb_read_ms = elapsed_ms(read_start);
        let actual_len = completion.actual_len;
        if let Err(err) = completion.status {
            emit_log(&app, "warn", format!("bulk IN transfer failed: {err}"));
            ep_in.submit(completion.buffer);
            continue;
        }

        {
            let bytes = &completion.buffer[..actual_len];
            let now_ms = unix_time_ms();
            tick_jaguar3_coex(
                &app,
                &device,
                chip_family,
                &mut last_jaguar3_coex_tick_ms,
                &mut jaguar3_power_tracking,
                now_ms,
            );
            match build_rx_batch(
                bytes,
                RxBatchContext {
                    receiver: &mut receiver,
                    adaptive: adaptive.as_mut(),
                    ep_out: ep_out.as_mut(),
                    now_ms,
                    rx_descriptor_kind: rx_descriptor_kind(chip_family),
                    usb_read_ms,
                    loop_start,
                    options: &receiver_options,
                    udp_sinks: udp_sinks.as_slice(),
                    tun: tun.as_mut(),
                    video_channel: video_channel.as_ref(),
                },
            ) {
                Ok(batch) => {
                    let _ = app.emit(RX_BATCH_EVENT, batch);
                }
                Err(err) => {
                    emit_log(&app, "error", err);
                }
            }
        }
        ep_in.submit(completion.buffer);
    }

    drop(ep_in);
    drop(ep_out);
    if let Err(err) = device.shutdown_monitor() {
        emit_log(&app, "warn", format!("monitor shutdown failed: {err}"));
    }
    emit_stopped(&app, "stopped", "Native RX loop stopped".to_owned());
    Ok(())
}

fn tick_jaguar3_coex(
    app: &AppHandle,
    device: &RealtekDevice,
    chip_family: ChipFamily,
    last_tick_ms: &mut u64,
    power_tracking: &mut Jaguar3PowerTrackingState,
    now_ms: u64,
) {
    if !chip_family.is_jaguar3() || now_ms.saturating_sub(*last_tick_ms) < JAGUAR3_COEX_TICK_MS {
        return;
    }
    *last_tick_ms = now_ms;
    if let Err(err) = device.run_jaguar3_coex_keepalive() {
        emit_log(app, "warn", format!("Jaguar3 coex keepalive failed: {err}"));
    }
    match device.tick_jaguar3_power_tracking(power_tracking) {
        Ok(report) if report.lck_ran => emit_log(
            app,
            "info",
            format!(
                "Jaguar3 LCK ran after thermal drift (A={}, B={})",
                report.thermal_raw[0], report.thermal_raw[1]
            ),
        ),
        Ok(_) => {}
        Err(err) => emit_log(
            app,
            "warn",
            format!("Jaguar3 thermal tracking failed: {err}"),
        ),
    }
}

pub(crate) fn tick_tx_idle(
    adaptive: &mut Option<AdaptiveRuntime>,
    tun: &mut Option<TunRuntime>,
    ep_out: Option<&mut nusb::Endpoint<Bulk, Out>>,
    now_ms: u64,
) {
    let Some(ep_out) = ep_out else {
        return;
    };
    if let Some(runtime) = adaptive.as_mut() {
        let _ = runtime.tick(now_ms, ep_out);
    }
    if let Some(runtime) = tun.as_mut() {
        let _ = runtime.tick(now_ms, ep_out);
    }
}

pub(crate) fn build_rx_batch(
    bytes: &[u8],
    mut context: RxBatchContext<'_>,
) -> Result<RxBatchPayload, String> {
    let parse_start = Instant::now();
    let packets = parse_rx_aggregate_with_kind(bytes, context.rx_descriptor_kind)
        .map_err(|err| format!("RX aggregate parse failed: {err}"))?;
    let parse_ms = elapsed_ms(parse_start);
    let mut adaptive_rx_ms = 0.0;

    if let Some(runtime) = context.adaptive.as_deref_mut() {
        for packet in &packets {
            if packet.attrib.crc_err
                || packet.attrib.icv_err
                || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
            {
                continue;
            }
            let adaptive_rx_start = Instant::now();
            if context.receiver.accepts_video_frame(packet.data) {
                runtime.record_rx(context.now_ms, &packet.attrib);
            }
            adaptive_rx_ms += elapsed_ms(adaptive_rx_start);
        }
    }

    let pipeline_start = Instant::now();
    let batch = context.receiver.push_rx_packets(packets, context.options);
    let pipeline_ms = elapsed_ms(pipeline_start);
    let counters = batch.fec_counters;
    let batch_counters = batch.counters;
    let rtp_status = batch.rtp_status;
    let rtp_reorder_status = batch.rtp_reorder_status;
    for payload in &batch.raw_payloads {
        if let Some(runtime) = context.tun.as_deref_mut() {
            if runtime.handles_route(payload.route_id) {
                runtime.write_downlink_payload(&payload.data)?;
            }
        }
        for sink in context
            .udp_sinks
            .iter()
            .filter(|sink| sink.route_id == payload.route_id)
        {
            let _ = sink.socket.send_to(&payload.data, sink.dest);
        }
    }
    let frames = if let Some(channel) = context.video_channel {
        for frame in &batch.frames {
            channel
                .send(tauri::ipc::Response::new(video_frame_binary(frame)))
                .map_err(|err| format!("send video frame to webview failed: {err}"))?;
        }
        Vec::new()
    } else {
        batch.frames.into_iter().map(video_frame_payload).collect()
    };
    let tun_route_id = context.tun.as_ref().map(|runtime| runtime.route_id);
    let raw_payloads: Vec<_> = batch
        .raw_payloads
        .into_iter()
        .filter(|payload| Some(payload.route_id) != tun_route_id)
        .map(raw_payload_payload)
        .collect();

    let mut link_quality = None;
    let mut adaptive_quality_ms = 0.0;
    let mut adaptive_tx_ms = 0.0;
    let mut adaptive_tx_frames = 0usize;
    let mut adaptive_tx_errors = 0usize;
    if let Some(runtime) = context.adaptive {
        let quality_start = Instant::now();
        runtime.record_pipeline(context.now_ms, counters);
        link_quality = Some(runtime.quality(context.now_ms));
        adaptive_quality_ms = elapsed_ms(quality_start);

        if let Some(ep_out) = context.ep_out.as_deref_mut() {
            let tx_start = Instant::now();
            match runtime.tick(context.now_ms, ep_out) {
                Ok(count) => adaptive_tx_frames = count,
                Err(_) => adaptive_tx_errors = 1,
            }
            adaptive_tx_ms = elapsed_ms(tx_start);
        }
    }
    if let Some(runtime) = context.tun {
        if let Some(ep_out) = context.ep_out {
            let tx_start = Instant::now();
            match runtime.tick(context.now_ms, ep_out) {
                Ok(count) => adaptive_tx_frames += count,
                Err(_) => adaptive_tx_errors += 1,
            }
            adaptive_tx_ms += elapsed_ms(tx_start);
        }
    }

    Ok(RxBatchPayload {
        frames,
        raw_payloads: raw_payloads.clone(),
        mavlink_payloads: raw_payloads,
        transfer_bytes: bytes.len(),
        packets: batch_counters.packets,
        accepted_packets: batch_counters.accepted_packets,
        dropped_packets: batch_counters.dropped_packets,
        crc_dropped: batch_counters.crc_dropped,
        icv_dropped: batch_counters.icv_dropped,
        report_dropped: batch_counters.report_dropped,
        ignored_frames: batch_counters.ignored_frames,
        sessions: batch_counters.sessions,
        wfb_payloads: batch_counters.wfb_payloads,
        rtp_packets: batch_counters.rtp_packets,
        video_frames: batch_counters.video_frames,
        raw_payload_count: batch_counters.raw_payload_count,
        raw_payload_bytes: batch_counters.raw_payload_bytes,
        mavlink_payload_count: batch_counters.raw_payload_count,
        mavlink_bytes: batch_counters.raw_payload_bytes,
        parse_ms,
        pipeline_ms,
        total_ms: elapsed_ms(context.loop_start),
        rtp_status: rtp_status_payload(rtp_status, rtp_reorder_status),
        fec_counters: fec_counters_payload(counters),
        link_quality,
        adaptive_tx_frames,
        adaptive_tx_errors,
        usb_read_ms: context.usb_read_ms,
        adaptive_rx_ms,
        adaptive_quality_ms,
        tx_power_ms: 0.0,
        adaptive_tx_ms,
    })
}
