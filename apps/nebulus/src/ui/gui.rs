use eframe::egui;

use crate::{app::NebulusApp, settings::GuiTheme};

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
        super::theme::apply(ui.ctx(), app.settings.gui_theme);
        ui.ctx().set_zoom_factor(1.0);
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
