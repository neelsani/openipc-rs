use std::sync::{Arc, Mutex};

use openipc_uplink::{SshClient, UserspaceNetwork, VtxController};

use super::{VtxControlEvent, VtxControlRequest};

pub(super) async fn process_request(
    controller: &mut Option<VtxController>,
    network: &Arc<Mutex<UserspaceNetwork>>,
    credentials: &openipc_uplink::SshCredentials,
    request: VtxControlRequest,
    mut emit: impl FnMut(VtxControlEvent),
) {
    if matches!(request, VtxControlRequest::Disconnect) {
        if let Some(active) = controller.take() {
            let _ = active.ssh().disconnect().await;
        }
        emit(VtxControlEvent::Disconnected);
        return;
    }

    if controller.is_none() {
        emit(VtxControlEvent::Connecting);
        let stream = match network
            .lock()
            .map_err(|_| "userspace network state poisoned".to_owned())
            .and_then(|mut network| network.connect_tcp(22).map_err(|error| error.to_string()))
        {
            Ok(stream) => stream,
            Err(error) => {
                emit(VtxControlEvent::Failed(error));
                return;
            }
        };
        match SshClient::connect(stream, credentials.clone()).await {
            Ok(ssh) => {
                *controller = Some(VtxController::new(ssh));
                emit(VtxControlEvent::Connected);
            }
            Err(error) => {
                emit(VtxControlEvent::Failed(error.to_string()));
                return;
            }
        }
    }

    let Some(active) = controller.as_ref() else {
        return;
    };
    let result = match request {
        VtxControlRequest::Connect => Ok(VtxControlEvent::Connected),
        VtxControlRequest::Refresh => active
            .read_config_bundle()
            .await
            .map(VtxControlEvent::Config),
        VtxControlRequest::SetWfbBatch(settings) => active
            .set_wfb_batch(&settings)
            .await
            .map(|()| VtxControlEvent::Applied("WFB settings")),
        VtxControlRequest::SetCameraBatch(settings) => active
            .set_camera_batch(settings)
            .await
            .map(|()| VtxControlEvent::Applied("camera setting")),
        VtxControlRequest::SetTelemetryBatch(settings) => active
            .set_telemetry_batch(settings)
            .await
            .map(|()| VtxControlEvent::Applied("telemetry setting")),
        VtxControlRequest::SetAdaptiveLink(setting) => active
            .set_adaptive_link(setting)
            .await
            .map(|()| VtxControlEvent::Applied("adaptive-link setting")),
        VtxControlRequest::GetVideoMode => legacy_request(network, "get_current_video_mode")
            .await
            .map(|response| {
                VtxControlEvent::VideoMode(String::from_utf8_lossy(&response).trim().to_owned())
            })
            .map_err(openipc_uplink::VtxSettingError::from),
        VtxControlRequest::SetVideoMode(mode) => {
            let mode =
                validate_video_mode(&mode).map_err(openipc_uplink::VtxSettingError::InvalidValue);
            match mode {
                Ok(mode) => legacy_send(network, &format!("set_simple_video_mode {mode}"))
                    .await
                    .map(|()| VtxControlEvent::Applied("simple video mode"))
                    .map_err(openipc_uplink::VtxSettingError::from),
                Err(error) => Err(error),
            }
        }
        VtxControlRequest::Reboot => active
            .reboot()
            .await
            .map(|()| VtxControlEvent::Applied("reboot request")),
        VtxControlRequest::Disconnect => unreachable!(),
    };
    match result {
        Ok(event) => emit(event),
        Err(error) => {
            if matches!(
                error,
                openipc_uplink::VtxSettingError::Ssh(openipc_uplink::SshError::Protocol(_))
            ) {
                *controller = None;
            }
            emit(VtxControlEvent::Failed(error.to_string()));
        }
    }
}

async fn legacy_send(
    network: &Arc<Mutex<UserspaceNetwork>>,
    command: &str,
) -> Result<(), openipc_uplink::SshError> {
    let stream = legacy_stream(network)?;
    openipc_uplink::LegacyControlClient::send(stream, command).await
}

async fn legacy_request(
    network: &Arc<Mutex<UserspaceNetwork>>,
    command: &str,
) -> Result<Vec<u8>, openipc_uplink::SshError> {
    let stream = legacy_stream(network)?;
    openipc_uplink::LegacyControlClient::request(stream, command).await
}

fn legacy_stream(
    network: &Arc<Mutex<UserspaceNetwork>>,
) -> Result<openipc_uplink::VirtualTcpStream, openipc_uplink::SshError> {
    network
        .lock()
        .map_err(|_| openipc_uplink::SshError::Protocol("userspace network state poisoned".into()))?
        .connect_tcp(12_355)
        .map_err(|error| openipc_uplink::SshError::Protocol(error.to_string()))
}

fn validate_video_mode(mode: &str) -> Result<&str, &'static str> {
    let mode = mode.trim();
    if mode.is_empty()
        || !mode.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b':' | b'_' | b'-' | b'.' | b'x')
        })
    {
        return Err("video mode contains unsupported characters");
    }
    Ok(mode)
}
