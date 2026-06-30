use super::*;
use crate::worker::run_rx_worker;

#[cfg(target_os = "android")]
use tauri_plugin_openipc_usb::OpenIpcUsbExt;

#[tauri::command]
pub(crate) fn openipc_list_devices(_app: AppHandle) -> Result<Vec<StationUsbDevice>, String> {
    #[cfg(target_os = "android")]
    {
        Ok(_app
            .openipc_usb()
            .list_devices()
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(station_device_from_android)
            .collect())
    }

    #[cfg(not(target_os = "android"))]
    Ok(list_supported_devices()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(station_device_from_summary)
        .collect())
}

#[tauri::command]
pub(crate) fn openipc_connect(
    request: ConnectRequest,
    state: State<'_, DesktopState>,
) -> Result<ConnectReport, String> {
    #[cfg(target_os = "android")]
    {
        let _ = request;
        let _ = state;
        Err(
            "Android USB connections must use openipc_connect_from_fd after UsbManager permission"
                .to_owned(),
        )
    }

    #[cfg(not(target_os = "android"))]
    {
        let mut driver_options = DriverOptions {
            skip_reset: request.skip_reset.unwrap_or(false),
            initialize_hardware: true,
            ..DriverOptions::default()
        };
        if let Some(device_id) = request
            .device_id
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            let (vendor_id, product_id) = parse_usb_id(device_id)?;
            driver_options.target_vendor_id = Some(vendor_id);
            driver_options.target_product_id = Some(product_id);
        }
        let summary = list_supported_devices()
            .map_err(|err| err.to_string())?
            .into_iter()
            .find(|device| {
                driver_options
                    .target_vendor_id
                    .is_none_or(|vendor_id| device.vendor_id == vendor_id)
                    && driver_options
                        .target_product_id
                        .is_none_or(|product_id| device.product_id == product_id)
            });
        let device = RealtekDevice::open_first(driver_options).map_err(|err| err.to_string())?;
        finish_connect(
            device,
            &request,
            summary.map(station_device_from_summary),
            state,
        )
    }
}

#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) fn openipc_connect_from_fd(
    request: ConnectFromFdRequest,
    state: State<'_, DesktopState>,
) -> Result<ConnectReport, String> {
    let owned_fd = duplicate_fd(request.fd)?;
    let nusb_device = nusb::Device::from_fd(owned_fd)
        .wait()
        .map_err(|err| format!("open USB device from fd failed: {err}"))?;
    let mut driver_options = DriverOptions {
        skip_reset: request.connect.skip_reset.unwrap_or(true),
        initialize_hardware: true,
        target_vendor_id: request.vendor_id,
        target_product_id: request.product_id,
        ..DriverOptions::default()
    };
    if let Some(device_id) = request
        .connect
        .device_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let (vendor_id, product_id) = parse_usb_id(device_id)?;
        driver_options.target_vendor_id = Some(vendor_id);
        driver_options.target_product_id = Some(product_id);
    }
    let summary = match (request.vendor_id, request.product_id) {
        (Some(vendor_id), Some(product_id)) => Some(StationUsbDevice {
            id: request.android_device_id,
            vendor_id,
            product_id,
            product: request.product,
            manufacturer: request.manufacturer,
        }),
        _ => None,
    };
    let device = RealtekDevice::from_nusb_device(nusb_device, driver_options)
        .map_err(|err| err.to_string())?;
    finish_connect(device, &request.connect, summary, state)
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
pub(crate) fn openipc_connect_from_fd(
    _request: ConnectFromFdRequest,
    _state: State<'_, DesktopState>,
) -> Result<ConnectReport, String> {
    Err("opening USB devices from file descriptors is only used by the Android backend".to_owned())
}

pub(crate) fn finish_connect(
    device: RealtekDevice,
    request: &ConnectRequest,
    summary: Option<StationUsbDevice>,
    state: State<'_, DesktopState>,
) -> Result<ConnectReport, String> {
    if state
        .worker
        .lock()
        .map_err(|_| "worker lock poisoned")?
        .is_some()
    {
        return Err("receiver is already running".to_owned());
    }

    let report = device
        .initialize_monitor_with_options(
            radio_config(
                request.channel,
                request.channel_width_mhz,
                request.channel_offset,
            )?,
            MonitorOptions::from_env(),
        )
        .map_err(|err| err.to_string())?;
    let device_id = summary
        .as_ref()
        .map(|device| {
            device
                .id
                .clone()
                .unwrap_or_else(|| usb_id(device.vendor_id, device.product_id))
        })
        .unwrap_or_else(|| report.chip.family.name().to_owned());
    let label = summary
        .as_ref()
        .map(|device| {
            device_label(
                device.manufacturer.as_deref(),
                device.product.as_deref(),
                &device_id,
            )
        })
        .unwrap_or_else(|| device_id.clone());

    let usb_info = UsbInfoPayload {
        label,
        bulk_in: device.bulk_in_ep,
        bulk_out: device.bulk_out_ep,
    };
    let chip_family = report.chip.family;
    let init_report = init_report_payload(report);

    *state.device.lock().map_err(|_| "device lock poisoned")? = Some(Arc::new(device));
    *state.chip_family.lock().map_err(|_| "chip lock poisoned")? = Some(chip_family);

    Ok(ConnectReport {
        device_id,
        usb_info,
        init_report,
    })
}

#[tauri::command]
pub(crate) fn openipc_start_rx(
    app: AppHandle,
    request: StartRxRequest,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    let mut worker = state.worker.lock().map_err(|_| "worker lock poisoned")?;
    if worker.is_some() {
        return Err("receiver is already running".to_owned());
    }
    let device = state
        .device
        .lock()
        .map_err(|_| "device lock poisoned")?
        .clone()
        .ok_or_else(|| "connect to a Realtek adapter before starting RX".to_owned())?;
    let chip_family = state
        .chip_family
        .lock()
        .map_err(|_| "chip lock poisoned")?
        .ok_or_else(|| "chip family is unknown; reconnect the adapter".to_owned())?;

    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let handle = thread::spawn(move || {
        if let Err(err) = run_rx_worker(app.clone(), device, chip_family, request, worker_stop) {
            emit_stopped(&app, "error", err);
        }
    });
    *worker = Some(RxWorker {
        stop,
        join: Some(handle),
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn openipc_stop_rx(state: State<'_, DesktopState>) -> Result<(), String> {
    let worker = state
        .worker
        .lock()
        .map_err(|_| "worker lock poisoned")?
        .take();
    if let Some(mut worker) = worker {
        worker.stop.store(true, Ordering::Relaxed);
        if let Some(join) = worker.join.take() {
            let _ = join.join();
        }
    }
    Ok(())
}
