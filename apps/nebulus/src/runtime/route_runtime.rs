use std::collections::{BTreeMap, BTreeSet};

use openipc_core::{
    ChannelId, PayloadRouteId, RadioPort, ReceiverBatchOptions, ReceiverRuntime, RtpPayloadTap,
    WfbKeypair,
};
use web_time::Instant;

use crate::{
    audio::AudioPlayer,
    model::AudioStats,
    recording::{AudioTrackConfig, RecordedAudioPacket},
    settings::{PayloadRouteSettings, RouteAction},
    telemetry::{TelemetryDecoder, TelemetryUpdate},
};

use super::{RouteMetricDelta, StartRequest};

pub(super) const VPN_ROUTE_ID: PayloadRouteId = PayloadRouteId::new(u64::MAX);

fn route_id_is_reserved(id: u64) -> bool {
    id <= 1 || id == VPN_ROUTE_ID.raw()
}

pub(super) struct RouteLog {
    pub(super) warning: bool,
    pub(super) message: String,
}

struct ActiveRoute {
    settings: PayloadRouteSettings,
    audio: Option<AudioPlayer>,
    audio_unavailable: Option<AudioStats>,
    #[cfg(not(target_arch = "wasm32"))]
    udp: Option<std::net::UdpSocket>,
    telemetry: Option<TelemetryDecoder>,
    telemetry_announced: bool,
    last_log: Option<Instant>,
}

pub(super) struct RouteProcessor {
    routes: BTreeMap<u64, ActiveRoute>,
    recording_audio_route: Option<u64>,
    startup_logs: Vec<RouteLog>,
}

impl RouteProcessor {
    pub(super) fn new(request: &StartRequest) -> Result<Self, String> {
        let mut routes = BTreeMap::new();
        let mut startup_logs = Vec::new();
        for settings in request.payload_routes.iter().filter(|route| route.enabled) {
            if route_id_is_reserved(settings.id) {
                return Err(format!(
                    "payload route id {} is reserved by the receiver",
                    settings.id
                ));
            }
            if routes.contains_key(&settings.id) {
                return Err(format!("duplicate payload route id {}", settings.id));
            }
            #[cfg(target_arch = "wasm32")]
            if settings.action == RouteAction::Udp {
                startup_logs.push(RouteLog {
                    warning: true,
                    message: format!(
                        "{} disabled: UDP forwarding is unavailable in browsers",
                        settings.name
                    ),
                });
                continue;
            }

            let (audio, audio_unavailable) = if settings.action == RouteAction::Audio {
                match AudioPlayer::new(
                    settings.sample_rate,
                    settings.channels,
                    request.audio_volume,
                ) {
                    Ok(player) => (Some(player), None),
                    Err(error) => {
                        startup_logs.push(RouteLog {
                            warning: true,
                            message: format!("{} audio unavailable: {error}", settings.name),
                        });
                        (
                            None,
                            Some(AudioStats {
                                enabled: true,
                                supported: false,
                                decoder_name: "Unavailable".to_owned(),
                                errors: 1,
                                ..AudioStats::default()
                            }),
                        )
                    }
                }
            } else {
                (None, None)
            };

            #[cfg(not(target_arch = "wasm32"))]
            let udp = if settings.action == RouteAction::Udp {
                let socket = std::net::UdpSocket::bind("0.0.0.0:0")
                    .map_err(|error| format!("{} UDP bind failed: {error}", settings.name))?;
                socket
                    .connect((settings.udp_host.as_str(), settings.udp_port))
                    .map_err(|error| {
                        format!("{} UDP destination failed: {error}", settings.name)
                    })?;
                Some(socket)
            } else {
                None
            };

            routes.insert(
                settings.id,
                ActiveRoute {
                    telemetry: (settings.action == RouteAction::Telemetry).then(|| {
                        TelemetryDecoder::new(settings.telemetry_protocol, &request.telemetry)
                    }),
                    settings: settings.clone(),
                    audio,
                    audio_unavailable,
                    #[cfg(not(target_arch = "wasm32"))]
                    udp,
                    telemetry_announced: false,
                    last_log: None,
                },
            );
        }
        let recording_audio_route = routes
            .iter()
            .find_map(|(id, route)| (route.settings.action == RouteAction::Audio).then_some(*id));
        Ok(Self {
            routes,
            recording_audio_route,
            startup_logs,
        })
    }

    pub(super) fn take_startup_logs(&mut self) -> Vec<RouteLog> {
        std::mem::take(&mut self.startup_logs)
    }

    pub(super) fn set_audio_volume(&mut self, volume: u8) {
        for route in self.routes.values_mut() {
            if let Some(audio) = route.audio.as_mut() {
                audio.set_volume(volume);
            }
        }
    }

    pub(super) fn process(
        &mut self,
        payloads: &[openipc_core::RoutePayload],
        capture_audio: bool,
    ) -> (
        Vec<RouteMetricDelta>,
        Vec<RouteLog>,
        Vec<RecordedAudioPacket>,
        Option<TelemetryUpdate>,
    ) {
        let mut updates = BTreeMap::<u64, RouteMetricDelta>::new();
        let mut logs = Vec::new();
        let mut recorded_audio = Vec::new();
        let mut telemetry = TelemetryUpdate::default();
        for payload in payloads {
            let Some(route) = self.routes.get_mut(&payload.route_id.raw()) else {
                continue;
            };
            let update = updates
                .entry(route.settings.id)
                .or_insert_with(|| RouteMetricDelta {
                    route_id: route.settings.id,
                    ..RouteMetricDelta::default()
                });
            update.packets = update.packets.saturating_add(1);
            update.bytes = update.bytes.saturating_add(payload.data.len() as u64);
            update.last_bytes = payload.data.len();

            let result = match route.settings.action {
                RouteAction::Inspect => Ok(()),
                RouteAction::Log => {
                    if log_due(&mut route.last_log) {
                        logs.push(RouteLog {
                            warning: false,
                            message: format!(
                                "{} seq={} bytes={} preview={}",
                                route.settings.name,
                                payload.packet_seq,
                                payload.data.len(),
                                hex_preview(&payload.data)
                            ),
                        });
                    }
                    Ok(())
                }
                RouteAction::Audio => {
                    if capture_audio && self.recording_audio_route == Some(route.settings.id) {
                        if let Ok(header) = openipc_core::RtpHeader::parse(&payload.data) {
                            recorded_audio.push(RecordedAudioPacket {
                                timestamp: header.timestamp,
                                data: header.payload(&payload.data).to_vec(),
                            });
                        }
                    }
                    route
                        .audio
                        .as_mut()
                        .ok_or_else(|| "audio output unavailable".to_owned())
                        .and_then(|audio| audio.push_rtp(&payload.data))
                }
                RouteAction::Telemetry => match route.telemetry.as_mut() {
                    Some(decoder) => {
                        let decoded = decoder.push(&payload.data);
                        if !decoded.is_empty() {
                            if !route.telemetry_announced {
                                let protocol = match (decoded.protocol, decoded.mavlink_version) {
                                    (
                                        Some(crate::telemetry::TelemetryProtocol::Mavlink),
                                        Some(version),
                                    ) => {
                                        format!("MAVLink v{version}")
                                    }
                                    (Some(protocol), _) => protocol.label().to_owned(),
                                    (None, _) => "telemetry".to_owned(),
                                };
                                let blocked = decoded.counters.accepted_frames == 0
                                    && (decoded.counters.rejected_frames > 0
                                        || decoded.counters.filtered_frames > 0);
                                logs.push(RouteLog {
                                    warning: blocked,
                                    message: if blocked {
                                        format!(
                                            "Telemetry protocol detected: {protocol} (route {}, radio port 0x{:02x}); frame blocked by telemetry policy",
                                            route.settings.name, route.settings.radio_port
                                        )
                                    } else {
                                        format!(
                                            "Telemetry protocol detected: {protocol} (route {}, radio port 0x{:02x})",
                                            route.settings.name, route.settings.radio_port
                                        )
                                    },
                                });
                                route.telemetry_announced = true;
                            }
                            telemetry.merge(decoded);
                        }
                        Ok(())
                    }
                    None => Err("telemetry decoder unavailable".to_owned()),
                },
                RouteAction::Udp => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        match route.udp.as_ref() {
                            Some(socket) => socket
                                .send(&payload.data)
                                .map(|_| ())
                                .map_err(|error| format!("UDP send failed: {error}")),
                            None => Err("UDP socket unavailable".to_owned()),
                        }
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        Err("UDP forwarding is unavailable in browsers".to_owned())
                    }
                }
            };
            if let Err(error) = result {
                update.errors = update.errors.saturating_add(1);
                if let Some(audio) = route.audio.as_mut() {
                    audio.record_error();
                }
                if log_due(&mut route.last_log) {
                    logs.push(RouteLog {
                        warning: true,
                        message: format!("{}: {error}", route.settings.name),
                    });
                }
            }
        }
        (
            updates.into_values().collect(),
            logs,
            recorded_audio,
            (!telemetry.is_empty()).then_some(telemetry),
        )
    }

    pub(super) fn recording_audio_config(&self) -> Option<AudioTrackConfig> {
        let id = self.recording_audio_route?;
        let route = self.routes.get(&id)?;
        Some(AudioTrackConfig {
            // Opus RTP always uses a 48 kHz timestamp clock, regardless of the
            // decoder output rate selected for playback.
            sample_rate: 48_000,
            channels: route.settings.channels.max(1),
        })
    }

    pub(super) fn audio_stats(&self) -> AudioStats {
        let mut combined = AudioStats::default();
        for route in self.routes.values() {
            let stats = route
                .audio
                .as_ref()
                .map(AudioPlayer::stats)
                .or_else(|| route.audio_unavailable.clone());
            let Some(stats) = stats else {
                continue;
            };
            combined.enabled |= stats.enabled;
            combined.supported |= stats.supported;
            if combined.decoder_name.is_empty() {
                combined.decoder_name = stats.decoder_name;
            }
            combined.packets = combined.packets.saturating_add(stats.packets);
            combined.bytes = combined.bytes.saturating_add(stats.bytes);
            combined.decoded_frames = combined.decoded_frames.saturating_add(stats.decoded_frames);
            combined.errors = combined.errors.saturating_add(stats.errors);
            combined.queued_ms = combined.queued_ms.max(stats.queued_ms);
        }
        combined
    }
}

pub(super) fn configure_receiver(
    receiver: &mut ReceiverRuntime,
    request: &StartRequest,
) -> Result<ReceiverBatchOptions, String> {
    let mut options = ReceiverBatchOptions::default();
    let mut ids = BTreeSet::new();
    for route in request.payload_routes.iter().filter(|route| route.enabled) {
        #[cfg(target_arch = "wasm32")]
        if route.action == RouteAction::Udp {
            continue;
        }
        if route_id_is_reserved(route.id) || !ids.insert(route.id) {
            return Err(format!(
                "invalid or duplicate payload route id {}",
                route.id
            ));
        }
        let route_id = PayloadRouteId::new(route.id);
        let channel_id =
            ChannelId::from_link_port(request.channel_id >> 8, RadioPort::Custom(route.radio_port));
        let keypair = WfbKeypair::from_bytes(&request.key_bytes)
            .map_err(|error| format!("{} key is invalid: {error}", route.name))?;
        receiver
            .add_keyed_route(route_id, channel_id, 0, keypair, request.minimum_epoch)
            .map_err(|error| format!("{} route setup failed: {error}", route.name))?;
        if route.action == RouteAction::Audio {
            options.rtp_payload_taps.push(RtpPayloadTap {
                route_id,
                payload_type: route.payload_type.min(127),
            });
        } else {
            options.raw_payload_routes.push(route_id);
        }
    }
    if request.vpn_enabled {
        let keypair = WfbKeypair::from_bytes(&request.key_bytes)
            .map_err(|error| format!("VPN key is invalid: {error}"))?;
        receiver
            .add_keyed_route(
                VPN_ROUTE_ID,
                ChannelId::from_link_port(request.channel_id >> 8, RadioPort::TunnelRx),
                0,
                keypair,
                request.minimum_epoch,
            )
            .map_err(|error| format!("VPN route setup failed: {error}"))?;
        options.raw_payload_routes.push(VPN_ROUTE_ID);
    }
    Ok(options)
}

#[cfg(debug_assertions)]
pub(super) fn configure_mock_receiver(
    receiver: &mut ReceiverRuntime,
    request: &StartRequest,
) -> ReceiverBatchOptions {
    let mut options = ReceiverBatchOptions::default();
    let channel_id = ChannelId::new(request.channel_id);
    for route in request
        .payload_routes
        .iter()
        .filter(|route| route.enabled && route.radio_port == RadioPort::Video.as_u8())
    {
        #[cfg(target_arch = "wasm32")]
        if route.action == RouteAction::Udp {
            continue;
        }

        let route_id = PayloadRouteId::new(route.id);
        receiver.add_mock_route(route_id, channel_id, 0);
        if route.action == RouteAction::Audio {
            options.rtp_payload_taps.push(RtpPayloadTap {
                route_id,
                payload_type: route.payload_type.min(127),
            });
        } else {
            options.raw_payload_routes.push(route_id);
        }
    }
    options
}

fn log_due(last: &mut Option<Instant>) -> bool {
    let now = Instant::now();
    if last.is_some_and(|previous| now.duration_since(previous).as_secs_f32() < 1.0) {
        return false;
    }
    *last = Some(now);
    true
}

fn hex_preview(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut preview = String::new();
    for byte in bytes.iter().take(16) {
        let _ = write!(preview, "{byte:02x}");
    }
    if bytes.len() > 16 {
        preview.push_str("...");
    }
    preview
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use std::time::Duration;

    use openipc_core::{
        ChannelId, FrameLayout, PayloadRouteId, RadioPort, ReceiverRuntime, RoutePayload,
        WfbKeypair,
    };

    #[cfg(not(target_arch = "wasm32"))]
    use super::configure_mock_receiver;
    use super::{configure_receiver, RouteProcessor};
    #[cfg(not(target_arch = "wasm32"))]
    use crate::settings::RouteAction;
    use crate::{runtime::StartRequest, settings::Settings, telemetry::crc8_dvb_s2};

    fn request_from_settings(settings: Settings) -> StartRequest {
        StartRequest {
            #[cfg(target_os = "android")]
            video_output: None,
            primary_device_id: None,
            device_ids: Vec::new(),
            channel: settings.channel,
            channel_width_mhz: settings.channel_width_mhz,
            channel_offset: settings.channel_offset,
            channel_id: settings.video_channel().raw(),
            minimum_epoch: settings.minimum_epoch,
            transfer_size: settings.transfer_size,
            codec_preference: settings.codec_preference,
            rtp_reorder: false,
            adaptive_link: false,
            tx_power: settings.tx_power,
            key_bytes: settings.key_bytes,
            audio_volume: settings.audio_volume,
            vpn_enabled: settings.vpn_enabled,
            payload_routes: settings.payload_routes,
            telemetry: settings.telemetry,
        }
    }

    #[test]
    fn default_audio_route_shares_the_video_runtime() {
        let settings = Settings::default();
        let keypair = WfbKeypair::from_bytes(&settings.key_bytes).unwrap();
        let mut receiver = ReceiverRuntime::with_keyed_video_route(
            FrameLayout::WithFcs,
            PayloadRouteId::new(1),
            ChannelId::new(settings.video_channel().raw()),
            0,
            keypair,
            settings.minimum_epoch,
        )
        .unwrap();
        let request = request_from_settings(settings);

        let options = configure_receiver(&mut receiver, &request).unwrap();
        assert_eq!(receiver.routes().runtime_count(), 2);
        assert_eq!(options.raw_payload_routes, [PayloadRouteId::new(2)]);
        assert_eq!(options.rtp_payload_taps.len(), 1);
        assert_eq!(options.rtp_payload_taps[0].route_id, PayloadRouteId::new(3));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn mock_udp_route_forwards_the_raw_mixed_rtp_packet() {
        let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        listener
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let mut settings = Settings::default();
        let route = settings
            .payload_routes
            .iter_mut()
            .find(|route| route.id == 3)
            .unwrap();
        route.action = RouteAction::Udp;
        route.udp_host = "127.0.0.1".to_owned();
        route.udp_port = listener.local_addr().unwrap().port();

        let request = request_from_settings(settings);
        let mut processor = RouteProcessor::new(&request).unwrap();
        let mut receiver = ReceiverRuntime::with_mock_video_route(
            FrameLayout::WithFcs,
            PayloadRouteId::new(1),
            ChannelId::new(request.channel_id),
            0,
        );
        let options = configure_mock_receiver(&mut receiver, &request);
        assert_eq!(options.raw_payload_routes, [PayloadRouteId::new(3)]);
        assert!(options.rtp_payload_taps.is_empty());

        let packet = [0x80, 98, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0xf8, 0xff, 0xfe];
        let batch = receiver
            .push_mock_payload(receiver.video_runtime(), 1, &packet, &options)
            .unwrap();
        assert_eq!(batch.raw_payloads.len(), 1);

        let (_, logs, _, _) = processor.process(&batch.raw_payloads, false);
        assert!(logs.is_empty());
        let mut received = [0_u8; 64];
        let received_len = listener.recv(&mut received).unwrap();
        assert_eq!(&received[..received_len], packet);
    }

    #[test]
    fn vpn_adds_a_raw_tunnel_route() {
        let settings = Settings {
            vpn_enabled: true,
            ..Settings::default()
        };
        let keypair = WfbKeypair::from_bytes(&settings.key_bytes).unwrap();
        let mut receiver = ReceiverRuntime::with_keyed_video_route(
            FrameLayout::WithFcs,
            PayloadRouteId::new(1),
            settings.video_channel(),
            0,
            keypair,
            settings.minimum_epoch,
        )
        .unwrap();
        let request = request_from_settings(settings);

        let options = configure_receiver(&mut receiver, &request).unwrap();
        assert!(options.raw_payload_routes.contains(&super::VPN_ROUTE_ID));
    }

    #[test]
    fn custom_routes_cannot_use_the_internal_vpn_route_id() {
        let mut settings = Settings::default();
        settings.payload_routes[0].id = super::VPN_ROUTE_ID.raw();
        let request = request_from_settings(settings);

        let error = RouteProcessor::new(&request)
            .err()
            .expect("reserved route id must be rejected");
        assert!(error.contains("reserved"));
    }

    #[test]
    fn recording_tap_removes_the_opus_rtp_header() {
        let request = request_from_settings(Settings::default());
        let mut routes = RouteProcessor::new(&request).unwrap();
        let opus = [0xf8, 0xff, 0xfe];
        let mut rtp = vec![0x80, 0x80 | openipc_core::rtp::RTP_PAYLOAD_TYPE_OPUS];
        rtp.extend_from_slice(&7u16.to_be_bytes());
        rtp.extend_from_slice(&48_000u32.to_be_bytes());
        rtp.extend_from_slice(&1u32.to_be_bytes());
        rtp.extend_from_slice(&opus);
        let payload = RoutePayload {
            route_id: PayloadRouteId::new(3),
            channel_id: ChannelId::from_link_port(request.channel_id >> 8, RadioPort::Video),
            packet_seq: 9,
            data: rtp,
        };

        let (_, _, recorded, _) = routes.process(&[payload], true);

        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].timestamp, 48_000);
        assert_eq!(recorded[0].data, opus);
    }

    #[test]
    fn telemetry_route_decodes_raw_radio_payload_for_the_osd() {
        let request = request_from_settings(Settings::default());
        let mut routes = RouteProcessor::new(&request).unwrap();
        let battery = [0x00, 0xa8, 0x00, 0xeb, 0x00, 0x04, 0xd2, 81];
        let mut frame = vec![0xc8, (battery.len() + 2) as u8, 0x08];
        frame.extend_from_slice(&battery);
        frame.push(crc8_dvb_s2(&frame[2..]));
        let payload = RoutePayload {
            route_id: PayloadRouteId::new(2),
            channel_id: ChannelId::from_link_port(request.channel_id >> 8, RadioPort::TelemetryRx),
            packet_seq: 10,
            data: frame,
        };

        let (metrics, logs, _, telemetry) = routes.process(std::slice::from_ref(&payload), false);
        let telemetry = telemetry.expect("telemetry OSD update");

        assert_eq!(metrics[0].packets, 1);
        assert_eq!(telemetry.battery_voltage_v, Some(16.8));
        assert_eq!(telemetry.battery_current_a, Some(23.5));
        assert_eq!(telemetry.battery_remaining_pct, Some(81));
        assert!(logs
            .iter()
            .any(|log| log.message.contains("Telemetry protocol detected: CRSF")));

        let (_, repeated_logs, _, _) = routes.process(&[payload], false);
        assert!(!repeated_logs
            .iter()
            .any(|log| log.message.contains("Telemetry protocol detected")));
    }
}
