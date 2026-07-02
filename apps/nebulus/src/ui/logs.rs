use eframe::egui;

use crate::{app::NebulusApp, model::LogLevel};

const MAX_RENDERED_LOG_ROWS: usize = 300;

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Logs");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Clear").clicked() {
                app.logs.clear();
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Capture");
        egui::ComboBox::from_id_salt("diagnostic-verbosity")
            .selected_text(app.settings.diagnostic_verbosity.label())
            .show_ui(ui, |ui| {
                for verbosity in [
                    crate::settings::DiagnosticVerbosity::Low,
                    crate::settings::DiagnosticVerbosity::Normal,
                    crate::settings::DiagnosticVerbosity::High,
                    crate::settings::DiagnosticVerbosity::VeryHigh,
                ] {
                    ui.selectable_value(
                        &mut app.settings.diagnostic_verbosity,
                        verbosity,
                        verbosity.label(),
                    );
                }
            });
        ui.label("Minimum level");
        egui::ComboBox::from_id_salt("log-minimum-level")
            .selected_text(app.log_filter.label())
            .show_ui(ui, |ui| {
                for level in [
                    LogLevel::Trace,
                    LogLevel::Debug,
                    LogLevel::Info,
                    LogLevel::Warn,
                    LogLevel::Error,
                ] {
                    ui.selectable_value(&mut app.log_filter, level, level.label());
                }
            });
        ui.label("Target or text");
        ui.add(
            egui::TextEdit::singleline(&mut app.log_search)
                .desired_width(180.0)
                .hint_text("usb, wfb, decoder..."),
        );
    });
    ui.separator();
    let search = app.log_search.trim().to_ascii_lowercase();
    let matching = app
        .logs
        .iter()
        .rev()
        .filter(|entry| {
            entry.level.priority() >= app.log_filter.priority()
                && (search.is_empty()
                    || entry.target.to_ascii_lowercase().contains(&search)
                    || entry.message.to_ascii_lowercase().contains(&search))
        })
        .collect::<Vec<_>>();
    if matching.len() > MAX_RENDERED_LOG_ROWS {
        ui.label(
            egui::RichText::new(format!(
                "Showing newest {MAX_RENDERED_LOG_ROWS} of {} matching records. Narrow the filter to inspect older entries.",
                matching.len()
            ))
            .small()
            .color(ui.visuals().weak_text_color()),
        );
        ui.separator();
    }
    for entry in matching.into_iter().take(MAX_RENDERED_LOG_ROWS) {
        ui.push_id(entry.sequence, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_sized(
                    [54.0, 18.0],
                    egui::Label::new(
                        egui::RichText::new(format!("{:>7.2}", entry.elapsed_seconds)).monospace(),
                    ),
                );
                ui.add_sized(
                    [42.0, 18.0],
                    egui::Label::new(
                        egui::RichText::new(entry.level.label())
                            .monospace()
                            .color(level_color(ui, entry.level)),
                    ),
                );
                ui.add_sized(
                    [104.0, 18.0],
                    egui::Label::new(
                        egui::RichText::new(&entry.target)
                            .monospace()
                            .color(ui.visuals().weak_text_color()),
                    )
                    .truncate(),
                )
                .on_hover_text(&entry.target);
                ui.add(egui::Label::new(&entry.message).wrap());
            });
            ui.separator();
        });
    }
}

fn level_color(ui: &egui::Ui, level: LogLevel) -> egui::Color32 {
    match level {
        LogLevel::Trace => ui.visuals().weak_text_color(),
        LogLevel::Debug => ui.visuals().weak_text_color(),
        LogLevel::Info => egui::Color32::from_rgb(78, 202, 157),
        LogLevel::Warn => egui::Color32::from_rgb(236, 181, 70),
        LogLevel::Error => egui::Color32::from_rgb(239, 86, 95),
    }
}
