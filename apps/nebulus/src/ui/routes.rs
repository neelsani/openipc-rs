use eframe::egui;

use crate::{
    app::NebulusApp,
    model::{ReceiverState, RouteStats},
    settings::{PayloadRouteSettings, RouteAction},
    telemetry::TelemetryProtocol,
};
const PORTS: &[(u8, &str)] = &[
    (0x00, "Video / mixed RTP"),
    (0x10, "Telemetry RX"),
    (0x20, "Data / tunnel RX"),
    (0x30, "Audio RX"),
    (0x90, "Telemetry TX"),
    (0xa0, "Tunnel TX"),
    (0xb0, "Audio TX"),
];

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let editable = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
    let route_stats = app.route_stats.clone();
    let mut remove = None;
    let telemetry_summary = match app.telemetry.protocol {
        Some(protocol)
            if app
                .telemetry
                .is_fresh(app.settings.telemetry.stale_timeout_ms) =>
        {
            format!("{} · {} msg", protocol.label(), app.telemetry.messages)
        }
        Some(protocol) => format!("{} · stale", protocol.label()),
        None => "waiting".to_owned(),
    };

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Payload routes").strong());
        ui.separator();
        ui.label(format!("Telemetry {telemetry_summary}"));
        ui.separator();
        ui.label(format!(
            "Audio {} · {:.0} ms",
            if app.audio.enabled { "on" } else { "off" },
            app.audio.queued_ms
        ));
    });
    ui.label(
        egui::RichText::new(
            "Each enabled route receives its radio port under the current Link ID.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );
    ui.horizontal(|ui| {
        ui.label("Output volume");
        if ui
            .add(
                egui::Slider::new(&mut app.settings.audio_volume, 0..=100)
                    .suffix("%")
                    .show_value(true),
            )
            .changed()
        {
            app.runtime.set_audio_volume(app.settings.audio_volume);
        }
    });
    ui.add_space(8.0);

    let routes = &mut app.settings.payload_routes;
    for route in routes.iter_mut() {
        let stats = route_stats.get(&route.id).cloned().unwrap_or_default();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut route.enabled, "");
                    ui.add(
                        egui::TextEdit::singleline(&mut route.name)
                            .desired_width(170.0)
                            .hint_text("Route name"),
                    );
                    if ui.button("Remove").clicked() {
                        remove = Some(route.id);
                    }
                });
                ui.add_space(4.0);
                egui::Grid::new(("route-grid", route.id))
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Radio port");
                        egui::ComboBox::from_id_salt(("route-port", route.id))
                            .selected_text(port_label(route.radio_port))
                            .show_ui(ui, |ui| {
                                for &(port, name) in PORTS {
                                    ui.selectable_value(
                                        &mut route.radio_port,
                                        port,
                                        format!("{name} (0x{port:02x})"),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label("Custom port");
                        ui.add(
                            egui::DragValue::new(&mut route.radio_port)
                                .range(0..=255)
                                .hexadecimal(2, false, true),
                        );
                        ui.end_row();
                        ui.label("Action");
                        egui::ComboBox::from_id_salt(("route-action", route.id))
                            .selected_text(route.action.label())
                            .show_ui(ui, |ui| {
                                for action in [
                                    RouteAction::Inspect,
                                    RouteAction::Log,
                                    RouteAction::Telemetry,
                                    RouteAction::Audio,
                                ] {
                                    ui.selectable_value(&mut route.action, action, action.label());
                                }
                                ui.add_enabled_ui(!cfg!(target_arch = "wasm32"), |ui| {
                                    ui.selectable_value(
                                        &mut route.action,
                                        RouteAction::Udp,
                                        RouteAction::Udp.label(),
                                    );
                                });
                            });
                        ui.end_row();
                    });

                match route.action {
                    RouteAction::Audio => audio_settings(ui, route),
                    RouteAction::Telemetry => telemetry_settings(ui, route),
                    RouteAction::Udp => udp_settings(ui, route),
                    RouteAction::Inspect | RouteAction::Log => {}
                }
            });
            route_status(ui, route, &stats);
        });
        ui.add_space(6.0);
    }

    if let Some(id) = remove {
        routes.retain(|route| route.id != id);
    }
    if ui
        .add_enabled(editable, egui::Button::new("Add route"))
        .clicked()
    {
        let id = next_route_id(routes);
        routes.push(PayloadRouteSettings {
            id,
            name: format!("Route {id}"),
            ..PayloadRouteSettings::default()
        });
    }
}

fn telemetry_settings(ui: &mut egui::Ui, route: &mut PayloadRouteSettings) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Format");
        egui::ComboBox::from_id_salt(("telemetry-format", route.id))
            .selected_text(route.telemetry_protocol.label())
            .show_ui(ui, |ui| {
                for protocol in TelemetryProtocol::ALL {
                    ui.selectable_value(&mut route.telemetry_protocol, protocol, protocol.label());
                }
            });
    });
    ui.label(
        egui::RichText::new(
            "Decoded values feed the video OSD. Auto mode locks after a supported checksum-valid MAVLink, MSP, or CRSF frame.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );
}

fn next_route_id(routes: &[PayloadRouteSettings]) -> u64 {
    (2..u64::MAX)
        .find(|candidate| routes.iter().all(|route| route.id != *candidate))
        .unwrap_or(2)
}

fn audio_settings(ui: &mut egui::Ui, route: &mut PayloadRouteSettings) {
    ui.separator();
    egui::Grid::new(("audio-route", route.id))
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("Codec");
            ui.label("Opus");
            ui.end_row();
            ui.label("RTP payload type");
            ui.add(egui::DragValue::new(&mut route.payload_type).range(0..=127));
            ui.end_row();
            ui.label("Sample rate");
            egui::ComboBox::from_id_salt(("audio-rate", route.id))
                .selected_text(format!("{} Hz", route.sample_rate))
                .show_ui(ui, |ui| {
                    for rate in [8_000, 12_000, 16_000, 24_000, 48_000] {
                        ui.selectable_value(&mut route.sample_rate, rate, format!("{rate} Hz"));
                    }
                });
            ui.end_row();
            ui.label("Channels");
            ui.add(egui::DragValue::new(&mut route.channels).range(1..=2));
            ui.end_row();
        });
}

fn udp_settings(ui: &mut egui::Ui, route: &mut PayloadRouteSettings) {
    ui.separator();
    if cfg!(target_arch = "wasm32") {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "UDP forwarding is unavailable in browsers.",
        );
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Destination");
        ui.add(egui::TextEdit::singleline(&mut route.udp_host).desired_width(140.0));
        ui.add(egui::DragValue::new(&mut route.udp_port).range(1..=65_535));
    });
}

fn route_status(ui: &mut egui::Ui, route: &PayloadRouteSettings, stats: &RouteStats) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("route {}", route.id))
                .monospace()
                .small(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} pkt · {} B · {} B last · {} errors",
                    stats.packets, stats.bytes, stats.last_bytes, stats.errors
                ))
                .monospace()
                .small()
                .color(ui.visuals().weak_text_color()),
            );
        });
    });
}

fn port_label(port: u8) -> String {
    PORTS
        .iter()
        .find(|(candidate, _)| *candidate == port)
        .map(|(_, name)| format!("{name} (0x{port:02x})"))
        .unwrap_or_else(|| format!("Custom (0x{port:02x})"))
}

#[cfg(test)]
mod tests {
    use super::next_route_id;
    use crate::settings::PayloadRouteSettings;

    #[test]
    fn next_route_id_fills_gaps_and_never_uses_internal_sentinels() {
        let routes = [2, 4, u64::MAX]
            .into_iter()
            .map(|id| PayloadRouteSettings {
                id,
                ..PayloadRouteSettings::default()
            })
            .collect::<Vec<_>>();
        assert_eq!(next_route_id(&routes), 3);
    }
}
