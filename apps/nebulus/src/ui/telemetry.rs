use eframe::egui;

use crate::{
    app::NebulusApp,
    model::ReceiverState,
    settings::RouteAction,
    telemetry::{
        MavlinkSigningPolicy, MspDirectionFilter, MspVersionFilter, TelemetryProtocol,
        CRSF_ANY_ADDRESS,
    },
};

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let editable = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
    status(app, ui);
    ui.add_space(8.0);

    section(ui, "Sources", |ui| {
        let routes = app
            .settings
            .payload_routes
            .iter()
            .filter(|route| route.enabled && route.action == RouteAction::Telemetry)
            .collect::<Vec<_>>();
        if routes.is_empty() {
            ui.colored_label(ui.visuals().warn_fg_color, "No telemetry route enabled");
        } else {
            egui::Grid::new("telemetry-routes")
                .num_columns(3)
                .striped(true)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    ui.strong("Route");
                    ui.strong("Radio port");
                    ui.strong("Format");
                    ui.end_row();
                    for route in routes {
                        ui.label(&route.name);
                        ui.monospace(format!("0x{:02X}", route.radio_port));
                        ui.label(route.telemetry_protocol.label());
                        ui.end_row();
                    }
                });
        }
        if ui.button("Edit routes").clicked() {
            app.active_tab = super::PanelTab::Data;
            app.data_page = super::DataPage::Routes;
        }
    });

    section(ui, "General", |ui| {
        ui.horizontal(|ui| {
            ui.label("Stale after");
            ui.add(
                egui::Slider::new(&mut app.settings.telemetry.stale_timeout_ms, 500..=30_000)
                    .suffix(" ms")
                    .logarithmic(true),
            );
        });
        if ui.small_button("Clear live telemetry").clicked() {
            app.telemetry.reset();
        }
    });

    section(ui, "MAVLink", |ui| mavlink_settings(app, ui, editable));
    section(ui, "MSP", |ui| msp_settings(app, ui, editable));
    section(ui, "CRSF", |ui| crsf_settings(app, ui, editable));
}

fn status(app: &NebulusApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.heading("Telemetry");
        ui.separator();
        ui.label(
            app.telemetry
                .protocol
                .map_or("Waiting", TelemetryProtocol::label),
        );
        ui.separator();
        ui.monospace(format!("{} decoded", app.telemetry.messages));
        if let Some(age) = app.telemetry.age_seconds() {
            ui.separator();
            ui.monospace(format!("{age:.1}s ago"));
        }
    });

    let counters = &app.telemetry.counters;
    egui::Grid::new("telemetry-status")
        .num_columns(2)
        .striped(true)
        .spacing([18.0, 6.0])
        .show(ui, |ui| {
            row(
                ui,
                "Stream",
                app.telemetry.frame_age_seconds().map_or_else(
                    || "No frames".to_owned(),
                    |age| format!("last frame {age:.1}s ago"),
                ),
            );
            row(ui, "Accepted frames", counters.accepted_frames.to_string());
            row(ui, "Rejected frames", counters.rejected_frames.to_string());
            row(ui, "Filtered frames", counters.filtered_frames.to_string());
        });
}

fn mavlink_settings(app: &mut NebulusApp, ui: &mut egui::Ui, editable: bool) {
    ui.add_enabled_ui(editable, |ui| {
        egui::Grid::new("mavlink-settings")
            .num_columns(2)
            .spacing([18.0, 7.0])
            .show(ui, |ui| {
                ui.label("Signing policy");
                egui::ComboBox::from_id_salt("mavlink-signing-policy")
                    .selected_text(app.settings.telemetry.mavlink_signing.label())
                    .show_ui(ui, |ui| {
                        for policy in MavlinkSigningPolicy::ALL {
                            ui.selectable_value(
                                &mut app.settings.telemetry.mavlink_signing,
                                policy,
                                policy.label(),
                            );
                        }
                    });
                ui.end_row();

                id_filter(
                    ui,
                    "System ID",
                    &mut app.settings.telemetry.mavlink_system_id,
                );
                id_filter(
                    ui,
                    "Component ID",
                    &mut app.settings.telemetry.mavlink_component_id,
                );
            });

        ui.horizontal(|ui| {
            if ui.button("Open signing key").clicked() {
                app.open_mavlink_key_file(ui.ctx());
            }
            if ui
                .add_enabled(
                    !app.settings.telemetry.mavlink_signing_key.is_empty(),
                    egui::Button::new("Remove key"),
                )
                .clicked()
            {
                app.clear_mavlink_key();
            }
        });
    });

    ui.horizontal_wrapped(|ui| {
        ui.label(&app.mavlink_key_name);
        ui.label(
            egui::RichText::new(format!(
                "{} bytes",
                app.settings.telemetry.mavlink_signing_key.len()
            ))
            .small()
            .color(ui.visuals().weak_text_color()),
        );
    });
    if let Some(error) = &app.mavlink_key_error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    } else if app.settings.telemetry.mavlink_signing.requires_key()
        && app.settings.telemetry.mavlink_signing_key.len() != 32
    {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "A 32-byte signing key is required by this policy",
        );
    }

    let counters = &app.telemetry.counters;
    egui::Grid::new("mavlink-status")
        .num_columns(2)
        .striped(true)
        .spacing([18.0, 6.0])
        .show(ui, |ui| {
            row(ui, "Dialect", "Common".to_owned());
            row(
                ui,
                "Source",
                match (
                    app.telemetry.mavlink_system_id,
                    app.telemetry.mavlink_component_id,
                ) {
                    (Some(system), Some(component)) => {
                        format!("system {system} / component {component}")
                    }
                    _ => "Not detected".to_owned(),
                },
            );
            row(
                ui,
                "Version",
                app.telemetry.mavlink_version.map_or_else(
                    || "Not detected".to_owned(),
                    |version| format!("MAVLink {version}"),
                ),
            );
            row(
                ui,
                "Signing link",
                app.telemetry
                    .mavlink_signing_link_id
                    .map_or_else(|| "None".to_owned(), |link| link.to_string()),
            );
            row(
                ui,
                "Last frame",
                app.telemetry.mavlink_last_signed.map_or_else(
                    || "Not detected".to_owned(),
                    |signed| if signed { "Signed" } else { "Unsigned" }.to_owned(),
                ),
            );
            row(ui, "Signed", counters.mavlink_signed_frames.to_string());
            row(ui, "Unsigned", counters.mavlink_unsigned_frames.to_string());
            row(ui, "Verified", counters.mavlink_verified_frames.to_string());
            row(
                ui,
                "Invalid signatures",
                counters.mavlink_invalid_signatures.to_string(),
            );
            row(ui, "Replays", counters.mavlink_replay_drops.to_string());
            row(
                ui,
                "Stale timestamps",
                counters.mavlink_stale_timestamp_drops.to_string(),
            );
            row(
                ui,
                "Missing key",
                counters.mavlink_missing_key_drops.to_string(),
            );
        });
}

fn msp_settings(app: &mut NebulusApp, ui: &mut egui::Ui, editable: bool) {
    ui.add_enabled_ui(editable, |ui| {
        egui::Grid::new("msp-settings")
            .num_columns(2)
            .spacing([18.0, 7.0])
            .show(ui, |ui| {
                ui.label("Version");
                egui::ComboBox::from_id_salt("msp-version")
                    .selected_text(app.settings.telemetry.msp_version.label())
                    .show_ui(ui, |ui| {
                        for version in MspVersionFilter::ALL {
                            ui.selectable_value(
                                &mut app.settings.telemetry.msp_version,
                                version,
                                version.label(),
                            );
                        }
                    });
                ui.end_row();
                ui.label("Direction");
                egui::ComboBox::from_id_salt("msp-direction")
                    .selected_text(app.settings.telemetry.msp_direction.label())
                    .show_ui(ui, |ui| {
                        for direction in MspDirectionFilter::ALL {
                            ui.selectable_value(
                                &mut app.settings.telemetry.msp_direction,
                                direction,
                                direction.label(),
                            );
                        }
                    });
                ui.end_row();
            });
    });
}

fn crsf_settings(app: &mut NebulusApp, ui: &mut egui::Ui, editable: bool) {
    ui.add_enabled_ui(editable, |ui| {
        ui.horizontal(|ui| {
            ui.label("Device address");
            egui::ComboBox::from_id_salt("crsf-address")
                .selected_text(crsf_address_label(app.settings.telemetry.crsf_address))
                .show_ui(ui, |ui| {
                    for (address, label) in [
                        (CRSF_ANY_ADDRESS, "Any address"),
                        (0x00, "Broadcast (0x00)"),
                        (0x14, "Video receiver (0x14)"),
                        (0xc8, "Flight controller (0xC8)"),
                        (0xec, "RC receiver (0xEC)"),
                        (0xee, "TX module (0xEE)"),
                    ] {
                        ui.selectable_value(
                            &mut app.settings.telemetry.crsf_address,
                            address,
                            label,
                        );
                    }
                });
            if app.settings.telemetry.crsf_address != CRSF_ANY_ADDRESS {
                ui.add(
                    egui::DragValue::new(&mut app.settings.telemetry.crsf_address)
                        .range(0..=255)
                        .hexadecimal(2, false, true),
                );
            }
        });
    });
}

fn id_filter(ui: &mut egui::Ui, label: &str, value: &mut u8) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut enabled = *value != 0;
        if ui.checkbox(&mut enabled, "Filter").changed() {
            *value = if enabled { 1 } else { 0 };
        }
        if enabled {
            ui.add(egui::DragValue::new(value).range(1..=255));
        } else {
            ui.add_enabled(false, egui::Label::new("Any"));
        }
    });
    ui.end_row();
}

fn crsf_address_label(address: u16) -> String {
    if address == CRSF_ANY_ADDRESS {
        "Any address".to_owned()
    } else {
        format!("0x{address:02X}")
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: String) {
    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
    ui.monospace(value);
    ui.end_row();
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show(ui, |ui| {
            ui.add_space(3.0);
            add(ui);
            ui.add_space(6.0);
        });
}
