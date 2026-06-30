use super::*;

const VIDEO_ROUTE_ID: PayloadRouteId = PayloadRouteId::new(1);
const DEFAULT_KEY_SLOT: u64 = 0;

pub(crate) struct UdpRouteSink {
    route_id: PayloadRouteId,
    dest: SocketAddr,
    socket: UdpSocket,
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

pub(crate) fn run_rx_worker(
    app: AppHandle,
    device: Arc<RealtekDevice>,
    chip_family: ChipFamily,
    request: StartRxRequest,
    stop: Arc<AtomicBool>,
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
    let raw_payload_routes: Vec<PayloadRouteId> = enabled_routes
        .iter()
        .filter(|route| route.action != PayloadRouteAction::Audio)
        .map(|route| PayloadRouteId::new(route.route_id))
        .collect();
    let rtp_payload_taps: Vec<RtpPayloadTap> = enabled_routes
        .iter()
        .filter(|route| route.action == PayloadRouteAction::Audio)
        .map(|route| RtpPayloadTap {
            route_id: PayloadRouteId::new(route.route_id),
            payload_type: route.payload_type.unwrap_or(RTP_PAYLOAD_TYPE_OPUS),
        })
        .collect();
    let udp_sinks = udp_route_sinks(&enabled_routes)?;
    let mut ep_in = device.bulk_in_endpoint().map_err(|err| err.to_string())?;
    let mut ep_out = if request.adaptive_enabled {
        Some(device.bulk_out_endpoint().map_err(|err| err.to_string())?)
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
        let link_id = request.channel_id >> 8;
        Some(AdaptiveRuntime {
            sender: AdaptiveLinkSender::new(link_id, tx_keypair, 0, 1, 5)
                .map_err(|err| err.to_string())?,
            last_counters: receiver.video_fec_counters(),
            tx_options: RealtekTxOptions {
                current_channel: request.rf_channel,
                is_8814a: chip_family == ChipFamily::Rtl8814,
                legacy_8812_descriptor: std::env::var_os("DEVOURER_TX_LEGACY_8812_DESC").is_some(),
                ..RealtekTxOptions::default()
            },
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

    while !stop.load(Ordering::Relaxed) {
        let loop_start = Instant::now();
        let read_start = Instant::now();
        let Some(completion) = ep_in.wait_next_complete(Duration::from_millis(1000)) else {
            let now_ms = unix_time_ms();
            tick_adaptive_idle(&mut adaptive, ep_out.as_mut(), now_ms);
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
            match build_rx_batch(
                bytes,
                RxBatchContext {
                    receiver: &mut receiver,
                    adaptive: adaptive.as_mut(),
                    ep_out: ep_out.as_mut(),
                    now_ms,
                    usb_read_ms,
                    loop_start,
                    raw_payload_routes: raw_payload_routes.as_slice(),
                    rtp_payload_taps: rtp_payload_taps.as_slice(),
                    udp_sinks: udp_sinks.as_slice(),
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

    emit_stopped(&app, "stopped", "Native RX loop stopped".to_owned());
    Ok(())
}

pub(crate) fn tick_adaptive_idle(
    adaptive: &mut Option<AdaptiveRuntime>,
    ep_out: Option<&mut nusb::Endpoint<Bulk, Out>>,
    now_ms: u64,
) {
    let Some(runtime) = adaptive.as_mut() else {
        return;
    };
    let Some(ep_out) = ep_out else {
        return;
    };
    let _ = runtime.tick(now_ms, ep_out);
}

pub(crate) fn build_rx_batch(
    bytes: &[u8],
    mut context: RxBatchContext<'_>,
) -> Result<RxBatchPayload, String> {
    let parse_start = Instant::now();
    let packets =
        parse_rx_aggregate(bytes).map_err(|err| format!("RX aggregate parse failed: {err}"))?;
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
    let batch = context.receiver.push_rx_packets(
        packets,
        &ReceiverBatchOptions {
            raw_payload_routes: context.raw_payload_routes.to_vec(),
            rtp_payload_taps: context.rtp_payload_taps.to_vec(),
            ..ReceiverBatchOptions::default()
        },
    );
    let pipeline_ms = elapsed_ms(pipeline_start);
    let counters = batch.fec_counters;
    let batch_counters = batch.counters;
    for payload in &batch.raw_payloads {
        for sink in context
            .udp_sinks
            .iter()
            .filter(|sink| sink.route_id == payload.route_id)
        {
            let _ = sink.socket.send_to(&payload.data, sink.dest);
        }
    }
    let frames = batch.frames.into_iter().map(video_frame_payload).collect();
    let raw_payloads: Vec<_> = batch
        .raw_payloads
        .into_iter()
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

        if let Some(ep_out) = context.ep_out {
            let tx_start = Instant::now();
            match runtime.tick(context.now_ms, ep_out) {
                Ok(count) => adaptive_tx_frames = count,
                Err(_) => adaptive_tx_errors = 1,
            }
            adaptive_tx_ms = elapsed_ms(tx_start);
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
