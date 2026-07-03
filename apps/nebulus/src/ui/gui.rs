use eframe::egui;

use crate::{
    app::NebulusApp,
    settings::{GuiTheme, HudMetric},
};

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    ui.heading("Interface");
    ui.label(
        egui::RichText::new("Appearance settings apply immediately and persist across launches.")
            .small()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(12.0);

    ui.strong("Theme");
    ui.add_space(4.0);
    for themes in GuiTheme::ALL.chunks_exact(2) {
        ui.columns(2, |columns| {
            for (column, theme) in columns.iter_mut().zip(themes.iter().copied()) {
                if theme_button(column, theme, app.settings.gui_theme).clicked() {
                    app.settings.gui_theme = theme;
                    super::theme::apply(column.ctx(), theme);
                }
            }
        });
        ui.add_space(4.0);
    }

    ui.add_space(14.0);
    ui.strong("Scale");
    ui.horizontal(|ui| {
        let changed = ui
            .add(
                egui::Slider::new(&mut app.settings.interface_scale_percent, 75..=150)
                    .suffix("%")
                    .step_by(5.0),
            )
            .changed();
        if changed {
            ui.ctx()
                .set_zoom_factor(f32::from(app.settings.interface_scale_percent) / 100.0);
        }
        if ui.small_button("Default").clicked() {
            app.settings.interface_scale_percent = 100;
            ui.ctx().set_zoom_factor(1.0);
        }
    });

    ui.add_space(14.0);
    ui.strong("Display");
    ui.checkbox(&mut app.settings.show_osd, "Link telemetry overlay");
    ui.horizontal(|ui| {
        if ui.button("Edit video HUD").clicked() {
            app.show_hud_editor = true;
        }
        if ui.button("Reset HUD").clicked() {
            app.settings.hud.reset_layout();
        }
    });
    if ui
        .checkbox(&mut app.settings.show_sidebar, "Controls panel visible")
        .changed()
        && !app.settings.show_sidebar
    {
        ui.ctx().request_repaint();
    }
    ui.label(
        egui::RichText::new("Use the Controls button in the header to restore a hidden panel.")
            .small()
            .color(ui.visuals().weak_text_color()),
    );

    ui.add_space(14.0);
    if ui.button("Reset GUI settings").clicked() {
        app.settings.gui_theme = GuiTheme::Macchiato;
        app.settings.interface_scale_percent = 100;
        app.settings.show_osd = true;
        app.settings.show_sidebar = true;
        app.settings.hud.reset_layout();
        super::theme::apply(ui.ctx(), app.settings.gui_theme);
        ui.ctx().set_zoom_factor(1.0);
    }
}

pub(crate) fn hud_editor(app: &mut NebulusApp, context: &egui::Context) {
    if !app.show_hud_editor {
        return;
    }
    let mut open = true;
    egui::Window::new("Video HUD editor")
        .id(egui::Id::new("video-hud-editor"))
        .open(&mut open)
        .resizable(true)
        .default_width(560.0)
        .show(context, |ui| {
            ui.label(
                egui::RichText::new(
                    "Drag values in the preview. Positions scale with the video on every target.",
                )
                .small()
                .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(8.0);
            let width = ui.available_width().clamp(280.0, 720.0);
            let (preview, _) = ui.allocate_exact_size(
                egui::vec2(width, (width * 9.0 / 16.0).max(180.0)),
                egui::Sense::hover(),
            );
            draw_hud_preview_background(ui.painter(), preview);
            ui.painter().rect_stroke(
                preview,
                4.0,
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                egui::StrokeKind::Inside,
            );
            let scale = f32::from(app.settings.hud.scale_percent) / 100.0;
            for item in &mut app.settings.hud.items {
                if !item.visible {
                    continue;
                }
                let center = egui::pos2(
                    egui::lerp(preview.x_range(), item.x),
                    egui::lerp(preview.y_range(), item.y),
                );
                let label = sample_value(item.metric);
                let galley = ui.painter().layout_no_wrap(
                    label.to_owned(),
                    egui::FontId::monospace(11.0 * scale),
                    egui::Color32::WHITE,
                );
                let item_rect = egui::Rect::from_center_size(
                    center,
                    galley.size() + egui::vec2(22.0, 12.0) * scale,
                );
                let response = ui.interact(
                    item_rect,
                    egui::Id::new(("hud-item", item.metric.label())),
                    egui::Sense::drag(),
                );
                if let Some(pointer) = response
                    .interact_pointer_pos()
                    .filter(|_| response.dragged())
                {
                    item.x = ((pointer.x - preview.left()) / preview.width()).clamp(0.03, 0.97);
                    item.y = ((pointer.y - preview.top()) / preview.height()).clamp(0.03, 0.97);
                }
                let color = if response.hovered() || response.dragged() {
                    egui::Color32::from_rgb(61, 214, 154)
                } else {
                    egui::Color32::WHITE
                };
                ui.painter().rect_filled(
                    item_rect,
                    3.0,
                    egui::Color32::from_black_alpha(app.settings.hud.background_opacity),
                );
                ui.painter()
                    .galley(item_rect.center() - galley.size() * 0.5, galley, color);
            }
            ui.add_space(10.0);
            egui::Grid::new("hud-item-visibility")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    for item in &mut app.settings.hud.items {
                        ui.checkbox(&mut item.visible, item.metric.label());
                        ui.monospace(format!(
                            "{:>3.0}%  {:>3.0}%",
                            item.x * 100.0,
                            item.y * 100.0
                        ));
                        ui.end_row();
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Text size");
                ui.add(
                    egui::Slider::new(&mut app.settings.hud.scale_percent, 70..=160).suffix("%"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Background");
                ui.add(
                    egui::Slider::new(&mut app.settings.hud.background_opacity, 0..=240)
                        .text("opacity"),
                );
            });
            ui.horizontal(|ui| {
                if ui.button("Reset layout").clicked() {
                    app.settings.hud.reset_layout();
                }
                if ui.button("Done").clicked() {
                    app.show_hud_editor = false;
                }
            });
        });
    app.show_hud_editor &= open;
}

fn draw_hud_preview_background(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(73, 112, 132));

    let horizon = rect.top() + rect.height() * 0.58;
    let ground = egui::Rect::from_min_max(egui::pos2(rect.left(), horizon), rect.right_bottom());
    painter.rect_filled(ground, 0.0, egui::Color32::from_rgb(58, 88, 62));

    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(rect.left(), horizon),
            egui::pos2(
                rect.left() + rect.width() * 0.22,
                horizon - rect.height() * 0.18,
            ),
            egui::pos2(rect.left() + rect.width() * 0.42, horizon),
        ],
        egui::Color32::from_rgb(62, 78, 88),
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(rect.left() + rect.width() * 0.3, horizon),
            egui::pos2(
                rect.left() + rect.width() * 0.58,
                horizon - rect.height() * 0.25,
            ),
            egui::pos2(rect.left() + rect.width() * 0.82, horizon),
        ],
        egui::Color32::from_rgb(72, 85, 96),
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(rect.left() + rect.width() * 0.68, horizon),
            egui::pos2(
                rect.left() + rect.width() * 0.88,
                horizon - rect.height() * 0.16,
            ),
            egui::pos2(rect.right(), horizon),
        ],
        egui::Color32::from_rgb(53, 73, 82),
        egui::Stroke::NONE,
    ));

    let runway_top = egui::pos2(rect.center().x, horizon + rect.height() * 0.03);
    painter.add(egui::Shape::convex_polygon(
        vec![
            runway_top - egui::vec2(rect.width() * 0.025, 0.0),
            runway_top + egui::vec2(rect.width() * 0.025, 0.0),
            egui::pos2(rect.right() - rect.width() * 0.16, rect.bottom()),
            egui::pos2(rect.left() + rect.width() * 0.16, rect.bottom()),
        ],
        egui::Color32::from_rgb(104, 105, 101),
        egui::Stroke::NONE,
    ));
    painter.line_segment(
        [runway_top, egui::pos2(rect.center().x, rect.bottom())],
        egui::Stroke::new(2.0, egui::Color32::from_rgb(232, 202, 92)),
    );

    let guide = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(65));
    painter.line_segment(
        [
            egui::pos2(rect.left() + rect.width() * 0.42, horizon),
            egui::pos2(rect.right() - rect.width() * 0.42, horizon),
        ],
        guide,
    );
    painter.circle_stroke(
        egui::pos2(rect.center().x, horizon),
        rect.height() * 0.035,
        guide,
    );
}

fn sample_value(metric: HudMetric) -> &'static str {
    match metric {
        HudMetric::Resolution => "1920x1080",
        HudMetric::FrameRate => "60 fps",
        HudMetric::Bitrate => "18.4 Mbps",
        HudMetric::Latency => "8.2 ms",
        HudMetric::Signal => "-58/-61 dBm",
        HudMetric::PacketLoss => "0 lost",
        HudMetric::LinkScore => "1680/1620",
    }
}

fn theme_button(ui: &mut egui::Ui, theme: GuiTheme, selected: GuiTheme) -> egui::Response {
    let palette = palette(theme);
    let desired = egui::vec2(ui.available_width().max(110.0), 44.0);
    let response = ui.add_sized(
        desired,
        egui::Button::new(egui::RichText::new(theme.label()).strong()).selected(theme == selected),
    );
    let swatch_size = 6.0;
    let start = egui::pos2(response.rect.left() + 10.0, response.rect.bottom() - 12.0);
    for (index, color) in palette.into_iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            egui::pos2(start.x + index as f32 * (swatch_size + 3.0), start.y),
            egui::vec2(swatch_size, swatch_size),
        );
        ui.painter().rect_filled(rect, 2.0, color);
    }
    response
}

fn palette(theme: GuiTheme) -> [egui::Color32; 4] {
    let theme = match theme {
        GuiTheme::Latte => catppuccin_egui::LATTE,
        GuiTheme::Frappe => catppuccin_egui::FRAPPE,
        GuiTheme::Macchiato => catppuccin_egui::MACCHIATO,
        GuiTheme::Mocha => catppuccin_egui::MOCHA,
    };
    [theme.base, theme.blue, theme.green, theme.mauve].map(|color| {
        let [red, green, blue, alpha] = color.to_array();
        egui::Color32::from_rgba_unmultiplied(red, green, blue, alpha)
    })
}
