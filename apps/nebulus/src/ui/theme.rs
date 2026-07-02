use eframe::egui::{self, Color32, Stroke};

macro_rules! color {
    ($value:expr) => {{
        let [red, green, blue, alpha] = $value.to_array();
        Color32::from_rgba_unmultiplied(red, green, blue, alpha)
    }};
}

pub(crate) fn apply(context: &egui::Context) {
    let theme = catppuccin_egui::MACCHIATO;
    let mut visuals = egui::Visuals::dark();

    visuals.hyperlink_color = color!(theme.rosewater);
    visuals.faint_bg_color = color!(theme.surface0);
    visuals.extreme_bg_color = color!(theme.crust);
    visuals.code_bg_color = color!(theme.mantle);
    visuals.warn_fg_color = color!(theme.peach);
    visuals.error_fg_color = color!(theme.maroon);
    visuals.window_fill = color!(theme.base);
    visuals.panel_fill = color!(theme.base);
    visuals.override_text_color = Some(color!(theme.text));
    visuals.weak_text_color = Some(color!(theme.subtext0));
    visuals.window_stroke.color = color!(theme.overlay1);

    let widget_stroke = color!(theme.overlay1);
    let widget_text = color!(theme.text);
    apply_widget(
        &mut visuals.widgets.noninteractive,
        color!(theme.base),
        widget_stroke,
        widget_text,
    );
    apply_widget(
        &mut visuals.widgets.inactive,
        color!(theme.surface0),
        widget_stroke,
        widget_text,
    );
    apply_widget(
        &mut visuals.widgets.hovered,
        color!(theme.surface2),
        widget_stroke,
        widget_text,
    );
    apply_widget(
        &mut visuals.widgets.active,
        color!(theme.surface1),
        widget_stroke,
        widget_text,
    );
    apply_widget(
        &mut visuals.widgets.open,
        color!(theme.surface0),
        widget_stroke,
        widget_text,
    );

    visuals.selection.bg_fill = color!(theme.blue).linear_multiply(0.2);
    visuals.selection.stroke.color = color!(theme.text);
    let shadow_color = Color32::from_black_alpha(96);
    visuals.window_shadow.color = shadow_color;
    visuals.popup_shadow.color = shadow_color;
    visuals.dark_mode = true;

    let egui_theme = egui::Theme::Dark;
    context.all_styles_mut(|style| style.visuals = visuals.clone());
    context.set_theme(egui_theme);
    context.request_repaint();
}

fn apply_widget(
    widget: &mut egui::style::WidgetVisuals,
    background: Color32,
    border: Color32,
    foreground: Color32,
) {
    widget.bg_fill = background;
    widget.weak_bg_fill = background;
    widget.bg_stroke = Stroke {
        color: border,
        ..widget.bg_stroke
    };
    widget.fg_stroke = Stroke {
        color: foreground,
        ..widget.fg_stroke
    };
}

#[cfg(test)]
mod tests {
    use super::apply;
    use eframe::egui;

    #[test]
    fn applies_macchiato() {
        let context = egui::Context::default();

        apply(&context);
        assert_eq!(context.theme(), egui::Theme::Dark);
        assert_eq!(
            context.style_of(egui::Theme::Dark).visuals.panel_fill,
            egui::Color32::from_rgb(36, 39, 58)
        );
    }
}
