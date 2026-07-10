use eframe::egui;
use egui_plot::{
    AxisHints, GridInput, GridMark, Line, MarkerShape, Plot, PlotPoint, PlotPoints, Points, Text,
};

use crate::{
    app::NebulusApp,
    model::{LiveMetrics, METRIC_WINDOW_SECONDS},
    settings::ReceiverSource,
    ui::format_bitrate,
};

pub(crate) fn show(app: &NebulusApp, ui: &mut egui::Ui) {
    latency_composition(app, ui);
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);
    let latest_time = app.metric_view_time();
    ui.scope(|ui| {
        ui.set_max_width((ui.available_width() - 14.0).max(0.0));
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.columns(2, |columns| {
            if app.settings.receiver_source == crate::settings::ReceiverSource::UdpRtp {
                plot_one(
                    &mut columns[0],
                    "Video bitrate (Mbps)",
                    app.history.bitrate.points(),
                    latest_time,
                    PlotScale::Dynamic {
                        non_negative: true,
                        minimum_span: 2.0,
                    },
                    FIRST_SERIES_COLOR,
                );
                plot_one(
                    &mut columns[1],
                    "Delivered video FPS",
                    app.history.receive_fps.points(),
                    latest_time,
                    PlotScale::Dynamic {
                        non_negative: true,
                        minimum_span: 10.0,
                    },
                    SECOND_SERIES_COLOR,
                );
                plot_one(
                    &mut columns[0],
                    "Decoded video FPS",
                    app.history.decode_fps.points(),
                    latest_time,
                    PlotScale::Dynamic {
                        non_negative: true,
                        minimum_span: 10.0,
                    },
                    FIRST_SERIES_COLOR,
                );
                plot_one(
                    &mut columns[1],
                    "Local processing (ms)",
                    app.history.local_processing_ms.points(),
                    latest_time,
                    PlotScale::Dynamic {
                        non_negative: true,
                        minimum_span: 2.0,
                    },
                    SECOND_SERIES_COLOR,
                );
                return;
            }
            plot_one(
                &mut columns[0],
                "Link score",
                app.history.link_score.points(),
                latest_time,
                PlotScale::Dynamic {
                    non_negative: true,
                    minimum_span: 100.0,
                },
                FIRST_SERIES_COLOR,
            );
            plot_one(
                &mut columns[1],
                "Unrecoverable loss (%)",
                app.history.loss.points(),
                latest_time,
                PlotScale::CenteredZero { minimum_span: 1.0 },
                SECOND_SERIES_COLOR,
            );
            plot_one(
                &mut columns[0],
                "FEC recovery (%)",
                app.history.fec_recovery.points(),
                latest_time,
                PlotScale::Dynamic {
                    non_negative: true,
                    minimum_span: 10.0,
                },
                FIRST_SERIES_COLOR,
            );
            plot_one(
                &mut columns[1],
                "Video bitrate (Mbps)",
                app.history.bitrate.points(),
                latest_time,
                PlotScale::Dynamic {
                    non_negative: true,
                    minimum_span: 2.0,
                },
                SECOND_SERIES_COLOR,
            );
            plot_one(
                &mut columns[0],
                "Delivered video FPS",
                app.history.receive_fps.points(),
                latest_time,
                PlotScale::Dynamic {
                    non_negative: true,
                    minimum_span: 10.0,
                },
                FIRST_SERIES_COLOR,
            );
            plot_one(
                &mut columns[1],
                "Local processing (ms)",
                app.history.local_processing_ms.points(),
                latest_time,
                PlotScale::Dynamic {
                    non_negative: true,
                    minimum_span: 2.0,
                },
                SECOND_SERIES_COLOR,
            );
        });
    });
    metrics_summary(app, ui);
}

#[derive(Clone, Copy)]
struct LatencyPart {
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    milliseconds: f64,
    color: egui::Color32,
}

const PARSE_COLOR: egui::Color32 = egui::Color32::from_rgb(116, 155, 240);
const PIPELINE_COLOR: egui::Color32 = egui::Color32::from_rgb(71, 160, 154);
const OTHER_COLOR: egui::Color32 = egui::Color32::from_rgb(166, 125, 205);
const SUBMIT_COLOR: egui::Color32 = egui::Color32::from_rgb(221, 138, 82);
const DECODE_COLOR: egui::Color32 = egui::Color32::from_rgb(218, 92, 112);
const PRESENT_COLOR: egui::Color32 = egui::Color32::from_rgb(190, 150, 55);

fn latency_composition(app: &NebulusApp, ui: &mut egui::Ui) {
    let parts = latency_parts(&app.metrics, app.settings.receiver_source);
    let mut displayed = Vec::with_capacity(parts.len());
    for part in parts {
        let value = ui.ctx().animate_value_with_time(
            ui.make_persistent_id(("latency-composition", part.id)),
            part.milliseconds.min(f64::from(f32::MAX)) as f32,
            0.2,
        );
        displayed.push((part, f64::from(value.max(0.0))));
    }
    let total = displayed.iter().map(|(_, value)| value).sum::<f64>();

    ui.horizontal(|ui| {
        ui.strong("Local latency composition");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if total > f64::EPSILON {
                ui.monospace(format!("{total:.1} ms measured"));
            } else {
                ui.label(
                    egui::RichText::new("Waiting for video")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });
    });
    ui.add_space(5.0);

    let bar_size = egui::vec2(ui.available_width().max(1.0), 34.0);
    let (bar, _) = ui.allocate_exact_size(bar_size, egui::Sense::hover());
    let painter = ui.painter_at(bar);
    painter.rect_filled(bar, 4.0, ui.visuals().extreme_bg_color);

    if total <= f64::EPSILON {
        painter.text(
            bar.center(),
            egui::Align2::CENTER_CENTER,
            "No latency samples",
            egui::FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
    } else {
        let mut left = bar.left();
        let last_index = displayed.len().saturating_sub(1);
        for (index, (part, value)) in displayed.iter().enumerate() {
            if *value <= f64::EPSILON {
                continue;
            }
            let right = if index == last_index {
                bar.right()
            } else {
                (left + bar.width() * (*value / total) as f32).min(bar.right())
            };
            let segment = egui::Rect::from_min_max(
                egui::pos2(left, bar.top()),
                egui::pos2(right, bar.bottom()),
            );
            painter.rect_filled(segment.shrink2(egui::vec2(0.5, 0.0)), 0.0, part.color);
            let response = ui.interact(
                segment,
                ui.make_persistent_id(("latency-part", part.id)),
                egui::Sense::hover(),
            );
            response.on_hover_ui(|ui| {
                ui.strong(part.label);
                ui.monospace(format!("{value:.2} ms  ·  {:.1}%", value / total * 100.0));
                ui.label(part.detail);
            });
            let label = if segment.width() >= 108.0 {
                Some(format!("{}  {:.1} ms", part.label, value))
            } else if segment.width() >= 58.0 {
                Some(part.label.to_owned())
            } else {
                None
            };
            if let Some(label) = label {
                painter.text(
                    segment.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(11.0),
                    contrasting_text(part.color),
                );
            }
            left = right;
        }
    }
    painter.rect_stroke(
        bar,
        4.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );

    ui.add_space(5.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(12.0, 3.0);
        for (part, value) in &displayed {
            ui.label(
                egui::RichText::new(format!("● {}  {:>5.1} ms", part.label, value))
                    .monospace()
                    .small()
                    .color(part.color),
            )
            .on_hover_text(part.detail);
        }
    });
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(latency_scope(app.settings.receiver_source))
            .small()
            .color(ui.visuals().weak_text_color()),
    );
}

fn latency_parts(metrics: &LiveMetrics, source: ReceiverSource) -> Vec<LatencyPart> {
    let parse = (source == ReceiverSource::Usb).then(|| latency(metrics.parse_latency_ms));
    let pipeline = latency(metrics.pipeline_latency_ms);
    let routes = (source == ReceiverSource::UdpRtp).then(|| latency(metrics.route_latency_ms));
    let submit = latency(metrics.decode_submit_latency_ms);
    let known_before_decode =
        parse.unwrap_or_default() + pipeline + routes.unwrap_or_default() + submit;
    let submit_path = latency(metrics.video_submit_path_ms).max(known_before_decode);
    let other = (submit_path - known_before_decode).max(0.0);

    let mut parts = Vec::with_capacity(6);
    if let Some(milliseconds) = parse {
        parts.push(LatencyPart {
            id: "parse",
            label: "Parse",
            detail: "Realtek aggregate and RX descriptor parsing",
            milliseconds,
            color: PARSE_COLOR,
        });
    }
    parts.push(LatencyPart {
        id: "pipeline",
        label: pipeline_label(source),
        detail: pipeline_detail(source),
        milliseconds: pipeline,
        color: PIPELINE_COLOR,
    });
    if let Some(milliseconds) = routes {
        parts.push(LatencyPart {
            id: "routes",
            label: "Routes",
            detail: "Configured routes and audio processing before decoder submission",
            milliseconds,
            color: PARSE_COLOR,
        });
    }
    parts.push(LatencyPart {
        id: "other",
        label: "Other",
        detail: other_detail(source),
        milliseconds: other,
        color: OTHER_COLOR,
    });
    parts.push(LatencyPart {
        id: "submit",
        label: submit_label(),
        detail: submit_detail(),
        milliseconds: submit,
        color: SUBMIT_COLOR,
    });
    parts.push(LatencyPart {
        id: "decode",
        label: "Decode",
        detail: "Platform decoder submission until decoded output becomes available",
        milliseconds: latency(metrics.decode_latency_ms),
        color: DECODE_COLOR,
    });
    parts.push(LatencyPart {
        id: "present",
        label: "Present",
        detail: "Decoded-frame event wait and GPU texture upload or surface latch",
        milliseconds: latency(metrics.presentation_queue_latency_ms),
        color: PRESENT_COLOR,
    });
    parts
}

fn latency(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn pipeline_label(source: ReceiverSource) -> &'static str {
    match source {
        ReceiverSource::UdpRtp => "RTP",
        ReceiverSource::Usb => {
            if cfg!(target_arch = "wasm32") {
                "WFB"
            } else {
                "WFB/RTP"
            }
        }
    }
}

fn pipeline_detail(source: ReceiverSource) -> &'static str {
    match source {
        ReceiverSource::UdpRtp => "RTP validation, reordering, and depacketization",
        ReceiverSource::Usb => {
            if cfg!(target_arch = "wasm32") {
                "WFB decrypt/FEC recovery and dispatch toward the browser RTP worker"
            } else {
                "WFB decrypt/FEC recovery, RTP reordering, and depacketization"
            }
        }
    }
}

fn other_detail(source: ReceiverSource) -> &'static str {
    match source {
        ReceiverSource::Usb => {
            "802.11 selection, diversity, recording tap, and unassigned receive overhead"
        }
        ReceiverSource::UdpRtp => "Recording tap and unassigned pre-decode overhead",
    }
}

fn submit_label() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "Dispatch"
    } else {
        "Submit"
    }
}

fn submit_detail() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "RTP batch transfer from the application to the browser worker"
    } else {
        "Encoded access-unit handoff to the platform decoder"
    }
}

fn latency_scope(source: ReceiverSource) -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "Measured browser path; worker queue/transit and display scanout are not yet represented."
    } else {
        match source {
            ReceiverSource::Usb => {
                "USB completion to GPU upload; excludes USB wait and display scanout."
            }
            ReceiverSource::UdpRtp => {
                "UDP datagram receipt to GPU upload; excludes socket wait and display scanout."
            }
        }
    }
}

fn contrasting_text(background: egui::Color32) -> egui::Color32 {
    let luminance =
        u16::from(background.r()) * 3 + u16::from(background.g()) * 6 + u16::from(background.b());
    if luminance > 1_450 {
        egui::Color32::from_rgb(24, 25, 38)
    } else {
        egui::Color32::WHITE
    }
}

fn metrics_summary(app: &NebulusApp, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Current values").strong());
    ui.add_space(4.0);

    let resolution = app
        .metrics
        .resolution
        .map(|[width, height]| format!("{width} x {height}"))
        .unwrap_or_else(|| "Waiting".to_owned());
    let radio = app
        .receiver_info
        .as_ref()
        .map(|receiver| {
            if app.receiver_infos.len() > 1 {
                format!("{} + {}", receiver.label, app.receiver_infos.len() - 1)
            } else {
                receiver.label.clone()
            }
        })
        .unwrap_or_else(|| "Not connected".to_owned());

    ui.columns(2, |columns| {
        metric_group(&mut columns[0], "STREAM", "metrics_stream", |ui| {
            metric_row(ui, "Resolution", &resolution);
            metric_row(
                ui,
                "Receive",
                &format!("{:.1} fps", app.metrics.receive_fps),
            );
            metric_row(ui, "Decode", &format!("{:.1} fps", app.metrics.decode_fps));
            metric_row(ui, "Render", &format!("{:.1} fps", app.metrics.render_fps));
            metric_row(ui, "Bitrate", &format_bitrate(app.metrics.bitrate_bps));
            metric_row(ui, "Input", &radio);
        });

        metric_group(
            &mut columns[1],
            "PIPELINE / LINK",
            "metrics_pipeline",
            |ui| {
                metric_row(ui, "Decoder", app.metrics.decoder_label());
                metric_row(
                    ui,
                    "Decode latency",
                    &format!("{:.1} ms", app.metrics.decode_latency_ms),
                );
                metric_row(
                    ui,
                    "Local processing",
                    &format!("{:.1} ms", app.metrics.local_processing_latency_ms),
                );
                metric_row(
                    ui,
                    "Decoder drops / errors",
                    &format!(
                        "{} / {}",
                        app.metrics.decoder_drops, app.metrics.decoder_errors
                    ),
                );
                metric_row(
                    ui,
                    "RSSI / SNR",
                    &if app.settings.receiver_source == crate::settings::ReceiverSource::UdpRtp {
                        "Not available for UDP RTP".to_owned()
                    } else {
                        format!(
                            "{}/{} dBm  {}/{} dB",
                            app.metrics.rssi[0],
                            app.metrics.rssi[1],
                            app.metrics.snr[0],
                            app.metrics.snr[1]
                        )
                    },
                );
                metric_row(
                    ui,
                    "FEC recovered / lost",
                    &format!(
                        "{} / {}",
                        app.metrics.recovered_packets, app.metrics.lost_packets
                    ),
                );
            },
        );
    });
}

fn metric_group(ui: &mut egui::Ui, heading: &str, id: &str, rows: impl FnOnce(&mut egui::Ui)) {
    ui.label(
        egui::RichText::new(heading)
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(2.0);
    egui::Grid::new(id)
        .num_columns(2)
        .striped(true)
        .spacing(egui::vec2(10.0, 5.0))
        .show(ui, rows);
}

fn metric_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(label)
            .small()
            .color(ui.visuals().weak_text_color()),
    );
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(value).monospace().strong())
                .truncate()
                .sense(egui::Sense::hover()),
        )
        .on_hover_text(value);
    });
    ui.end_row();
}

#[derive(Clone, Copy)]
enum PlotScale {
    Dynamic {
        non_negative: bool,
        minimum_span: f64,
    },
    CenteredZero {
        minimum_span: f64,
    },
}

const FIRST_SERIES_COLOR: egui::Color32 = egui::Color32::from_rgb(237, 135, 150);
const SECOND_SERIES_COLOR: egui::Color32 = egui::Color32::from_rgb(138, 173, 244);
const LIVE_GUTTER_SECONDS: f64 = 1.5;
const PLOT_SLOT_HEIGHT: f32 = 152.0;

fn plot_one<Series>(
    ui: &mut egui::Ui,
    id: &str,
    points: Series,
    latest_time: f64,
    scale: PlotScale,
    color: egui::Color32,
) where
    Series: Iterator<Item = [f64; 2]>,
{
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, PLOT_SLOT_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        move |ui| {
            // Offscreen plots still occupy their full slot. Without this,
            // skipping their paint work collapses the scrollable content and
            // clamps the panel back toward the top while the user scrolls.
            ui.set_min_height(PLOT_SLOT_HEIGHT);
            if !ui.is_rect_visible(ui.max_rect()) {
                return;
            }
            let window_end = latest_time.max(METRIC_WINDOW_SECONDS)
                + if latest_time >= METRIC_WINDOW_SECONDS {
                    LIVE_GUTTER_SECONDS
                } else {
                    0.0
                };
            let window_start = window_end - METRIC_WINDOW_SECONDS;
            let points = points.collect::<Vec<_>>();
            let latest = points.last().copied();
            let y_bounds = match scale {
                PlotScale::Dynamic {
                    non_negative,
                    minimum_span,
                } => dynamic_y_bounds(&points, non_negative, minimum_span),
                PlotScale::CenteredZero { minimum_span } => {
                    centered_zero_y_bounds(&points, minimum_span)
                }
            };
            let hide_negative_ticks = matches!(scale, PlotScale::CenteredZero { .. });

            ui.add_sized(
                [ui.available_width(), 20.0],
                egui::Label::new(egui::RichText::new(id).strong())
                    .halign(egui::Align::Center)
                    .truncate(),
            );

            let axis = AxisHints::new_y()
                .formatter(move |mark, _| {
                    if hide_negative_ticks && mark.value < 0.0 {
                        String::new()
                    } else {
                        format_axis_tick(mark.value, mark.step_size)
                    }
                })
                .min_thickness(48.0)
                .label_spacing(0.0..=1.0)
                .tick_label_color(ui.visuals().text_color())
                .tick_label_font(egui::FontId::monospace(10.0));
            let plot = Plot::new(id)
                .width(ui.available_width())
                .height(104.0)
                .sense(egui::Sense::hover())
                .allow_drag(false)
                .allow_axis_zoom_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .allow_double_click_reset(false)
                .auto_bounds([false, false])
                .show_axes([false, true])
                .show_grid([true, true])
                .y_grid_spacer(stable_y_grid)
                .grid_fade(0.25)
                .custom_y_axes(vec![axis]);

            plot.show(ui, |plot| {
                plot.set_plot_bounds_x(window_start..=window_end);
                plot.set_plot_bounds_y(y_bounds);
                plot.line(
                    Line::new(id, PlotPoints::from(points))
                        .color(color)
                        .width(1.5)
                        .allow_hover(false),
                );
                if let Some(point) = latest {
                    endpoint(plot, id, id, point, color, egui::Align2::RIGHT_CENTER);
                }
            });
        },
    );
}

fn dynamic_y_bounds(
    points: &[[f64; 2]],
    non_negative: bool,
    minimum_span: f64,
) -> std::ops::RangeInclusive<f64> {
    let mut count = 0usize;
    let mut sum = 0.0;
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for value in points
        .iter()
        .map(|point| point[1])
        .filter(|value| value.is_finite())
    {
        count += 1;
        sum += value;
        minimum = minimum.min(value);
        maximum = maximum.max(value);
    }
    if count == 0 {
        return 0.0..=minimum_span;
    }

    let average = sum / count as f64;
    let observed_span = maximum - minimum;

    let (mut lower, mut upper) = if observed_span >= minimum_span {
        let padding = observed_span * 0.15;
        (minimum - padding, maximum + padding)
    } else {
        let half_span = minimum_span * 0.5;
        (average - half_span, average + half_span)
    };

    if non_negative && lower < 0.0 {
        upper -= lower;
        lower = 0.0;
    }
    lower..=upper
}

fn centered_zero_y_bounds(points: &[[f64; 2]], minimum_span: f64) -> std::ops::RangeInclusive<f64> {
    let has_nonzero_value = points
        .iter()
        .map(|point| point[1])
        .any(|value| value.is_finite() && value.abs() > f64::EPSILON);
    if has_nonzero_value {
        dynamic_y_bounds(points, true, minimum_span)
    } else {
        let half_span = minimum_span * 0.5;
        -half_span..=half_span
    }
}

fn format_axis_tick(value: f64, step: f64) -> String {
    if step.abs() >= 1.0 {
        format!("{value:.0}")
    } else if step.abs() >= 0.1 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn stable_y_grid(input: GridInput) -> Vec<GridMark> {
    let (lower, upper) = input.bounds;
    let span = upper - lower;
    if !lower.is_finite() || !upper.is_finite() || span <= f64::EPSILON {
        return Vec::new();
    }
    let step = nice_step(span / 5.0);
    let first = (lower / step).ceil() * step;
    let mut marks = Vec::with_capacity(6);
    let mut value = first;
    while value <= upper + step * 1e-9 && marks.len() < 8 {
        marks.push(GridMark {
            value: if value.abs() < step * 1e-9 {
                0.0
            } else {
                value
            },
            step_size: step,
        });
        value += step;
    }
    marks
}

fn nice_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let magnitude = 10.0_f64.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let factor = if normalized < 1.5 {
        1.0
    } else if normalized < 2.25 {
        2.0
    } else if normalized < 3.75 {
        2.5
    } else if normalized < 7.5 {
        5.0
    } else {
        10.0
    };
    factor * magnitude
}

fn endpoint(
    plot: &mut egui_plot::PlotUi<'_>,
    plot_id: &str,
    series_name: &str,
    point: [f64; 2],
    color: egui::Color32,
    anchor: egui::Align2,
) {
    let item_id = format!("{plot_id}-{series_name}-live");
    plot.points(
        Points::new(format!("{item_id}-point"), vec![point])
            .shape(MarkerShape::Circle)
            .radius(4.5)
            .filled(true)
            .color(color)
            .allow_hover(false),
    );
    plot.text(
        Text::new(
            format!("{item_id}-value"),
            PlotPoint::from(point),
            egui::RichText::new(format_live_value(point[1]))
                .monospace()
                .strong()
                .background_color(egui::Color32::from_rgba_unmultiplied(24, 25, 38, 232)),
        )
        .anchor(anchor)
        .color(color)
        .allow_hover(false),
    );
}

fn format_live_value(value: f64) -> String {
    format!(" {value:.2} ")
}

#[cfg(test)]
mod tests {
    use super::{
        centered_zero_y_bounds, dynamic_y_bounds, format_axis_tick, format_live_value,
        latency_parts, nice_step, stable_y_grid, GridInput, LatencyPart,
    };
    use crate::{model::LiveMetrics, settings::ReceiverSource};

    fn part<'a>(parts: &'a [LatencyPart], id: &str) -> &'a LatencyPart {
        parts.iter().find(|part| part.id == id).unwrap()
    }

    #[test]
    fn usb_composition_uses_only_non_overlapping_video_stages() {
        let metrics = LiveMetrics {
            parse_latency_ms: 1.0,
            pipeline_latency_ms: 2.0,
            route_latency_ms: 9.0,
            decode_submit_latency_ms: 0.5,
            video_submit_path_ms: 5.0,
            decode_latency_ms: 4.0,
            presentation_queue_latency_ms: 1.0,
            ..LiveMetrics::default()
        };
        let parts = latency_parts(&metrics, ReceiverSource::Usb);

        assert_eq!(part(&parts, "parse").milliseconds, 1.0);
        assert_eq!(part(&parts, "pipeline").milliseconds, 2.0);
        assert_eq!(part(&parts, "other").milliseconds, 1.5);
        assert_eq!(part(&parts, "submit").milliseconds, 0.5);
        assert_eq!(part(&parts, "decode").milliseconds, 4.0);
        assert_eq!(part(&parts, "present").milliseconds, 1.0);
        assert!(!parts.iter().any(|part| part.id == "routes"));
        assert_eq!(
            parts.iter().map(|part| part.milliseconds).sum::<f64>(),
            10.0
        );
    }

    #[test]
    fn udp_composition_accounts_for_routes_before_decode() {
        let metrics = LiveMetrics {
            parse_latency_ms: 8.0,
            pipeline_latency_ms: 1.0,
            route_latency_ms: 0.4,
            decode_submit_latency_ms: 0.2,
            video_submit_path_ms: 2.0,
            decode_latency_ms: 3.0,
            presentation_queue_latency_ms: 0.5,
            ..LiveMetrics::default()
        };
        let parts = latency_parts(&metrics, ReceiverSource::UdpRtp);

        assert!(!parts.iter().any(|part| part.id == "parse"));
        assert_eq!(part(&parts, "routes").milliseconds, 0.4);
        assert!((part(&parts, "other").milliseconds - 0.4).abs() < f64::EPSILON * 4.0);
        assert!(
            (parts.iter().map(|part| part.milliseconds).sum::<f64>() - 5.5).abs()
                < f64::EPSILON * 4.0
        );
    }

    #[test]
    fn live_value_uses_stable_exact_precision() {
        assert_eq!(format_live_value(-67.125), " -67.12 ");
        assert_eq!(format_live_value(30.0), " 30.00 ");
    }

    #[test]
    fn dynamic_bounds_follow_a_nonzero_signal() {
        let points = [[0.0, 78.0], [1.0, 80.0], [2.0, 82.0]];
        let bounds = dynamic_y_bounds(&points, true, 10.0);
        assert_eq!(bounds, 75.0..=85.0);
    }

    #[test]
    fn dynamic_bounds_keep_zero_as_a_floor_when_required() {
        let points = [[0.0, 0.0], [1.0, 0.0]];
        let bounds = dynamic_y_bounds(&points, true, 1.0);
        assert_eq!(bounds, 0.0..=1.0);
    }

    #[test]
    fn flat_packet_loss_centers_zero_until_loss_arrives() {
        assert_eq!(
            centered_zero_y_bounds(&[[0.0, 0.0], [1.0, 0.0]], 1.0),
            -0.5..=0.5
        );
        let active = centered_zero_y_bounds(&[[0.0, 0.0], [1.0, 1.0]], 1.0);
        assert_eq!(*active.start(), 0.0);
        assert!((*active.end() - 1.3).abs() < f64::EPSILON * 2.0);
    }

    #[test]
    fn axis_tick_precision_tracks_grid_spacing() {
        assert_eq!(format_axis_tick(80.2, 10.0), "80");
        assert_eq!(format_axis_tick(1.25, 0.5), "1.2");
        assert_eq!(format_axis_tick(0.125, 0.05), "0.12");
    }

    #[test]
    fn grid_uses_stable_readable_steps() {
        assert_eq!(nice_step(0.42), 0.5);
        assert_eq!(nice_step(2.8), 2.5);
        assert_eq!(nice_step(21.0), 20.0);

        let marks = stable_y_grid(GridInput {
            bounds: (14.5, 18.5),
            base_step_size: 0.1,
        });
        assert_eq!(
            marks.iter().map(|mark| mark.value).collect::<Vec<_>>(),
            vec![15.0, 16.0, 17.0, 18.0]
        );
    }
}
