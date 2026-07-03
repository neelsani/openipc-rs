use crate::{
    app::NebulusApp,
    settings::{RouteAction, MAX_LINK_ID},
};

/// Result severity for one preflight check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreflightSeverity {
    Pass,
    Warning,
    Fail,
}

/// One actionable preflight result.
#[derive(Debug, Clone)]
pub(crate) struct PreflightCheck {
    pub(crate) name: &'static str,
    pub(crate) detail: String,
    pub(crate) severity: PreflightSeverity,
}

/// Latest complete preflight report.
#[derive(Debug, Clone, Default)]
pub(crate) struct PreflightReport {
    pub(crate) checks: Vec<PreflightCheck>,
}

impl PreflightReport {
    pub(crate) fn run(app: &NebulusApp) -> Self {
        let mut checks = Vec::new();
        let selected = app.settings.device_id.as_deref();
        checks.push(match selected {
            Some(id) if app.devices.iter().any(|device| device.id == id) => {
                pass("Receiver", format!("Selected adapter {id} is available"))
            }
            Some(id) => warning(
                "Receiver",
                format!(
                    "Adapter {id} is not in the current discovery list; refresh or reconnect it"
                ),
            ),
            None if cfg!(target_arch = "wasm32") => warning(
                "Receiver",
                "The browser will open its WebUSB device picker when RX starts".to_owned(),
            ),
            None if app.devices.is_empty() => fail(
                "Receiver",
                "No supported USB adapter is selected or visible".to_owned(),
            ),
            None => fail("Receiver", "Select a USB adapter".to_owned()),
        });

        checks.push(
            match openipc_core::WfbKeypair::from_bytes(&app.settings.key_bytes) {
                Ok(_) => pass(
                    "Ground-station key",
                    format!("Valid {}-byte WFB key", app.settings.key_bytes.len()),
                ),
                Err(error) => fail("Ground-station key", error.to_string()),
            },
        );

        checks.push(
            if (1..=177).contains(&app.settings.channel)
                && [5, 10, 20, 40, 80].contains(&app.settings.channel_width_mhz)
                && app.settings.channel_offset <= 4
                && app.settings.link_id <= MAX_LINK_ID
            {
                pass(
                    "Radio configuration",
                    format!(
                        "Channel {} / {} MHz / offset {} / link 0x{:06x}",
                        app.settings.channel,
                        app.settings.channel_width_mhz,
                        app.settings.channel_offset,
                        app.settings.link_id
                    ),
                )
            } else {
                fail(
                    "Radio configuration",
                    "Channel, width, offset, or link ID is outside the supported range".to_owned(),
                )
            },
        );

        let enabled_routes = app
            .settings
            .payload_routes
            .iter()
            .filter(|route| route.enabled)
            .collect::<Vec<_>>();
        let mut route_errors = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for route in &enabled_routes {
            if !ids.insert(route.id) {
                route_errors.push(format!("duplicate route id {}", route.id));
            }
            match route.action {
                RouteAction::Udp if cfg!(target_arch = "wasm32") => {
                    route_errors.push(format!(
                        "{} uses UDP, which browsers cannot open",
                        route.name
                    ));
                }
                RouteAction::Udp if route.udp_host.trim().is_empty() || route.udp_port == 0 => {
                    route_errors.push(format!("{} has an invalid UDP destination", route.name));
                }
                RouteAction::Audio
                    if route.sample_rate == 0 || !matches!(route.channels, 1 | 2) =>
                {
                    route_errors.push(format!("{} has an invalid audio format", route.name));
                }
                _ => {}
            }
        }
        checks.push(if route_errors.is_empty() {
            pass(
                "Payload routes",
                format!("{} enabled route(s) validated", enabled_routes.len()),
            )
        } else {
            fail("Payload routes", route_errors.join("; "))
        });

        checks.push(if app.settings.vpn_enabled && !app.vpn_available() {
            fail(
                "VPN/TUN",
                "VPN is enabled but this target has no available TUN backend".to_owned(),
            )
        } else if app.settings.vpn_enabled {
            pass(
                "VPN/TUN",
                "Native tunnel will be created on start".to_owned(),
            )
        } else {
            pass("VPN/TUN", "Disabled".to_owned())
        });

        checks.push(if app.settings.adaptive_link {
            pass(
                "Adaptive link",
                format!(
                    "Feedback uplink enabled at TX power {}",
                    app.settings.tx_power
                ),
            )
        } else {
            warning(
                "Adaptive link",
                "Feedback is disabled; the VTX will not receive live link-quality reports"
                    .to_owned(),
            )
        });

        checks.push(if app.environment.decoder_backend.is_empty() {
            warning(
                "Video decoder",
                "Backend capabilities are verified while the receiver connects".to_owned(),
            )
        } else {
            pass(
                "Video decoder",
                format!(
                    "{}; H.264 {}; H.265 {}",
                    app.environment.decoder_backend, app.environment.h264, app.environment.h265
                ),
            )
        });

        if let Some(result) = app
            .scan_results
            .iter()
            .find(|result| result.channel == app.settings.channel)
        {
            checks.push(if result.wfb_frames > 0 {
                pass(
                    "Channel survey",
                    format!(
                        "Channel {} observed {} WFB frame(s) at {}/{} dBm average RSSI",
                        result.channel,
                        result.wfb_frames,
                        result.average_rssi_dbm[0],
                        result.average_rssi_dbm[1]
                    ),
                )
            } else {
                warning(
                    "Channel survey",
                    format!(
                        "The latest survey saw no recognizable WFB frames on channel {}",
                        result.channel
                    ),
                )
            });
        }

        Self { checks }
    }

    pub(crate) fn can_start(&self) -> bool {
        !self.checks.is_empty()
            && self
                .checks
                .iter()
                .all(|check| check.severity != PreflightSeverity::Fail)
    }

    pub(crate) fn counts(&self) -> [usize; 3] {
        let mut counts = [0; 3];
        for check in &self.checks {
            counts[match check.severity {
                PreflightSeverity::Pass => 0,
                PreflightSeverity::Warning => 1,
                PreflightSeverity::Fail => 2,
            }] += 1;
        }
        counts
    }
}

fn pass(name: &'static str, detail: String) -> PreflightCheck {
    PreflightCheck {
        name,
        detail,
        severity: PreflightSeverity::Pass,
    }
}

fn warning(name: &'static str, detail: String) -> PreflightCheck {
    PreflightCheck {
        name,
        detail,
        severity: PreflightSeverity::Warning,
    }
}

fn fail(name: &'static str, detail: String) -> PreflightCheck {
    PreflightCheck {
        name,
        detail,
        severity: PreflightSeverity::Fail,
    }
}
