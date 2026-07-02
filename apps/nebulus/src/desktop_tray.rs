//! macOS menu-bar and Windows notification-area controls.

use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

use crate::{model::ReceiverState, ui::PanelTab, NebulusApp};

const SHOW_ID: &str = "nebulus.show";
const HIDE_ID: &str = "nebulus.hide";
const RECEIVER_ID: &str = "nebulus.receiver";
const VPN_ID: &str = "nebulus.vpn.enabled";
const VPN_SETTINGS_ID: &str = "nebulus.vpn.settings";
const QUIT_ID: &str = "nebulus.quit";

#[derive(Debug, Clone, Copy)]
enum TrayCommand {
    Show,
    Hide,
    ToggleReceiver,
    ToggleVpn,
    OpenVpn,
    Quit,
}

/// Owns the platform tray icon and translates menu events into app actions.
pub(crate) struct DesktopTray {
    _icon: TrayIcon,
    events: Receiver<TrayCommand>,
    receiver: MenuItem,
    vpn: CheckMenuItem,
}

impl DesktopTray {
    pub(crate) fn new(context: &egui::Context) -> Result<Self, String> {
        let show = MenuItem::with_id(SHOW_ID, "Show Nebulus", true, None);
        let hide = MenuItem::with_id(HIDE_ID, "Hide Nebulus", true, None);
        let receiver = MenuItem::with_id(RECEIVER_ID, "Start RX", true, None);
        let vpn = CheckMenuItem::with_id(VPN_ID, "Enable VPN on next start", true, false, None);
        let vpn_settings = MenuItem::with_id(VPN_SETTINGS_ID, "Open VPN Settings", true, None);
        let separator = PredefinedMenuItem::separator();
        let quit = MenuItem::with_id(QUIT_ID, "Quit Nebulus", true, None);
        let menu = Menu::with_items(&[
            &show,
            &hide,
            &separator,
            &receiver,
            &vpn,
            &vpn_settings,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|error| format!("create tray menu failed: {error}"))?;

        let icon = tray_icon().map_err(|error| format!("create tray icon failed: {error}"))?;
        let icon = TrayIconBuilder::new()
            .with_id("nebulus")
            .with_tooltip("Nebulus OpenIPC ground station")
            .with_icon(icon)
            .with_icon_as_template(cfg!(target_os = "macos"))
            .with_menu(Box::new(menu))
            .build()
            .map_err(|error| format!("install tray icon failed: {error}"))?;

        let (sender, events) = mpsc::channel();
        let repaint = context.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let command = match event.id().0.as_str() {
                SHOW_ID => Some(TrayCommand::Show),
                HIDE_ID => Some(TrayCommand::Hide),
                RECEIVER_ID => Some(TrayCommand::ToggleReceiver),
                VPN_ID => Some(TrayCommand::ToggleVpn),
                VPN_SETTINGS_ID => Some(TrayCommand::OpenVpn),
                QUIT_ID => Some(TrayCommand::Quit),
                _ => None,
            };
            if let Some(command) = command {
                let _ = sender.send(command);
                repaint.request_repaint();
            }
        }));

        Ok(Self {
            _icon: icon,
            events,
            receiver,
            vpn,
        })
    }

    fn drain(&self) -> Vec<TrayCommand> {
        self.events.try_iter().collect()
    }

    pub(crate) fn sync(&self, state: ReceiverState, vpn_enabled: bool) {
        let idle = matches!(state, ReceiverState::Idle | ReceiverState::Failed);
        self.receiver
            .set_text(if idle { "Start RX" } else { "Stop RX" });
        self.receiver.set_enabled(!matches!(
            state,
            ReceiverState::Connecting | ReceiverState::Stopping
        ));
        self.vpn.set_checked(vpn_enabled);
        self.vpn.set_enabled(idle);
    }
}

impl NebulusApp {
    pub(crate) fn process_tray(&mut self, context: &egui::Context) {
        let Some(tray) = self.desktop_tray.as_ref() else {
            return;
        };
        let commands = tray.drain();
        for command in commands {
            match command {
                TrayCommand::Show => show_window(context),
                TrayCommand::Hide => {
                    context.send_viewport_cmd(egui::ViewportCommand::Visible(false))
                }
                TrayCommand::ToggleReceiver => match self.state {
                    ReceiverState::Idle | ReceiverState::Failed => self.start_receiver(context),
                    ReceiverState::Receiving | ReceiverState::Ready => self.stop_receiver(),
                    ReceiverState::Connecting | ReceiverState::Stopping => {}
                },
                TrayCommand::ToggleVpn => {
                    if matches!(self.state, ReceiverState::Idle | ReceiverState::Failed) {
                        self.settings.vpn_enabled = !self.settings.vpn_enabled;
                    }
                }
                TrayCommand::OpenVpn => {
                    self.settings.show_sidebar = true;
                    self.active_tab = PanelTab::Vpn;
                    show_window(context);
                }
                TrayCommand::Quit => {
                    if !matches!(self.state, ReceiverState::Idle | ReceiverState::Failed) {
                        self.stop_receiver();
                    }
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if let Some(tray) = self.desktop_tray.as_ref() {
            tray.sync(self.state, self.settings.vpn_enabled);
        }
    }
}

fn show_window(context: &egui::Context) {
    context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    context.send_viewport_cmd(egui::ViewportCommand::Focus);
}

fn tray_icon() -> Result<Icon, tray_icon::BadIcon> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];
    let color = if cfg!(target_os = "macos") {
        [0, 0, 0, 255]
    } else {
        [61, 214, 154, 255]
    };

    for y in 4..28 {
        for x in 4..28 {
            let border = !(8..24).contains(&x) || !(8..24).contains(&y);
            let diagonal = (x as i32 - y as i32).unsigned_abs() <= 2;
            if border || diagonal {
                let offset = ((y * SIZE + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE)
}
