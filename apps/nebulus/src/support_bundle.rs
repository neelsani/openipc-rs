use std::io::{Cursor, Write as _};

use serde_json::{json, Value};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::app::NebulusApp;

pub(crate) struct SupportBundle {
    pub(crate) filename: String,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn build(app: &NebulusApp) -> Result<SupportBundle, String> {
    let build = crate::build_info::current();
    let timestamp = web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let routes = app
        .settings
        .payload_routes
        .iter()
        .map(|route| {
            json!({
                "id": route.id,
                "enabled": route.enabled,
                "name": route.name,
                "radio_port": route.radio_port,
                "action": format!("{:?}", route.action),
                "telemetry_protocol": format!("{:?}", route.telemetry_protocol),
                "payload_type": route.payload_type,
                "sample_rate": route.sample_rate,
                "channels": route.channels,
                "udp_host": route.udp_host,
                "udp_port": route.udp_port,
            })
        })
        .collect::<Vec<_>>();
    let profiles = app
        .settings
        .profiles
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "receiver_source": profile.receiver_source.label(),
                "udp_bind_address": profile.udp_bind_address,
                "udp_bind_port": profile.udp_bind_port,
                "device_id": profile.device_id,
                "diversity_device_ids": profile.diversity_device_ids,
                "channel": profile.channel,
                "channel_width_mhz": profile.channel_width_mhz,
                "channel_offset": profile.channel_offset,
                "link_id": format!("0x{:06x}", profile.link_id),
                "minimum_epoch": profile.minimum_epoch,
                "codec": profile.codec_preference.label(),
                "rtp_reorder": profile.rtp_reorder,
                "adaptive_link": profile.adaptive_link,
                "tx_power": profile.tx_power,
                "audio_volume": profile.audio_volume,
                "transfer_size": profile.transfer_size,
                "vpn": profile.vpn_enabled,
                "vtx_control": profile.vtx_control_enabled,
                "osd_profile_id": profile.osd_profile_id,
                "routes": profile.payload_routes.len(),
                "key_bytes": profile.key_bytes.len(),
                "key_sha256": sha256_hex(&profile.key_bytes),
                "telemetry_signing": profile.telemetry.mavlink_signing.label(),
                "telemetry_signing_key_bytes": profile.telemetry.mavlink_signing_key.len(),
                "telemetry_signing_key_sha256": sha256_hex(&profile.telemetry.mavlink_signing_key),
            })
        })
        .collect::<Vec<_>>();
    let osd_profiles = app
        .settings
        .osd_profiles
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "indicators": profile.hud.items.len(),
                "visible_indicators": profile
                    .hud
                    .items
                    .iter()
                    .filter(|item| item.visible)
                    .count(),
                "scale_percent": profile.hud.scale_percent,
                "background_opacity": profile.hud.background_opacity,
            })
        })
        .collect::<Vec<_>>();
    let scan_results = app
        .scan_results
        .iter()
        .map(|result| {
            json!({
                "channel": result.channel,
                "packets": result.packets,
                "bytes": result.bytes,
                "wfb_frames": result.wfb_frames,
                "average_rssi_dbm": result.average_rssi_dbm,
                "strongest_rssi_dbm": result.strongest_rssi_dbm,
                "average_snr_db": result.average_snr_db,
                "average_evm_db": result.average_evm_db,
                "retune_us": result.retune_us,
                "used_fast_retune": result.used_fast_retune,
                "dwell_ms": result.dwell_ms,
            })
        })
        .collect::<Vec<_>>();
    let receiver_attempts = if app.receiver_attempts.is_empty() {
        &app.receiver_infos
    } else {
        &app.receiver_attempts
    };
    let receivers = receiver_attempts
        .iter()
        .map(receiver_summary_json)
        .collect::<Vec<_>>();
    let discovered_devices = app
        .devices
        .iter()
        .map(|device| {
            json!({
                "id": device.id,
                "label": device.label,
                "vendor_id": format!("{:04x}", device.vendor_id),
                "product_id": format!("{:04x}", device.product_id),
                "location": device.location,
            })
        })
        .collect::<Vec<_>>();
    let driver_init = json!({
        "schema_version": 2,
        "generated_unix_seconds": timestamp,
        "attempts": receiver_attempts
            .iter()
            .map(receiver_driver_json)
            .collect::<Vec<_>>(),
    });
    let stage_latencies = app
        .diagnostics
        .stages
        .iter()
        .map(|(name, values)| {
            let summary = values.summary();
            json!({
                "stage": name,
                "last_ms": summary.last,
                "average_ms": summary.average,
                "p95_ms": summary.p95,
                "maximum_ms": summary.maximum,
                "samples": summary.samples,
            })
        })
        .collect::<Vec<_>>();
    let route_metrics = app
        .route_stats
        .iter()
        .map(|(id, stats)| {
            json!({
                "route_id": id,
                "packets": stats.packets,
                "bytes": stats.bytes,
                "last_bytes": stats.last_bytes,
                "errors": stats.errors,
            })
        })
        .collect::<Vec<_>>();
    let preflight = app
        .preflight
        .checks
        .iter()
        .map(|check| {
            json!({
                "name": check.name,
                "severity": format!("{:?}", check.severity),
                "detail": check.detail,
            })
        })
        .collect::<Vec<_>>();
    let telemetry_configuration = json!({
        "stale_timeout_ms": app.settings.telemetry.stale_timeout_ms,
        "mavlink_signing": app.settings.telemetry.mavlink_signing.label(),
        "mavlink_signing_key_bytes": app.settings.telemetry.mavlink_signing_key.len(),
        "mavlink_signing_key_sha256": sha256_hex(&app.settings.telemetry.mavlink_signing_key),
        "mavlink_system_id": app.settings.telemetry.mavlink_system_id,
        "mavlink_component_id": app.settings.telemetry.mavlink_component_id,
        "msp_version": app.settings.telemetry.msp_version.label(),
        "msp_direction": app.settings.telemetry.msp_direction.label(),
        "crsf_address": app.settings.telemetry.crsf_address,
    });
    let telemetry = json!({
        "protocol": app.telemetry.protocol.map(|protocol| protocol.label()),
        "messages": app.telemetry.messages,
        "fresh": app.telemetry.is_fresh(app.settings.telemetry.stale_timeout_ms),
        "age_seconds": app.telemetry.age_seconds(),
        "frame_age_seconds": app.telemetry.frame_age_seconds(),
        "accepted_frames": app.telemetry.counters.accepted_frames,
        "rejected_frames": app.telemetry.counters.rejected_frames,
        "filtered_frames": app.telemetry.counters.filtered_frames,
        "mavlink_version": app.telemetry.mavlink_version,
        "mavlink_system_id": app.telemetry.mavlink_system_id,
        "mavlink_component_id": app.telemetry.mavlink_component_id,
        "mavlink_last_signed": app.telemetry.mavlink_last_signed,
        "mavlink_signing_link_id": app.telemetry.mavlink_signing_link_id,
        "mavlink_signed_frames": app.telemetry.counters.mavlink_signed_frames,
        "mavlink_unsigned_frames": app.telemetry.counters.mavlink_unsigned_frames,
        "mavlink_verified_frames": app.telemetry.counters.mavlink_verified_frames,
        "mavlink_invalid_signatures": app.telemetry.counters.mavlink_invalid_signatures,
        "mavlink_replay_drops": app.telemetry.counters.mavlink_replay_drops,
        "mavlink_stale_timestamp_drops": app.telemetry.counters.mavlink_stale_timestamp_drops,
        "mavlink_missing_key_drops": app.telemetry.counters.mavlink_missing_key_drops,
        "armed": app.telemetry.armed,
        "flight_mode": app.telemetry.flight_mode,
        "battery_voltage_v": app.telemetry.battery_voltage_v,
        "battery_current_a": app.telemetry.battery_current_a,
        "battery_consumed_mah": app.telemetry.battery_consumed_mah,
        "battery_remaining_pct": app.telemetry.battery_remaining_pct,
        "gps_fix": app.telemetry.gps_fix,
        "satellites": app.telemetry.satellites,
        "altitude_m": app.telemetry.altitude_m,
        "relative_altitude_m": app.telemetry.relative_altitude_m,
        "ground_speed_mps": app.telemetry.ground_speed_mps,
        "air_speed_mps": app.telemetry.air_speed_mps,
        "vertical_speed_mps": app.telemetry.vertical_speed_mps,
        "heading_deg": app.telemetry.heading_deg,
        "home_distance_m": app.telemetry.home_distance_m,
        "rc_link_quality_pct": app.telemetry.rc_link_quality_pct,
        "position_available": app.telemetry.latitude_deg.is_some()
            && app.telemetry.longitude_deg.is_some(),
    });
    let adapter_metrics = app
        .adapter_metrics
        .iter()
        .map(|adapter| {
            json!({
                "source_id": adapter.source_id,
                "device_id": adapter.device_id,
                "label": adapter.label,
                "online": adapter.online,
                "transfers": adapter.transfers,
                "transfer_bytes": adapter.transfer_bytes,
                "usb_errors": adapter.usb_errors,
                "queue_drops": adapter.queue_drops,
                "rssi": adapter.rssi,
                "snr": adapter.snr,
                "accepted": adapter.accepted,
                "duplicates": adapter.duplicates,
                "descriptor_kind": adapter.descriptor_kind,
                "first_descriptor_sample_hex": adapter.first_descriptor_sample,
                "first_transfer_len": adapter.first_transfer_len,
                "first_transfer_latency_ms": adapter.first_transfer_latency_ms,
                "first_transfer_sample_hex": adapter.first_transfer_sample,
                "zero_length_transfers": adapter.zero_length_transfers,
                "aggregate_descriptors": adapter.aggregate_descriptors,
                "aggregate_trailing_events": adapter.aggregate_trailing_events,
                "aggregate_trailing_bytes": adapter.aggregate_trailing_bytes,
                "aggregate_trailing_nonzero_bytes": adapter.aggregate_trailing_nonzero_bytes,
                "alignment_padding_bytes": adapter.alignment_padding_bytes,
                "final_alignment_shortfall_bytes": adapter.final_alignment_shortfall_bytes,
                "descriptor_too_short": adapter.descriptor_too_short,
                "invalid_packet_length": adapter.invalid_packet_length,
                "crc_packets": adapter.crc_packets,
                "icv_packets": adapter.icv_packets,
                "report_packets": adapter.report_packets,
                "wifi_parse_errors": adapter.wifi_parse_errors,
                "first_parse_error": adapter.first_parse_error,
                "first_parse_error_sample_hex": adapter.first_parse_error_sample,
                "usb_stalls": adapter.usb_stalls,
                "usb_disconnects": adapter.usb_disconnects,
                "usb_other_errors": adapter.usb_other_errors,
                "last_usb_error": adapter.last_usb_error,
            })
        })
        .collect::<Vec<_>>();
    let vtx_settings = &app.settings.vtx;
    let vtx_configuration = json!({
        "control_enabled": app.settings.vtx_control_enabled,
        "ssh_username": app.settings.vtx_ssh_username,
        "ssh_password_configured": !app.settings.vtx_ssh_password.is_empty(),
        "host_key_sha256": app.settings.vtx_host_key_sha256,
        "mcs_index": vtx_settings.mcs_index,
        "stbc": vtx_settings.stbc,
        "ldpc": vtx_settings.ldpc,
        "fec_k": vtx_settings.fec_k,
        "fec_n": vtx_settings.fec_n,
        "multi_link": vtx_settings.multi_link,
        "mirror": vtx_settings.mirror,
        "flip": vtx_settings.flip,
        "contrast": vtx_settings.contrast,
        "hue": vtx_settings.hue,
        "saturation": vtx_settings.saturation,
        "luminance": vtx_settings.luminance,
        "resolution": vtx_settings.resolution,
        "fps": vtx_settings.fps,
        "bitrate_kbps": vtx_settings.bitrate_kbps,
        "codec": vtx_settings.codec,
        "gop_size": vtx_settings.gop_size,
        "rate_control": vtx_settings.rate_control,
        "simple_video_mode": vtx_settings.simple_video_mode,
        "recording_enabled": vtx_settings.recording_enabled,
        "recording_split_seconds": vtx_settings.recording_split_seconds,
        "recording_max_usage": vtx_settings.recording_max_usage,
        "exposure": vtx_settings.exposure,
        "anti_flicker": vtx_settings.anti_flicker,
        "sensor_config": vtx_settings.sensor_config,
        "fpv_enabled": vtx_settings.fpv_enabled,
        "noise_level": vtx_settings.noise_level,
        "telemetry_serial": vtx_settings.telemetry_serial,
        "telemetry_router": vtx_settings.telemetry_router,
        "telemetry_osd_fps": vtx_settings.telemetry_osd_fps,
        "telemetry_gs_rendering": vtx_settings.telemetry_gs_rendering,
        "adaptive_service_enabled": vtx_settings.adaptive_service_enabled,
        "adaptive_variable": vtx_settings.adaptive_variable,
        "adaptive_value": vtx_settings.adaptive_value,
        "tx_profiles": vtx_settings.tx_profiles,
    });
    let capture_stats = crate::logging::capture_stats();
    let uplink_scheduler = json!({
        "control_packets_queued": app.vtx_control.tx.control_packets_queued,
        "tunnel_packets_queued": app.vtx_control.tx.tunnel_packets_queued,
        "control_queue_full": app.vtx_control.tx.control_queue_full,
        "tunnel_queue_full": app.vtx_control.tx.tunnel_queue_full,
        "aggregates_created": app.vtx_control.tx.aggregates_created,
        "aggregates_completed": app.vtx_control.tx.aggregates_completed,
        "aggregates_failed": app.vtx_control.tx.aggregates_failed,
        "ip_packets_aggregated": app.vtx_control.tx.ip_packets_aggregated,
        "aggregate_payload_bytes": app.vtx_control.tx.aggregate_payload_bytes,
        "aggregate_bytes_completed": app.vtx_control.tx.aggregate_bytes_completed,
        "batches_submitted": app.vtx_control.tx.batches_submitted,
        "frames_submitted": app.vtx_control.tx.frames_submitted,
        "frames_completed": app.vtx_control.tx.frames_completed,
        "frames_failed": app.vtx_control.tx.frames_failed,
        "frames_retried": app.vtx_control.tx.frames_retried,
        "frames_dropped": app.vtx_control.tx.frames_dropped,
        "short_writes": app.vtx_control.tx.short_writes,
        "stalls": app.vtx_control.tx.stalls,
        "timeouts": app.vtx_control.tx.timeouts,
        "fatal_failures": app.vtx_control.tx.fatal_failures,
    });
    let report = json!({
        "schema_version": 2,
        "generated_unix_seconds": timestamp,
        "build": {
            "version": build.version,
            "commit": build.commit,
            "tag": build.tag,
        },
        "application": {
            "state": format!("{:?}", app.state),
            "active_profile_id": app.settings.active_profile_id,
            "active_osd_profile_id": app.settings.active_osd_profile_id,
            "auto_recover": app.settings.auto_recover,
            "recovery_attempt": app.recovery.attempt,
            "last_recovery_error": sanitize(&app.recovery.last_error),
            "recording_state": format!("{:?}", app.recording.state),
            "receiver_key_source": sanitize(&app.key_name),
            "receiver_key_error": app.key_error.as_deref().map(sanitize),
            "mavlink_key_source": sanitize(&app.mavlink_key_name),
            "mavlink_key_error": app.mavlink_key_error.as_deref().map(sanitize),
            "preset_error": app.preset_error.as_deref().map(sanitize),
            "channel_scan_error": app.scan_error.as_deref().map(sanitize),
        },
        "logging": {
            "configured_verbosity": app.settings.diagnostic_verbosity.label(),
            "visible_log_records": app.logs.len(),
            "retained_session_records": app.support_logs.len(),
            "session_records_evicted": app.support_logs_dropped,
            "high_rate_trace_records_sampled_out": capture_stats.sampled_trace_records,
            "global_capture_records_trimmed": capture_stats.trimmed_records,
            "egui_feedback_records_excluded": capture_stats.egui_records_excluded,
            "high_rate_sample_ratio": 128,
        },
        "environment": {
            "platform": app.environment.platform,
            "architecture": app.environment.architecture,
            "runtime": app.environment.runtime,
            "renderer": app.environment.renderer,
            "logical_processors": app.environment.logical_processors,
            "user_agent": app.environment.user_agent,
            "decoder_backend": app.environment.decoder_backend,
            "h264": app.environment.h264,
            "h265": app.environment.h265,
            "native_surfaces": app.environment.native_surfaces,
        },
        "receivers": receivers,
        "discovered_supported_devices": discovered_devices,
        "configuration": {
            "receiver_source": app.settings.receiver_source.label(),
            "udp_bind_address": app.settings.udp_bind_address,
            "udp_bind_port": app.settings.udp_bind_port,
            "device_id": app.settings.device_id,
            "diversity_device_ids": app.settings.diversity_device_ids,
            "channel": app.settings.channel,
            "channel_width_mhz": app.settings.channel_width_mhz,
            "channel_offset": app.settings.channel_offset,
            "link_id": format!("0x{:06x}", app.settings.link_id),
            "minimum_epoch": app.settings.minimum_epoch,
            "codec_preference": app.settings.codec_preference.label(),
            "rtp_reorder": app.settings.rtp_reorder,
            "adaptive_link": app.settings.adaptive_link,
            "tx_power": app.settings.tx_power,
            "audio_volume": app.settings.audio_volume,
            "transfer_size": app.settings.transfer_size,
            "vpn_enabled": app.settings.vpn_enabled,
            "key_bytes": app.settings.key_bytes.len(),
            "key_is_default": app.settings.key_bytes == crate::settings::DEFAULT_KEY_BYTES,
            "key_sha256": sha256_hex(&app.settings.key_bytes),
            "telemetry": telemetry_configuration,
            "routes": routes,
            "profiles": profiles,
            "osd_profiles": osd_profiles,
        },
        "metrics": {
            "input_bytes": app.metrics.usb_bytes,
            "input_events": app.metrics.usb_transfers,
            "input_packets": app.metrics.wifi_packets,
            "usb_bytes": app.metrics.usb_bytes,
            "usb_transfers": app.metrics.usb_transfers,
            "wifi_packets": app.metrics.wifi_packets,
            "rtp_packets": app.metrics.rtp_packets,
            "encoded_frames": app.metrics.encoded_frames,
            "decoded_frames": app.metrics.decoded_frames,
            "render_frames": app.metrics.render_frames,
            "fec_total_packets": app.metrics.fec_total_packets,
            "recovered_packets": app.metrics.recovered_packets,
            "lost_packets": app.metrics.lost_packets,
            "decoder_drops": app.metrics.decoder_drops,
            "decoder_waiting_drops": app.metrics.decoder_waiting_drops,
            "decoder_backpressure_drops": app.metrics.decoder_backpressure_drops,
            "decoder_output_drops": app.metrics.decoder_output_drops,
            "decoder_transport_drops": app.metrics.decoder_transport_drops,
            "decoder_frames_in_flight": app.metrics.decoder_frames_in_flight,
            "decoder_max_latency_ms": app.metrics.decoder_max_latency_ms,
            "decoder_errors": app.metrics.decoder_errors,
            "bitrate_bps": app.metrics.bitrate_bps,
            "receive_fps": app.metrics.receive_fps,
            "decode_fps": app.metrics.decode_fps,
            "render_fps": app.metrics.render_fps,
            "rssi": app.metrics.rssi,
            "snr": app.metrics.snr,
            "link_score": app.metrics.link_score,
            "usb_latency_ms": app.metrics.usb_latency_ms,
            "parse_latency_ms": app.metrics.parse_latency_ms,
            "pipeline_latency_ms": app.metrics.pipeline_latency_ms,
            "route_latency_ms": app.metrics.route_latency_ms,
            "decode_submit_latency_ms": app.metrics.decode_submit_latency_ms,
            "video_submit_path_ms": app.metrics.video_submit_path_ms,
            "decode_latency_ms": app.metrics.decode_latency_ms,
            "presentation_queue_latency_ms": app.metrics.presentation_queue_latency_ms,
            "resolution": app.metrics.resolution,
            "decoder": app.metrics.decoder_name,
        },
        "receive_milestones_seconds": app.diagnostics.milestones,
        "pipeline_error_histogram": app.diagnostics.pipeline_errors,
        "packet_counters": packet_counters_json(app.diagnostics.counters),
        "rtp_status": rtp_status_json(app.diagnostics.rtp),
        "rtp_reorder_status": rtp_reorder_json(app.diagnostics.reorder),
        "diversity": {
            "accepted": app.diagnostics.diversity.accepted,
            "duplicates": app.diagnostics.diversity.duplicates,
            "passthrough": app.diagnostics.diversity.passthrough,
            "cached_packets": app.diagnostics.diversity.cached_packets,
            "adapters": adapter_metrics,
        },
        "stage_latencies": stage_latencies,
        "route_metrics": route_metrics,
        "telemetry": telemetry,
        "preflight": preflight,
        "audio": {
            "enabled": app.audio.enabled,
            "supported": app.audio.supported,
            "decoder": app.audio.decoder_name,
            "packets": app.audio.packets,
            "bytes": app.audio.bytes,
            "decoded_frames": app.audio.decoded_frames,
            "errors": app.audio.errors,
            "queued_ms": app.audio.queued_ms,
        },
        "vpn": {
            "active": app.vpn.active,
            "interface_name": app.vpn.interface_name,
            "downlink_packets": app.vpn.downlink_packets,
            "downlink_bytes": app.vpn.downlink_bytes,
            "uplink_packets": app.vpn.uplink_packets,
            "uplink_bytes": app.vpn.uplink_bytes,
            "errors": app.vpn.errors,
        },
        "vtx_control": {
            "state": format!("{:?}", app.vtx_control.state),
            "video_mode": app.vtx_control.video_mode,
            "tunnel_packets_received": app.vtx_control.network.tunnel_packets_received,
            "tunnel_bytes_received": app.vtx_control.network.tunnel_bytes_received,
            "tunnel_packets_sent": app.vtx_control.network.tunnel_packets_sent,
            "tunnel_bytes_sent": app.vtx_control.network.tunnel_bytes_sent,
            "malformed_tunnel_packets": app.vtx_control.network.malformed_tunnel_packets,
            "tcp_connections_opened": app.vtx_control.network.tcp_connections_opened,
            "tcp_connection_failures": app.vtx_control.network.tcp_connection_failures,
            "tcp_connections_active": app.vtx_control.network.tcp_connections_active,
            "udp_datagrams_queued": app.vtx_control.network.udp_datagrams_queued,
            "udp_bytes_queued": app.vtx_control.network.udp_bytes_queued,
            "udp_send_failures": app.vtx_control.network.udp_send_failures,
            "raw_ip_packets_queued": app.vtx_control.network.raw_ip_packets_queued,
            "raw_ip_bytes_queued": app.vtx_control.network.raw_ip_bytes_queued,
            "raw_ip_send_failures": app.vtx_control.network.raw_ip_send_failures,
            "inbound_queue_full": app.vtx_control.network.inbound_queue_full,
            "outbound_queue_full": app.vtx_control.network.outbound_queue_full,
            "scheduler": uplink_scheduler,
            "configuration": vtx_configuration,
        },
        "channel_scan": scan_results,
    });
    let report = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("serialize support report failed: {error}"))?;
    let driver_init = serde_json::to_vec_pretty(&driver_init)
        .map_err(|error| format!("serialize driver initialization report failed: {error}"))?;
    let visible_logs = format_logs(app.logs.iter());
    let session_logs = format_logs(app.support_logs.iter());
    let manifest = serde_json::to_vec_pretty(&json!({
        "schema_version": 2,
        "generated_unix_seconds": timestamp,
        "files": {
            "report.json": "Sanitized application, configuration, pipeline, media, and runtime state",
            "driver_init.json": "Persistent probe, EFUSE, initialization-stage, register-trace, and post-init evidence",
            "logs.txt": "Current filtered log-panel contents",
            "session-logs.txt": "Latest bounded session log history before UI verbosity filtering",
            "README.txt": "Bundle contents and privacy notes",
        },
        "redacted_or_excluded": [
            "WFB key bytes",
            "MAVLink signing key bytes",
            "VTX SSH password and private credentials",
            "absolute home-directory prefixes",
            "recorded video and audio payloads",
        ],
        "included_sensitive_hardware_evidence": [
            "SHA-256 fingerprints (never bytes) of configured WFB and MAVLink keys",
            "USB identifiers and device labels",
            "EFUSE fingerprint and decoded board calibration summary",
            "raw register values used during driver initialization",
            "bounded USB and malformed-descriptor hex samples",
        ],
    }))
    .map_err(|error| format!("serialize support manifest failed: {error}"))?;

    let bytes = archive(
        &manifest,
        &report,
        &driver_init,
        &visible_logs,
        &session_logs,
    )?;
    Ok(SupportBundle {
        filename: format!("nebulus-support-{timestamp}.zip"),
        bytes,
    })
}

fn receiver_summary_json(receiver: &crate::runtime::ReceiverInfo) -> Value {
    let diagnostics = receiver.driver_diagnostics.as_ref();
    json!({
        "transport": format!("{:?}", receiver.transport),
        "id": receiver.id,
        "source_id": receiver.source_id,
        "label": receiver.label,
        "vendor_id": receiver.vendor_id.map(|id| format!("{id:04x}")),
        "product_id": receiver.product_id.map(|id| format!("{id:04x}")),
        "chip": receiver.chip,
        "rf_paths": receiver.rf_paths,
        "cut_version": receiver.cut_version,
        "usb_speed": receiver.usb_speed,
        "bulk_in_endpoint": receiver.bulk_in_endpoint.map(|endpoint| format!("0x{endpoint:02x}")),
        "bulk_out_endpoint": receiver.bulk_out_endpoint.map(|endpoint| format!("0x{endpoint:02x}")),
        "initialization": receiver.initialization,
        "firmware_downloaded": receiver.firmware_downloaded,
        "rx_descriptor": receiver.rx_descriptor,
        "failure": receiver.failure.as_deref().map(sanitize),
        "driver_diagnostics": diagnostics.map(|diagnostics| json!({
            "schema_version": diagnostics.schema_version,
            "completed": diagnostics.completed,
            "duration_us": diagnostics.duration_us,
            "stages": diagnostics.stages.len(),
            "register_io": register_io_json(&diagnostics.register_io),
            "register_trace_entries": diagnostics.register_trace.len(),
            "register_trace_dropped": diagnostics.register_trace_dropped,
            "error": diagnostics.error.as_deref().map(sanitize),
        })),
    })
}

fn receiver_driver_json(receiver: &crate::runtime::ReceiverInfo) -> Value {
    json!({
        "receiver": receiver_summary_json(receiver),
        "driver": receiver
            .driver_diagnostics
            .as_ref()
            .map(|diagnostics| driver_diagnostics_json(diagnostics)),
    })
}

fn driver_diagnostics_json(diagnostics: &openipc_rtl88xx::DriverDiagnostics) -> Value {
    json!({
        "schema_version": diagnostics.schema_version,
        "started_us": diagnostics.started_us,
        "duration_us": diagnostics.duration_us,
        "completed": diagnostics.completed,
        "error": diagnostics.error.as_deref().map(sanitize),
        "radio": {
            "channel": diagnostics.channel,
            "channel_width_mhz": diagnostics.channel_width_mhz,
            "channel_offset": diagnostics.channel_offset,
        },
        "effective_options": diagnostics.effective_options,
        "probe": diagnostics.probe.as_ref().map(|probe| json!({
            "vendor_id": format!("{:04x}", probe.vendor_id),
            "product_id": format!("{:04x}", probe.product_id),
            "sys_cfg": format!("0x{:08x}", probe.sys_cfg),
            "sys_cfg2_chip_id": format!("0x{:02x}", probe.sys_cfg2_chip_id),
            "selected_family": probe.chip.family.name(),
            "selected_cut_version": probe.chip.cut_version,
            "selected_rf_type": format!("{:?}", probe.chip.rf_type),
            "selected_rx_descriptor": format!("{:?}", probe.rx_descriptor),
        })),
        "efuse": diagnostics.efuse.as_ref().map(|efuse| json!({
            "eeprom_id": format!("0x{:04x}", efuse.eeprom_id),
            "autoload_valid": efuse.autoload_valid,
            "logical_map_fingerprint": format!("0x{:016x}", efuse.map_fingerprint),
            "programmed_bytes": efuse.programmed_bytes,
            "rfe_type": efuse.rfe_type,
            "board_type": format!("0x{:02x}", efuse.board_type),
            "external_pa_2g": efuse.external_pa_2g,
            "external_pa_5g": efuse.external_pa_5g,
            "external_lna_2g": efuse.external_lna_2g,
            "external_lna_5g": efuse.external_lna_5g,
            "crystal_cap": efuse.crystal_cap,
            "thermal_meter": efuse.thermal_meter,
            "thermal_meter_paths": efuse.thermal_meter_paths,
            "tx_power_source": if efuse.tx_power_defaults { "IC defaults" } else { "EFUSE" },
            "mac_present": efuse.mac_present,
        })),
        "stages": diagnostics.stages.iter().map(|stage| json!({
            "name": stage.name,
            "started_us": stage.started_us,
            "duration_us": stage.duration_us,
            "success": stage.success,
            "error": stage.error.as_deref().map(sanitize),
            "register_io": register_io_json(&stage.register_io),
        })).collect::<Vec<_>>(),
        "register_io": register_io_json(&diagnostics.register_io),
        "register_trace": diagnostics.register_trace.iter().map(|entry| json!({
            "sequence": entry.sequence,
            "offset_us": entry.offset_us,
            "stage": entry.stage,
            "operation": entry.operation,
            "register": format!("0x{:04x}", entry.register),
            "length": entry.bytes.len(),
            "bytes_hex": hex_bytes(&entry.bytes),
            "success": entry.success,
            "error": entry.error.as_deref().map(sanitize),
        })).collect::<Vec<_>>(),
        "register_trace_dropped": diagnostics.register_trace_dropped,
        "post_init_registers": diagnostics.post_init_registers.iter().map(|register| json!({
            "name": register.name,
            "address": format!("0x{:04x}", register.address),
            "width": register.width,
            "value": register.value,
            "error": register.error.as_deref().map(sanitize),
        })).collect::<Vec<_>>(),
    })
}

fn register_io_json(io: &openipc_rtl88xx::RegisterIoDiagnostics) -> Value {
    json!({
        "reads": io.reads,
        "writes": io.writes,
        "read_bytes": io.read_bytes,
        "write_bytes": io.write_bytes,
        "failures": io.failures,
        "ordered_fingerprint": format!("0x{:016x}", io.fingerprint),
    })
}

fn packet_counters_json(counters: openipc_core::ReceiverBatchCounters) -> Value {
    json!({
        "realtek_packets": counters.packets,
        "accepted_packets": counters.accepted_packets,
        "wifi_frames": counters.wifi_frames,
        "matching_channel_frames": counters.matched_frames,
        "wifi_parse_dropped": counters.wifi_parse_dropped,
        "dropped_packets": counters.dropped_packets,
        "crc_dropped": counters.crc_dropped,
        "icv_dropped": counters.icv_dropped,
        "report_packets_dropped": counters.report_dropped,
        "ignored_frames": counters.ignored_frames,
        "wfb_sessions": counters.sessions,
        "wfb_payloads": counters.wfb_payloads,
        "rtp_packets": counters.rtp_packets,
        "video_frames": counters.video_frames,
        "raw_payload_count": counters.raw_payload_count,
        "raw_payload_bytes": counters.raw_payload_bytes,
        "route_errors": counters.route_errors,
    })
}

fn rtp_status_json(status: openipc_core::RtpDepacketizerStatus) -> Value {
    json!({
        "packets": status.packets,
        "frames_emitted": status.frames_emitted,
        "config_wait_drops": status.config_wait_drops,
        "keyframes_with_prepended_config": status.keyframes_with_prepended_config,
        "parameter_sets_prepended": status.parameter_sets_prepended,
        "fragment_sequence_gaps": status.fragment_sequence_gaps,
        "damaged_frames_forwarded": status.damaged_frames_forwarded,
        "damaged_frames_dropped": status.damaged_frames_dropped,
        "fragment_overflows": status.fragment_overflows,
        "unsupported_payloads": status.unsupported_payloads,
        "malformed_packets": status.malformed_packets,
        "last_payload_type": status.last_payload_type,
        "last_sequence_number": status.last_sequence_number,
        "last_timestamp": status.last_timestamp,
        "last_codec": status.last_codec.map(|codec| format!("{codec:?}")),
        "last_nal_type": status.last_nal_type,
        "codec_config": {
            "h264_sps": status.codec_config.h264_sps,
            "h264_pps": status.codec_config.h264_pps,
            "h265_vps": status.codec_config.h265_vps,
            "h265_sps": status.codec_config.h265_sps,
            "h265_pps": status.codec_config.h265_pps,
        },
    })
}

fn rtp_reorder_json(status: openipc_core::RtpReorderStatus) -> Value {
    json!({
        "buffered_packets": status.buffered_packets,
        "reordered_packets": status.reordered_packets,
        "late_packets": status.late_packets,
        "forced_flushes": status.forced_flushes,
    })
}

fn format_logs<'a>(entries: impl IntoIterator<Item = &'a crate::model::LogEntry>) -> String {
    entries
        .into_iter()
        .map(|entry| {
            format!(
                "#{:<10} {:>10.3} {:<5} {:<32} {}",
                entry.sequence,
                entry.elapsed_seconds,
                entry.level.label(),
                entry.target,
                sanitize(&entry.message)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn sha256_hex(bytes: &[u8]) -> Option<String> {
    use sha2::{Digest as _, Sha256};

    (!bytes.is_empty()).then(|| hex_bytes(&Sha256::digest(bytes)))
}

fn archive(
    manifest: &[u8],
    report: &[u8],
    driver_init: &[u8],
    logs: &str,
    session_logs: &str,
) -> Result<Vec<u8>, String> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("manifest.json", options)
            .map_err(|error| format!("create manifest entry failed: {error}"))?;
        zip.write_all(manifest)
            .map_err(|error| format!("write manifest entry failed: {error}"))?;
        zip.start_file("report.json", options)
            .map_err(|error| format!("create report entry failed: {error}"))?;
        zip.write_all(report)
            .map_err(|error| format!("write report entry failed: {error}"))?;
        zip.start_file("driver_init.json", options)
            .map_err(|error| format!("create driver initialization entry failed: {error}"))?;
        zip.write_all(driver_init)
            .map_err(|error| format!("write driver initialization entry failed: {error}"))?;
        zip.start_file("logs.txt", options)
            .map_err(|error| format!("create logs entry failed: {error}"))?;
        zip.write_all(logs.as_bytes())
            .map_err(|error| format!("write logs entry failed: {error}"))?;
        zip.start_file("session-logs.txt", options)
            .map_err(|error| format!("create session logs entry failed: {error}"))?;
        zip.write_all(session_logs.as_bytes())
            .map_err(|error| format!("write session logs entry failed: {error}"))?;
        zip.start_file("README.txt", options)
            .map_err(|error| format!("create bundle README failed: {error}"))?;
        zip.write_all(
            b"Nebulus support bundle\n\nStart with report.json and its receive_milestones_seconds field to find the first pipeline stage that did not occur. driver_init.json preserves every receiver attempt, raw probe decisions, decoded EFUSE state, timed initialization stages, a bounded ordered register trace with actual values, and post-init register snapshots. session-logs.txt contains the latest bounded session history; logs.txt is only the currently visible filtered log panel.\n\nSecret key material, VTX credentials, media payloads, and absolute home-directory prefixes are excluded. Hardware identifiers, EFUSE fingerprints, register values, and bounded packet samples are included because they are required to compare a failing adapter against a working Devourer/OpenIPC-WASM trace. See manifest.json for the complete disclosure.\n",
        )
        .map_err(|error| format!("write bundle README failed: {error}"))?;
        zip.finish()
            .map_err(|error| format!("finish support bundle failed: {error}"))?;
    }
    Ok(output.into_inner())
}

fn sanitize(message: &str) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(home) = std::env::var_os("HOME")
        .and_then(|home| home.into_string().ok())
        .filter(|home| !home.is_empty())
    {
        return message.replace(&home, "~");
    }
    message.to_owned()
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub(crate) fn save(bundle: SupportBundle) -> Result<String, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Save Nebulus support bundle")
        .set_file_name(&bundle.filename)
        .add_filter("ZIP archive", &["zip"])
        .save_file()
    else {
        return Ok("Support bundle export cancelled".to_owned());
    };
    std::fs::write(&path, bundle.bytes)
        .map_err(|error| format!("write {} failed: {error}", path.display()))?;
    Ok(format!("Support bundle saved to {}", path.display()))
}

#[cfg(target_os = "android")]
pub(crate) fn save(bundle: SupportBundle) -> Result<String, String> {
    crate::android::save_file(&bundle.filename, &bundle.bytes)?;
    Ok("Android document picker opened for the support bundle".to_owned())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn save(bundle: SupportBundle) -> Result<String, String> {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast as _;

    let parts = js_sys::Array::new();
    let bytes = js_sys::Uint8Array::from(bundle.bytes.as_slice());
    parts.push(&bytes.buffer());
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/zip");
    let blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &options)
        .map_err(js_error)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_error)?;
    let window = web_sys::window().ok_or_else(|| "browser window is unavailable".to_owned())?;
    let document = window
        .document()
        .ok_or_else(|| "browser document is unavailable".to_owned())?;
    let body = document
        .body()
        .ok_or_else(|| "browser document body is unavailable".to_owned())?;
    let anchor = document
        .create_element("a")
        .map_err(js_error)?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "could not create support-bundle download link".to_owned())?;
    anchor.set_href(&url);
    anchor.set_download(&bundle.filename);
    body.append_child(&anchor).map_err(js_error)?;
    anchor.click();

    let revoke_url = url.clone();
    let cleanup_body = body;
    let cleanup_anchor = anchor;
    let revoke = Closure::once_into_js(move || {
        let _ = cleanup_body.remove_child(&cleanup_anchor);
        let _ = web_sys::Url::revoke_object_url(&revoke_url);
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 1_000)
        .map_err(js_error)?;
    Ok(format!("Downloaded {}", bundle.filename))
}

#[cfg(target_arch = "wasm32")]
fn js_error(error: wasm_bindgen::JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read as _};

    #[test]
    fn archive_contains_structured_driver_and_session_evidence() {
        let bytes = super::archive(
            br#"{"schema_version":2}"#,
            br#"{"version":"test"}"#,
            br#"{"attempts":[]}"#,
            "INFO visible",
            "INFO session",
        )
        .expect("archive");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");
        assert_eq!(archive.len(), 6);

        let mut report = String::new();
        archive
            .by_name("report.json")
            .expect("report entry")
            .read_to_string(&mut report)
            .expect("read report");
        assert_eq!(report, r#"{"version":"test"}"#);
        assert!(archive.by_name("manifest.json").is_ok());
        assert!(archive.by_name("driver_init.json").is_ok());
        assert!(archive.by_name("logs.txt").is_ok());
        assert!(archive.by_name("session-logs.txt").is_ok());
        assert!(archive.by_name("README.txt").is_ok());
    }

    #[test]
    fn sanitize_hides_home_directory_prefix() {
        let message = super::sanitize("no absolute path required");
        assert_eq!(message, "no absolute path required");
    }
}
