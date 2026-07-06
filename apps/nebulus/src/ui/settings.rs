use eframe::egui;
use openipc_core::channel::DEFAULT_LINK_ID;

use crate::{
    app::NebulusApp,
    model::{ReceiverState, VtxControlState},
    runtime::VtxControlRequest,
    settings::{ReceiverSource, DEFAULT_CHANNEL, DEFAULT_CHANNEL_OFFSET, MAX_LINK_ID},
};

use super::SettingsPage;

pub(crate) fn show(app: &mut NebulusApp, ui: &mut egui::Ui) {
    match app.settings_page {
        SettingsPage::Receiver => receiver_page(app, ui),
        SettingsPage::Media => media_page(app, ui),
        SettingsPage::Profiles => profiles_page(app, ui),
        SettingsPage::Network => network_page(app, ui),
        SettingsPage::Vtx => vtx_page(app, ui),
    }
}

fn vtx_page(app: &mut NebulusApp, ui: &mut egui::Ui) {
    section(ui, "Connection", |ui| {
        ui.label(
            egui::RichText::new(
                "Controls unmodified OpenIPC firmware at 10.5.0.10 through the WFB tunnel. The optional OS VPN is not required.",
            )
            .small()
            .color(ui.visuals().weak_text_color()),
        );
        let idle = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
        ui.add_enabled_ui(idle, |ui| {
            ui.checkbox(
                &mut app.settings.vtx_control_enabled,
                "Enable VTX control for this receiver profile",
            );
            egui::Grid::new("vtx-credentials")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Username");
                    ui.text_edit_singleline(&mut app.settings.vtx_ssh_username);
                    ui.end_row();
                    ui.label("Password");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.settings.vtx_ssh_password)
                            .password(true),
                    );
                    ui.end_row();
                    ui.label("Host key SHA-256");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.settings.vtx_host_key_sha256)
                            .hint_text("Empty accepts the current VTX key"),
                    );
                    ui.end_row();
                });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("State: {:?}", app.vtx_control.state));
            let receiving =
                app.state == ReceiverState::Receiving && app.settings.vtx_control_enabled;
            if ui
                .add_enabled(receiving, egui::Button::new("Connect"))
                .clicked()
            {
                app.request_vtx(VtxControlRequest::Connect);
            }
            if ui
                .add_enabled(receiving, egui::Button::new("Refresh config"))
                .clicked()
            {
                app.request_vtx(VtxControlRequest::Refresh);
            }
            if ui
                .add_enabled(
                    app.vtx_control.state == VtxControlState::Connected,
                    egui::Button::new("Disconnect"),
                )
                .clicked()
            {
                app.request_vtx(VtxControlRequest::Disconnect);
            }
        });
        if !app.vtx_control.last_error.is_empty() {
            ui.colored_label(
                ui.visuals().error_fg_color,
                app.vtx_control.last_error.clone(),
            );
        }
        let network = app.vtx_control.network;
        if app.state == ReceiverState::Receiving && app.settings.vtx_control_enabled {
            egui::Grid::new("vtx-network-status")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Tunnel downlink");
                    ui.monospace(format!(
                        "{} packets / {} bytes",
                        network.tunnel_packets_received, network.tunnel_bytes_received
                    ));
                    ui.end_row();
                    ui.label("Tunnel uplink");
                    ui.monospace(format!(
                        "{} packets / {} bytes",
                        network.tunnel_packets_sent, network.tunnel_bytes_sent
                    ));
                    ui.end_row();
                    ui.label("TCP connections");
                    ui.monospace(format!(
                        "{} active / {} opened / {} failed",
                        network.tcp_connections_active,
                        network.tcp_connections_opened,
                        network.tcp_connection_failures
                    ));
                    ui.end_row();
                    ui.label("Malformed tunnel payloads");
                    ui.monospace(network.malformed_tunnel_packets.to_string());
                    ui.end_row();
                });
        }
    });

    let active = app.vtx_control.state == VtxControlState::Connected;
    ui.add_enabled_ui(active, |ui| {
        section(ui, "WFB radio", |ui| {
            egui::Grid::new("vtx-wfb-radio")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Channel");
                    ui.add(egui::DragValue::new(&mut app.settings.channel).range(1..=196));
                    ui.end_row();
                    ui.label("Width");
                    egui::ComboBox::from_id_salt("vtx-width")
                        .selected_text(format!("{} MHz", app.settings.channel_width_mhz))
                        .show_ui(ui, |ui| {
                            for width in [10, 20, 40] {
                                ui.selectable_value(
                                    &mut app.settings.channel_width_mhz,
                                    width,
                                    format!("{width} MHz"),
                                );
                            }
                        });
                    ui.end_row();
                    ui.label("TX power");
                    ui.add(egui::Slider::new(&mut app.settings.tx_power, 1..=58));
                    ui.end_row();
                });
            if ui.button("Apply radio").clicked() {
                app.confirm_vtx(
                    "Change VTX radio settings?",
                    format!(
                        "This restarts WFB on channel {} at {} MHz. If the receiver is not tuned to the same channel and width, the link will be lost.",
                        app.settings.channel, app.settings.channel_width_mhz
                    ),
                    "Apply radio settings",
                    VtxControlRequest::SetWfbBatch(vec![
                        openipc_uplink::WfbSetting::Channel(u16::from(app.settings.channel)),
                        openipc_uplink::WfbSetting::ChannelWidth(app.settings.channel_width_mhz),
                        openipc_uplink::WfbSetting::TxPower(app.settings.tx_power),
                    ]),
                );
            }
        });

        section(ui, "WFB broadcast", |ui| {
            let vtx = &mut app.settings.vtx;
            egui::Grid::new("vtx-wfb-broadcast")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    ui.label("MCS index");
                    ui.add(egui::Slider::new(&mut vtx.mcs_index, 0..=10));
                    ui.end_row();
                    ui.label("FEC k / n");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut vtx.fec_k).range(0..=15));
                        ui.add(egui::DragValue::new(&mut vtx.fec_n).range(0..=15));
                    });
                    ui.end_row();
                    ui.label("Coding");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut vtx.stbc, "STBC");
                        ui.checkbox(&mut vtx.ldpc, "LDPC");
                        ui.label("Multi-link");
                        ui.add(egui::DragValue::new(&mut vtx.multi_link).range(1_500..=4_000));
                    });
                    ui.end_row();
                });
            let values = (
                vtx.mcs_index,
                vtx.fec_k,
                vtx.fec_n,
                vtx.stbc,
                vtx.ldpc,
                vtx.multi_link,
            );
            if ui.button("Apply broadcast").clicked() {
                app.confirm_vtx(
                    "Change WFB broadcast settings?",
                    format!(
                        "This restarts the VTX broadcast with MCS {} and FEC {}/{}. Unsupported or overly aggressive values can prevent the receiver from recovering the stream.",
                        values.0, values.1, values.2
                    ),
                    "Apply broadcast settings",
                    VtxControlRequest::SetWfbBatch(vec![
                        openipc_uplink::WfbSetting::McsIndex(values.0),
                        openipc_uplink::WfbSetting::FecK(values.1),
                        openipc_uplink::WfbSetting::FecN(values.2),
                        openipc_uplink::WfbSetting::Stbc(values.3),
                        openipc_uplink::WfbSetting::Ldpc(values.4),
                        openipc_uplink::WfbSetting::MultiLink(values.5),
                    ]),
                );
            }
        });

        vtx_camera_controls(app, ui);
        vtx_telemetry_controls(app, ui);
        vtx_adaptive_controls(app, ui);

        section(ui, "System", |ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Reboot immediately restarts the air unit.",
            );
            if ui.button("Reboot VTX").clicked() {
                app.confirm_vtx(
                    "Reboot the VTX?",
                    "The air unit will restart immediately. Video, telemetry, adaptive link, and VTX control will be unavailable until it finishes booting.",
                    "Reboot VTX",
                    VtxControlRequest::Reboot,
                );
            }
        });
    });

    if let Some(config) = app.vtx_control.config.as_ref() {
        section(ui, "Configuration snapshot", |ui| {
            for (label, bytes) in [
                ("majestic.yaml", &config.majestic_yaml),
                ("wfb.yaml", &config.wfb_yaml),
                ("alink.conf", &config.adaptive_link),
                ("txprofiles.conf", &config.tx_profiles),
            ] {
                ui.collapsing(format!("{label} · {} bytes", bytes.len()), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(String::from_utf8_lossy(bytes)).monospace(),
                        )
                        .wrap(),
                    );
                });
            }
        });
    }
}

fn vtx_camera_controls(app: &mut NebulusApp, ui: &mut egui::Ui) {
    section(ui, "Camera / encoder", |ui| {
        let vtx = &mut app.settings.vtx;
        egui::Grid::new("vtx-camera")
            .num_columns(2)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                ui.label("Orientation");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut vtx.mirror, "Mirror");
                    ui.checkbox(&mut vtx.flip, "Flip");
                });
                ui.end_row();
                for (label, value) in [
                    ("Contrast", &mut vtx.contrast),
                    ("Hue", &mut vtx.hue),
                    ("Saturation", &mut vtx.saturation),
                    ("Luminance", &mut vtx.luminance),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(value));
                    ui.end_row();
                }
                ui.label("Resolution");
                ui.text_edit_singleline(&mut vtx.resolution);
                ui.end_row();
                ui.label("FPS / bitrate kbps");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut vtx.fps).range(1..=240));
                    ui.add(egui::DragValue::new(&mut vtx.bitrate_kbps).range(1..=30_720));
                });
                ui.end_row();
                ui.label("Codec / rate control");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut vtx.codec);
                    ui.text_edit_singleline(&mut vtx.rate_control);
                });
                ui.end_row();
                ui.label("GOP size");
                ui.add(egui::DragValue::new(&mut vtx.gop_size).range(0..=10));
                ui.end_row();
                ui.label("Simple video mode");
                ui.text_edit_singleline(&mut vtx.simple_video_mode);
                ui.end_row();
                ui.label("VTX recording");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut vtx.recording_enabled, "Enabled");
                    ui.label("split");
                    ui.add(egui::DragValue::new(&mut vtx.recording_split_seconds));
                    ui.label("max %");
                    ui.add(egui::DragValue::new(&mut vtx.recording_max_usage).range(0..=100));
                });
                ui.end_row();
                ui.label("ISP");
                ui.horizontal(|ui| {
                    ui.label("exposure");
                    ui.add(egui::DragValue::new(&mut vtx.exposure).range(5..=50));
                    ui.label("anti-flicker");
                    ui.text_edit_singleline(&mut vtx.anti_flicker);
                });
                ui.end_row();
                ui.label("Sensor config");
                ui.text_edit_singleline(&mut vtx.sensor_config);
                ui.end_row();
                ui.label("FPV tuning");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut vtx.fpv_enabled, "Enabled");
                    ui.label("noise");
                    ui.add(egui::DragValue::new(&mut vtx.noise_level).range(0..=1));
                });
                ui.end_row();
            });
        let draft = vtx.clone();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Read video mode").clicked() {
                app.request_vtx(VtxControlRequest::GetVideoMode);
            }
            if ui.button("Apply simple mode").clicked() {
                app.confirm_vtx(
                    "Change the VTX video mode?",
                    format!(
                        "Changing the camera mode to '{}' interrupts the encoded stream and may require a new decoder keyframe.",
                        draft.simple_video_mode
                    ),
                    "Apply video mode",
                    VtxControlRequest::SetVideoMode(draft.simple_video_mode.clone()),
                );
            }
            if ui.button("Apply image").clicked() {
                app.request_vtx(VtxControlRequest::SetCameraBatch(vec![
                    openipc_uplink::CameraSetting::Mirror(draft.mirror),
                    openipc_uplink::CameraSetting::Flip(draft.flip),
                    openipc_uplink::CameraSetting::Contrast(draft.contrast),
                    openipc_uplink::CameraSetting::Hue(draft.hue),
                    openipc_uplink::CameraSetting::Saturation(draft.saturation),
                    openipc_uplink::CameraSetting::Luminance(draft.luminance),
                ]));
            }
            if ui.button("Apply encoder").clicked() {
                app.confirm_vtx(
                    "Change encoder settings?",
                    format!(
                        "Majestic will reload with {} at {} FPS using {}. The video stream will be interrupted briefly, and unsupported settings may leave it unavailable.",
                        draft.resolution, draft.fps, draft.codec
                    ),
                    "Apply encoder settings",
                    VtxControlRequest::SetCameraBatch(vec![
                        openipc_uplink::CameraSetting::Resolution(draft.resolution.clone()),
                        openipc_uplink::CameraSetting::Fps(draft.fps),
                        openipc_uplink::CameraSetting::BitrateKbps(draft.bitrate_kbps),
                        openipc_uplink::CameraSetting::Codec(draft.codec.clone()),
                        openipc_uplink::CameraSetting::GopSize(draft.gop_size),
                        openipc_uplink::CameraSetting::RateControl(draft.rate_control.clone()),
                    ]),
                );
            }
            if ui.button("Apply recording").clicked() {
                app.request_vtx(VtxControlRequest::SetCameraBatch(vec![
                    openipc_uplink::CameraSetting::RecordingEnabled(draft.recording_enabled),
                    openipc_uplink::CameraSetting::RecordingSplitSeconds(
                        draft.recording_split_seconds,
                    ),
                    openipc_uplink::CameraSetting::RecordingMaxUsage(draft.recording_max_usage),
                ]));
            }
            if ui.button("Apply ISP / FPV").clicked() {
                let mut settings = vec![
                    openipc_uplink::CameraSetting::Exposure(draft.exposure),
                    openipc_uplink::CameraSetting::AntiFlicker(draft.anti_flicker.clone()),
                    openipc_uplink::CameraSetting::FpvEnabled(draft.fpv_enabled),
                    openipc_uplink::CameraSetting::NoiseLevel(draft.noise_level),
                ];
                if !draft.sensor_config.trim().is_empty() {
                    settings.push(openipc_uplink::CameraSetting::SensorConfig(
                        draft.sensor_config,
                    ));
                }
                app.confirm_vtx(
                    "Change ISP and FPV settings?",
                    "Majestic will reload the image pipeline. A bad sensor configuration can stop camera output until the setting is corrected or the VTX is restored.",
                    "Apply ISP / FPV settings",
                    VtxControlRequest::SetCameraBatch(settings),
                );
            }
        });
    });
}

fn vtx_telemetry_controls(app: &mut NebulusApp, ui: &mut egui::Ui) {
    section(ui, "Air telemetry", |ui| {
        let vtx = &mut app.settings.vtx;
        egui::Grid::new("vtx-telemetry")
            .num_columns(2)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                ui.label("Serial");
                ui.text_edit_singleline(&mut vtx.telemetry_serial);
                ui.end_row();
                ui.label("Router");
                ui.text_edit_singleline(&mut vtx.telemetry_router);
                ui.end_row();
                ui.label("OSD FPS");
                ui.add(egui::DragValue::new(&mut vtx.telemetry_osd_fps).range(0..=240));
                ui.end_row();
                ui.label("GS rendering");
                ui.checkbox(
                    &mut vtx.telemetry_gs_rendering,
                    "Forward telemetry to 10.5.0.1",
                );
                ui.end_row();
            });
        let values = (
            vtx.telemetry_serial.clone(),
            vtx.telemetry_router.clone(),
            vtx.telemetry_osd_fps,
            vtx.telemetry_gs_rendering,
        );
        if ui.button("Apply telemetry").clicked() {
            app.confirm_vtx(
                "Change air telemetry settings?",
                "This changes the VTX serial/router configuration and restarts WFB. Telemetry and tunnel traffic will be interrupted during the restart.",
                "Apply telemetry settings",
                VtxControlRequest::SetTelemetryBatch(vec![
                    openipc_uplink::TelemetrySetting::Serial(values.0),
                    openipc_uplink::TelemetrySetting::Router(values.1),
                    openipc_uplink::TelemetrySetting::OsdFps(values.2),
                    openipc_uplink::TelemetrySetting::GroundStationRendering(values.3),
                ]),
            );
        }
    });
}

fn vtx_adaptive_controls(app: &mut NebulusApp, ui: &mut egui::Ui) {
    section(ui, "VTX adaptive link", |ui| {
        let (values, tx_profiles) = {
            let vtx = &mut app.settings.vtx;
            ui.checkbox(
                &mut vtx.adaptive_service_enabled,
                "Run alink_drone on the VTX",
            );
            ui.horizontal(|ui| {
                ui.label("Variable");
                ui.text_edit_singleline(&mut vtx.adaptive_variable);
                ui.label("Value");
                ui.text_edit_singleline(&mut vtx.adaptive_value);
            });
            ui.label("TX profiles");
            ui.add(
                egui::TextEdit::multiline(&mut vtx.tx_profiles)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(8),
            );
            (
                (
                    vtx.adaptive_service_enabled,
                    vtx.adaptive_variable.clone(),
                    vtx.adaptive_value.clone(),
                ),
                vtx.tx_profiles.as_bytes().to_vec(),
            )
        };
        ui.horizontal(|ui| {
            if ui.button("Apply service state").clicked() {
                app.confirm_vtx(
                    "Change adaptive-link service state?",
                    "Adaptive link can change the active VTX transmission profile. Disabling it leaves the current static WFB settings in effect.",
                    "Apply service state",
                    VtxControlRequest::SetAdaptiveLink(
                        openipc_uplink::AdaptiveLinkSetting::Enabled(values.0),
                    ),
                );
            }
            if ui.button("Apply variable").clicked() {
                app.confirm_vtx(
                    "Change an adaptive-link parameter?",
                    format!(
                        "This writes '{}' in alink.conf and restarts alink_drone. Invalid tuning can make profile selection unstable.",
                        values.1
                    ),
                    "Apply parameter",
                    VtxControlRequest::SetAdaptiveLink(
                        openipc_uplink::AdaptiveLinkSetting::Variable {
                            name: values.1,
                            value: values.2,
                        },
                    ),
                );
            }
        });
        if ui.button("Upload TX profiles").clicked() {
            app.confirm_vtx(
                "Replace VTX transmission profiles?",
                "This overwrites /etc/txprofiles.conf and restarts adaptive link. Invalid profiles can interrupt or destabilize the radio link.",
                "Upload TX profiles",
                VtxControlRequest::SetAdaptiveLink(
                    openipc_uplink::AdaptiveLinkSetting::TxProfiles(tx_profiles),
                ),
            );
        }
    });
}

fn receiver_page(app: &mut NebulusApp, ui: &mut egui::Ui) {
    if app.receiver_info.is_some() {
        connected_receiver(app, ui);
    }
    let editable = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
    ui.add_enabled_ui(editable, |ui| {
        section(ui, "Receiver", |ui| {
            receiver_source_selector(app, ui);
            match app.settings.receiver_source {
                ReceiverSource::Usb => usb_receiver_settings(app, ui),
                ReceiverSource::UdpRtp => udp_receiver_settings(app, ui),
            }
            ui.horizontal(|ui| {
                if ui.button("Run preflight").clicked() {
                    app.run_preflight();
                }
                if app.settings.receiver_source == ReceiverSource::Usb
                    && ui.button("Scan channels").clicked()
                {
                    app.show_channel_scanner = true;
                }
            });
        });

        if app.settings.receiver_source == ReceiverSource::Usb {
            section(ui, "Radio", |ui| {
                radio_settings(app, ui);
            });
        }

        section(ui, "Link", |ui| {
            ui.checkbox(
                &mut app.settings.auto_recover,
                "Automatically recover a dropped receiver",
            );
            if app.settings.receiver_source == ReceiverSource::Usb {
                ui.checkbox(&mut app.settings.adaptive_link, "Adaptive link feedback");
            } else {
                ui.label(
                    egui::RichText::new(
                        "UDP input uses the RTP sequence number for optional reordering; WFB adaptive feedback is not applicable.",
                    )
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );
            }
            if cfg!(target_arch = "wasm32") {
                ui.label(
                    egui::RichText::new(
                        "Browser reconnects require a user gesture; automatic retries run in native builds.",
                    )
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );
            }
            if app.recovery.active {
                ui.horizontal_wrapped(|ui| {
                    let remaining = app
                        .recovery
                        .scheduled_at
                        .map_or(0.0, |at| {
                            at.saturating_duration_since(web_time::Instant::now())
                                .as_secs_f32()
                        });
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!(
                            "Recovery attempt {} in {:.1}s",
                            app.recovery.attempt, remaining
                        ),
                    );
                    if ui.small_button("Cancel").clicked() {
                        app.cancel_recovery();
                    }
                });
            }
            if app.settings.receiver_source == ReceiverSource::Usb && app.settings.adaptive_link {
                ui.horizontal(|ui| {
                    ui.label("Uplink TX power");
                    ui.add(egui::Slider::new(&mut app.settings.tx_power, 0..=127));
                });
            }
        });

        if app.settings.receiver_source == ReceiverSource::Usb {
            section(ui, "Receiver key", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open file").clicked() {
                    app.open_key_file(ui.ctx());
                }
                if ui.button("Use default").clicked() {
                    app.reset_key();
                }
            });
            ui.horizontal(|ui| {
                ui.label(&app.key_name);
                ui.label(
                    egui::RichText::new(format!("{} bytes", app.settings.key_bytes.len()))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            });
            if let Some(error) = &app.key_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            } else {
                ui.label(
                    egui::RichText::new("You can also drop a gs.key file anywhere on the window")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
            });
        }
    });
}

fn media_page(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let editable = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
    ui.add_enabled_ui(editable, |ui| {
        section(ui, "Video", |ui| {
            ui.horizontal(|ui| {
                ui.label("Codec preference");
                egui::ComboBox::from_id_salt("codec-preference")
                    .selected_text(app.settings.codec_preference.label())
                    .show_ui(ui, |ui| {
                        for preference in [
                            crate::settings::CodecPreference::Auto,
                            crate::settings::CodecPreference::H264,
                            crate::settings::CodecPreference::H265,
                        ] {
                            ui.selectable_value(
                                &mut app.settings.codec_preference,
                                preference,
                                preference.label(),
                            );
                        }
                    });
            });
            ui.checkbox(&mut app.settings.rtp_reorder, "RTP reorder buffer");
            egui::Grid::new("decoder-settings")
                .num_columns(2)
                .spacing([18.0, 7.0])
                .show(ui, |ui| {
                    ui.label("Decoder queue");
                    ui.label("3 frames, latest-frame output");
                    ui.end_row();
                });
        });
        section(ui, "Recording", |ui| recording_settings(app, ui));
    });
}

fn profiles_page(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let editable = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
    ui.add_enabled_ui(editable, |ui| {
        section(ui, "Receiver profiles", |ui| profile_editor(app, ui));
        section(ui, "Preset packs", |ui| super::presets::section(app, ui));
    });
}

fn network_page(app: &mut NebulusApp, ui: &mut egui::Ui) {
    section(ui, "VPN / tunnel", |ui| {
        if app.settings.receiver_source == ReceiverSource::Usb {
            super::vpn(app, ui);
        } else {
            ui.label(
                egui::RichText::new(
                    "VPN/TUN requires the bidirectional WFB radio transport and is unavailable for direct UDP RTP input.",
                )
                .small()
                .color(ui.visuals().weak_text_color()),
            );
        }
    });

    if app.settings.receiver_source == ReceiverSource::Usb {
        let editable = matches!(app.state, ReceiverState::Idle | ReceiverState::Failed);
        ui.add_enabled_ui(editable, |ui| {
            section(ui, "USB transport", |ui| {
                egui::Grid::new("advanced-settings")
                    .num_columns(2)
                    .spacing([18.0, 7.0])
                    .show(ui, |ui| {
                        ui.label("Minimum epoch");
                        ui.add(egui::DragValue::new(&mut app.settings.minimum_epoch));
                        ui.end_row();
                        ui.label("Transfer size");
                        ui.add(
                            egui::DragValue::new(&mut app.settings.transfer_size)
                                .range(4_096..=1_048_576),
                        );
                        ui.end_row();
                    });
            });
        });
    }
}

fn radio_settings(app: &mut NebulusApp, ui: &mut egui::Ui) {
    egui::Grid::new("radio-settings")
        .num_columns(3)
        .spacing([18.0, 8.0])
        .show(ui, |ui| {
            ui.label("Channel");
            ui.add(
                egui::Slider::new(&mut app.settings.channel, 1..=177)
                    .show_value(true)
                    .text(""),
            );
            if ui.small_button("Default").clicked() {
                app.settings.channel = DEFAULT_CHANNEL;
            }
            ui.end_row();
            ui.label("Width");
            egui::ComboBox::from_id_salt("channel-width")
                .selected_text(format!("{} MHz", app.settings.channel_width_mhz))
                .show_ui(ui, |ui| {
                    for width in [5, 10, 20, 40, 80] {
                        ui.selectable_value(
                            &mut app.settings.channel_width_mhz,
                            width,
                            format!("{width} MHz"),
                        );
                    }
                });
            ui.label("");
            ui.end_row();
            ui.label("Offset");
            ui.add(
                egui::Slider::new(&mut app.settings.channel_offset, 0..=4)
                    .show_value(true)
                    .text(""),
            );
            if ui.small_button("Default").clicked() {
                app.settings.channel_offset = DEFAULT_CHANNEL_OFFSET;
            }
            ui.end_row();
            ui.label("Link ID");
            ui.add(
                egui::Slider::new(&mut app.settings.link_id, 0..=MAX_LINK_ID)
                    .show_value(true)
                    .custom_formatter(|value, _| format!("0x{:06X}", value as u32))
                    .custom_parser(parse_link_id),
            );
            if ui.small_button("Default").clicked() {
                app.settings.link_id = DEFAULT_LINK_ID;
            }
            ui.end_row();
        });
}

fn receiver_source_selector(app: &mut NebulusApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    ui.horizontal(|ui| {
        ui.label("Source");
        ui.selectable_value(
            &mut app.settings.receiver_source,
            ReceiverSource::Usb,
            ReceiverSource::Usb.label(),
        );
        ui.selectable_value(
            &mut app.settings.receiver_source,
            ReceiverSource::UdpRtp,
            ReceiverSource::UdpRtp.label(),
        );
    });

    #[cfg(target_arch = "wasm32")]
    {
        app.settings.receiver_source = ReceiverSource::Usb;
        ui.horizontal(|ui| {
            ui.label("Source");
            ui.strong("WebUSB");
        });
    }
}

fn usb_receiver_settings(app: &mut NebulusApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("Primary receiver and uplink")
            .small()
            .color(ui.visuals().weak_text_color()),
    );
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("usb-device")
            .selected_text(selected_device_label(app))
            .width(230.0)
            .show_ui(ui, |ui| {
                for device in &app.devices {
                    let changed = ui
                        .selectable_value(
                            &mut app.settings.device_id,
                            Some(device.id.clone()),
                            format!("{} — {}", device.label, device.location),
                        )
                        .changed();
                    if changed {
                        app.settings
                            .diversity_device_ids
                            .retain(|id| Some(id) != app.settings.device_id.as_ref());
                    }
                }
            });
        if ui.button("Refresh").clicked() {
            app.refresh_devices();
        }
        #[cfg(target_arch = "wasm32")]
        if ui.button("Add adapter").clicked() {
            app.authorize_webusb_adapter();
        }
    });
    if app.devices.is_empty() {
        ui.label(
            egui::RichText::new("No supported USB adapter found")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
    }
    if app.devices.len() > 1 {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Diversity receivers")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        for device in &app.devices {
            if app.settings.device_id.as_deref() == Some(device.id.as_str()) {
                continue;
            }
            let mut selected = app
                .settings
                .diversity_device_ids
                .iter()
                .any(|id| id == &device.id);
            if ui
                .checkbox(
                    &mut selected,
                    format!("{} — {}", device.label, device.location),
                )
                .changed()
            {
                if selected {
                    app.settings.diversity_device_ids.push(device.id.clone());
                } else {
                    app.settings
                        .diversity_device_ids
                        .retain(|id| id != &device.id);
                }
                app.settings.normalize();
            }
        }
        ui.label(
            egui::RichText::new(format!(
                "{} adapters selected; first valid WFB packet wins",
                app.settings.selected_device_ids().len()
            ))
            .small()
            .color(ui.visuals().weak_text_color()),
        );
    }
}

fn udp_receiver_settings(_app: &mut NebulusApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        egui::Grid::new("udp-rtp-listener")
            .num_columns(3)
            .spacing([12.0, 7.0])
            .show(ui, |ui| {
                ui.label("Bind address");
                ui.add(
                    egui::TextEdit::singleline(&mut _app.settings.udp_bind_address)
                        .desired_width(150.0)
                        .char_limit(64),
                );
                if ui.small_button("Default").clicked() {
                    _app.settings.udp_bind_address = "0.0.0.0".to_owned();
                }
                ui.end_row();

                ui.label("Port");
                ui.add(egui::DragValue::new(&mut _app.settings.udp_bind_port).range(1..=65_535));
                if ui.small_button("Default").clicked() {
                    _app.settings.udp_bind_port = crate::settings::DEFAULT_UDP_RTP_PORT;
                }
                ui.end_row();
            });
        ui.label(
            egui::RichText::new(
                "Listens for one RTP packet per UDP datagram. H.264/H.265, mixed Opus audio, recording, and payload routes use the normal receive pipeline.",
            )
            .small()
            .color(ui.visuals().weak_text_color()),
        );
    }

    #[cfg(target_arch = "wasm32")]
    ui.label(
        egui::RichText::new("Direct UDP sockets are unavailable in browsers")
            .small()
            .color(ui.visuals().warn_fg_color),
    );
}

fn recording_settings(_app: &mut NebulusApp, ui: &mut egui::Ui) {
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    {
        ui.label("Recording folder");
        ui.add(
            egui::Label::new(egui::RichText::new(_app.recording_directory_display()).monospace())
                .wrap(),
        );
        ui.horizontal(|ui| {
            if ui.button("Choose folder").clicked() {
                _app.choose_recording_directory();
            }
            if ui.button("Use default").clicked() {
                _app.reset_recording_directory();
            }
        });
        ui.label(
            egui::RichText::new(
                "Record creates a unique MP4 here immediately; it never opens a save dialog.",
            )
            .small()
            .color(ui.visuals().weak_text_color()),
        );
    }

    #[cfg(target_os = "android")]
    ui.label(
        egui::RichText::new(
            "Recordings are written to Nebulus app storage without opening a document picker.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );

    #[cfg(target_arch = "wasm32")]
    ui.label(
        egui::RichText::new(
            "The browser buffers the MP4 and starts a download when recording stops. The browser controls the download folder.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );
}

fn profile_editor(app: &mut NebulusApp, ui: &mut egui::Ui) {
    let active_id = app
        .settings
        .active_profile_id
        .or_else(|| app.settings.profiles.first().map(|profile| profile.id));
    let selected_name = active_id
        .and_then(|id| {
            app.settings
                .profiles
                .iter()
                .find(|profile| profile.id == id)
        })
        .map(|profile| profile.name.clone())
        .unwrap_or_else(|| "No profile".to_owned());
    let mut selected = active_id;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("receiver-profile")
            .selected_text(selected_name)
            .width(180.0)
            .show_ui(ui, |ui| {
                for profile in &app.settings.profiles {
                    ui.selectable_value(&mut selected, Some(profile.id), &profile.name);
                }
            });
        if selected != active_id {
            if let Some(id) = selected {
                app.apply_profile(id);
            }
        }
        if ui.button("New").clicked() {
            app.create_profile();
        }
    });
    if let Some(id) = app.settings.active_profile_id {
        if let Some(profile) = app
            .settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
        {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut profile.name)
                        .desired_width(180.0)
                        .char_limit(48),
                );
            });
        }
    }
    let active_osd = app.settings.active_osd_profile_id;
    let selected_osd_name = active_osd
        .and_then(|id| {
            app.settings
                .osd_profiles
                .iter()
                .find(|profile| profile.id == id)
        })
        .map_or("No OSD", |profile| profile.name.as_str());
    let mut selected_osd = active_osd;
    ui.horizontal(|ui| {
        ui.label("OSD");
        egui::ComboBox::from_id_salt("receiver-profile-osd")
            .selected_text(selected_osd_name)
            .width(180.0)
            .show_ui(ui, |ui| {
                for profile in &app.settings.osd_profiles {
                    ui.selectable_value(&mut selected_osd, Some(profile.id), &profile.name);
                }
            });
    });
    if selected_osd != active_osd {
        if let Some(osd_id) = selected_osd {
            app.apply_osd_profile(osd_id);
            if let Some(profile_id) = app.settings.active_profile_id {
                if let Some(profile) = app
                    .settings
                    .profiles
                    .iter_mut()
                    .find(|profile| profile.id == profile_id)
                {
                    profile.osd_profile_id = Some(osd_id);
                }
            }
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Save current").clicked() {
            app.save_active_profile();
        }
        if ui
            .add_enabled(app.settings.profiles.len() > 1, egui::Button::new("Delete"))
            .clicked()
        {
            app.delete_active_profile();
        }
    });
    ui.label(
        egui::RichText::new(
            "Profiles include receiver hardware, radio, keys, routes, telemetry, audio, VPN, decoder settings, and a reference to a reusable OSD profile.",
        )
        .small()
        .color(ui.visuals().weak_text_color()),
    );
}

fn connected_receiver(app: &NebulusApp, ui: &mut egui::Ui) {
    section(ui, "Connected receivers", |ui| {
        for receiver in &app.receiver_infos {
            let source_label = if receiver.transport == crate::runtime::ReceiverTransport::Usb {
                "Radio"
            } else {
                "Input"
            };
            ui.strong(format!(
                "{source_label} {} · {}",
                receiver.source_id + 1,
                receiver.label
            ));
            egui::Grid::new(("connected-receiver-info", receiver.source_id))
                .num_columns(2)
                .striped(true)
                .spacing([18.0, 7.0])
                .show(ui, |ui| {
                    receiver_row(ui, "Adapter", &receiver.label);
                    match receiver.transport {
                        crate::runtime::ReceiverTransport::Usb => {
                            receiver_row(ui, "Device", &receiver.id);
                            receiver_row(
                                ui,
                                "USB ID",
                                &match (receiver.vendor_id, receiver.product_id) {
                                    (Some(vendor), Some(product)) => {
                                        format!("{vendor:04x}:{product:04x}")
                                    }
                                    _ => "Not applicable".to_owned(),
                                },
                            );
                            receiver_row(ui, "Chipset", &receiver.chip);
                            receiver_row(ui, "RF paths", &receiver.rf_paths);
                            receiver_row(
                                ui,
                                "Cut revision",
                                &receiver.cut_version.map_or_else(
                                    || "Not reported".to_owned(),
                                    |cut| cut.to_string(),
                                ),
                            );
                            receiver_row(ui, "USB link", &receiver.usb_speed);
                            receiver_row(
                                ui,
                                "Bulk endpoints",
                                &match (receiver.bulk_in_endpoint, receiver.bulk_out_endpoint) {
                                    (Some(input), Some(output)) => {
                                        format!("IN 0x{input:02x} / OUT 0x{output:02x}")
                                    }
                                    _ => "Not applicable".to_owned(),
                                },
                            );
                        }
                        crate::runtime::ReceiverTransport::UdpRtp => {
                            receiver_row(ui, "Listen address", &receiver.id);
                            receiver_row(ui, "Transport", "UDP / RTP");
                            receiver_row(ui, "Processing", "Direct RTP depacketization");
                        }
                        crate::runtime::ReceiverTransport::Synthetic => {
                            receiver_row(ui, "Source", &receiver.id);
                            receiver_row(ui, "Transport", "Synthetic RTP");
                            receiver_row(ui, "Processing", &receiver.chip);
                        }
                    }
                    receiver_row(ui, "Initialization", &receiver.initialization);
                    if receiver.transport == crate::runtime::ReceiverTransport::Usb {
                        receiver_row(
                            ui,
                            "Firmware downloaded",
                            match receiver.firmware_downloaded {
                                Some(true) => "Yes",
                                Some(false) => "No",
                                None => "Not applicable",
                            },
                        );
                    }
                    receiver_row(
                        ui,
                        "Role",
                        if receiver.transport != crate::runtime::ReceiverTransport::Usb {
                            "Video/audio receive"
                        } else if receiver.source_id == 0 {
                            "Primary RX and uplink"
                        } else {
                            "Diversity RX"
                        },
                    );
                });
            ui.add_space(8.0);
        }
        if app.settings.receiver_source == ReceiverSource::Usb {
            egui::Grid::new("connected-radio-config")
                .num_columns(2)
                .striped(true)
                .spacing([18.0, 7.0])
                .show(ui, |ui| {
                    receiver_row(
                        ui,
                        "RF configuration",
                        &format!(
                            "channel {} / {} MHz / offset {}",
                            app.settings.channel,
                            app.settings.channel_width_mhz,
                            app.settings.channel_offset
                        ),
                    );
                    receiver_row(
                        ui,
                        "Video channel",
                        &format!(
                            "Link 0x{:06x} / port 0x{:02x}",
                            app.settings.link_id,
                            openipc_core::RadioPort::Video.as_u8()
                        ),
                    );
                });
        }
    });
}

fn receiver_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
    ui.monospace(value);
    ui.end_row();
}

fn parse_link_id(value: &str) -> Option<f64> {
    let value = value.trim();
    let parsed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(
            || value.parse::<u32>().ok(),
            |hex| u32::from_str_radix(hex, 16).ok(),
        )?;
    (parsed <= MAX_LINK_ID).then_some(f64::from(parsed))
}

fn selected_device_label(app: &NebulusApp) -> String {
    app.settings
        .device_id
        .as_deref()
        .and_then(|id| app.devices.iter().find(|device| device.id == id))
        .map(|device| format!("{} — {}", device.label, device.location))
        .or_else(|| app.settings.device_id.clone())
        .unwrap_or_else(|| "Select an adapter".to_owned())
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show(ui, |ui| {
            ui.add_space(4.0);
            add(ui);
            ui.add_space(8.0);
        });
}

#[cfg(test)]
mod tests {
    use super::parse_link_id;

    #[test]
    fn link_id_parser_accepts_decimal_and_hex() {
        assert_eq!(parse_link_id("7669206"), Some(7_669_206.0));
        assert_eq!(parse_link_id("0x7505D6"), Some(7_669_206.0));
    }

    #[test]
    fn link_id_parser_rejects_values_above_24_bits() {
        assert_eq!(parse_link_id("0x1000000"), None);
    }
}
