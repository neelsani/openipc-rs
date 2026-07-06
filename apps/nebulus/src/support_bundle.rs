use std::io::{Cursor, Write as _};

use serde_json::json;
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
                "codec": profile.codec_preference.label(),
                "adaptive_link": profile.adaptive_link,
                "vpn": profile.vpn_enabled,
                "key_bytes": profile.key_bytes.len(),
                "telemetry_signing": profile.telemetry.mavlink_signing.label(),
                "telemetry_signing_key_bytes": profile.telemetry.mavlink_signing_key.len(),
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
    let receivers = app
        .receiver_infos
        .iter()
        .map(|receiver| {
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
                "bulk_in_endpoint": receiver.bulk_in_endpoint,
                "bulk_out_endpoint": receiver.bulk_out_endpoint,
                "initialization": receiver.initialization,
                "firmware_downloaded": receiver.firmware_downloaded,
            })
        })
        .collect::<Vec<_>>();
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
    let report = json!({
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
            "decoder_errors": app.metrics.decoder_errors,
            "bitrate_bps": app.metrics.bitrate_bps,
            "receive_fps": app.metrics.receive_fps,
            "decode_fps": app.metrics.decode_fps,
            "render_fps": app.metrics.render_fps,
            "rssi": app.metrics.rssi,
            "snr": app.metrics.snr,
            "link_score": app.metrics.link_score,
            "usb_latency_ms": app.metrics.usb_latency_ms,
            "pipeline_latency_ms": app.metrics.pipeline_latency_ms,
            "decode_latency_ms": app.metrics.decode_latency_ms,
            "presentation_queue_latency_ms": app.metrics.presentation_queue_latency_ms,
            "resolution": app.metrics.resolution,
            "decoder": app.metrics.decoder_name,
        },
        "packet_counters": format!("{:?}", app.diagnostics.counters),
        "rtp_status": format!("{:?}", app.diagnostics.rtp),
        "rtp_reorder_status": format!("{:?}", app.diagnostics.reorder),
        "diversity": {
            "accepted": app.diagnostics.diversity.accepted,
            "duplicates": app.diagnostics.diversity.duplicates,
            "passthrough": app.diagnostics.diversity.passthrough,
            "cached_packets": app.diagnostics.diversity.cached_packets,
            "adapters": app.adapter_metrics.iter().map(|adapter| json!({
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
            })).collect::<Vec<_>>(),
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
        },
        "channel_scan": scan_results,
    });
    let report = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("serialize support report failed: {error}"))?;
    let logs = app
        .logs
        .iter()
        .map(|entry| {
            format!(
                "{:>10.3} {:<5} {:<32} {}",
                entry.elapsed_seconds,
                entry.level.label(),
                entry.target,
                sanitize(&entry.message)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let bytes = archive(&report, &logs)?;
    Ok(SupportBundle {
        filename: format!("nebulus-support-{timestamp}.zip"),
        bytes,
    })
}

fn archive(report: &[u8], logs: &str) -> Result<Vec<u8>, String> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("report.json", options)
            .map_err(|error| format!("create report entry failed: {error}"))?;
        zip.write_all(report)
            .map_err(|error| format!("write report entry failed: {error}"))?;
        zip.start_file("logs.txt", options)
            .map_err(|error| format!("create logs entry failed: {error}"))?;
        zip.write_all(logs.as_bytes())
            .map_err(|error| format!("write logs entry failed: {error}"))?;
        zip.start_file("README.txt", options)
            .map_err(|error| format!("create bundle README failed: {error}"))?;
        zip.write_all(
            b"Nebulus support bundle\n\nThe WFB key itself is intentionally excluded. report.json contains configuration, hardware, pipeline, and performance state; logs.txt contains the bounded in-app log history.\n",
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
    fn archive_contains_report_logs_and_readme() {
        let bytes = super::archive(br#"{"version":"test"}"#, "INFO test").expect("archive");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");
        assert_eq!(archive.len(), 3);

        let mut report = String::new();
        archive
            .by_name("report.json")
            .expect("report entry")
            .read_to_string(&mut report)
            .expect("read report");
        assert_eq!(report, r#"{"version":"test"}"#);
        assert!(archive.by_name("logs.txt").is_ok());
        assert!(archive.by_name("README.txt").is_ok());
    }
}
