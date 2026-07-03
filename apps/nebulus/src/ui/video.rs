use eframe::egui;

use crate::{app::NebulusApp, model::ReceiverState, settings::HudMetric, ui::format_bitrate};

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(3, 7, 8));

    let mut video_rect = rect;
    if let Some(frame_size) = app.frame_size {
        let source_aspect = frame_size[0] as f32 / frame_size[1].max(1) as f32;
        let target_aspect = rect.width() / rect.height().max(1.0);
        let size = if target_aspect > source_aspect {
            egui::vec2(rect.height() * source_aspect, rect.height())
        } else {
            egui::vec2(rect.width(), rect.width() / source_aspect)
        };
        let image_rect = egui::Rect::from_center_size(rect.center(), size);
        video_rect = image_rect;
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
            ReceiverState::Scanning => (
                "Scanning radio channels",
                "Receiver start is disabled until the idle survey completes",
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
        draw_hud(app, &painter, video_rect);
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

    let fullscreen_rect = egui::Rect::from_min_size(
        rect.right_bottom() - egui::vec2(42.0, 42.0),
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

fn draw_hud(app: &NebulusApp, painter: &egui::Painter, video_rect: egui::Rect) {
    let compact = video_rect.width() < 620.0;
    let scale = f32::from(app.settings.hud.scale_percent) / 100.0;
    for item in app.settings.hud.items.iter().filter(|item| item.visible) {
        let value = hud_value(app, item.metric, compact);
        let font = egui::FontId::monospace(if compact { 8.5 } else { 10.0 } * scale);
        let color = egui::Color32::from_gray(226);
        let galley = painter.layout_no_wrap(value, font, color);
        let icon_size = if compact { 10.0 } else { 12.0 } * scale;
        let padding = egui::vec2(6.0 * scale, 4.0 * scale);
        let size = egui::vec2(
            icon_size + 5.0 * scale + galley.size().x + padding.x * 2.0,
            galley.size().y.max(icon_size) + padding.y * 2.0,
        );
        let requested = egui::pos2(
            egui::lerp(video_rect.x_range(), item.x),
            egui::lerp(video_rect.y_range(), item.y),
        );
        let center = egui::pos2(
            clamp_center(requested.x, video_rect.left(), video_rect.right(), size.x),
            clamp_center(requested.y, video_rect.top(), video_rect.bottom(), size.y),
        );
        let item_rect = egui::Rect::from_center_size(center, size);
        painter.rect_filled(
            item_rect,
            3.0,
            egui::Color32::from_black_alpha(app.settings.hud.background_opacity),
        );
        let icon_rect = egui::Rect::from_min_size(
            item_rect.left_center() + egui::vec2(padding.x, -icon_size * 0.5),
            egui::vec2(icon_size, icon_size),
        );
        draw_hud_icon(
            painter,
            icon_rect,
            hud_icon(item.metric),
            egui::Stroke::new(1.2 * scale, color),
        );
        painter.galley(
            egui::pos2(
                icon_rect.right() + 5.0 * scale,
                item_rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            color,
        );
    }
}

fn clamp_center(value: f32, minimum: f32, maximum: f32, item_size: f32) -> f32 {
    if item_size >= maximum - minimum {
        (minimum + maximum) * 0.5
    } else {
        value.clamp(minimum + item_size * 0.5, maximum - item_size * 0.5)
    }
}

fn hud_value(app: &NebulusApp, metric: HudMetric, compact: bool) -> String {
    match metric {
        HudMetric::Resolution if compact => app
            .metrics
            .resolution
            .map(|[_, height]| format!("{height}p"))
            .unwrap_or_else(|| "--".to_owned()),
        HudMetric::Resolution => app
            .metrics
            .resolution
            .map(|[width, height]| format!("{width}x{height}"))
            .unwrap_or_else(|| "--".to_owned()),
        HudMetric::FrameRate if compact => format!("{:.0}", app.metrics.decode_fps),
        HudMetric::FrameRate => format!("{:.0} fps", app.metrics.decode_fps),
        HudMetric::Bitrate if compact => format_compact_bitrate(app.metrics.bitrate_bps),
        HudMetric::Bitrate => format_bitrate(app.metrics.bitrate_bps),
        HudMetric::Latency if compact => {
            format!("{:.0}ms", app.metrics.local_processing_latency_ms)
        }
        HudMetric::Latency => format!("{:.1} ms", app.metrics.local_processing_latency_ms),
        HudMetric::Signal if compact => app.metrics.rssi[0].max(app.metrics.rssi[1]).to_string(),
        HudMetric::Signal => format!("{}/{} dBm", app.metrics.rssi[0], app.metrics.rssi[1]),
        HudMetric::PacketLoss if compact => app.metrics.lost_packets.to_string(),
        HudMetric::PacketLoss => format!("{} lost", app.metrics.lost_packets),
        HudMetric::LinkScore if compact => app.metrics.link_score[0]
            .max(app.metrics.link_score[1])
            .to_string(),
        HudMetric::LinkScore => format!(
            "{}/{}",
            app.metrics.link_score[0], app.metrics.link_score[1]
        ),
    }
}

const fn hud_icon(metric: HudMetric) -> HudIcon {
    match metric {
        HudMetric::Resolution => HudIcon::Display,
        HudMetric::FrameRate => HudIcon::Fps,
        HudMetric::Bitrate => HudIcon::Bitrate,
        HudMetric::Latency => HudIcon::Latency,
        HudMetric::Signal => HudIcon::Signal,
        HudMetric::PacketLoss => HudIcon::Loss,
        HudMetric::LinkScore => HudIcon::Link,
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
