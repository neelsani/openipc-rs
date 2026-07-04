use eframe::egui;
use openipc_core::channel::DEFAULT_LINK_ID;

use crate::{
    app::NebulusApp,
    model::ReceiverState,
    settings::{DEFAULT_CHANNEL, DEFAULT_CHANNEL_OFFSET, MAX_LINK_ID},
};

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    if app.receiver_info.is_some() {
        connected_receiver(app, ui);
    }
    let editable = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
    ui.add_enabled_ui(editable, |ui| {
        section(ui, "Profiles", |ui| profile_editor(app, ui));

        section(ui, "Receiver", |ui| {
            ui.label(
                egui::RichText::new("Primary receiver and uplink")
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("usb-device")
                    .selected_text(selected_device_label(app))
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for device in &app.devices {
                            let changed = ui
                                .selectable_value(
                                    &mut app.settings.device_id,
                                    Some(device.id.clone()),
                                    format!("{} — {}", device.label, device.location),
                                )
                                .changed();
                            if changed {
                                app.settings
                                    .diversity_device_ids
                                    .retain(|id| Some(id) != app.settings.device_id.as_ref());
                            }
                        }
                    });
                if ui.button("Refresh").clicked() {
                    app.refresh_devices();
                }
                #[cfg(target_arch = "wasm32")]
                if ui.button("Add adapter").clicked() {
                    app.authorize_webusb_adapter();
                }
            });
            if app.devices.is_empty() {
                ui.label(
                    egui::RichText::new("No supported USB adapter found")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
            if app.devices.len() > 1 {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Diversity receivers")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                for device in &app.devices {
                    if app.settings.device_id.as_deref() == Some(device.id.as_str()) {
                        continue;
                    }
                    let mut selected = app
                        .settings
                        .diversity_device_ids
                        .iter()
                        .any(|id| id == &device.id);
                    if ui
                        .checkbox(
                            &mut selected,
                            format!("{} — {}", device.label, device.location),
                        )
                        .changed()
                    {
                        if selected {
                            app.settings.diversity_device_ids.push(device.id.clone());
                        } else {
                            app.settings
                                .diversity_device_ids
                                .retain(|id| id != &device.id);
                        }
                        app.settings.normalize();
                    }
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} adapters selected; first valid WFB packet wins",
                        app.settings.selected_device_ids().len()
                    ))
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );
            }
            ui.horizontal(|ui| {
                if ui.button("Run preflight").clicked() {
                    app.run_preflight();
                }
                if ui.button("Scan channels").clicked() {
                    app.show_channel_scanner = true;
                }
            });
        });

        section(ui, "Radio", |ui| {
            egui::Grid::new("radio-settings")
                .num_columns(3)
                .spacing([18.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Channel");
                    ui.add(
                        egui::Slider::new(&mut app.settings.channel, 1..=177)
                            .show_value(true)
                            .text(""),
                    );
                    if ui.small_button("Default").clicked() {
                        app.settings.channel = DEFAULT_CHANNEL;
                    }
                    ui.end_row();
                    ui.label("Width");
                    egui::ComboBox::from_id_salt("channel-width")
                        .selected_text(format!("{} MHz", app.settings.channel_width_mhz))
                        .show_ui(ui, |ui| {
                            for width in [5, 10, 20, 40, 80] {
                                ui.selectable_value(
                                    &mut app.settings.channel_width_mhz,
                                    width,
                                    format!("{width} MHz"),
                                );
                            }
                        });
                    ui.label("");
                    ui.end_row();
                    ui.label("Offset");
                    ui.add(
                        egui::Slider::new(&mut app.settings.channel_offset, 0..=4)
                            .show_value(true)
                            .text(""),
                    );
                    if ui.small_button("Default").clicked() {
                        app.settings.channel_offset = DEFAULT_CHANNEL_OFFSET;
                    }
                    ui.end_row();
                    ui.label("Link ID");
                    ui.add(
                        egui::Slider::new(&mut app.settings.link_id, 0..=MAX_LINK_ID)
                            .show_value(true)
                            .custom_formatter(|value, _| format!("0x{:06X}", value as u32))
                            .custom_parser(parse_link_id),
                    );
                    if ui.small_button("Default").clicked() {
                        app.settings.link_id = DEFAULT_LINK_ID;
                    }
                    ui.end_row();
                });
        });

        section(ui, "Link", |ui| {
            ui.checkbox(&mut app.settings.rtp_reorder, "RTP reorder buffer");
            ui.checkbox(&mut app.settings.adaptive_link, "Adaptive link feedback");
            ui.checkbox(
                &mut app.settings.auto_recover,
                "Automatically recover a dropped receiver",
            );
            if cfg!(target_arch = "wasm32") {
                ui.label(
                    egui::RichText::new(
                        "Browser reconnects require a user gesture; automatic retries run in native builds.",
                    )
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );
            }
            if app.recovery.active {
                ui.horizontal_wrapped(|ui| {
                    let remaining = app
                        .recovery
                        .scheduled_at
                        .map_or(0.0, |at| {
                            at.saturating_duration_since(web_time::Instant::now())
                                .as_secs_f32()
                        });
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!(
                            "Recovery attempt {} in {:.1}s",
                            app.recovery.attempt, remaining
                        ),
                    );
                    if ui.small_button("Cancel").clicked() {
                        app.cancel_recovery();
                    }
                });
            }
            if app.settings.adaptive_link {
                ui.horizontal(|ui| {
                    ui.label("Uplink TX power");
                    ui.add(egui::Slider::new(&mut app.settings.tx_power, 0..=127));
                });
            }
        });

        section(ui, "Receiver key", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open file").clicked() {
                    app.open_key_file(ui.ctx());
                }
                if ui.button("Use default").clicked() {
                    app.reset_key();
                }
            });
            ui.horizontal(|ui| {
                ui.label(&app.key_name);
                ui.label(
                    egui::RichText::new(format!("{} bytes", app.settings.key_bytes.len()))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            });
            if let Some(error) = &app.key_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            } else {
                ui.label(
                    egui::RichText::new("You can also drop a gs.key file anywhere on the window")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });

        section(ui, "Video", |ui| {
            ui.horizontal(|ui| {
                ui.label("Codec preference");
                egui::ComboBox::from_id_salt("codec-preference")
                    .selected_text(app.settings.codec_preference.label())
                    .show_ui(ui, |ui| {
                        for preference in [
                            crate::settings::CodecPreference::Auto,
                            crate::settings::CodecPreference::H264,
                            crate::settings::CodecPreference::H265,
                        ] {
                            ui.selectable_value(
                                &mut app.settings.codec_preference,
                                preference,
                                preference.label(),
                            );
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Decoder queue");
                ui.label("3 frames, latest-frame output");
            });
        });

        section(ui, "Recording", |ui| recording_settings(app, ui));

        section(ui, "Advanced", |ui| {
            egui::Grid::new("advanced-settings")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Minimum epoch");
                    ui.add(egui::DragValue::new(&mut app.settings.minimum_epoch));
                    ui.end_row();
                    ui.label("USB transfer size");
                    ui.add(
                        egui::DragValue::new(&mut app.settings.transfer_size)
                            .range(4_096..=1_048_576),
                    );
                    ui.end_row();
                });
        });
    });
    let vpn_section = egui::CollapsingHeader::new("VPN / tunnel")
        .default_open(true)
        .show(ui, |ui| {
            ui.add_space(3.0);
            super::vpn(app, ui);
            ui.add_space(6.0);
        });
    if std::mem::take(&mut app.focus_vpn_settings) {
        vpn_section
            .header_response
            .scroll_to_me(Some(egui::Align::TOP));
    }
}

fn recording_settings(_app: &mut NebulusApp, ui: &mut egui::Ui) {
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    {
        ui.label("Recording folder");
        ui.add(
            egui::Label::new(egui::RichText::new(_app.recording_directory_display()).monospace())
                .wrap(),
        );
        ui.horizontal(|ui| {
            if ui.button("Choose folder").clicked() {
                _app.choose_recording_directory();
            }
            if ui.button("Use default").clicked() {
                _app.reset_recording_directory();
            }
        });
        ui.label(
            egui::RichText::new(
                "Record creates a unique MP4 here immediately; it never opens a save dialog.",
            )
            .small()
            .color(ui.visuals().weak_text_color()),
        );
    }

    #[cfg(target_os = "android")]
    ui.label(
        egui::RichText::new(
            "Recordings are written to Nebulus app storage without opening a document picker.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );

    #[cfg(target_arch = "wasm32")]
    ui.label(
        egui::RichText::new(
            "The browser buffers the MP4 and starts a download when recording stops. The browser controls the download folder.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );
}

fn profile_editor(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let active_id = app
        .settings
        .active_profile_id
        .or_else(|| app.settings.profiles.first().map(|profile| profile.id));
    let selected_name = active_id
        .and_then(|id| {
            app.settings
                .profiles
                .iter()
                .find(|profile| profile.id == id)
        })
        .map(|profile| profile.name.clone())
        .unwrap_or_else(|| "No profile".to_owned());
    let mut selected = active_id;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("receiver-profile")
            .selected_text(selected_name)
            .width(180.0)
            .show_ui(ui, |ui| {
                for profile in &app.settings.profiles {
                    ui.selectable_value(&mut selected, Some(profile.id), &profile.name);
                }
            });
        if selected != active_id {
            if let Some(id) = selected {
                app.apply_profile(id);
            }
        }
        if ui.button("New").clicked() {
            app.create_profile();
        }
    });
    if let Some(id) = app.settings.active_profile_id {
        if let Some(profile) = app
            .settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
        {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut profile.name)
                        .desired_width(180.0)
                        .char_limit(48),
                );
            });
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Save current").clicked() {
            app.save_active_profile();
        }
        if ui
            .add_enabled(app.settings.profiles.len() > 1, egui::Button::new("Delete"))
            .clicked()
        {
            app.delete_active_profile();
        }
    });
    ui.label(
        egui::RichText::new(
            "Profiles include adapters, radio, link, keys, routes, telemetry, audio, VPN, and decoder settings.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );
}

fn connected_receiver(app: &NebulusApp, ui: &mut egui::Ui) {
    section(ui, "Connected receivers", |ui| {
        for receiver in &app.receiver_infos {
            ui.strong(format!(
                "Radio {} · {}",
                receiver.source_id + 1,
                receiver.label
            ));
            egui::Grid::new(("connected-receiver-info", receiver.source_id))
                .num_columns(2)
                .striped(true)
                .spacing([18.0, 7.0])
                .show(ui, |ui| {
                    receiver_row(ui, "Adapter", &receiver.label);
                    receiver_row(ui, "Device", &receiver.id);
                    receiver_row(
                        ui,
                        "USB ID",
                        &match (receiver.vendor_id, receiver.product_id) {
                            (Some(vendor), Some(product)) => format!("{vendor:04x}:{product:04x}"),
                            _ => "Not applicable".to_owned(),
                        },
                    );
                    receiver_row(ui, "Chipset", &receiver.chip);
                    receiver_row(ui, "RF paths", &receiver.rf_paths);
                    receiver_row(
                        ui,
                        "Cut revision",
                        &receiver
                            .cut_version
                            .map_or_else(|| "Not reported".to_owned(), |cut| cut.to_string()),
                    );
                    receiver_row(ui, "USB link", &receiver.usb_speed);
                    receiver_row(
                        ui,
                        "Bulk endpoints",
                        &match (receiver.bulk_in_endpoint, receiver.bulk_out_endpoint) {
                            (Some(input), Some(output)) => {
                                format!("IN 0x{input:02x} / OUT 0x{output:02x}")
                            }
                            _ => "Not applicable".to_owned(),
                        },
                    );
                    receiver_row(ui, "Initialization", &receiver.initialization);
                    receiver_row(
                        ui,
                        "Firmware downloaded",
                        match receiver.firmware_downloaded {
                            Some(true) => "Yes",
                            Some(false) => "No",
                            None => "Not applicable",
                        },
                    );
                    receiver_row(
                        ui,
                        "Role",
                        if receiver.source_id == 0 {
                            "Primary RX and uplink"
                        } else {
                            "Diversity RX"
                        },
                    );
                });
            ui.add_space(8.0);
        }
        egui::Grid::new("connected-radio-config")
            .num_columns(2)
            .striped(true)
            .spacing([18.0, 7.0])
            .show(ui, |ui| {
                receiver_row(
                    ui,
                    "RF configuration",
                    &format!(
                        "channel {} / {} MHz / offset {}",
                        app.settings.channel,
                        app.settings.channel_width_mhz,
                        app.settings.channel_offset
                    ),
                );
                receiver_row(
                    ui,
                    "Video channel",
                    &format!(
                        "Link 0x{:06x} / port 0x{:02x}",
                        app.settings.link_id,
                        openipc_core::RadioPort::Video.as_u8()
                    ),
                );
            });
    });
}

fn receiver_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
    ui.monospace(value);
    ui.end_row();
}

fn parse_link_id(value: &str) -> Option<f64> {
    let value = value.trim();
    let parsed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(
            || value.parse::<u32>().ok(),
            |hex| u32::from_str_radix(hex, 16).ok(),
        )?;
    (parsed <= MAX_LINK_ID).then_some(f64::from(parsed))
}

fn selected_device_label(app: &NebulusApp) -> String {
    app.settings
        .device_id
        .as_deref()
        .and_then(|id| app.devices.iter().find(|device| device.id == id))
        .map(|device| format!("{} — {}", device.label, device.location))
        .or_else(|| app.settings.device_id.clone())
        .unwrap_or_else(|| "Select an adapter".to_owned())
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show(ui, |ui| {
            ui.add_space(4.0);
            add(ui);
            ui.add_space(8.0);
        });
}

#[cfg(test)]
mod tests {
    use super::parse_link_id;

    #[test]
    fn link_id_parser_accepts_decimal_and_hex() {
        assert_eq!(parse_link_id("7669206"), Some(7_669_206.0));
        assert_eq!(parse_link_id("0x7505D6"), Some(7_669_206.0));
    }

    #[test]
    fn link_id_parser_rejects_values_above_24_bits() {
        assert_eq!(parse_link_id("0x1000000"), None);
    }
}
