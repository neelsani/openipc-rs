use eframe::egui;

use crate::{app::NebulusApp, model::ReceiverState};

const ALL_CHANNELS: [u8; 42] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108,
    112, 116, 120, 124, 128, 132, 136, 140, 144, 149, 153, 157, 161, 165, 169, 173, 177,
];

pub(crate) fn dialog(app: &mut NebulusApp, context: &egui::Context) {
    if !app.show_channel_scanner {
        return;
    }
    let mut open = true;
    let scanning = app.state == ReceiverState::Scanning;
    let idle = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
    egui::Window::new("Channel scanner")
        .id(egui::Id::new("channel-scanner-window"))
        .open(&mut open)
        .resizable(true)
        .default_size([660.0, 540.0])
        .show(context, |ui| {
            ui.label(
                egui::RichText::new(
                    "Surveys RF traffic while RX is stopped. WFB activity and stronger RSSI help locate an active VTX.",
                )
                .small()
                .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(8.0);
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Common FPV").clicked() {
                        app.scan_channels = vec![
                            36, 40, 44, 48, 100, 104, 108, 112, 116, 120, 124, 128, 132,
                            136, 140, 144, 149, 153, 157, 161, 165, 169, 173, 177,
                        ];
                    }
                    if ui.button("2.4 GHz").clicked() {
                        app.scan_channels = (1..=14).collect();
                    }
                    if ui.button("Clear").clicked() {
                        app.scan_channels.clear();
                    }
                });
                egui::ScrollArea::horizontal()
                    .id_salt("scanner-channel-list")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for channel in ALL_CHANNELS {
                                let mut selected = app.scan_channels.contains(&channel);
                                if ui.toggle_value(&mut selected, channel.to_string()).changed() {
                                    if selected {
                                        app.scan_channels.push(channel);
                                        app.scan_channels.sort_unstable();
                                        app.scan_channels.dedup();
                                    } else {
                                        app.scan_channels.retain(|value| *value != channel);
                                    }
                                }
                            }
                        });
                    });
                ui.horizontal(|ui| {
                    ui.label("Dwell per channel");
                    ui.add(
                        egui::Slider::new(&mut app.scan_dwell_ms, 75..=1_000)
                            .suffix(" ms")
                            .logarithmic(true),
                    );
                });
            });
            ui.horizontal(|ui| {
                if scanning {
                    if ui.button("Stop scan").clicked() {
                        app.stop_receiver();
                    }
                } else if ui
                    .add_enabled(idle, egui::Button::new("Start survey"))
                    .clicked()
                {
                    app.start_channel_scan(context);
                }
                if let Some((done, total)) = app.scan_progress {
                    let fraction = if total == 0 {
                        0.0
                    } else {
                        done as f32 / total as f32
                    };
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(220.0)
                            .text(format!("{done}/{total}")),
                    );
                }
            });
            if let Some(error) = &app.scan_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();
            if app.scan_results.is_empty() {
                ui.label(
                    egui::RichText::new("No survey results yet")
                        .color(ui.visuals().weak_text_color()),
                );
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("channel-scan-results")
                    .show(ui, |ui| {
                        egui::Grid::new("channel-scan-grid")
                            .num_columns(8)
                            .striped(true)
                            .spacing([14.0, 7.0])
                            .show(ui, |ui| {
                                for heading in [
                                    "Channel", "MHz", "Packets", "WFB", "RSSI A", "RSSI B",
                                    "Traffic", "",
                                ] {
                                    ui.strong(heading);
                                }
                                ui.end_row();
                                let results = app.scan_results.clone();
                                for result in results {
                                    ui.monospace(result.channel.to_string());
                                    ui.monospace(frequency_mhz(result.channel).to_string());
                                    ui.monospace(result.packets.to_string());
                                    ui.monospace(result.wfb_frames.to_string());
                                    ui.monospace(rssi_label(result.average_rssi_dbm[0]));
                                    ui.monospace(rssi_label(result.average_rssi_dbm[1]));
                                    let mbps = if result.dwell_ms == 0 {
                                        0.0
                                    } else {
                                        result.bytes as f64 * 8.0 / result.dwell_ms as f64 / 1_000.0
                                    };
                                    ui.monospace(format!("{mbps:.2} Mbps"));
                                    if ui.small_button("Use").clicked() {
                                        app.use_scanned_channel(result.channel);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            }
        });
    app.show_channel_scanner &= open;
}

fn frequency_mhz(channel: u8) -> u16 {
    match channel {
        14 => 2_484,
        1..=13 => 2_407 + u16::from(channel) * 5,
        _ => 5_000 + u16::from(channel) * 5,
    }
}

fn rssi_label(value: i32) -> String {
    if value == 0 {
        "--".to_owned()
    } else {
        format!("{value} dBm")
    }
}
