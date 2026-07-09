//! Native OpenIPC layer-three tunnel bridge.

use std::io;

pub(crate) const ADDRESS: &str = "10.5.0.3";
pub(crate) const PREFIX_LENGTH: u8 = 24;
#[cfg(all(unix, not(target_os = "android")))]
const DESTINATION: &str = "10.5.0.1";
#[cfg(not(target_os = "android"))]
const NETMASK: &str = "255.255.255.0";
const MTU: u16 = 1500;

#[cfg(any(all(unix, not(target_os = "android")), target_os = "windows"))]
use tun::AbstractDevice as _;

#[cfg(all(unix, not(target_os = "android")))]
pub(crate) struct TunBridge {
    device: tun::Device,
    name: String,
    read_buffer: Vec<u8>,
}

#[cfg(all(unix, not(target_os = "android")))]
impl TunBridge {
    pub(crate) fn open_default() -> Result<Self, String> {
        let mut config = tun::Configuration::default();
        config
            .address(ADDRESS)
            .destination(DESTINATION)
            .netmask(NETMASK)
            .mtu(MTU)
            .layer(tun::Layer::L3)
            .up();
        #[cfg(target_os = "linux")]
        {
            config.tun_name("openipc%d");
            config.platform_config(|platform| {
                platform.ensure_root_privileges(true);
            });
        }
        let device = tun::create(&config)
            .map_err(|error| format!("create VPN interface failed: {error}"))?;
        device
            .set_nonblock()
            .map_err(|error| format!("set VPN nonblocking failed: {error}"))?;
        let mut name = device
            .tun_name()
            .map_err(|error| format!("read VPN interface name failed: {error}"))?;
        if name.is_empty() {
            name = "OpenIPC VPN".to_owned();
        }
        Ok(Self {
            device,
            name,
            read_buffer: vec![0; usize::from(MTU) + 512],
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn write_downlink(&mut self, payload: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        for packet in tunnel_packets(payload) {
            written += self.device.send(packet?)?;
        }
        Ok(written)
    }

    pub(crate) fn read_uplink(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            match self
                .device
                .recv_timeout(&mut self.read_buffer, std::time::Duration::ZERO)
            {
                Ok(0) => return Ok(None),
                Ok(amount) => return Ok(Some(self.read_buffer[..amount].to_vec())),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) struct TunBridge {
    device: tun::Device,
    name: String,
    read_buffer: Vec<u8>,
    service_fd: i32,
}

#[cfg(target_os = "android")]
impl TunBridge {
    pub(crate) fn open_default() -> Result<Self, String> {
        let opened = crate::android::open_vpn()?;
        // SAFETY: `opened.fd` is a live ParcelFileDescriptor owned by the Java
        // VpnService. Rust owns and closes only this duplicated descriptor.
        let duplicate = unsafe { libc::dup(opened.fd) };
        if duplicate < 0 {
            crate::android::close_vpn(opened.fd);
            return Err(format!(
                "duplicate Android VPN fd failed: {}",
                io::Error::last_os_error()
            ));
        }
        let mut config = tun::Configuration::default();
        config
            .raw_fd(duplicate)
            .close_fd_on_drop(true)
            .layer(tun::Layer::L3)
            .mtu(MTU)
            .up();
        let device = match tun::create(&config) {
            Ok(device) => device,
            Err(error) => {
                // SAFETY: ownership transfers to `tun::Device` only on a
                // successful create. Close our duplicate on this error path.
                unsafe { libc::close(duplicate) };
                crate::android::close_vpn(opened.fd);
                return Err(format!("open Android VPN interface failed: {error}"));
            }
        };
        if let Err(error) = device.set_nonblock() {
            drop(device);
            crate::android::close_vpn(opened.fd);
            return Err(format!("set Android VPN nonblocking failed: {error}"));
        }
        Ok(Self {
            device,
            name: opened.interface_name,
            read_buffer: vec![0; usize::from(MTU) + 512],
            service_fd: opened.fd,
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn write_downlink(&mut self, payload: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        for packet in tunnel_packets(payload) {
            written += self.device.send(packet?)?;
        }
        Ok(written)
    }

    pub(crate) fn read_uplink(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self
            .device
            .recv_timeout(&mut self.read_buffer, std::time::Duration::ZERO)
        {
            Ok(0) => Ok(None),
            Ok(amount) => Ok(Some(self.read_buffer[..amount].to_vec())),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(target_os = "android")]
impl Drop for TunBridge {
    fn drop(&mut self) {
        crate::android::close_vpn(self.service_fd);
    }
}

#[cfg(target_os = "windows")]
pub(crate) struct TunBridge {
    name: String,
    downlink: std::sync::mpsc::Sender<Vec<u8>>,
    uplink: std::sync::mpsc::Receiver<Vec<u8>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl TunBridge {
    pub(crate) fn open_default() -> Result<Self, String> {
        let wintun_path = crate::wintun::locate().ok_or_else(|| {
            "Wintun is not installed; install it from Nebulus Settings".to_owned()
        })?;
        let mut config = tun::Configuration::default();
        config
            .tun_name("OpenIPC Nebulus")
            .address(ADDRESS)
            .netmask(NETMASK)
            .mtu(MTU)
            .layer(tun::Layer::L3)
            .up();
        config.platform_config(|platform| {
            platform.wintun_file(wintun_path.as_os_str());
            platform.wait_for_interfaces(true, false, std::time::Duration::from_secs(5));
        });
        let device = tun::create_as_async(&config)
            .map_err(|error| format!("create Wintun interface failed: {error}"))?;
        let name = device
            .tun_name()
            .map_err(|error| format!("read VPN interface name failed: {error}"))?;
        let (downlink_tx, downlink_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (uplink_tx, uplink_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            use std::sync::atomic::Ordering;
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            else {
                return;
            };
            let device = device;
            let mut read_buffer = vec![0; usize::from(MTU) + 512];
            while !worker_stop.load(Ordering::Relaxed) {
                while let Ok(packet) = downlink_rx.try_recv() {
                    let _ = runtime.block_on(device.send(&packet));
                }
                let received = runtime.block_on(async {
                    tokio::time::timeout(
                        std::time::Duration::from_millis(10),
                        device.recv(&mut read_buffer),
                    )
                    .await
                });
                if let Ok(Ok(amount)) = received {
                    let _ = uplink_tx.send(read_buffer[..amount].to_vec());
                }
            }
        });
        Ok(Self {
            name,
            downlink: downlink_tx,
            uplink: uplink_rx,
            stop,
            worker: Some(worker),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn write_downlink(&mut self, payload: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        for packet in tunnel_packets(payload) {
            let packet = packet?;
            written += packet.len();
            self.downlink
                .send(packet.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "VPN worker stopped"))?;
        }
        Ok(written)
    }

    pub(crate) fn read_uplink(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self.uplink.try_recv() {
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
        use std::sync::atomic::Ordering;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn tunnel_packets(payload: &[u8]) -> impl Iterator<Item = io::Result<&[u8]>> {
    openipc_uplink::parse_tunnel_packets(payload)
        .map(|packet| packet.map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)))
}

#[cfg(test)]
mod tests {
    use super::tunnel_packets;

    #[test]
    fn tunnel_framing_round_trips() {
        let payload = openipc_uplink::frame_ip_packet(&[0x45, 1, 2, 3]).unwrap();
        let packets = tunnel_packets(&payload)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(packets, [&[0x45, 1, 2, 3][..]]);
    }

    #[test]
    fn tunnel_framing_preserves_aggregated_packets() {
        let mut payload = openipc_uplink::frame_ip_packet(&[0x45, 1]).unwrap();
        payload.extend(openipc_uplink::frame_ip_packet(&[0x45, 2]).unwrap());
        let packets = tunnel_packets(&payload)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(packets, [&[0x45, 1][..], &[0x45, 2][..]]);
    }
}
