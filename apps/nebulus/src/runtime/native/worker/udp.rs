use super::*;

pub(super) fn run(
    request: StartRequest,
    stop: &AtomicBool,
    audio_volume: &AtomicU8,
    recording_control: &Mutex<super::super::RecordingControl>,
    events: &super::super::EventQueue,
    context: &eframe::egui::Context,
) -> Result<(), String> {
    let input = crate::runtime::udp_input::UdpRtpInput::bind(
        &request.udp_bind_address,
        request.udp_bind_port,
    )?;
    let local_address = input.local_address();
    let mut decoder = create_decoder(&request)
        .map_err(|error| format!("UDP RTP video decoder unavailable: {error}"))?;
    let presenter = FramePresenter::new(events, context);
    let mut route_processor = RouteProcessor::new(&request)?;
    let recording_audio_config = route_processor.recording_audio_config();
    for entry in route_processor.take_startup_logs() {
        log(
            events,
            context,
            if entry.warning {
                LogLevel::Warn
            } else {
                LogLevel::Info
            },
            "route",
            entry.message,
        );
    }

    let mut receiver = ReceiverRuntime::with_direct_video_route(
        FrameLayout::WithFcs,
        VIDEO_ROUTE,
        ChannelId::new(request.channel_id),
        0,
    );
    receiver.set_rtp_reorder_enabled(request.rtp_reorder);
    let options = crate::runtime::route_runtime::configure_direct_receiver(&mut receiver, &request);
    let runtime = receiver.video_runtime();

    send(
        events,
        context,
        RuntimeEvent::Connected {
            receivers: vec![crate::runtime::ReceiverInfo::udp_rtp(
                local_address.to_string(),
            )],
            decoder: decoder_environment(decoder.capabilities()),
        },
    );
    send(events, context, RuntimeEvent::Started);
    log(
        events,
        context,
        LogLevel::Info,
        "udp",
        format!(
            "UDP RTP receiver listening on {local_address}; socket buffer {}",
            input.receive_buffer_size().map_or_else(
                || "not reported".to_owned(),
                |bytes| format!("{bytes} bytes")
            )
        ),
    );

    let mut packet_buffer = vec![0; crate::runtime::udp_input::MAX_UDP_DATAGRAM_SIZE];
    let mut payload_sequence = 1u64;
    let mut metrics_throttle = MetricsThrottle::new();
    let mut recorder: Option<EncodedRecorder> = None;
    let mut armed_path: Option<PathBuf> = None;
    let mut first_peer = None;
    let mut last_decode_errors = 0;

    while !stop.load(Ordering::Relaxed) {
        let receive_started = Instant::now();
        let Some((packet_length, peer)) = input.receive(&mut packet_buffer)? else {
            update_recording(
                &[],
                recording_control,
                &mut armed_path,
                &mut recorder,
                recording_audio_config,
                events,
                context,
            );
            present_latest(&mut decoder, &presenter);
            if let Some(metrics) = metrics_throttle.flush() {
                send(events, context, RuntimeEvent::Batch(Box::new(metrics)));
            }
            continue;
        };
        let receive_wait_ms = receive_started.elapsed().as_secs_f64() * 1_000.0;
        if first_peer.is_none() {
            first_peer = Some(peer);
            log(
                events,
                context,
                LogLevel::Info,
                "udp",
                format!("Receiving RTP datagrams from {peer}"),
            );
        }

        let metrics = process_packet(
            &request,
            &packet_buffer[..packet_length],
            payload_sequence,
            receive_wait_ms,
            &mut receiver,
            runtime,
            &options,
            &mut route_processor,
            &mut decoder,
            recording_control,
            &mut armed_path,
            &mut recorder,
            recording_audio_config,
            audio_volume,
            events,
            context,
        )?;
        payload_sequence = payload_sequence.wrapping_add(1);
        if metrics.video_frames > 0 {
            present_latest(&mut decoder, &presenter);
        }
        let decode_errors = decoder.stats().decode_errors;
        if decode_errors > last_decode_errors {
            last_decode_errors = decode_errors;
            log(
                events,
                context,
                LogLevel::Warn,
                "decoder",
                format!("decoder errors: {last_decode_errors}"),
            );
        }
        if let Some(metrics) = metrics_throttle.push(metrics) {
            send(events, context, RuntimeEvent::Batch(Box::new(metrics)));
        }
    }

    if let Some(metrics) = metrics_throttle.flush() {
        send(events, context, RuntimeEvent::Batch(Box::new(metrics)));
    }
    let _ = decoder.flush();
    present_latest(&mut decoder, &presenter);
    drop(presenter);
    finish_recording(&mut recorder, events, context);
    log(
        events,
        context,
        LogLevel::Info,
        "udp",
        "UDP RTP receiver stopped",
    );
    send(events, context, RuntimeEvent::Stopped);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_packet(
    request: &StartRequest,
    packet: &[u8],
    payload_sequence: u64,
    receive_wait_ms: f64,
    receiver: &mut ReceiverRuntime,
    runtime: openipc_core::PayloadRuntimeKey,
    options: &openipc_core::ReceiverBatchOptions,
    route_processor: &mut RouteProcessor,
    decoder: &mut AppDecoder,
    recording_control: &Mutex<super::super::RecordingControl>,
    armed_path: &mut Option<PathBuf>,
    recorder: &mut Option<EncodedRecorder>,
    recording_audio_config: Option<crate::recording::AudioTrackConfig>,
    audio_volume: &AtomicU8,
    events: &super::super::EventQueue,
    context: &eframe::egui::Context,
) -> Result<BatchMetrics, String> {
    let batch_started = Instant::now();
    let pipeline_started = Instant::now();
    let mut batch = receiver
        .push_direct_payload(runtime, payload_sequence, packet, options)
        .map_err(|error| format!("route UDP RTP payload failed: {error}"))?;
    let pipeline_latency_ms = pipeline_started.elapsed().as_secs_f64() * 1_000.0;
    let video_frames = batch.frames.len();
    let video_bytes = batch.frames.iter().map(|frame| frame.data.len()).sum();

    update_recording(
        &batch.frames,
        recording_control,
        armed_path,
        recorder,
        recording_audio_config,
        events,
        context,
    );
    route_processor.set_audio_volume(audio_volume.load(Ordering::Relaxed));
    let route_started = Instant::now();
    let (route_updates, route_logs, recorded_audio, telemetry) =
        route_processor.process(&batch.raw_payloads, recorder.is_some());
    record_audio_packets(recorder, recorded_audio, events, context);
    let route_latency_ms = route_started.elapsed().as_secs_f64() * 1_000.0;
    for entry in route_logs {
        log(
            events,
            context,
            if entry.warning {
                LogLevel::Warn
            } else {
                LogLevel::Info
            },
            "route",
            entry.message,
        );
    }

    let decode_submit_started = Instant::now();
    for frame in std::mem::take(&mut batch.frames)
        .into_iter()
        .filter(|frame| request.codec_preference.accepts(frame.codec))
    {
        if let Err(error) = decoder.submit(frame.into()) {
            log(
                events,
                context,
                LogLevel::Warn,
                "decoder",
                format!("decode submit failed: {error}"),
            );
        }
    }
    let decode_submit_latency_ms = decode_submit_started.elapsed().as_secs_f64() * 1_000.0;
    let stats = decoder.stats();

    Ok(BatchMetrics {
        transfers: 1,
        transfer_bytes: packet.len(),
        packets: 1,
        rtp_packets: batch.counters.rtp_packets,
        video_frames,
        video_bytes,
        usb_latency_ms: receive_wait_ms,
        pipeline_latency_ms,
        route_latency_ms,
        decode_submit_latency_ms,
        video_submit_path_ms: batch_started.elapsed().as_secs_f64() * 1_000.0,
        batch_latency_ms: batch_started.elapsed().as_secs_f64() * 1_000.0,
        decoder_drops: stats.waiting_drops + stats.backpressure_drops + stats.output_drops,
        decoder_errors: stats.decode_errors,
        fec: batch.fec_counters,
        counters: batch.counters,
        rtp: batch.rtp_status,
        reorder: batch.rtp_reorder_status,
        routes: route_updates,
        telemetry,
        audio: route_processor.audio_stats(),
        ..BatchMetrics::default()
    })
}

fn present_latest(decoder: &mut AppDecoder, presenter: &FramePresenter) {
    if let Some(decoded) = decoder.latest_frame() {
        presenter.submit(
            decoded,
            decoder.stats().last_decode_latency_us as f64 / 1_000.0,
        );
    }
}
