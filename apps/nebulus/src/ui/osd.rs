use eframe::egui;

use crate::{
    app::NebulusApp,
    model::MetricSeries,
    settings::{HudItemSettings, HudMetric},
    ui::format_bitrate,
};

const GOOD: egui::Color32 = egui::Color32::from_rgb(64, 218, 157);
const FAIR: egui::Color32 = egui::Color32::from_rgb(236, 190, 75);
const WEAK: egui::Color32 = egui::Color32::from_rgb(242, 139, 65);
const CRITICAL: egui::Color32 = egui::Color32::from_rgb(244, 88, 96);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(126, 166, 255);
const UNKNOWN: egui::Color32 = egui::Color32::from_gray(145);
const TEXT: egui::Color32 = egui::Color32::from_gray(226);
const MUTED: egui::Color32 = egui::Color32::from_gray(155);

pub(crate) fn draw(app: &NebulusApp, painter: &egui::Painter, video_rect: egui::Rect) {
    let compact = video_rect.width() < 620.0;
    let global_scale = f32::from(app.settings.hud.scale_percent) / 100.0;
    for item in app.settings.hud.items.iter().filter(|item| item.visible) {
        let value = hud_value(app, item.metric, compact);
        if item.hide_when_unavailable && value.is_none() {
            continue;
        }
        let value = value.unwrap_or_else(|| "--".to_owned());
        let size = live_item_size(painter, item, &value, compact, global_scale);
        let requested = egui::pos2(
            egui::lerp(video_rect.x_range(), item.x),
            egui::lerp(video_rect.y_range(), item.y),
        );
        let rect = positioned_rect(video_rect, requested, size);
        draw_live_item(app, painter, rect, item, &value, compact, global_scale);
    }
}

pub(crate) fn preview_item_size(
    painter: &egui::Painter,
    item: &HudItemSettings,
    global_scale: f32,
) -> egui::Vec2 {
    item_size(
        painter,
        item,
        sample_value(item.metric),
        false,
        global_scale,
    )
}

pub(crate) fn positioned_rect(
    bounds: egui::Rect,
    requested: egui::Pos2,
    size: egui::Vec2,
) -> egui::Rect {
    let center = egui::pos2(
        clamp_center(requested.x, bounds.left(), bounds.right(), size.x),
        clamp_center(requested.y, bounds.top(), bounds.bottom(), size.y),
    );
    egui::Rect::from_center_size(center, size)
}

pub(crate) fn draw_preview_item(
    painter: &egui::Painter,
    rect: egui::Rect,
    item: &HudItemSettings,
    global_scale: f32,
    global_background_opacity: u8,
    highlighted: bool,
) {
    let scale = item_scale(item, global_scale);
    let accent = if highlighted {
        GOOD
    } else if item.colorize {
        preview_metric_color(item.metric)
    } else {
        TEXT
    };
    draw_item_shell(
        painter,
        rect,
        item,
        sample_value(item.metric),
        sample_rssi(),
        false,
        scale,
        effective_background_opacity(item, global_background_opacity),
        accent,
    );
    if item.show_graph && item.metric.supports_graph() {
        let samples = preview_samples(item.metric);
        draw_preview_graph(
            painter,
            graph_rect(rect, scale),
            samples,
            preview_graph_range(item.metric),
            accent,
            scale,
            item.graph_fill,
        );
    }
}

fn live_item_size(
    painter: &egui::Painter,
    item: &HudItemSettings,
    value: &str,
    compact: bool,
    global_scale: f32,
) -> egui::Vec2 {
    item_size(painter, item, value, compact, global_scale)
}

fn item_size(
    painter: &egui::Painter,
    item: &HudItemSettings,
    value: &str,
    compact: bool,
    global_scale: f32,
) -> egui::Vec2 {
    let scale = item_scale(item, global_scale);
    let header = header_size(painter, item, value, compact, scale);
    if item.show_graph && item.metric.supports_graph() {
        let compact_factor = if compact { 0.82 } else { 1.0 };
        egui::vec2(
            (f32::from(item.graph_width) * compact_factor * scale).max(header.x),
            (f32::from(item.graph_height) * compact_factor * scale).max(header.y + 14.0 * scale),
        )
    } else {
        header
    }
}

fn header_size(
    painter: &egui::Painter,
    item: &HudItemSettings,
    value: &str,
    compact: bool,
    scale: f32,
) -> egui::Vec2 {
    let font = value_font(compact, scale);
    let label_font = label_font(compact, scale);
    let icon_size = icon_size(compact, scale);
    let mut width = 12.0 * scale;
    let mut height = icon_size;
    let mut components: usize = 0;

    if item.show_icon {
        width += icon_size;
        components += 1;
    }
    if item.show_signal_bars && item.metric.supports_signal_bars() {
        width += 25.0 * scale;
        components += 1;
    }
    if item.show_label {
        let galley = painter.layout_no_wrap(hud_label(item.metric).to_owned(), label_font, MUTED);
        width += galley.size().x;
        height = height.max(galley.size().y);
        components += 1;
    }
    if item.show_value {
        let galley = painter.layout_no_wrap(value.to_owned(), font, TEXT);
        width += galley.size().x;
        height = height.max(galley.size().y);
        components += 1;
    }
    width += components.saturating_sub(1) as f32 * 5.0 * scale;
    egui::vec2(width, height + 8.0 * scale)
}

fn draw_live_item(
    app: &NebulusApp,
    painter: &egui::Painter,
    rect: egui::Rect,
    item: &HudItemSettings,
    value: &str,
    compact: bool,
    global_scale: f32,
) {
    let scale = item_scale(item, global_scale);
    let accent = if item.colorize {
        live_metric_color(app, item.metric)
    } else {
        TEXT
    };
    draw_item_shell(
        painter,
        rect,
        item,
        value,
        best_rssi(app),
        compact,
        scale,
        effective_background_opacity(item, app.settings.hud.background_opacity),
        accent,
    );

    if item.show_graph && item.metric.supports_graph() {
        if let Some(spec) = live_graph(app, item.metric) {
            draw_live_graph(
                painter,
                graph_rect(rect, scale),
                spec.series,
                f64::from(item.graph_seconds),
                spec.range,
                accent,
                scale,
                item.graph_fill,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_item_shell(
    painter: &egui::Painter,
    rect: egui::Rect,
    item: &HudItemSettings,
    value: &str,
    rssi: i32,
    compact: bool,
    scale: f32,
    background_opacity: u8,
    accent: egui::Color32,
) {
    panel_background(painter, rect, background_opacity);
    let header_height = if item.show_graph && item.metric.supports_graph() {
        (18.0 * scale).min(rect.height())
    } else {
        rect.height()
    };
    let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_height));
    let icon_size = icon_size(compact, scale);
    let mut cursor = header.left() + 6.0 * scale;

    if item.show_icon {
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(cursor + icon_size * 0.5, header.center().y),
            egui::vec2(icon_size, icon_size),
        );
        draw_hud_icon(
            painter,
            icon_rect,
            hud_icon(item.metric),
            egui::Stroke::new(1.2 * scale, accent),
        );
        cursor = icon_rect.right() + 5.0 * scale;
    }

    if item.show_signal_bars && item.metric.supports_signal_bars() {
        let bars = egui::Rect::from_min_size(
            egui::pos2(cursor, header.center().y - 7.0 * scale),
            egui::vec2(24.0 * scale, 14.0 * scale),
        );
        draw_signal_bar_glyph(painter, bars, rssi, scale, accent);
        cursor = bars.right() + 5.0 * scale;
    }

    if item.show_label {
        let galley = painter.layout_no_wrap(
            hud_label(item.metric).to_owned(),
            label_font(compact, scale),
            MUTED,
        );
        painter.galley(
            egui::pos2(cursor, header.center().y - galley.size().y * 0.5),
            galley.clone(),
            MUTED,
        );
        cursor += galley.size().x + 5.0 * scale;
    }

    if item.show_value {
        let galley = painter.layout_no_wrap(value.to_owned(), value_font(compact, scale), accent);
        painter.galley(
            egui::pos2(cursor, header.center().y - galley.size().y * 0.5),
            galley,
            accent,
        );
    }
}

fn draw_signal_bar_glyph(
    painter: &egui::Painter,
    rect: egui::Rect,
    rssi: i32,
    scale: f32,
    color: egui::Color32,
) {
    let active = signal_level(rssi);
    let bar_width = 2.6 * scale;
    let gap = 2.0 * scale;
    for index in 0..5 {
        let height = (4.0 + index as f32 * 2.2) * scale;
        let left = rect.left() + index as f32 * (bar_width + gap);
        let bar = egui::Rect::from_min_max(
            egui::pos2(left, rect.bottom() - height),
            egui::pos2(left + bar_width, rect.bottom()),
        );
        painter.rect_filled(
            bar,
            0.7 * scale,
            if index < active {
                color
            } else {
                egui::Color32::from_gray(65)
            },
        );
    }
}

#[derive(Clone, Copy)]
enum SparklineRange {
    Fixed {
        minimum: f64,
        maximum: f64,
    },
    Dynamic {
        floor: Option<f64>,
        minimum_span: f64,
    },
}

struct LiveGraph<'a> {
    series: &'a MetricSeries,
    range: SparklineRange,
}

fn live_graph(app: &NebulusApp, metric: HudMetric) -> Option<LiveGraph<'_>> {
    let (series, range) = match metric {
        HudMetric::FrameRate => (
            &app.history.decode_fps,
            SparklineRange::Dynamic {
                floor: None,
                minimum_span: 10.0,
            },
        ),
        HudMetric::Bitrate => (
            &app.history.bitrate,
            SparklineRange::Dynamic {
                floor: None,
                minimum_span: 2.0,
            },
        ),
        HudMetric::Latency => (
            &app.history.local_processing_ms,
            SparklineRange::Dynamic {
                floor: Some(0.0),
                minimum_span: 15.0,
            },
        ),
        HudMetric::Signal => (
            &app.history.rssi,
            SparklineRange::Fixed {
                minimum: -100.0,
                maximum: -40.0,
            },
        ),
        HudMetric::PacketLoss => (
            &app.history.loss,
            SparklineRange::Dynamic {
                floor: Some(0.0),
                minimum_span: 5.0,
            },
        ),
        HudMetric::LinkScore => (
            &app.history.link_score,
            SparklineRange::Dynamic {
                floor: None,
                minimum_span: 100.0,
            },
        ),
        _ => return None,
    };
    Some(LiveGraph { series, range })
}

fn preview_graph_range(metric: HudMetric) -> SparklineRange {
    match metric {
        HudMetric::Signal => SparklineRange::Fixed {
            minimum: -100.0,
            maximum: -40.0,
        },
        HudMetric::PacketLoss => SparklineRange::Dynamic {
            floor: Some(0.0),
            minimum_span: 5.0,
        },
        HudMetric::Latency => SparklineRange::Dynamic {
            floor: Some(0.0),
            minimum_span: 15.0,
        },
        HudMetric::FrameRate => SparklineRange::Dynamic {
            floor: None,
            minimum_span: 10.0,
        },
        HudMetric::Bitrate => SparklineRange::Dynamic {
            floor: None,
            minimum_span: 2.0,
        },
        _ => SparklineRange::Dynamic {
            floor: None,
            minimum_span: 100.0,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_live_graph(
    painter: &egui::Painter,
    rect: egui::Rect,
    series: &MetricSeries,
    seconds: f64,
    range: SparklineRange,
    color: egui::Color32,
    scale: f32,
    fill: bool,
) {
    let Some(latest) = series.latest_time() else {
        return;
    };
    let start = latest - seconds;
    let points = series
        .points()
        .filter(|point| point[0] >= start)
        .collect::<Vec<_>>();
    let bounds = graph_bounds(points.iter().map(|point| point[1]), range);
    let mapped = points
        .into_iter()
        .map(|[time, sample]| {
            let x = ((time - start) / seconds.max(f64::EPSILON)).clamp(0.0, 1.0) as f32;
            map_graph_point(rect, x, sample, bounds)
        })
        .collect::<Vec<_>>();
    paint_graph(painter, rect, &mapped, color, scale, fill);
}

fn draw_preview_graph(
    painter: &egui::Painter,
    rect: egui::Rect,
    samples: &[f64],
    range: SparklineRange,
    color: egui::Color32,
    scale: f32,
    fill: bool,
) {
    let bounds = graph_bounds(samples.iter().copied(), range);
    let denominator = samples.len().saturating_sub(1).max(1) as f32;
    let points = samples
        .iter()
        .copied()
        .enumerate()
        .map(|(index, sample)| map_graph_point(rect, index as f32 / denominator, sample, bounds))
        .collect::<Vec<_>>();
    paint_graph(painter, rect, &points, color, scale, fill);
}

fn graph_bounds(values: impl Iterator<Item = f64>, range: SparklineRange) -> (f64, f64) {
    if let SparklineRange::Fixed { minimum, maximum } = range {
        return (minimum, maximum);
    }
    let SparklineRange::Dynamic {
        floor,
        minimum_span,
    } = range
    else {
        unreachable!();
    };
    let (mut minimum, mut maximum) = values.fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
    );
    if !minimum.is_finite() || !maximum.is_finite() {
        minimum = floor.unwrap_or(0.0);
        maximum = minimum + minimum_span;
    }
    if let Some(floor) = floor {
        minimum = floor;
    }
    let center = (minimum + maximum) * 0.5;
    let span = (maximum - minimum).max(minimum_span) * 1.12;
    let mut lower = center - span * 0.5;
    let upper = center + span * 0.5;
    if let Some(floor) = floor {
        lower = floor;
        return (lower, upper.max(floor + minimum_span));
    }
    (lower, upper)
}

fn map_graph_point(rect: egui::Rect, x: f32, sample: f64, bounds: (f64, f64)) -> egui::Pos2 {
    let y = ((sample - bounds.0) / (bounds.1 - bounds.0).max(f64::EPSILON)).clamp(0.0, 1.0) as f32;
    egui::pos2(
        egui::lerp(rect.x_range(), x),
        egui::lerp(rect.y_range(), 1.0 - y),
    )
}

fn paint_graph(
    painter: &egui::Painter,
    rect: egui::Rect,
    points: &[egui::Pos2],
    color: egui::Color32,
    scale: f32,
    fill: bool,
) {
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(0.7 * scale, egui::Color32::from_white_alpha(45)),
    );
    if fill && points.len() >= 2 {
        let fill_color = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 32);
        for pair in points.windows(2) {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(pair[0].x, rect.bottom()),
                    pair[0],
                    pair[1],
                    egui::pos2(pair[1].x, rect.bottom()),
                ],
                fill_color,
                egui::Stroke::NONE,
            ));
        }
    }
    for pair in points.windows(2) {
        painter.line_segment([pair[0], pair[1]], egui::Stroke::new(1.25 * scale, color));
    }
    if let Some(last) = points.last() {
        painter.circle_filled(*last, 1.8 * scale, color);
    }
}

fn graph_rect(rect: egui::Rect, scale: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(rect.left() + 6.0 * scale, rect.top() + 18.0 * scale),
        egui::pos2(rect.right() - 6.0 * scale, rect.bottom() - 4.0 * scale),
    )
}

fn panel_background(painter: &egui::Painter, rect: egui::Rect, opacity: u8) {
    if opacity == 0 {
        return;
    }
    painter.rect_filled(rect, 3.0, egui::Color32::from_black_alpha(opacity));
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(0.7, egui::Color32::from_white_alpha(32)),
        egui::StrokeKind::Inside,
    );
}

fn clamp_center(value: f32, minimum: f32, maximum: f32, item_size: f32) -> f32 {
    if item_size >= maximum - minimum {
        (minimum + maximum) * 0.5
    } else {
        value.clamp(minimum + item_size * 0.5, maximum - item_size * 0.5)
    }
}

fn item_scale(item: &HudItemSettings, global_scale: f32) -> f32 {
    global_scale * f32::from(item.scale_percent) / 100.0
}

fn effective_background_opacity(item: &HudItemSettings, global_opacity: u8) -> u8 {
    if !item.show_background {
        return 0;
    }
    (u16::from(global_opacity) * u16::from(item.background_opacity_percent) / 100) as u8
}

fn icon_size(compact: bool, scale: f32) -> f32 {
    (if compact { 10.0 } else { 12.0 }) * scale
}

fn value_font(compact: bool, scale: f32) -> egui::FontId {
    egui::FontId::monospace(if compact { 8.5 } else { 10.0 } * scale)
}

fn label_font(compact: bool, scale: f32) -> egui::FontId {
    egui::FontId::monospace(if compact { 7.5 } else { 8.5 } * scale)
}

fn best_rssi(app: &NebulusApp) -> i32 {
    app.metrics.rssi[0].max(app.metrics.rssi[1])
}

fn sample_rssi() -> i32 {
    -58
}

fn recent_loss(app: &NebulusApp) -> f64 {
    app.history.loss.latest_value().unwrap_or(0.0)
}

fn signal_value(rssi: i32, compact: bool) -> String {
    if rssi >= 0 {
        "--".to_owned()
    } else if compact {
        rssi.to_string()
    } else {
        format!("{rssi} dBm")
    }
}

fn signal_level(rssi: i32) -> usize {
    match rssi {
        -55..=-1 => 5,
        -65..=-56 => 4,
        -75..=-66 => 3,
        -85..=-76 => 2,
        i32::MIN..=-86 => 1,
        _ => 0,
    }
}

fn signal_color(rssi: i32) -> egui::Color32 {
    match rssi {
        -67..=-1 => GOOD,
        -76..=-68 => FAIR,
        -84..=-77 => WEAK,
        i32::MIN..=-85 => CRITICAL,
        _ => UNKNOWN,
    }
}

fn loss_color(loss: f64) -> egui::Color32 {
    if loss >= 10.0 {
        CRITICAL
    } else if loss >= 3.0 {
        WEAK
    } else if loss >= 1.0 {
        FAIR
    } else {
        GOOD
    }
}

fn latency_color(latency: f64) -> egui::Color32 {
    if latency <= 0.0 {
        UNKNOWN
    } else if latency <= 12.0 {
        GOOD
    } else if latency <= 25.0 {
        FAIR
    } else if latency <= 45.0 {
        WEAK
    } else {
        CRITICAL
    }
}

#[derive(Clone, Copy)]
enum LinkHealth {
    Waiting,
    Good,
    Fair,
    Weak,
    Critical,
}

impl LinkHealth {
    const fn label(self) -> &'static str {
        match self {
            Self::Waiting => "WAIT",
            Self::Good => "GOOD",
            Self::Fair => "FAIR",
            Self::Weak => "WEAK",
            Self::Critical => "CRITICAL",
        }
    }

    const fn color(self) -> egui::Color32 {
        match self {
            Self::Waiting => UNKNOWN,
            Self::Good => GOOD,
            Self::Fair => FAIR,
            Self::Weak => WEAK,
            Self::Critical => CRITICAL,
        }
    }
}

fn link_health(app: &NebulusApp) -> LinkHealth {
    link_health_for(best_rssi(app), recent_loss(app))
}

fn link_health_for(rssi: i32, loss: f64) -> LinkHealth {
    if rssi >= 0 {
        LinkHealth::Waiting
    } else if rssi <= -85 || loss >= 10.0 {
        LinkHealth::Critical
    } else if rssi <= -78 || loss >= 3.0 {
        LinkHealth::Weak
    } else if rssi <= -70 || loss >= 1.0 {
        LinkHealth::Fair
    } else {
        LinkHealth::Good
    }
}

fn live_metric_color(app: &NebulusApp, metric: HudMetric) -> egui::Color32 {
    if metric.requires_telemetry()
        && !app
            .telemetry
            .is_fresh(app.settings.telemetry.stale_timeout_ms)
    {
        return UNKNOWN;
    }
    match metric {
        HudMetric::Signal | HudMetric::LinkScore => signal_color(best_rssi(app)),
        HudMetric::PacketLoss => loss_color(recent_loss(app)),
        HudMetric::Latency => latency_color(app.metrics.local_processing_latency_ms),
        HudMetric::LinkHealth => link_health(app).color(),
        HudMetric::Armed => match app.telemetry.armed {
            Some(true) => CRITICAL,
            Some(false) => GOOD,
            None => UNKNOWN,
        },
        HudMetric::BatteryRemaining => app
            .telemetry
            .battery_remaining_pct
            .map_or(UNKNOWN, quality_color),
        HudMetric::GpsStatus => match app.telemetry.gps_fix {
            Some(3..) => GOOD,
            Some(2) => FAIR,
            Some(_) => CRITICAL,
            None => UNKNOWN,
        },
        HudMetric::RcLinkQuality => app
            .telemetry
            .rc_link_quality_pct
            .map_or(UNKNOWN, quality_color),
        HudMetric::StatusText => FAIR,
        _ => ACCENT,
    }
}

fn quality_color(percent: u8) -> egui::Color32 {
    match percent {
        60..=u8::MAX => GOOD,
        30..=59 => FAIR,
        15..=29 => WEAK,
        _ => CRITICAL,
    }
}

fn preview_metric_color(metric: HudMetric) -> egui::Color32 {
    match metric {
        HudMetric::Signal
        | HudMetric::PacketLoss
        | HudMetric::LinkScore
        | HudMetric::Latency
        | HudMetric::LinkHealth => GOOD,
        HudMetric::Armed => CRITICAL,
        HudMetric::StatusText => FAIR,
        metric if metric.requires_telemetry() => GOOD,
        _ => ACCENT,
    }
}

fn hud_value(app: &NebulusApp, metric: HudMetric, compact: bool) -> Option<String> {
    if app.settings.receiver_source == crate::settings::ReceiverSource::UdpRtp
        && matches!(
            metric,
            HudMetric::Signal
                | HudMetric::PacketLoss
                | HudMetric::LinkScore
                | HudMetric::LinkHealth
        )
    {
        return None;
    }
    if metric.requires_telemetry()
        && !app
            .telemetry
            .is_fresh(app.settings.telemetry.stale_timeout_ms)
    {
        return None;
    }
    Some(match metric {
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
        HudMetric::Signal if compact => signal_value(best_rssi(app), true),
        HudMetric::Signal => format!(
            "{}/{} dBm",
            signal_path_value(app.metrics.rssi[0]),
            signal_path_value(app.metrics.rssi[1])
        ),
        HudMetric::PacketLoss => format!("{:.1}%", recent_loss(app)),
        HudMetric::LinkScore if compact => app.metrics.link_score[0]
            .max(app.metrics.link_score[1])
            .to_string(),
        HudMetric::LinkScore => format!(
            "{}/{}",
            app.metrics.link_score[0], app.metrics.link_score[1]
        ),
        HudMetric::LinkHealth => link_health(app).label().to_owned(),
        HudMetric::Armed => match app.telemetry.armed? {
            true => "ARMED".to_owned(),
            false => "DISARMED".to_owned(),
        },
        HudMetric::FlightMode => app.telemetry.flight_mode.clone()?,
        HudMetric::BatteryVoltage => format!("{:.1} V", app.telemetry.battery_voltage_v?),
        HudMetric::BatteryCurrent => format!("{:.1} A", app.telemetry.battery_current_a?),
        HudMetric::BatteryRemaining => {
            format!("{}%", app.telemetry.battery_remaining_pct?)
        }
        HudMetric::GpsStatus => {
            gps_status(app.telemetry.gps_fix, app.telemetry.satellites, compact)?
        }
        HudMetric::Altitude => format!(
            "{:.1} m",
            app.telemetry
                .relative_altitude_m
                .or(app.telemetry.altitude_m)?
        ),
        HudMetric::GroundSpeed => format!("{:.1} m/s", app.telemetry.ground_speed_mps?),
        HudMetric::VerticalSpeed => format!("{:+.1} m/s", app.telemetry.vertical_speed_mps?),
        HudMetric::Heading => format!("{:03.0}°", app.telemetry.heading_deg?.rem_euclid(360.0)),
        HudMetric::HomeDistance => format_distance(app.telemetry.home_distance_m?),
        HudMetric::Throttle => format!("{}%", app.telemetry.throttle_pct?),
        HudMetric::Attitude => format!(
            "R {:+.0}°  P {:+.0}°",
            app.telemetry.roll_deg?, app.telemetry.pitch_deg?
        ),
        HudMetric::StatusText => app
            .telemetry
            .status_text
            .as_deref()?
            .chars()
            .take(if compact { 24 } else { 48 })
            .collect(),
        HudMetric::Coordinates => format!(
            "{:.5}, {:.5}",
            app.telemetry.latitude_deg?, app.telemetry.longitude_deg?
        ),
        HudMetric::RcLinkQuality => format!("{}%", app.telemetry.rc_link_quality_pct?),
        HudMetric::SignalBars
        | HudMetric::SignalTrend
        | HudMetric::LossTrend
        | HudMetric::LatencyTrend => return None,
    })
}

fn gps_status(fix: Option<u8>, satellites: Option<u8>, compact: bool) -> Option<String> {
    let fix = fix?;
    let label = match fix {
        0 => "NO GPS",
        1 => "NO FIX",
        2 => "2D",
        3 => "3D",
        4 => "DGPS",
        5 => "RTK F",
        _ => "RTK",
    };
    Some(match satellites {
        Some(satellites) if compact => format!("{label} {satellites}"),
        Some(satellites) => format!("{label} · {satellites} SAT"),
        None => label.to_owned(),
    })
}

fn format_distance(meters: f32) -> String {
    if meters >= 1_000.0 {
        format!("{:.2} km", meters / 1_000.0)
    } else {
        format!("{meters:.0} m")
    }
}

fn signal_path_value(rssi: i32) -> String {
    if rssi < 0 {
        rssi.to_string()
    } else {
        "--".to_owned()
    }
}

fn sample_value(metric: HudMetric) -> &'static str {
    match metric {
        HudMetric::Resolution => "1920x1080",
        HudMetric::FrameRate => "60 fps",
        HudMetric::Bitrate => "18.4 Mbps",
        HudMetric::Latency => "8.2 ms",
        HudMetric::Signal => "-58/-61 dBm",
        HudMetric::PacketLoss => "0.4%",
        HudMetric::LinkScore => "1680/1620",
        HudMetric::LinkHealth => "GOOD",
        HudMetric::Armed => "ARMED",
        HudMetric::FlightMode => "LOITER",
        HudMetric::BatteryVoltage => "16.8 V",
        HudMetric::BatteryCurrent => "23.5 A",
        HudMetric::BatteryRemaining => "72%",
        HudMetric::GpsStatus => "3D · 14 SAT",
        HudMetric::Altitude => "42.7 m",
        HudMetric::GroundSpeed => "18.2 m/s",
        HudMetric::VerticalSpeed => "+1.4 m/s",
        HudMetric::Heading => "087°",
        HudMetric::HomeDistance => "126 m",
        HudMetric::Throttle => "54%",
        HudMetric::Attitude => "R +4°  P -2°",
        HudMetric::StatusText => "GPS home acquired",
        HudMetric::Coordinates => "41.88183, -87.62318",
        HudMetric::RcLinkQuality => "94%",
        HudMetric::SignalBars
        | HudMetric::SignalTrend
        | HudMetric::LossTrend
        | HudMetric::LatencyTrend => "",
    }
}

fn preview_samples(metric: HudMetric) -> &'static [f64] {
    match metric {
        HudMetric::FrameRate => &[58.0, 60.0, 59.0, 61.0, 60.0, 60.0],
        HudMetric::Bitrate => &[17.2, 18.7, 17.8, 19.1, 18.0, 18.4],
        HudMetric::Latency => &[7.0, 9.0, 8.0, 11.0, 7.5, 8.2],
        HudMetric::Signal => &[-72.0, -68.0, -70.0, -63.0, -65.0, -58.0],
        HudMetric::PacketLoss => &[0.0, 0.0, 1.2, 0.4, 0.0, 0.4],
        HudMetric::LinkScore => &[1510.0, 1570.0, 1540.0, 1630.0, 1600.0, 1680.0],
        _ => &[],
    }
}

const fn hud_label(metric: HudMetric) -> &'static str {
    match metric {
        HudMetric::Resolution => "RES",
        HudMetric::FrameRate => "FPS",
        HudMetric::Bitrate => "RATE",
        HudMetric::Latency => "LAT",
        HudMetric::Signal => "RSSI",
        HudMetric::PacketLoss => "LOSS",
        HudMetric::LinkScore => "LINK",
        HudMetric::LinkHealth => "HEALTH",
        HudMetric::Armed => "STATE",
        HudMetric::FlightMode => "MODE",
        HudMetric::BatteryVoltage => "BAT",
        HudMetric::BatteryCurrent => "CURR",
        HudMetric::BatteryRemaining => "BATT",
        HudMetric::GpsStatus => "GPS",
        HudMetric::Altitude => "ALT",
        HudMetric::GroundSpeed => "SPEED",
        HudMetric::VerticalSpeed => "VARIO",
        HudMetric::Heading => "HDG",
        HudMetric::HomeDistance => "HOME",
        HudMetric::Throttle => "THR",
        HudMetric::Attitude => "ATT",
        HudMetric::StatusText => "STATUS",
        HudMetric::Coordinates => "POS",
        HudMetric::RcLinkQuality => "RC",
        HudMetric::SignalBars
        | HudMetric::SignalTrend
        | HudMetric::LossTrend
        | HudMetric::LatencyTrend => "",
    }
}

const fn hud_icon(metric: HudMetric) -> HudIcon {
    match metric {
        HudMetric::Resolution => HudIcon::Display,
        HudMetric::FrameRate => HudIcon::Fps,
        HudMetric::Bitrate => HudIcon::Bitrate,
        HudMetric::Latency | HudMetric::LatencyTrend => HudIcon::Latency,
        HudMetric::Signal | HudMetric::SignalBars | HudMetric::SignalTrend => HudIcon::Signal,
        HudMetric::PacketLoss | HudMetric::LossTrend => HudIcon::Loss,
        HudMetric::LinkScore | HudMetric::LinkHealth => HudIcon::Link,
        HudMetric::Armed => HudIcon::Armed,
        HudMetric::FlightMode | HudMetric::StatusText => HudIcon::Mode,
        HudMetric::BatteryVoltage | HudMetric::BatteryRemaining => HudIcon::Battery,
        HudMetric::BatteryCurrent => HudIcon::Current,
        HudMetric::GpsStatus | HudMetric::Coordinates => HudIcon::Gps,
        HudMetric::Altitude | HudMetric::VerticalSpeed => HudIcon::Altitude,
        HudMetric::GroundSpeed => HudIcon::Speed,
        HudMetric::Heading | HudMetric::Attitude => HudIcon::Heading,
        HudMetric::HomeDistance => HudIcon::Home,
        HudMetric::Throttle => HudIcon::Throttle,
        HudMetric::RcLinkQuality => HudIcon::Signal,
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
    Armed,
    Mode,
    Battery,
    Current,
    Gps,
    Altitude,
    Speed,
    Heading,
    Home,
    Throttle,
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
        HudIcon::Armed => {
            let body = egui::Rect::from_min_max(
                rect.left_bottom() - egui::vec2(0.0, rect.height() * 0.58),
                rect.right_bottom(),
            );
            painter.rect_stroke(body, 1.0, stroke, egui::StrokeKind::Middle);
            painter.circle_stroke(
                egui::pos2(rect.center().x, body.top()),
                rect.width() * 0.28,
                stroke,
            );
        }
        HudIcon::Mode => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "M",
                egui::FontId::monospace(rect.height() * 0.8),
                stroke.color,
            );
        }
        HudIcon::Battery => {
            let body = egui::Rect::from_min_max(
                rect.left_center() - egui::vec2(0.0, rect.height() * 0.3),
                rect.right_center() + egui::vec2(-2.0, rect.height() * 0.3),
            );
            painter.rect_stroke(body, 1.0, stroke, egui::StrokeKind::Middle);
            painter.line_segment(
                [
                    egui::pos2(body.right() + 1.0, body.center().y - 2.0),
                    egui::pos2(body.right() + 1.0, body.center().y + 2.0),
                ],
                stroke,
            );
        }
        HudIcon::Current => {
            painter.add(egui::Shape::line(
                vec![
                    rect.center_top(),
                    rect.left_center(),
                    rect.center(),
                    rect.center_bottom(),
                    rect.right_center(),
                ],
                stroke,
            ));
        }
        HudIcon::Gps => {
            painter.circle_stroke(rect.center_top() + egui::vec2(0.0, 3.0), 3.0, stroke);
            painter.line_segment(
                [rect.center() + egui::vec2(0.0, 1.0), rect.center_bottom()],
                stroke,
            );
        }
        HudIcon::Altitude => {
            painter.arrow(
                rect.center_bottom(),
                egui::vec2(0.0, -rect.height()),
                stroke,
            );
        }
        HudIcon::Speed => {
            painter.arrow(rect.left_center(), egui::vec2(rect.width(), 0.0), stroke);
        }
        HudIcon::Heading => {
            painter.circle_stroke(rect.center(), rect.width() * 0.45, stroke);
            painter.line_segment([rect.center(), rect.center_top()], stroke);
        }
        HudIcon::Home => {
            painter.add(egui::Shape::closed_line(
                vec![rect.left_center(), rect.center_top(), rect.right_center()],
                stroke,
            ));
            painter.rect_stroke(
                egui::Rect::from_min_max(rect.left_center(), rect.right_bottom()),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        HudIcon::Throttle => {
            for (index, height) in [4.0, 7.0, 10.0].into_iter().enumerate() {
                let x = rect.left() + 1.0 + index as f32 * 4.0;
                painter.line_segment(
                    [
                        egui::pos2(x, rect.bottom()),
                        egui::pos2(x, rect.bottom() - height),
                    ],
                    stroke,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_thresholds_are_monotonic() {
        assert_eq!(signal_level(0), 0);
        assert_eq!(signal_level(-90), 1);
        assert_eq!(signal_level(-80), 2);
        assert_eq!(signal_level(-70), 3);
        assert_eq!(signal_level(-60), 4);
        assert_eq!(signal_level(-50), 5);
    }

    #[test]
    fn link_health_combines_recent_loss_and_rssi() {
        assert!(matches!(link_health_for(-58, 0.0), LinkHealth::Good));
        assert!(matches!(link_health_for(-58, 5.0), LinkHealth::Weak));
        assert!(matches!(link_health_for(-90, 0.0), LinkHealth::Critical));
    }

    #[test]
    fn dynamic_graph_bounds_preserve_a_readable_span() {
        let bounds = graph_bounds(
            [59.0, 60.0, 61.0].into_iter(),
            SparklineRange::Dynamic {
                floor: None,
                minimum_span: 10.0,
            },
        );
        assert!(bounds.1 - bounds.0 >= 10.0);
        assert!(bounds.0 < 59.0);
        assert!(bounds.1 > 61.0);
    }
}
