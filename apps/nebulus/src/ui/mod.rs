mod diagnostics;
mod gui;
mod logs;
mod metrics;
mod osd;
mod presets;
mod routes;
mod scanner;
mod settings;
mod telemetry;
pub(crate) mod theme;
mod video;

use eframe::egui::{self, Color32, CornerRadius, Stroke};

use crate::{app::NebulusApp, model::ReceiverState};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum PanelTab {
    #[default]
    Setup,
    Data,
    Display,
    Monitor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DataPage {
    #[default]
    Routes,
    Telemetry,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MonitorPage {
    #[default]
    Metrics,
    Health,
    Rtp,
    Latency,
    System,
    Logs,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    #[default]
    Receiver,
    Media,
    Profiles,
    Network,
    Vtx,
}

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    if app.video_fullscreen {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ui.visuals().extreme_bg_color))
            .show(ui, |ui| video::show(app, ui));
        return;
    }

    let compact = ui.available_width() < 860.0;
    egui::Panel::top("nebulus-header")
        .exact_size(if compact { 78.0 } else { 48.0 })
        .frame(header_frame(ui))
        .show(ui, |ui| header(app, ui, compact));

    if app.settings.show_sidebar {
        if compact {
            let panel_height = (ui.available_height() * 0.46).clamp(220.0, 430.0);
            egui::Panel::bottom("nebulus-control-panel-mobile-v2")
                .default_size(panel_height)
                .min_size(180.0)
                .max_size(ui.available_height() * 0.72)
                .resizable(true)
                .frame(panel_frame(ui))
                .show(ui, |ui| side_panel(app, ui));
        } else {
            egui::Panel::right("nebulus-control-panel-v3")
                .default_size(420.0)
                .min_size(360.0)
                .max_size(520.0)
                .resizable(true)
                .frame(panel_frame(ui))
                .show(ui, |ui| side_panel(app, ui));
        }
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(ui.visuals().extreme_bg_color))
        .show(ui, |ui| video::show(app, ui));

    about_dialog(app, ui.ctx());
    vtx_confirmation_dialog(app, ui.ctx());
    gui::osd_editor(app, ui.ctx());
    preflight_dialog(app, ui.ctx());
    scanner::dialog(app, ui.ctx());
    presets::dialog(app, ui.ctx());
}

fn vtx_confirmation_dialog(app: &mut NebulusApp, context: &egui::Context) {
    let Some(pending) = app.pending_vtx_confirmation.clone() else {
        return;
    };
    let width = (context.content_rect().width() - 32.0).clamp(280.0, 430.0);
    let mut confirmed = false;
    let response =
        egui::Modal::new(egui::Id::new("nebulus-vtx-confirmation")).show(context, |ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(&pending.title).strong().size(19.0));
            ui.add_space(6.0);
            ui.label(&pending.message);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    ui.close();
                }
                if ui
                    .button(
                        egui::RichText::new(&pending.confirm_label)
                            .strong()
                            .color(ui.visuals().warn_fg_color),
                    )
                    .clicked()
                {
                    confirmed = true;
                    ui.close();
                }
            });
        });
    if confirmed {
        app.pending_vtx_confirmation = None;
        app.request_vtx(pending.request);
    } else if response.should_close() {
        app.pending_vtx_confirmation = None;
    }
}

fn preflight_dialog(app: &mut NebulusApp, context: &egui::Context) {
    if !app.show_preflight {
        return;
    }
    let mut open = true;
    let can_start = app.preflight.can_start()
        && matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
    egui::Window::new("Receiver preflight")
        .id(egui::Id::new("receiver-preflight"))
        .open(&mut open)
        .resizable(true)
        .default_width(520.0)
        .show(context, |ui| {
            let [passed, warnings, failures] = app.preflight.counts();
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(Color32::from_rgb(61, 214, 154), format!("{passed} passed"));
                ui.colored_label(
                    Color32::from_rgb(236, 181, 70),
                    format!("{warnings} warnings"),
                );
                ui.colored_label(Color32::from_rgb(239, 86, 95), format!("{failures} failed"));
            });
            ui.separator();
            for check in &app.preflight.checks {
                let (symbol, color) = match check.severity {
                    crate::preflight::PreflightSeverity::Pass => {
                        ("OK", Color32::from_rgb(61, 214, 154))
                    }
                    crate::preflight::PreflightSeverity::Warning => {
                        ("WARN", Color32::from_rgb(236, 181, 70))
                    }
                    crate::preflight::PreflightSeverity::Fail => {
                        ("FAIL", Color32::from_rgb(239, 86, 95))
                    }
                };
                ui.horizontal_top(|ui| {
                    ui.add_sized(
                        [42.0, 18.0],
                        egui::Label::new(egui::RichText::new(symbol).monospace().color(color)),
                    );
                    ui.vertical(|ui| {
                        ui.strong(check.name);
                        ui.label(
                            egui::RichText::new(&check.detail)
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    });
                });
                ui.add_space(5.0);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Run again").clicked() {
                    app.preflight = crate::preflight::PreflightReport::run(app);
                }
                if ui
                    .add_enabled(can_start, egui::Button::new("Start RX"))
                    .clicked()
                {
                    app.show_preflight = false;
                    app.start_receiver(context);
                }
            });
        });
    app.show_preflight &= open;
}

fn header(app: &mut NebulusApp, ui: &mut egui::Ui, compact: bool) {
    let state = app.state;
    let connected_label = app.receiver_info.as_ref().map(|receiver| {
        if app.receiver_infos.len() > 1 {
            format!(
                "{} + {} diversity",
                receiver.label,
                app.receiver_infos.len() - 1
            )
        } else {
            receiver.label.clone()
        }
    });
    if compact {
        egui::containers::Sides::new()
            .height(28.0)
            .shrink_left()
            .truncate()
            .show(
                ui,
                |ui| {
                    brand(ui);
                    build_badges(ui);
                },
                |ui| {
                    status_badge(ui, state);
                    panel_button(app, ui);
                },
            );
        ui.add_space(5.0);
        ui.horizontal_centered(|ui| {
            about_button(app, ui);
            receiver_buttons(app, ui);
            if let Some(label) = &connected_label {
                ui.label(
                    egui::RichText::new(label)
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });
        return;
    }

    egui::containers::Sides::new()
        .height(32.0)
        .spacing(12.0)
        .shrink_left()
        .truncate()
        .show(
            ui,
            |ui| {
                brand(ui);
                build_badges(ui);
                status_badge(ui, state);
                if let Some(label) = &connected_label {
                    device_badge(ui, label);
                }
            },
            |ui| {
                about_button(app, ui);
                panel_button(app, ui);
                ui.separator();
                receiver_buttons(app, ui);
            },
        );
}

fn about_button(app: &mut NebulusApp, ui: &mut egui::Ui) {
    if ui
        .add(
            egui::Button::new("About")
                .corner_radius(4)
                .min_size(egui::vec2(56.0, 27.0)),
        )
        .on_hover_text("About Nebulus")
        .clicked()
    {
        app.show_about = true;
    }
}

fn about_dialog(app: &mut NebulusApp, context: &egui::Context) {
    if !app.show_about {
        return;
    }
    let width = (context.content_rect().width() - 32.0).clamp(280.0, 410.0);
    let build = crate::build_info::current();
    let response = egui::Modal::new(egui::Id::new("nebulus-about-dialog")).show(context, |ui| {
        ui.set_width(width);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("Nebulus")
                        .strong()
                        .size(22.0)
                        .color(ui.visuals().strong_text_color()),
                );
                ui.label(
                    egui::RichText::new(format!("OpenIPC ground station v{}", build.version))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if ui.small_button("Close").clicked() {
                    ui.close();
                }
            });
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label("A low-latency OpenIPC FPV receiver built in Rust for desktop, Android, and the browser.");
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("Contact for inquiries:")
                    .color(ui.visuals().weak_text_color()),
            );
            ui.hyperlink_to("neel@neels.dev", "mailto:neel@neels.dev");
        });
        ui.add_space(12.0);
        if cfg!(target_arch = "wasm32") {
            ui.label(
                egui::RichText::new("For the lowest latency")
                    .strong()
                    .color(Color32::from_rgb(61, 214, 154)),
            );
            ui.label("Download the native desktop or Android app for direct USB access, platform hardware decoding, and less browser overhead.");
        } else {
            ui.label(
                egui::RichText::new("Try it in your browser")
                    .strong()
                    .color(Color32::from_rgb(61, 214, 154)),
            );
            ui.label("Open the hosted WebUSB version without installing another application.");
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    egui::Button::new(if cfg!(target_arch = "wasm32") {
                        "Download app"
                    } else {
                        "Try web version"
                    })
                        .fill(Color32::from_rgb(36, 132, 99))
                        .min_size(egui::vec2(116.0, 30.0)),
                )
                .clicked()
            {
                context.open_url(egui::OpenUrl::new_tab(if cfg!(target_arch = "wasm32") {
                    crate::build_info::RELEASES_URL
                } else {
                    crate::build_info::WEB_APP_URL
                }));
            }
            ui.hyperlink_to("Docs", crate::build_info::DOCS_URL);
            ui.hyperlink_to("GitHub", crate::build_info::REPOSITORY_URL);
            if let Some(commit) = build.short_commit() {
                ui.hyperlink_to(format!("Commit {commit}"), build.commit_url());
            }
        });
    });
    if response.should_close() {
        app.show_about = false;
    }
}

fn brand(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 19.0), egui::Sense::hover());
        ui.painter().rect_filled(
            rect,
            1.5,
            Color32::from_rgb(61, 214, 154).gamma_multiply(0.9),
        );
        ui.label(
            egui::RichText::new("Nebulus")
                .strong()
                .size(17.0)
                .color(ui.visuals().strong_text_color()),
        );
    });
}

fn build_badges(ui: &mut egui::Ui) {
    let build = crate::build_info::current();
    let response = egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .stroke(Stroke::new(
            1.0,
            ui.visuals()
                .widgets
                .noninteractive
                .bg_stroke
                .color
                .gamma_multiply(0.7),
        ))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let release_color = if build.tag.is_some() {
                    Color32::from_rgb(61, 214, 154)
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.label(
                    egui::RichText::new(build.release_label())
                        .monospace()
                        .strong()
                        .size(9.0)
                        .color(release_color),
                );
                if let Some(commit) = build.short_commit() {
                    ui.add(
                        egui::Hyperlink::from_label_and_url(
                            egui::RichText::new(format!("git:{commit}"))
                                .monospace()
                                .size(9.0)
                                .color(ui.visuals().weak_text_color()),
                            build.commit_url(),
                        )
                        .open_in_new_tab(true),
                    );
                }
            });
        })
        .response;
    response.on_hover_text(build.description());
}

fn device_badge(ui: &mut egui::Ui, label: &str) {
    egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color.gamma_multiply(0.7))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(9.0)
                    .color(ui.visuals().weak_text_color()),
            );
        });
}

fn panel_button(app: &mut NebulusApp, ui: &mut egui::Ui) {
    if ui
        .add(
            egui::Button::new("Controls")
                .selected(app.settings.show_sidebar)
                .corner_radius(4)
                .min_size(egui::vec2(64.0, 27.0)),
        )
        .on_hover_text(if app.settings.show_sidebar {
            "Hide controls"
        } else {
            "Show controls"
        })
        .clicked()
    {
        app.settings.show_sidebar = !app.settings.show_sidebar;
    }
}

fn receiver_buttons(app: &mut NebulusApp, ui: &mut egui::Ui) {
    match app.state {
        ReceiverState::Idle | ReceiverState::Failed => {
            if action_button(ui, "Start RX", ActionTone::Primary).clicked() {
                app.start_receiver(ui.ctx());
            }
            #[cfg(debug_assertions)]
            if action_button(
                ui,
                match app.settings.codec_preference.mock_codec() {
                    openipc_core::Codec::H264 => "H.264 mock",
                    openipc_core::Codec::H265 => "H.265 mock",
                },
                ActionTone::Neutral,
            )
            .on_hover_text("Uses the codec preference under Setup > Media")
            .clicked()
            {
                app.start_codec_mock(ui.ctx());
            }
        }
        ReceiverState::Receiving | ReceiverState::Ready => {
            if app.state == ReceiverState::Receiving {
                let label = match app.recording.state {
                    crate::model::RecordingState::Idle => "Record",
                    crate::model::RecordingState::Armed => "Cancel record",
                    crate::model::RecordingState::Recording => "Stop record",
                };
                if action_button(ui, label, ActionTone::Record).clicked() {
                    app.toggle_recording();
                }
            }
            if action_button(ui, "Stop RX", ActionTone::Neutral).clicked() {
                app.stop_receiver();
            }
        }
        ReceiverState::Scanning => {
            if action_button(ui, "Stop scan", ActionTone::Neutral).clicked() {
                app.stop_receiver();
            }
        }
        _ => {
            ui.add_enabled(
                false,
                egui::Button::new("Working")
                    .corner_radius(4)
                    .min_size(egui::vec2(72.0, 27.0)),
            );
        }
    }
}

#[derive(Clone, Copy)]
enum ActionTone {
    Primary,
    Record,
    Neutral,
}

fn action_button(ui: &mut egui::Ui, label: &str, tone: ActionTone) -> egui::Response {
    let (fill, stroke, text) = match tone {
        ActionTone::Primary => {
            let color = Color32::from_rgb(61, 214, 154);
            (
                color.gamma_multiply(0.16),
                color.gamma_multiply(0.65),
                color,
            )
        }
        ActionTone::Record => {
            let color = Color32::from_rgb(237, 100, 116);
            (color.gamma_multiply(0.14), color.gamma_multiply(0.6), color)
        }
        ActionTone::Neutral => (
            ui.visuals().widgets.inactive.weak_bg_fill,
            ui.visuals().widgets.inactive.bg_stroke.color,
            ui.visuals().text_color(),
        ),
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(11.0).strong().color(text))
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .corner_radius(4)
            .min_size(egui::vec2(72.0, 27.0)),
    )
}

fn side_panel(app: &mut NebulusApp, ui: &mut egui::Ui) {
    if std::mem::take(&mut app.focus_vpn_settings) {
        app.active_tab = PanelTab::Setup;
        app.settings_page = SettingsPage::Network;
    }

    primary_navigation(app, ui);
    secondary_navigation(app, ui);
    ui.separator();
    let viewport_height = ui.available_height().max(0.0);
    let page_key = active_page_key(app);
    egui::ScrollArea::vertical()
        .id_salt(("nebulus-control-scroll-v2", app.active_tab, page_key))
        .max_height(viewport_height)
        .min_scrolled_height(viewport_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            match app.active_tab {
                PanelTab::Setup => settings::show(app, ui),
                PanelTab::Data => match app.data_page {
                    DataPage::Routes => routes::show(app, ui),
                    DataPage::Telemetry => telemetry::show(app, ui),
                },
                PanelTab::Display => gui::show(app, ui),
                PanelTab::Monitor => match app.monitor_page {
                    MonitorPage::Metrics => metrics::show(app, ui),
                    MonitorPage::Health => diagnostics::health(app, ui),
                    MonitorPage::Rtp => diagnostics::rtp(app, ui),
                    MonitorPage::Latency => diagnostics::latency(app, ui),
                    MonitorPage::System => diagnostics::system(app, ui),
                    MonitorPage::Logs => logs::show(app, ui),
                },
            }
            ui.add_space(12.0);
        });
}

fn primary_navigation(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let spacing = 5.0;
    let button_width = ((ui.available_width() - spacing * 3.0) / 4.0).max(64.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = spacing;
        for (tab, label) in [
            (PanelTab::Setup, "Setup"),
            (PanelTab::Data, "Data"),
            (PanelTab::Display, "Display"),
            (PanelTab::Monitor, "Monitor"),
        ] {
            if ui
                .add_sized(
                    [button_width, 27.0],
                    egui::Button::new(label).selected(app.active_tab == tab),
                )
                .clicked()
            {
                app.active_tab = tab;
            }
        }
    });
}

fn secondary_navigation(app: &mut NebulusApp, ui: &mut egui::Ui) {
    match app.active_tab {
        PanelTab::Setup => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for (page, label) in [
                    (SettingsPage::Receiver, "Receiver"),
                    (SettingsPage::Media, "Media"),
                    (SettingsPage::Profiles, "Profiles"),
                    (SettingsPage::Network, "Network"),
                    (SettingsPage::Vtx, "VTX"),
                ] {
                    ui.selectable_value(&mut app.settings_page, page, label);
                }
            });
        }
        PanelTab::Data => {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.data_page, DataPage::Routes, "Routes");
                ui.selectable_value(&mut app.data_page, DataPage::Telemetry, "Telemetry");
            });
        }
        PanelTab::Display => {}
        PanelTab::Monitor => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for (page, label) in [
                    (MonitorPage::Metrics, "Metrics"),
                    (MonitorPage::Health, "Health"),
                    (MonitorPage::Rtp, "RTP"),
                    (MonitorPage::Latency, "Latency"),
                    (MonitorPage::System, "System"),
                    (MonitorPage::Logs, "Logs"),
                ] {
                    ui.selectable_value(&mut app.monitor_page, page, label);
                }
            });
        }
    }
}

fn active_page_key(app: &NebulusApp) -> u8 {
    match app.active_tab {
        PanelTab::Setup => app.settings_page as u8,
        PanelTab::Data => app.data_page as u8,
        PanelTab::Display => 0,
        PanelTab::Monitor => app.monitor_page as u8,
    }
}

fn vpn(app: &mut NebulusApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("Bridges radio tunnel RX 0x20 and TX 0xa0 to a native L3 interface.")
            .small()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(10.0);
    #[cfg(target_os = "windows")]
    wintun_install(app, ui);
    let supported = !cfg!(target_arch = "wasm32");
    let available = app.vpn_available();
    ui.add_enabled_ui(
        supported && available && matches!(app.state, ReceiverState::Idle | ReceiverState::Failed),
        |ui| {
            ui.checkbox(
                &mut app.settings.vpn_enabled,
                "Enable VPN on receiver start",
            );
        },
    );
    if !supported {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "VPN/TUN is unavailable in browsers.",
        );
    } else if !available {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Install Wintun before enabling the Windows VPN interface.",
        );
    }
    ui.separator();
    egui::Grid::new("vpn-status").num_columns(2).show(ui, |ui| {
        #[cfg(target_os = "windows")]
        diagnostic_row(
            ui,
            "Wintun",
            if app.wintun_state.is_ready() {
                "Available"
            } else {
                "Not installed"
            },
        );
        diagnostic_row(
            ui,
            "State",
            if app.vpn.active { "Active" } else { "Inactive" },
        );
        diagnostic_row(
            ui,
            "Interface",
            if app.vpn.interface_name.is_empty() {
                "Created on start"
            } else {
                &app.vpn.interface_name
            },
        );
        diagnostic_row(ui, "Local address", "10.5.0.3/24");
        diagnostic_row(
            ui,
            "Downlink",
            &format!(
                "{} packets / {}",
                app.vpn.downlink_packets,
                format_bytes(app.vpn.downlink_bytes)
            ),
        );
        diagnostic_row(
            ui,
            "Uplink",
            &format!(
                "{} packets / {}",
                app.vpn.uplink_packets,
                format_bytes(app.vpn.uplink_bytes)
            ),
        );
        diagnostic_row(ui, "Errors", &app.vpn.errors.to_string());
    });
}

#[cfg(target_os = "windows")]
fn wintun_install(app: &mut NebulusApp, ui: &mut egui::Ui) {
    use crate::wintun::InstallState;

    let state = app.wintun_state.clone();
    if matches!(state, InstallState::Ready) {
        return;
    }

    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong("Wintun required");
            ui.label(
                egui::RichText::new(
                    "Nebulus uses the signed Wintun driver only for the optional Windows VPN interface.",
                )
                .small()
                .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(6.0);
            match state {
                InstallState::Missing => {
                    if ui.button("Install Wintun").clicked() {
                        app.install_wintun(ui.ctx());
                    }
                }
                InstallState::Downloading { downloaded, total } => {
                    let progress = total
                        .filter(|total| *total > 0)
                        .map_or(0.0, |total| downloaded as f32 / total as f32)
                        .clamp(0.0, 1.0);
                    let text = total.map_or_else(
                        || format!("Downloading {}", format_bytes(downloaded)),
                        |total| {
                            format!(
                                "Downloading {} / {}",
                                format_bytes(downloaded),
                                format_bytes(total)
                            )
                        },
                    );
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .animate(total.is_none())
                            .desired_width(ui.available_width())
                            .text(text),
                    );
                }
                InstallState::Installing => {
                    ui.add(
                        egui::ProgressBar::new(1.0)
                            .animate(true)
                            .desired_width(ui.available_width())
                            .text("Verifying and installing Wintun"),
                    );
                }
                InstallState::Failed(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    if ui.button("Retry installation").clicked() {
                        app.install_wintun(ui.ctx());
                    }
                }
                InstallState::Ready => {}
            }
        });
    ui.add_space(10.0);
}

fn diagnostic_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
    ui.monospace(value);
    ui.end_row();
}

fn status_badge(ui: &mut egui::Ui, state: ReceiverState) {
    let color = match state {
        ReceiverState::Receiving => Color32::from_rgb(61, 214, 154),
        ReceiverState::Connecting | ReceiverState::Scanning | ReceiverState::Stopping => {
            Color32::from_rgb(244, 183, 64)
        }
        ReceiverState::Failed => Color32::from_rgb(244, 88, 96),
        _ => ui.visuals().weak_text_color(),
    };
    egui::Frame::NONE
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.0, color);
                ui.label(
                    egui::RichText::new(state.label())
                        .size(9.0)
                        .strong()
                        .color(color),
                );
            });
        });
}

fn header_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::NONE
        .fill(ui.visuals().panel_fill)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .stroke(Stroke::new(
            1.0,
            ui.visuals()
                .widgets
                .noninteractive
                .bg_stroke
                .color
                .gamma_multiply(0.65),
        ))
}

fn panel_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::NONE
        .fill(ui.visuals().panel_fill)
        .inner_margin(egui::Margin::same(10))
        .stroke(Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes < 1_000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{:.2} MB", bytes as f64 / 1_000_000.0)
    }
}

pub(crate) fn format_bitrate(bits: f64) -> String {
    if bits < 1_000_000.0 {
        format!("{:.0} Kbps", bits / 1_000.0)
    } else {
        format!("{:.2} Mbps", bits / 1_000_000.0)
    }
}
