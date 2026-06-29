#[cfg(target_os = "android")]
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use nusb::transfer::{Bulk, Out};
#[cfg(target_os = "android")]
use nusb::MaybeFuture;
use openipc_core::realtek::{parse_rx_aggregate, RxPacketAttrib, RxPacketType};
use openipc_core::realtek_tx::RealtekTxOptions;
use openipc_core::rtp::{Codec, DepacketizedFrame};
use openipc_core::{
    AdaptiveLinkSender, ChannelId, FecCounters, FrameLayout, PayloadPipeline, PayloadPipelineEvent,
    PipelineEvent, RadioPort, ReceiverPipeline, WfbKeypair, WfbTxKeypair,
};
#[cfg(not(target_os = "android"))]
use openipc_rtl88xx::{list_supported_devices, UsbDeviceSummary};
use openipc_rtl88xx::{
    ChannelWidth, ChipFamily, DriverOptions, InitReport, InitStatus, MonitorOptions, RadioConfig,
    RealtekDevice,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

mod commands;
mod events;
mod payloads;
mod platform;
mod types;
mod worker;

pub(crate) use events::*;
pub(crate) use payloads::*;
pub(crate) use platform::*;
pub(crate) use types::*;

#[cfg(target_os = "android")]
#[tauri::mobile_entry_point]
fn android_entry() {
    crate::run();
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_openipc_usb::init())
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            commands::openipc_list_devices,
            commands::openipc_connect,
            commands::openipc_connect_from_fd,
            commands::openipc_start_rx,
            commands::openipc_stop_rx,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenIPC Station desktop app");
}
