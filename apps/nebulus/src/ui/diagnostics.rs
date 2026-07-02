use eframe::egui;

use crate::{app::NebulusApp, model::ReceiverState};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum View {
    #[default]
    Health,
    Rtp,
    Latency,
    Environment,
}

pub(crate) fn show(app: &NebulusApp, ui: &mut egui::Ui) {
    let id = ui.make_persistent_id("diagnostics-view");
    let mut view = ui.data(|data| data.get_temp::<View>(id).unwrap_or_default());
    ui.horizontal_wrapped(|ui| {
        for (candidate, label) in [
            (View::Health, "Pipeline health"),
            (View::Rtp, "RTP"),
            (View::Latency, "Stage latency"),
            (View::Environment, "Environment"),
        ] {
            if ui.selectable_label(view == candidate, label).clicked() {
                view = candidate;
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(id, view));
    ui.separator();
    match view {
        View::Health => health(app, ui),
        View::Rtp => rtp(app, ui),
        View::Latency => latency(app, ui),
        View::Environment => environment(app, ui),
    }
}

fn health(app: &NebulusApp, ui: &mut egui::Ui) {
    let counters = app.diagnostics.counters;
    health_row(ui, "USB adapter initialized", app.chip.is_some());
    health_row(ui, "USB transfers arriving", app.metrics.usb_transfers > 0);
    health_row(ui, "802.11 packets parsed", app.metrics.wifi_packets > 0);
    health_row(ui, "Frames accepted", counters.accepted_packets > 0);
    health_row(ui, "WFB payload recovered", counters.wfb_payloads > 0);
    health_row(ui, "RTP packets arriving", app.metrics.rtp_packets > 0);
    health_row(ui, "Codec configuration ready", codec_config_ready(app));
    health_row(
        ui,
        "Encoded frames extracted",
        app.metrics.encoded_frames > 0,
    );
    health_row(
        ui,
        "Platform decoder active",
        app.metrics.decoded_frames > 0,
    );
    health_row(
        ui,
        "Audio route healthy",
        !app.audio.enabled || (app.audio.supported && app.audio.errors == 0),
    );
    health_row(
        ui,
        "VPN bridge healthy",
        !app.settings.vpn_enabled || (app.vpn.active && app.vpn.errors == 0),
    );

    ui.add_space(10.0);
    ui.heading("Packet filtering");
    value_grid(ui, "health-counters", |ui| {
        row(ui, "Accepted", counters.accepted_packets);
        row(ui, "Dropped", counters.dropped_packets);
        row(ui, "CRC drops", counters.crc_dropped);
        row(ui, "ICV drops", counters.icv_dropped);
        row(ui, "Reports dropped", counters.report_dropped);
        row(ui, "Ignored frames", counters.ignored_frames);
        row(ui, "WFB sessions", counters.sessions);
        row(ui, "Route errors", counters.route_errors);
    });
}

fn rtp(app: &NebulusApp, ui: &mut egui::Ui) {
    let status = app.diagnostics.rtp;
    let reorder = app.diagnostics.reorder;
    value_grid(ui, "rtp-diagnostics", |ui| {
        text_row(ui, "Codec", &format!("{:?}", status.last_codec));
        text_row(
            ui,
            "H.264 config",
            &format!(
                "SPS {} / PPS {}",
                yes_no(status.codec_config.h264_sps),
                yes_no(status.codec_config.h264_pps),
            ),
        );
        text_row(
            ui,
            "H.265 config",
            &format!(
                "VPS {} / SPS {} / PPS {}",
                yes_no(status.codec_config.h265_vps),
                yes_no(status.codec_config.h265_sps),
                yes_no(status.codec_config.h265_pps),
            ),
        );
        option_row(ui, "Payload type", status.last_payload_type);
        option_row(ui, "NAL type", status.last_nal_type);
        option_row(ui, "Sequence", status.last_sequence_number);
        option_row(ui, "RTP timestamp", status.last_timestamp);
        row(ui, "Packets", status.packets);
        row(ui, "Frames emitted", status.frames_emitted);
        row(ui, "Config wait drops", status.config_wait_drops);
        row(
            ui,
            "Keyframes with config",
            status.keyframes_with_prepended_config,
        );
        row(
            ui,
            "Parameter sets prepended",
            status.parameter_sets_prepended,
        );
        row(ui, "Fragment gaps", status.fragment_sequence_gaps);
        row(ui, "Fragment overflows", status.fragment_overflows);
        row(ui, "Malformed", status.malformed_packets);
        row(ui, "Unsupported payloads", status.unsupported_payloads);
        row(ui, "Reorder buffered", reorder.buffered_packets);
        row(ui, "Reordered", reorder.reordered_packets);
        row(ui, "Late packets", reorder.late_packets);
        row(ui, "Forced flushes", reorder.forced_flushes);
    });
}

fn latency(app: &NebulusApp, ui: &mut egui::Ui) {
    egui::Grid::new("latency-stages")
        .num_columns(6)
        .striped(true)
        .spacing([12.0, 7.0])
        .show(ui, |ui| {
            for heading in ["Stage", "Last", "Average", "P95", "Maximum", "Samples"] {
                ui.strong(heading);
            }
            ui.end_row();
            for (name, values) in &app.diagnostics.stages {
                let summary = values.summary();
                ui.label(*name);
                ui.monospace(format!("{:.2} ms", summary.last));
                ui.monospace(format!("{:.2} ms", summary.average));
                ui.monospace(format!("{:.2} ms", summary.p95));
                ui.monospace(format!("{:.2} ms", summary.maximum));
                ui.monospace(summary.samples.to_string());
                ui.end_row();
            }
        });
}

fn environment(app: &NebulusApp, ui: &mut egui::Ui) {
    let environment = &app.environment;
    let maximum_fps = if environment.maximum_observed_fps > 0.0 {
        format!("{:.1} FPS observed", environment.maximum_observed_fps)
    } else {
        "Not reported; waiting for stream".to_owned()
    };
    value_grid(ui, "environment-details", |ui| {
        text_row(ui, "Platform", &environment.platform);
        text_row(ui, "Architecture", &environment.architecture);
        text_row(ui, "Runtime", &environment.runtime);
        text_row(ui, "Renderer", &environment.renderer);
        text_row(ui, "Logical processors", &environment.logical_processors);
        text_row(ui, "User agent", &environment.user_agent);
        text_row(ui, "Media backend", &environment.decoder_backend);
        text_row(ui, "H.264", &environment.h264);
        text_row(ui, "H.265", &environment.h265);
        text_row(
            ui,
            "Native/GPU surfaces",
            if environment.native_surfaces {
                "Yes"
            } else {
                "No"
            },
        );
        text_row(
            ui,
            "Maximum resolution",
            &environment
                .maximum_observed_resolution
                .map(|[width, height]| format!("{width} x {height} observed"))
                .unwrap_or_else(|| "Not reported; waiting for stream".to_owned()),
        );
        text_row(ui, "Maximum frame rate", &maximum_fps);
        text_row(
            ui,
            "Receiver state",
            match app.state {
                ReceiverState::Idle => "Idle",
                ReceiverState::Connecting => "Connecting",
                ReceiverState::Ready => "Ready",
                ReceiverState::Receiving => "Receiving",
                ReceiverState::Stopping => "Stopping",
                ReceiverState::Failed => "Failed",
            },
        );
        text_row(
            ui,
            "USB API",
            if cfg!(target_arch = "wasm32") {
                "nusb WebUSB"
            } else if cfg!(target_os = "android") {
                "Android UsbManager + nusb fd"
            } else {
                "nusb native"
            },
        );
        text_row(
            ui,
            "VPN/TUN",
            if cfg!(target_arch = "wasm32") {
                "Unavailable in browser"
            } else {
                "Native TUN supported"
            },
        );
    });
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(
            "Platform decoders generally do not expose a reliable global maximum resolution or frame rate. Nebulus reports the highest stream values observed in this session.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );
}

fn codec_config_ready(app: &NebulusApp) -> bool {
    app.diagnostics
        .rtp
        .last_codec
        .is_some_and(|codec| app.diagnostics.rtp.codec_config.is_complete_for(codec))
}

fn health_row(ui: &mut egui::Ui, label: &str, healthy: bool) {
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        let color = if healthy {
            egui::Color32::from_rgb(61, 214, 154)
        } else {
            ui.visuals().weak_text_color()
        };
        if healthy {
            ui.painter().circle_filled(rect.center(), 4.0, color);
        } else {
            ui.painter()
                .circle_stroke(rect.center(), 4.5, egui::Stroke::new(1.5, color));
        }
        response.on_hover_text(if healthy { "Healthy" } else { "Waiting" });
        ui.label(label);
    });
}

fn value_grid(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    let max_column_width = ((ui.available_width() - 20.0) * 0.5).max(80.0);
    egui::Grid::new(id)
        .num_columns(2)
        .striped(true)
        .max_col_width(max_column_width)
        .spacing([20.0, 7.0])
        .show(ui, add);
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: impl std::fmt::Display) {
    text_row(ui, label, &value.to_string());
}

fn option_row(ui: &mut egui::Ui, label: &str, value: Option<impl std::fmt::Display>) {
    text_row(
        ui,
        label,
        &value.map_or_else(|| "--".to_owned(), |value| value.to_string()),
    );
}

fn text_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
    ui.monospace(value);
    ui.end_row();
}
