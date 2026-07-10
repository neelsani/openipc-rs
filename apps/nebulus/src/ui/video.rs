use eframe::egui;

use crate::{app::NebulusApp, model::ReceiverState};

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
        draw_osd(app, &painter, video_rect);
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

    let fullscreen_rect = fullscreen_button_rect(rect, cfg!(target_os = "android"));
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

fn fullscreen_button_rect(video_rect: egui::Rect, avoid_bottom_system_ui: bool) -> egui::Rect {
    const BUTTON_SIZE: f32 = 36.0;
    const EDGE_INSET: f32 = 6.0;

    let min = if avoid_bottom_system_ui {
        egui::pos2(
            video_rect.right() - BUTTON_SIZE - EDGE_INSET,
            video_rect.top() + EDGE_INSET,
        )
    } else {
        egui::pos2(
            video_rect.right() - BUTTON_SIZE - EDGE_INSET,
            video_rect.bottom() - BUTTON_SIZE - EDGE_INSET,
        )
    };
    egui::Rect::from_min_size(min, egui::vec2(BUTTON_SIZE, BUTTON_SIZE))
}

fn draw_osd(app: &NebulusApp, painter: &egui::Painter, video_rect: egui::Rect) {
    super::osd::draw(app, painter, video_rect);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_fullscreen_button_avoids_the_bottom_system_area() {
        let video = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(410.0, 820.0));
        let android = fullscreen_button_rect(video, true);
        let desktop = fullscreen_button_rect(video, false);

        assert_eq!(android.right(), video.right() - 6.0);
        assert_eq!(android.top(), video.top() + 6.0);
        assert_eq!(desktop.bottom(), video.bottom() - 6.0);
        assert!(android.bottom() < desktop.top());
    }
}
