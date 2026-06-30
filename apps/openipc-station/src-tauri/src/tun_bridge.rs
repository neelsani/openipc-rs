use super::*;

pub(crate) const OPENIPC_VPN_ADDRESS: &str = "10.5.0.3";
#[cfg(all(unix, not(target_os = "android")))]
pub(crate) const OPENIPC_VPN_DESTINATION: &str = "10.5.0.1";
pub(crate) const OPENIPC_VPN_NETMASK: &str = "255.255.255.0";
pub(crate) const OPENIPC_VPN_PREFIX_LEN: u8 = 24;
pub(crate) const OPENIPC_VPN_MTU: u16 = 1500;

#[cfg(any(unix, target_os = "windows"))]
use tun::AbstractDevice;

#[cfg(unix)]
pub(crate) struct TunBridge {
    device: tun::Device,
    name: String,
    read_buf: Vec<u8>,
}

#[cfg(unix)]
impl TunBridge {
    pub(crate) fn open_default(raw_fd: Option<i32>) -> Result<Self, String> {
        let mut config = tun::Configuration::default();
        config
            .address(OPENIPC_VPN_ADDRESS)
            .netmask(OPENIPC_VPN_NETMASK)
            .mtu(OPENIPC_VPN_MTU)
            .layer(tun::Layer::L3)
            .up();

        if let Some(fd) = raw_fd {
            config.raw_fd(duplicate_tun_fd(fd)?);
            config.close_fd_on_drop(true);
        } else {
            #[cfg(target_os = "android")]
            return Err(
                "Android VPN requires a VpnService file descriptor from the OpenIPC USB plugin"
                    .to_owned(),
            );

            #[cfg(not(target_os = "android"))]
            {
                config.destination(OPENIPC_VPN_DESTINATION);
            }
        }

        #[cfg(target_os = "linux")]
        {
            config.tun_name("openipc%d");
            config.platform_config(|platform| {
                platform.ensure_root_privileges(true);
            });
        }

        let device =
            tun::create(&config).map_err(|err| format!("create VPN interface failed: {err}"))?;
        device
            .set_nonblock()
            .map_err(|err| format!("set VPN interface nonblocking failed: {err}"))?;
        let mut name = device
            .tun_name()
            .map_err(|err| format!("read VPN interface name failed: {err}"))?;
        if name.is_empty() {
            name = "OpenIPC VPN".to_owned();
        }
        Ok(Self {
            device,
            name,
            read_buf: vec![0; usize::from(OPENIPC_VPN_MTU) + 512],
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn write_downlink_payload(&mut self, payload: &[u8]) -> io::Result<usize> {
        let Some(packet) = tunnel_payload_packet(payload) else {
            return Ok(0);
        };
        self.device.send(packet)
    }

    pub(crate) fn read_uplink_payload(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            match self
                .device
                .recv_timeout(&mut self.read_buf, Duration::from_millis(0))
            {
                Ok(0) => return Ok(None),
                Ok(amount) => return Ok(Some(length_prefixed_payload(&self.read_buf[..amount]))),
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    if err.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Ok(None);
                }
                Err(err) => return Err(err),
            }
        }
    }
}

#[cfg(unix)]
fn duplicate_tun_fd(fd: i32) -> Result<i32, String> {
    if fd < 0 {
        return Err(format!("invalid VPN file descriptor {fd}"));
    }
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(format!(
            "duplicate VPN file descriptor failed: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(dup_fd)
}

#[cfg(target_os = "windows")]
pub(crate) struct TunBridge {
    name: String,
    downlink_tx: std::sync::mpsc::Sender<Vec<u8>>,
    uplink_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl TunBridge {
    pub(crate) fn open_default(raw_fd: Option<i32>) -> Result<Self, String> {
        if raw_fd.is_some() {
            return Err("Windows VPN does not accept Android-style raw TUN fds".to_owned());
        }

        let mut config = tun::Configuration::default();
        config
            .tun_name("OpenIPC Station")
            .address(OPENIPC_VPN_ADDRESS)
            .netmask(OPENIPC_VPN_NETMASK)
            .mtu(OPENIPC_VPN_MTU)
            .layer(tun::Layer::L3)
            .up();
        config.platform_config(|platform| {
            platform.wait_for_interfaces(true, false, Duration::from_secs(5));
        });

        let device = tun::create_as_async(&config)
            .map_err(|err| format!("create Wintun VPN interface failed: {err}"))?;
        let name = device
            .tun_name()
            .map_err(|err| format!("read VPN interface name failed: {err}"))?;
        let (downlink_tx, downlink_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (uplink_tx, uplink_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let join = thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            else {
                return;
            };
            let mut device = device;
            let mut read_buf = vec![0; usize::from(OPENIPC_VPN_MTU) + 512];
            while !worker_stop.load(Ordering::Relaxed) {
                while let Ok(packet) = downlink_rx.try_recv() {
                    let _ = runtime.block_on(device.send(&packet));
                }
                let received = runtime.block_on(async {
                    tokio::time::timeout(Duration::from_millis(10), device.recv(&mut read_buf))
                        .await
                });
                if let Ok(Ok(amount)) = received {
                    let _ = uplink_tx.send(length_prefixed_payload(&read_buf[..amount]));
                }
            }
        });
        Ok(Self {
            name,
            downlink_tx,
            uplink_rx,
            stop,
            join: Some(join),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn write_downlink_payload(&mut self, payload: &[u8]) -> io::Result<usize> {
        let Some(packet) = tunnel_payload_packet(payload) else {
            return Ok(0);
        };
        self.downlink_tx
            .send(packet.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "VPN worker stopped"))?;
        Ok(packet.len())
    }

    pub(crate) fn read_uplink_payload(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self.uplink_rx.try_recv() {
            Ok(payload) => Ok(Some(payload)),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "VPN worker stopped",
            )),
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for TunBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) struct TunBridge;

#[cfg(not(any(unix, target_os = "windows")))]
impl TunBridge {
    pub(crate) fn open_default(_raw_fd: Option<i32>) -> Result<Self, String> {
        Err("VPN routing is not enabled for this target".to_owned())
    }

    pub(crate) fn name(&self) -> &str {
        "unsupported"
    }

    pub(crate) fn write_downlink_payload(&mut self, _payload: &[u8]) -> io::Result<usize> {
        Ok(0)
    }

    pub(crate) fn read_uplink_payload(&mut self) -> io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

fn tunnel_payload_packet(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < 2 {
        return None;
    }
    let declared_len = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    let body = &payload[2..];
    if body.is_empty() {
        return None;
    }
    if declared_len == 0 || declared_len > body.len() {
        Some(body)
    } else {
        Some(&body[..declared_len])
    }
}

fn length_prefixed_payload(packet: &[u8]) -> Vec<u8> {
    let amount_u16 = packet.len().min(u16::MAX as usize) as u16;
    let mut payload = Vec::with_capacity(packet.len() + 2);
    payload.extend_from_slice(&amount_u16.to_be_bytes());
    payload.extend_from_slice(packet);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_payload_uses_valid_declared_length() {
        let payload = [0, 3, 1, 2, 3, 4, 5];
        assert_eq!(tunnel_payload_packet(&payload), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn tunnel_payload_falls_back_to_body_when_declared_length_is_too_large() {
        let payload = [0, 8, 1, 2, 3];
        assert_eq!(tunnel_payload_packet(&payload), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn uplink_payload_adds_big_endian_length() {
        assert_eq!(length_prefixed_payload(&[1, 2, 3]), vec![0, 3, 1, 2, 3]);
    }
}
