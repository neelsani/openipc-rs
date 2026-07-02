use eframe::egui;

use crate::{app::NebulusApp, model::LogLevel};

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Logs");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Clear").clicked() {
                app.logs.clear();
            }
        });
    });
    ui.separator();
    for entry in app.logs.iter().rev() {
        ui.horizontal_top(|ui| {
            ui.monospace(format!("{:>8.2}", entry.elapsed_seconds));
            ui.label(
                egui::RichText::new(entry.level.label())
                    .monospace()
                    .color(level_color(ui, entry.level)),
            );
            ui.label(
                egui::RichText::new(entry.target)
                    .monospace()
                    .color(ui.visuals().weak_text_color()),
            );
            ui.label(&entry.message);
        });
        ui.separator();
    }
}

fn level_color(ui: &egui::Ui, level: LogLevel) -> egui::Color32 {
    match level {
        LogLevel::Debug => ui.visuals().weak_text_color(),
        LogLevel::Info => egui::Color32::from_rgb(78, 202, 157),
        LogLevel::Warn => egui::Color32::from_rgb(236, 181, 70),
        LogLevel::Error => egui::Color32::from_rgb(239, 86, 95),
    }
}
