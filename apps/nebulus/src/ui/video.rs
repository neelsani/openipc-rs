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
            egui::pos2(rect.left(), rect.bottom() - 42.0),
            rect.right_bottom(),
        );
        painter.rect_filled(bar, 0.0, egui::Color32::from_black_alpha(175));
        let text = if rect.width() < 680.0 {
            format!(
                "RSSI {}/{}  LOSS {}  {}  {:.0} FPS",
                app.metrics.rssi[0],
                app.metrics.rssi[1],
                app.metrics.lost_packets,
                format_bitrate(app.metrics.bitrate_bps),
                app.metrics.decode_fps,
            )
        } else {
            let resolution = app
                .metrics
                .resolution
                .map(|[width, height]| format!("{width}x{height}"))
                .unwrap_or_else(|| "--".to_owned());
            format!(
                "{resolution}    RSSI {}/{} dBm    SNR {}/{} dB    FEC +{}    LOSS {}    {}    {:.1} FPS    LINK {}/{}",
                app.metrics.rssi[0],
                app.metrics.rssi[1],
                app.metrics.snr[0],
                app.metrics.snr[1],
                app.metrics.recovered_packets,
                app.metrics.lost_packets,
                format_bitrate(app.metrics.bitrate_bps),
                app.metrics.decode_fps,
                app.metrics.link_score[0],
                app.metrics.link_score[1],
            )
        };
        painter.text(
            bar.left_center() + egui::vec2(14.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::monospace(12.0),
            egui::Color32::from_gray(225),
        );
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
        rect.right_top() + egui::vec2(-116.0, 12.0),
        egui::vec2(104.0, 30.0),
    );
    if ui
        .put(
            fullscreen_rect,
            egui::Button::new(if app.video_fullscreen {
                "Exit fullscreen"
            } else {
                "Fullscreen"
            }),
        )
        .clicked()
    {
        app.set_video_fullscreen(ui.ctx(), !app.video_fullscreen);
    }

    if response.double_clicked() {
        app.set_video_fullscreen(ui.ctx(), !app.video_fullscreen);
    }
}
