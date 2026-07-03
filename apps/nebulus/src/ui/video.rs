use eframe::egui;

use crate::{app::NebulusApp, model::ReceiverState, ui::format_bitrate};

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(3, 7, 8));

    if let Some(frame_size) = app.frame_size {
        let source_aspect = frame_size[0] as f32 / frame_size[1].max(1) as f32;
        let target_aspect = rect.width() / rect.height().max(1.0);
        let size = if target_aspect > source_aspect {
            egui::vec2(rect.height() * source_aspect, rect.height())
        } else {
            egui::vec2(rect.width(), rect.width() / source_aspect)
        };
        let image_rect = egui::Rect::from_center_size(rect.center(), size);
        if let Some(renderer) = app.video_renderer.as_ref() {
            renderer.paint(&painter, image_rect);
        } else if let Some(texture) = app.texture.as_ref() {
            painter.image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    } else {
        let (title, detail) = match app.state {
            ReceiverState::Receiving => (
                "Waiting for an IDR frame",
                "Packets are arriving; waiting for codec configuration and a keyframe",
            ),
            ReceiverState::Connecting => (
                "Initializing receiver",
                "Configuring the USB adapter and radio",
            ),
            ReceiverState::Failed => (
                "Receiver error",
                "Open Diagnostics or Logs for the failure details",
            ),
            _ => (
                "Receiver not started",
                "Select an adapter, confirm the key, then start RX",
            ),
        };
        painter.text(
            rect.center() - egui::vec2(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(22.0),
            egui::Color32::from_gray(190),
        );
        painter.text(
            rect.center() + egui::vec2(0.0, 18.0),
            egui::Align2::CENTER_CENTER,
            detail,
            egui::FontId::proportional(13.0),
            egui::Color32::from_gray(110),
        );
    }

    if app.settings.show_osd && app.state == ReceiverState::Receiving {
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - 44.0),
            rect.right_bottom(),
        );
        painter.rect_filled(bar, 0.0, egui::Color32::from_black_alpha(205));
        painter.line_segment(
            [bar.left_top(), bar.right_top()],
            egui::Stroke::new(1.0, egui::Color32::from_rgb(61, 214, 154)),
        );
        let resolution = app
            .metrics
            .resolution
            .map(|[width, height]| format!("{width}x{height}"))
            .unwrap_or_else(|| "--".to_owned());
        let mut x = bar.left() + 13.0;
        let y = bar.center().y;
        if rect.width() < 620.0 {
            let base_widths = [44.0, 34.0, 44.0, 39.0, 35.0, 32.0, 38.0];
            let base_total = base_widths.iter().sum::<f32>();
            let spacing =
                ((bar.width() - 26.0 - base_total) / base_widths.len() as f32).clamp(0.0, 10.0);
            let compact_resolution = app
                .metrics
                .resolution
                .map(|[_, height]| format!("{height}p"))
                .unwrap_or_else(|| "--".to_owned());
            let strongest_rssi = app.metrics.rssi[0].max(app.metrics.rssi[1]);
            let strongest_link = app.metrics.link_score[0].max(app.metrics.link_score[1]);
            x = compact_hud_item(
                &painter,
                x,
                y,
                HudIcon::Display,
                &compact_resolution,
                base_widths[0] + spacing,
            );
            x = compact_hud_item(
                &painter,
                x,
                y,
                HudIcon::Fps,
                &format!("{:.0}", app.metrics.decode_fps),
                base_widths[1] + spacing,
            );
            x = compact_hud_item(
                &painter,
                x,
                y,
                HudIcon::Bitrate,
                &format_compact_bitrate(app.metrics.bitrate_bps),
                base_widths[2] + spacing,
            );
            x = compact_hud_item(
                &painter,
                x,
                y,
                HudIcon::Latency,
                &format!("{:.0}ms", app.metrics.local_processing_latency_ms),
                base_widths[3] + spacing,
            );
            x = compact_hud_item(
                &painter,
                x,
                y,
                HudIcon::Signal,
                &strongest_rssi.to_string(),
                base_widths[4] + spacing,
            );
            x = compact_hud_item(
                &painter,
                x,
                y,
                HudIcon::Loss,
                &app.metrics.lost_packets.to_string(),
                base_widths[5] + spacing,
            );
            let _ = compact_hud_item(
                &painter,
                x,
                y,
                HudIcon::Link,
                &strongest_link.to_string(),
                base_widths[6] + spacing,
            );
        } else {
            x = hud_item(&painter, x, y, HudIcon::Display, &resolution, 82.0);
            x = hud_item(
                &painter,
                x,
                y,
                HudIcon::Fps,
                &format!("{:.0} fps", app.metrics.decode_fps),
                72.0,
            );
            x = hud_item(
                &painter,
                x,
                y,
                HudIcon::Bitrate,
                &format_bitrate(app.metrics.bitrate_bps),
                96.0,
            );
            x = hud_item(
                &painter,
                x,
                y,
                HudIcon::Latency,
                &format!("{:.1} ms", app.metrics.local_processing_latency_ms),
                78.0,
            );
            x = hud_item(
                &painter,
                x,
                y,
                HudIcon::Signal,
                &format!("{}/{} dBm", app.metrics.rssi[0], app.metrics.rssi[1]),
                96.0,
            );
            x = hud_item(
                &painter,
                x,
                y,
                HudIcon::Loss,
                &format!("{} lost", app.metrics.lost_packets),
                78.0,
            );
            let _ = hud_item(
                &painter,
                x,
                y,
                HudIcon::Link,
                &format!(
                    "{}/{}",
                    app.metrics.link_score[0], app.metrics.link_score[1]
                ),
                72.0,
            );
        }
    }

    if app.recording.state != crate::model::RecordingState::Idle {
        painter.text(
            rect.left_top() + egui::vec2(14.0, 14.0),
            egui::Align2::LEFT_TOP,
            if app.recording.state == crate::model::RecordingState::Armed {
                "REC ARMED - waiting for keyframe"
            } else {
                "REC"
            },
            egui::FontId::monospace(12.0),
            egui::Color32::from_rgb(244, 88, 96),
        );
    }

    let compact_osd =
        app.settings.show_osd && app.state == ReceiverState::Receiving && rect.width() < 620.0;
    let fullscreen_rect = egui::Rect::from_min_size(
        rect.right_bottom() - egui::vec2(42.0, if compact_osd { 88.0 } else { 42.0 }),
        egui::vec2(36.0, 36.0),
    );
    let fullscreen = ui
        .put(
            fullscreen_rect,
            egui::Button::new("")
                .selected(app.video_fullscreen)
                .corner_radius(4),
        )
        .on_hover_text(if app.video_fullscreen {
            "Exit fullscreen"
        } else {
            "Enter fullscreen"
        });
    draw_fullscreen_icon(ui, &fullscreen, app.video_fullscreen);
    if fullscreen.clicked() {
        app.set_video_fullscreen(ui.ctx(), !app.video_fullscreen);
    }

    if response.double_clicked() {
        app.set_video_fullscreen(ui.ctx(), !app.video_fullscreen);
    }
}

#[derive(Clone, Copy)]
enum HudIcon {
    Display,
    Fps,
    Bitrate,
    Latency,
    Signal,
    Loss,
    Link,
}

fn hud_item(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    icon: HudIcon,
    value: &str,
    slot_width: f32,
) -> f32 {
    let color = egui::Color32::from_gray(218);
    let galley = painter.layout_no_wrap(value.to_owned(), egui::FontId::monospace(10.0), color);
    let icon_rect = egui::Rect::from_center_size(egui::pos2(x + 6.0, y), egui::vec2(12.0, 12.0));
    draw_hud_icon(painter, icon_rect, icon, egui::Stroke::new(1.3, color));
    let text_x = x + 17.0;
    painter.galley(
        egui::pos2(text_x, y - galley.size().y * 0.5),
        galley.clone(),
        color,
    );
    x + slot_width
}

fn compact_hud_item(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    icon: HudIcon,
    value: &str,
    slot_width: f32,
) -> f32 {
    let color = egui::Color32::from_gray(218);
    let galley = painter.layout_no_wrap(value.to_owned(), egui::FontId::monospace(8.5), color);
    let icon_rect = egui::Rect::from_center_size(egui::pos2(x + 5.0, y), egui::vec2(10.0, 10.0));
    draw_hud_icon(painter, icon_rect, icon, egui::Stroke::new(1.15, color));
    painter.galley(
        egui::pos2(x + 14.0, y - galley.size().y * 0.5),
        galley,
        color,
    );
    x + slot_width
}

fn format_compact_bitrate(bits_per_second: f64) -> String {
    if bits_per_second >= 1_000_000.0 {
        format!("{:.1}M", bits_per_second / 1_000_000.0)
    } else if bits_per_second >= 1_000.0 {
        format!("{:.0}k", bits_per_second / 1_000.0)
    } else {
        format!("{bits_per_second:.0}")
    }
}

fn draw_hud_icon(painter: &egui::Painter, rect: egui::Rect, icon: HudIcon, stroke: egui::Stroke) {
    match icon {
        HudIcon::Display => {
            painter.rect_stroke(rect.shrink(1.0), 1.0, stroke, egui::StrokeKind::Middle);
            painter.line_segment(
                [
                    egui::pos2(rect.center().x, rect.bottom() - 1.0),
                    egui::pos2(rect.center().x, rect.bottom() + 1.5),
                ],
                stroke,
            );
        }
        HudIcon::Fps => {
            painter.add(egui::Shape::convex_polygon(
                vec![rect.left_top(), rect.left_bottom(), rect.right_center()],
                stroke.color,
                egui::Stroke::NONE,
            ));
        }
        HudIcon::Bitrate => {
            painter.arrow(rect.left_center(), egui::vec2(9.0, -4.0), stroke);
            painter.arrow(rect.right_center(), egui::vec2(-9.0, 4.0), stroke);
        }
        HudIcon::Latency => {
            painter.circle_stroke(rect.center(), 5.0, stroke);
            painter.line_segment(
                [rect.center(), rect.center() + egui::vec2(0.0, -3.0)],
                stroke,
            );
            painter.line_segment(
                [rect.center(), rect.center() + egui::vec2(2.5, 1.5)],
                stroke,
            );
        }
        HudIcon::Signal => {
            for (index, height) in [3.0, 5.0, 8.0, 11.0].into_iter().enumerate() {
                let x = rect.left() + index as f32 * 3.2;
                painter.line_segment(
                    [
                        egui::pos2(x, rect.bottom()),
                        egui::pos2(x, rect.bottom() - height),
                    ],
                    stroke,
                );
            }
        }
        HudIcon::Loss => {
            painter.add(egui::Shape::closed_line(
                vec![rect.center_top(), rect.left_bottom(), rect.right_bottom()],
                stroke,
            ));
            painter.line_segment(
                [
                    rect.center() - egui::vec2(0.0, 2.0),
                    rect.center() + egui::vec2(0.0, 1.0),
                ],
                stroke,
            );
            painter.circle_filled(
                rect.center_bottom() - egui::vec2(0.0, 1.5),
                0.8,
                stroke.color,
            );
        }
        HudIcon::Link => {
            painter.circle_stroke(rect.left_center() + egui::vec2(2.0, 0.0), 3.0, stroke);
            painter.circle_stroke(rect.right_center() - egui::vec2(2.0, 0.0), 3.0, stroke);
            painter.line_segment(
                [
                    rect.left_center() + egui::vec2(5.0, 0.0),
                    rect.right_center() - egui::vec2(5.0, 0.0),
                ],
                stroke,
            );
        }
    }
}

fn draw_fullscreen_icon(ui: &egui::Ui, response: &egui::Response, active: bool) {
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    let stroke = egui::Stroke::new(1.8, color);
    let outer = response.rect.shrink(10.0);
    let arm = 5.0;
    let painter = ui.painter();
    if active {
        let inner = outer.shrink(3.0);
        for (corner, horizontal, vertical) in [
            (inner.left_top(), -arm, -arm),
            (inner.right_top(), arm, -arm),
            (inner.left_bottom(), -arm, arm),
            (inner.right_bottom(), arm, arm),
        ] {
            painter.line_segment([corner, corner + egui::vec2(horizontal, 0.0)], stroke);
            painter.line_segment([corner, corner + egui::vec2(0.0, vertical)], stroke);
        }
    } else {
        for (corner, horizontal, vertical) in [
            (outer.left_top(), arm, arm),
            (outer.right_top(), -arm, arm),
            (outer.left_bottom(), arm, -arm),
            (outer.right_bottom(), -arm, -arm),
        ] {
            painter.line_segment([corner, corner + egui::vec2(horizontal, 0.0)], stroke);
            painter.line_segment([corner, corner + egui::vec2(0.0, vertical)], stroke);
        }
    }
}
