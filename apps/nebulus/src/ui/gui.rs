use eframe::egui;

use crate::{
    app::NebulusApp,
    settings::{GuiTheme, HudItemSettings, HudMetric, HudSettings},
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
    ui.checkbox(&mut app.settings.show_osd, "Video OSD");
    ui.horizontal(|ui| {
        if ui.button("Edit video OSD").clicked() {
            app.show_osd_editor = true;
        }
        if ui.button("Reset OSD").clicked() {
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

pub(crate) fn osd_editor(app: &mut NebulusApp, context: &egui::Context) {
    if !app.show_osd_editor {
        app.osd_edit_history.end_session();
        return;
    }
    app.osd_edit_history.begin_session();
    handle_editor_shortcuts(app, context);
    let hud_before = app.settings.hud.clone();

    let screen = context.content_rect();
    let default_size = egui::vec2(
        (screen.width() - 32.0).clamp(360.0, 1_050.0),
        (screen.height() - 32.0).clamp(460.0, 760.0),
    );
    let mut open = true;
    let mut done = false;
    let mut toolbar_undo = false;
    let can_undo = app.osd_edit_history.can_undo();
    egui::Window::new("Video OSD editor")
        .id(egui::Id::new("video-osd-editor-v2"))
        .open(&mut open)
        .resizable(true)
        .default_size(default_size)
        .min_width(360.0)
        .max_height((screen.height() - 16.0).max(420.0))
        .show(context, |ui| {
            editor_toolbar(
                ui,
                &mut app.settings.hud,
                &mut app.selected_hud_metric,
                &mut done,
                can_undo,
                &mut toolbar_undo,
            );
            ui.separator();

            let available = ui.available_size();
            if available.x >= 820.0 {
                wide_editor(
                    ui,
                    &mut app.settings.hud,
                    &mut app.selected_hud_metric,
                    available,
                );
            } else {
                stacked_editor(
                    ui,
                    &mut app.settings.hud,
                    &mut app.selected_hud_metric,
                    available,
                );
            }
        });
    if toolbar_undo {
        if app.osd_edit_history.undo(&mut app.settings.hud) {
            context.request_repaint();
        }
    } else {
        let pointer_down = context.input(|input| input.pointer.any_down());
        app.osd_edit_history
            .observe(hud_before, &app.settings.hud, pointer_down);
    }
    app.show_osd_editor = open && !done;
    if !app.show_osd_editor {
        app.osd_edit_history.end_session();
    }
}

fn handle_editor_shortcuts(app: &mut NebulusApp, context: &egui::Context) {
    let (redo, undo, delete) = context.input_mut(|input| {
        let redo = input.consume_key(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        ) || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y);
        let undo = !redo && input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z);
        let delete = input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
            || input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace);
        (redo, undo, delete)
    });

    let restored = if redo {
        app.osd_edit_history.redo(&mut app.settings.hud)
    } else if undo {
        app.osd_edit_history.undo(&mut app.settings.hud)
    } else {
        false
    };
    if restored {
        context.request_repaint();
    }

    if delete {
        let before = app.settings.hud.clone();
        if let Some(item) = app
            .settings
            .hud
            .items
            .iter_mut()
            .find(|item| item.metric == app.selected_hud_metric)
        {
            item.visible = false;
        }
        app.osd_edit_history.record(before, &app.settings.hud);
        context.request_repaint();
    }
}

fn editor_toolbar(
    ui: &mut egui::Ui,
    hud: &mut HudSettings,
    selected: &mut HudMetric,
    done: &mut bool,
    can_undo: bool,
    undo_requested: &mut bool,
) {
    ui.horizontal_wrapped(|ui| {
        indicator_picker(ui, hud, selected);
        if cfg!(target_os = "android")
            && ui
                .add_enabled(can_undo, egui::Button::new("Undo").small())
                .clicked()
        {
            *undo_requested = true;
        }
        ui.separator();
        ui.label("HUD size");
        if ui
            .add_sized(
                [130.0, 20.0],
                egui::Slider::new(&mut hud.scale_percent, 70..=160).suffix("%"),
            )
            .changed()
        {
            ui.ctx().request_repaint();
        }
        ui.label("Panel opacity");
        if ui
            .add_sized(
                [130.0, 20.0],
                egui::Slider::new(&mut hud.background_opacity, 0..=240),
            )
            .changed()
        {
            ui.ctx().request_repaint();
        }
        ui.separator();
        if ui.button("Reset all").clicked() {
            hud.reset_layout();
            ui.ctx().request_repaint();
        }
        if ui.button("Done").clicked() {
            *done = true;
        }
    });
}

fn indicator_picker(ui: &mut egui::Ui, hud: &mut HudSettings, selected: &mut HudMetric) {
    let visible = hud.items.iter().filter(|item| item.visible).count();
    ui.menu_button(
        format!("Indicators  {visible}/{}", HudMetric::ALL.len()),
        |ui| {
            ui.set_min_width(270.0);
            for (heading, telemetry) in [("Ground station", false), ("Flight telemetry", true)] {
                ui.label(
                    egui::RichText::new(heading)
                        .small()
                        .strong()
                        .color(ui.visuals().weak_text_color()),
                );
                for item in hud
                    .items
                    .iter_mut()
                    .filter(|item| item.metric.requires_telemetry() == telemetry)
                {
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut item.visible, "").changed() {
                            ui.ctx().request_repaint();
                        }
                        if ui
                            .selectable_label(item.metric == *selected, item.metric.label())
                            .clicked()
                        {
                            *selected = item.metric;
                            ui.close();
                        }
                    });
                }
                if !telemetry {
                    ui.separator();
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Show all").clicked() {
                    for item in &mut hud.items {
                        item.visible = true;
                    }
                    ui.ctx().request_repaint();
                }
                if ui.button("Hide all").clicked() {
                    for item in &mut hud.items {
                        item.visible = false;
                    }
                    ui.ctx().request_repaint();
                }
            });
        },
    );
}

fn wide_editor(
    ui: &mut egui::Ui,
    hud: &mut HudSettings,
    selected: &mut HudMetric,
    available: egui::Vec2,
) {
    let inspector_width = (available.x * 0.34).clamp(310.0, 370.0);
    let preview_width = (available.x - inspector_width - 14.0).max(320.0);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(preview_width, available.y),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let max_preview = egui::vec2(preview_width, (available.y - 30.0).max(220.0));
                center_preview(ui, hud, selected, max_preview);
            },
        );
        ui.separator();
        ui.allocate_ui_with_layout(
            egui::vec2(inspector_width, available.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| selected_item_inspector(ui, hud, selected),
        );
    });
}

fn stacked_editor(
    ui: &mut egui::Ui,
    hud: &mut HudSettings,
    selected: &mut HudMetric,
    available: egui::Vec2,
) {
    let preview_height = (available.y * 0.52).clamp(180.0, 340.0);
    ui.allocate_ui_with_layout(
        egui::vec2(available.x, preview_height),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            center_preview(ui, hud, selected, egui::vec2(available.x, preview_height));
        },
    );
    ui.separator();
    selected_item_inspector(ui, hud, selected);
}

fn center_preview(
    ui: &mut egui::Ui,
    hud: &mut HudSettings,
    selected: &mut HudMetric,
    maximum: egui::Vec2,
) {
    let size = fit_16_by_9(maximum);
    let spare_height = (maximum.y - size.y - 22.0).max(0.0);
    ui.add_space(spare_height * 0.5);
    draw_editor_preview(ui, hud, selected, size);
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.strong(selected.label());
        ui.label(
            egui::RichText::new(format!(
                "{:.0}% / {:.0}%",
                selected_item(hud, *selected).map_or(0.0, |item| item.x * 100.0),
                selected_item(hud, *selected).map_or(0.0, |item| item.y * 100.0)
            ))
            .monospace()
            .small()
            .color(ui.visuals().weak_text_color()),
        );
    });
}

fn fit_16_by_9(maximum: egui::Vec2) -> egui::Vec2 {
    let mut width = maximum.x.max(240.0);
    let mut height = width * 9.0 / 16.0;
    if height > maximum.y {
        height = maximum.y.max(135.0);
        width = height * 16.0 / 9.0;
    }
    egui::vec2(width, height)
}

fn draw_editor_preview(
    ui: &mut egui::Ui,
    hud: &mut HudSettings,
    selected: &mut HudMetric,
    size: egui::Vec2,
) {
    let (preview, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    draw_osd_preview_background(ui.painter(), preview);
    ui.painter().rect_stroke(
        preview,
        4.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );

    let scale = f32::from(hud.scale_percent) / 100.0;
    for item in &mut hud.items {
        if !item.visible {
            continue;
        }
        let center = egui::pos2(
            egui::lerp(preview.x_range(), item.x),
            egui::lerp(preview.y_range(), item.y),
        );
        let item_size = super::osd::preview_item_size(ui.painter(), item, scale);
        let item_rect = super::osd::positioned_rect(preview, center, item_size);
        let response = ui
            .interact(
                item_rect,
                egui::Id::new(("osd-editor-item", item.metric.label())),
                egui::Sense::click_and_drag(),
            )
            .on_hover_text(item.metric.label());
        if response.clicked() {
            *selected = item.metric;
        }
        if let Some(pointer) = response
            .interact_pointer_pos()
            .filter(|_| response.dragged())
        {
            *selected = item.metric;
            item.x = ((pointer.x - preview.left()) / preview.width()).clamp(0.03, 0.97);
            item.y = ((pointer.y - preview.top()) / preview.height()).clamp(0.03, 0.97);
        }
        let highlighted = item.metric == *selected || response.hovered() || response.dragged();
        super::osd::draw_preview_item(
            ui.painter(),
            item_rect,
            item,
            scale,
            hud.background_opacity,
            highlighted,
        );
        if item.metric == *selected {
            ui.painter().rect_stroke(
                item_rect.expand(3.0),
                3.0,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(64, 218, 157)),
                egui::StrokeKind::Outside,
            );
        }
    }
}

fn selected_item_inspector(ui: &mut egui::Ui, hud: &mut HudSettings, selected: &mut HudMetric) {
    ui.horizontal(|ui| {
        ui.label("Indicator");
        egui::ComboBox::from_id_salt("osd-selected-indicator")
            .selected_text(selected.label())
            .width((ui.available_width() - 8.0).max(150.0))
            .show_ui(ui, |ui| {
                for metric in HudMetric::ALL {
                    ui.selectable_value(selected, metric, metric.label());
                }
            });
    });

    let metric = *selected;
    let Some(index) = hud.items.iter().position(|item| item.metric == metric) else {
        return;
    };
    let mut reset = false;
    ui.horizontal(|ui| {
        ui.checkbox(&mut hud.items[index].visible, "Visible");
        let source = if metric.requires_telemetry() {
            "Flight telemetry"
        } else {
            "Ground station"
        };
        ui.label(
            egui::RichText::new(source)
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            reset = ui.small_button("Reset").clicked();
        });
    });
    if reset {
        hud.reset_item(metric);
        ui.ctx().request_repaint();
    }
    ui.separator();

    let available_height = ui.available_height().max(120.0);
    egui::ScrollArea::vertical()
        .id_salt(("osd-selected-inspector", metric.label()))
        .auto_shrink([false, false])
        .max_height(available_height)
        .show(ui, |ui| {
            let item = &mut hud.items[index];
            ui.add_enabled_ui(item.visible, |ui| {
                if hud_item_controls(ui, item) {
                    ui.ctx().request_repaint();
                }
            });
        });
}

fn selected_item(hud: &HudSettings, metric: HudMetric) -> Option<&HudItemSettings> {
    hud.items.iter().find(|item| item.metric == metric)
}

fn hud_item_controls(ui: &mut egui::Ui, item: &mut HudItemSettings) -> bool {
    let mut changed = false;

    ui.strong("Content");
    egui::Grid::new(("osd-item-content", item.metric.label()))
        .num_columns(2)
        .spacing([12.0, 5.0])
        .show(ui, |ui| {
            changed |= ui.checkbox(&mut item.show_icon, "Icon").changed();
            changed |= ui.checkbox(&mut item.show_value, "Value").changed();
            ui.end_row();
            changed |= ui.checkbox(&mut item.show_label, "Label").changed();
            changed |= ui.checkbox(&mut item.colorize, "Status colors").changed();
            ui.end_row();
            changed |= ui
                .checkbox(&mut item.show_background, "Background")
                .changed();
            if item.metric.supports_signal_bars() {
                changed |= ui
                    .checkbox(&mut item.show_signal_bars, "Signal bars")
                    .changed();
            } else {
                ui.label("");
            }
            ui.end_row();
        });
    if item.metric.requires_telemetry() {
        changed |= ui
            .checkbox(
                &mut item.hide_when_unavailable,
                "Hide when telemetry is stale",
            )
            .changed();
    }

    ui.add_space(10.0);
    ui.strong("Appearance");
    changed |= labeled_slider(
        ui,
        "Size",
        egui::Slider::new(&mut item.scale_percent, 60..=200).suffix("%"),
    );
    if item.show_background {
        changed |= labeled_slider(
            ui,
            "Background",
            egui::Slider::new(&mut item.background_opacity_percent, 0..=100).suffix("%"),
        );
    }

    ui.add_space(10.0);
    ui.strong("Position");
    let mut x = (item.x * 100.0).round() as u16;
    let mut y = (item.y * 100.0).round() as u16;
    if labeled_slider(
        ui,
        "Horizontal",
        egui::Slider::new(&mut x, 3..=97).suffix("%"),
    ) {
        item.x = f32::from(x) / 100.0;
        changed = true;
    }
    if labeled_slider(
        ui,
        "Vertical",
        egui::Slider::new(&mut y, 3..=97).suffix("%"),
    ) {
        item.y = f32::from(y) / 100.0;
        changed = true;
    }

    if item.metric.supports_graph() {
        ui.add_space(10.0);
        ui.strong("Mini graph");
        changed |= ui.checkbox(&mut item.show_graph, "Show graph").changed();
        if item.show_graph {
            changed |= labeled_slider(
                ui,
                "History",
                egui::Slider::new(&mut item.graph_seconds, 5..=60).suffix(" s"),
            );
            changed |= labeled_slider(
                ui,
                "Width",
                egui::Slider::new(&mut item.graph_width, 80..=260).suffix(" px"),
            );
            changed |= labeled_slider(
                ui,
                "Height",
                egui::Slider::new(&mut item.graph_height, 32..=120).suffix(" px"),
            );
            changed |= ui.checkbox(&mut item.graph_fill, "Filled area").changed();
        }
    }

    changed
}

fn labeled_slider(ui: &mut egui::Ui, label: &str, slider: egui::Slider<'_>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = ui.add_sized([210.0, 20.0], slider).changed();
        });
    });
    changed
}

fn draw_osd_preview_background(painter: &egui::Painter, rect: egui::Rect) {
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
