use eframe::egui;

use crate::{app::NebulusApp, presets::PresetPack, remote_presets::DEFAULT_REGISTRY_URL};

pub(crate) fn section(app: &mut NebulusApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Manage presets").clicked() {
            app.show_preset_manager = true;
            app.preset_error = None;
        }
        if ui.button("Install file").clicked() {
            app.show_preset_manager = true;
            app.open_preset_file(ui.ctx());
        }
        if ui.button("Export current").clicked() {
            app.show_preset_manager = true;
            app.begin_preset_export();
        }
    });
    ui.label(
        egui::RichText::new(format!(
            "{} installed version(s). Packs contain portable OSD, theme, route, telemetry, and performance settings only.",
            app.settings.installed_presets.len()
        ))
        .small()
        .color(ui.visuals().weak_text_color()),
    );
}

pub(crate) fn dialog(app: &mut NebulusApp, context: &egui::Context) {
    if !app.show_preset_manager && app.preset_install.is_none() && app.preset_export.is_none() {
        return;
    }
    let screen = context.content_rect();
    let editing = app.preset_install.is_some()
        || app.preset_export.is_some()
        || app.preset_registry.is_some();
    let default_height = if editing {
        screen.height().mul_add(0.75, 0.0).min(500.0)
    } else {
        (145.0 + app.settings.installed_presets.len() as f32 * 82.0)
            .min(screen.height().mul_add(0.72, 0.0))
    };
    let default_size = egui::vec2(screen.width().mul_add(0.92, 0.0).min(600.0), default_height);
    let min_size = egui::vec2(
        (screen.width() - 16.0).clamp(280.0, 380.0),
        (if editing { 320.0_f32 } else { 130.0_f32 }).min(screen.height() - 16.0),
    );
    let window_id = if app.preset_install.is_some() {
        "nebulus-preset-install-v2"
    } else if app.preset_export.is_some() {
        "nebulus-preset-export-v2"
    } else if app.preset_registry.is_some() {
        "nebulus-preset-registry-v2"
    } else {
        "nebulus-preset-library-v2"
    };
    let mut open = true;
    egui::Window::new("Nebulus preset packs")
        .id(egui::Id::new(window_id))
        .open(&mut open)
        .resizable(true)
        .scroll([false, true])
        .default_size(default_size)
        .min_size(min_size)
        .max_size(egui::vec2(
            (screen.width() - 12.0).max(280.0),
            (screen.height() - 12.0).max(280.0),
        ))
        .show(context, |ui| {
            if app.preset_install.is_some() {
                install_preview(app, ui);
            } else if app.preset_export.is_some() {
                export_editor(app, ui);
            } else if app.preset_registry.is_some() {
                registry_browser(app, ui);
            } else {
                installed_library(app, ui);
            }
        });
    if !open {
        app.show_preset_manager = false;
        app.preset_install = None;
        app.preset_export = None;
        app.preset_registry = None;
        app.preset_error = None;
    }
}

fn installed_library(app: &mut NebulusApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Install from file").clicked() {
            app.open_preset_file(ui.ctx());
        }
        if ui.button("Export current setup").clicked() {
            app.begin_preset_export();
        }
    });
    remote_source_editor(app, ui);
    security_note(ui);
    show_error(app, ui);
    ui.separator();

    if app.settings.installed_presets.is_empty() {
        ui.weak("No preset packs installed.");
        return;
    }

    let mut apply = None;
    let mut remove = None;
    for (index, pack) in app.settings.installed_presets.iter().enumerate() {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.strong(format!("{}  v{}", pack.name, pack.version));
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {} · {}",
                        pack.author, pack.license, pack.id
                    ))
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );
                ui.label(
                    egui::RichText::new(component_summary(pack))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
                if ui.small_button("Apply").clicked() {
                    apply = Some(index);
                }
            });
        });
        if !pack.description.is_empty() {
            ui.label(&pack.description);
        }
        ui.separator();
    }
    if let Some(index) = apply {
        app.preview_installed_preset(index);
    } else if let Some(index) = remove {
        app.remove_installed_preset(index);
    }
}

fn remote_source_editor(app: &mut NebulusApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.strong("Preset or registry URL");
    ui.horizontal(|ui| {
        let loading = app.preset_remote_loading.is_some();
        let input_width = (ui.available_width() - 86.0).max(120.0);
        ui.add_enabled(
            !loading,
            egui::TextEdit::singleline(&mut app.settings.preset_source_url)
                .desired_width(input_width)
                .hint_text("https://github.com/.../registry.json"),
        );
        if loading {
            ui.spinner();
        } else if ui.button("Open URL").clicked() {
            app.open_preset_url(ui.ctx());
        }
    });
    if let Some(url) = app.preset_remote_loading.as_deref() {
        ui.label(
            egui::RichText::new(format!("Downloading {url}"))
                .small()
                .color(ui.visuals().weak_text_color()),
        );
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(
                    "Accepts direct preset JSON, a registry index, or a GitHub blob URL.",
                )
                .small()
                .color(ui.visuals().weak_text_color()),
            );
            if app.settings.preset_source_url != DEFAULT_REGISTRY_URL
                && ui.small_button("Use official registry").clicked()
            {
                app.settings.preset_source_url = DEFAULT_REGISTRY_URL.to_owned();
            }
        });
    }
}

fn registry_browser(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let mut install_remote = None;
    let mut apply_installed = None;
    let mut close_registry = false;
    let mut refresh = false;
    let registry = app.preset_registry.as_ref().expect("checked above");

    ui.horizontal_wrapped(|ui| {
        if ui.button("Installed presets").clicked() {
            close_registry = true;
        }
        if ui
            .add_enabled(
                app.preset_remote_loading.is_none(),
                egui::Button::new("Refresh"),
            )
            .clicked()
        {
            refresh = true;
        }
        if app.preset_remote_loading.is_some() {
            ui.spinner();
        }
    });
    ui.heading(&registry.name);
    if !registry.description.is_empty() {
        ui.label(&registry.description);
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format!("{} versions", registry.presets.len()))
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        if let Some(homepage) = registry.homepage.as_deref() {
            ui.hyperlink_to("Registry homepage", homepage);
        }
    });
    show_error(app, ui);
    ui.separator();

    for (index, entry) in registry.presets.iter().enumerate() {
        let installed = app
            .settings
            .installed_presets
            .iter()
            .position(|pack| pack.id == entry.id && pack.version == entry.version);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.strong(format!("{}  v{}", entry.name, entry.version));
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {} · {}",
                        entry.author, entry.license, entry.id
                    ))
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );
                ui.label(
                    egui::RichText::new(if entry.sha256.is_some() {
                        "SHA-256 pinned"
                    } else {
                        "No checksum published"
                    })
                    .small()
                    .color(if entry.sha256.is_some() {
                        ui.visuals().strong_text_color()
                    } else {
                        ui.visuals().warn_fg_color
                    }),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if let Some(installed_index) = installed {
                    if ui.button("Apply").clicked() {
                        apply_installed = Some(installed_index);
                    }
                    ui.weak("Installed");
                } else if ui
                    .add_enabled(
                        app.preset_remote_loading.is_none(),
                        egui::Button::new("Install"),
                    )
                    .clicked()
                {
                    install_remote = Some(index);
                }
            });
        });
        if !entry.description.is_empty() {
            ui.label(&entry.description);
        }
        ui.separator();
    }

    if close_registry {
        app.preset_registry = None;
    } else if refresh {
        app.open_preset_url(ui.ctx());
    } else if let Some(index) = apply_installed {
        app.preview_installed_preset(index);
    } else if let Some(index) = install_remote {
        app.install_registry_preset(index, ui.ctx());
    }
}

fn install_preview(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let mut apply = false;
    let mut cancel = false;
    {
        let draft = app.preset_install.as_mut().expect("checked above");
        ui.heading(&draft.pack.name);
        ui.label(format!(
            "v{} by {} · {}",
            draft.pack.version, draft.pack.author, draft.pack.license
        ));
        ui.monospace(&draft.pack.id);
        if !draft.pack.description.is_empty() {
            ui.add_space(6.0);
            ui.label(&draft.pack.description);
        }
        ui.add_space(10.0);
        ui.strong("Components to apply");
        if let Some(osd) = draft.pack.components.osd.as_ref() {
            ui.checkbox(&mut draft.install_osd, format!("OSD layout: {}", osd.name));
        }
        if draft.pack.components.theme.is_some() {
            ui.checkbox(&mut draft.install_theme, "Application theme");
        }
        if !draft.pack.components.routes.is_empty() {
            ui.checkbox(
                &mut draft.install_routes,
                format!("{} route template(s)", draft.pack.components.routes.len()),
            );
        }
        if draft.pack.components.telemetry.is_some() {
            ui.checkbox(&mut draft.install_telemetry, "Telemetry policy");
        }
        if draft.pack.components.performance.is_some() {
            ui.checkbox(&mut draft.install_performance, "Low-latency decoder policy");
        }
        for warning in &draft.warnings {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Applying updates only the selected portable fields. Local keys, adapters, radio configuration, paths, and signing keys remain unchanged.",
            )
            .small()
            .color(ui.visuals().weak_text_color()),
        );
        ui.separator();
        ui.horizontal(|ui| {
            apply = ui
                .add_enabled(
                    draft.has_selection(),
                    egui::Button::new("Install and apply"),
                )
                .clicked();
            cancel = ui.button("Cancel").clicked();
        });
    }
    show_error(app, ui);
    if apply {
        app.apply_preset_install(ui.ctx());
    } else if cancel {
        app.preset_install = None;
        app.preset_error = None;
    }
}

fn export_editor(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let mut export = false;
    let mut cancel = false;
    {
        let draft = app.preset_export.as_mut().expect("checked above");
        ui.heading("Export portable preset");
        ui.label(
            egui::RichText::new(
                "The exported JSON cannot contain receiver keys, signing keys, USB identities, paths, Link IDs, channels, or UDP destinations.",
            )
            .small()
            .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(8.0);
        egui::Grid::new("preset-export-metadata")
            .num_columns(2)
            .spacing([12.0, 7.0])
            .show(ui, |ui| {
                field(ui, "ID", &mut draft.id, 96);
                field(ui, "Version", &mut draft.version, 32);
                field(ui, "Name", &mut draft.name, 96);
                field(ui, "Author", &mut draft.author, 96);
                field(ui, "License", &mut draft.license, 64);
            });
        ui.label("Description");
        ui.add(
            egui::TextEdit::multiline(&mut draft.description)
                .desired_rows(3)
                .char_limit(1_024),
        );
        ui.add_space(8.0);
        ui.strong("Components");
        ui.checkbox(&mut draft.include_osd, "Current OSD layout");
        ui.checkbox(&mut draft.include_theme, "Application theme");
        ui.checkbox(&mut draft.include_routes, "Payload route templates");
        ui.checkbox(&mut draft.include_telemetry, "Telemetry policy");
        ui.checkbox(
            &mut draft.include_performance,
            "Codec preference and RTP reorder policy",
        );
        let any = draft.include_osd
            || draft.include_theme
            || draft.include_routes
            || draft.include_telemetry
            || draft.include_performance;
        ui.separator();
        ui.horizontal(|ui| {
            export = ui
                .add_enabled(any, egui::Button::new("Export JSON"))
                .clicked();
            cancel = ui.button("Cancel").clicked();
        });
    }
    show_error(app, ui);
    if export {
        app.finish_preset_export();
    } else if cancel {
        app.preset_export = None;
        app.preset_error = None;
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, limit: usize) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(300.0)
            .char_limit(limit),
    );
    ui.end_row();
}

fn security_note(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(
            "Preset packs are declarative JSON, never executable code. Updates are installed side-by-side and applied explicitly.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );
}

fn show_error(app: &NebulusApp, ui: &mut egui::Ui) {
    if let Some(error) = app.preset_error.as_deref() {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
}

fn component_summary(pack: &PresetPack) -> String {
    let mut components = Vec::new();
    if pack.components.osd.is_some() {
        components.push("OSD".to_owned());
    }
    if pack.components.theme.is_some() {
        components.push("theme".to_owned());
    }
    if !pack.components.routes.is_empty() {
        components.push(format!("{} routes", pack.components.routes.len()));
    }
    if pack.components.telemetry.is_some() {
        components.push("telemetry".to_owned());
    }
    if pack.components.performance.is_some() {
        components.push("performance".to_owned());
    }
    components.join(" · ")
}
