#[path = "../common.rs"]
mod common;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

#[cfg(unix)]
fn run() -> common::CliResult<()> {
    unix_impl::run()
}

#[cfg(not(unix))]
fn run() -> common::CliResult<()> {
    Err("wfb_tun is currently implemented for Unix targets only".into())
}

#[cfg(unix)]
mod unix_impl {
    use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
    use std::time::{Duration, Instant};

    use tun::AbstractDevice;

    use crate::common::{next_arg, parse_u16, parse_u64, CliResult};

    const MTU: usize = 1445;
    const TUN_HEADER_LEN: usize = 2;
    const DEFAULT_TUN_ADDR: &str = "10.5.0.2/24";

    #[derive(Debug, Clone)]
    struct Config {
        tun_name: String,
        tun_addr: String,
        peer: SocketAddr,
        listen_port: u16,
        agg_timeout: Duration,
    }

    impl Config {
        fn parse(args: impl Iterator<Item = String>) -> CliResult<Self> {
            let mut config = Self {
                tun_name: "wfb-tun".to_owned(),
                tun_addr: DEFAULT_TUN_ADDR.to_owned(),
                peer: "127.0.0.1:5801".parse()?,
                listen_port: 5800,
                agg_timeout: Duration::from_millis(5),
            };
            let mut peer_addr = "127.0.0.1".to_owned();
            let mut peer_port = 5801u16;

            let mut args = args.peekable();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "-h" | "--help" => {
                        print_help();
                        std::process::exit(0);
                    }
                    "-t" => config.tun_name = next_arg(&mut args, "-t")?,
                    "-a" => config.tun_addr = next_arg(&mut args, "-a")?,
                    "-c" => peer_addr = next_arg(&mut args, "-c")?,
                    "-u" => peer_port = parse_u16(&next_arg(&mut args, "-u")?)?,
                    "-l" => config.listen_port = parse_u16(&next_arg(&mut args, "-l")?)?,
                    "-T" => {
                        config.agg_timeout =
                            Duration::from_millis(parse_u64(&next_arg(&mut args, "-T")?)?);
                    }
                    _ => return Err(format!("unknown option: {arg}").into()),
                }
            }
            config.peer = format!("{peer_addr}:{peer_port}").parse()?;
            Ok(config)
        }
    }

    pub fn run() -> CliResult<()> {
        let config = Config::parse(std::env::args().skip(1))?;
        let (addr, netmask) = parse_cidr(&config.tun_addr)?;

        let mut tun_config = tun::Configuration::default();
        tun_config
            .tun_name(&config.tun_name)
            .address(addr)
            .netmask(netmask)
            .mtu((MTU - TUN_HEADER_LEN) as u16)
            .layer(tun::Layer::L3)
            .up();
        #[cfg(target_os = "linux")]
        tun_config.platform_config(|platform| {
            platform.ensure_root_privileges(true);
        });

        let device = tun::create(&tun_config)?;
        device.set_nonblock()?;
        eprintln!(
            "wfb_tun: interface={} addr={} listen=0.0.0.0:{} peer={} agg_timeout_ms={}",
            device.tun_name()?,
            config.tun_addr,
            config.listen_port,
            config.peer,
            config.agg_timeout.as_millis()
        );

        let socket = UdpSocket::bind(("0.0.0.0", config.listen_port))?;
        socket.set_nonblocking(true)?;
        let mut tun_buf = vec![0u8; MTU + 512];
        let mut udp_buf = vec![0u8; MTU + 512];
        let mut batch = Vec::<u8>::with_capacity(MTU * 2);
        let mut first_batch_at: Option<Instant> = None;
        let mut pkt_sem = 0u8;
        let mut last_ping = Instant::now();

        loop {
            drain_tun(
                &device,
                &mut tun_buf,
                &mut batch,
                &mut first_batch_at,
                config.agg_timeout,
            )?;
            flush_batch_if_ready(
                &socket,
                config.peer,
                &mut batch,
                &mut first_batch_at,
                config.agg_timeout,
                &mut pkt_sem,
            )?;
            drain_socket(&socket, &device, &mut udp_buf)?;

            if last_ping.elapsed() >= Duration::from_millis(500) {
                if pkt_sem == 0 {
                    let _ = socket.send_to(&[], config.peer);
                } else {
                    pkt_sem = pkt_sem.saturating_sub(1);
                }
                last_ping = Instant::now();
            }

            // The tunnel is intentionally non-blocking so the aggregation
            // deadline remains the only added latency. A scheduler yield is
            // preferable to a fixed 1 ms polling delay here.
            std::thread::yield_now();
        }
    }

    fn drain_tun(
        device: &tun::Device,
        tun_buf: &mut [u8],
        batch: &mut Vec<u8>,
        first_batch_at: &mut Option<Instant>,
        agg_timeout: Duration,
    ) -> std::io::Result<()> {
        loop {
            match device.recv_timeout(tun_buf, Duration::from_millis(0)) {
                Ok(0) => return Ok(()),
                Ok(amount) => {
                    let packet = &tun_buf[..amount.min(u16::MAX as usize)];
                    let needed = TUN_HEADER_LEN + packet.len();
                    if batch.len() + needed > MTU && !batch.is_empty() {
                        return Ok(());
                    }
                    if batch.is_empty() {
                        *first_batch_at = Some(Instant::now());
                    }
                    batch.extend_from_slice(&(packet.len() as u16).to_be_bytes());
                    batch.extend_from_slice(packet);
                    if batch.len() >= MTU || agg_timeout.is_zero() {
                        return Ok(());
                    }
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(());
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn flush_batch_if_ready(
        socket: &UdpSocket,
        peer: SocketAddr,
        batch: &mut Vec<u8>,
        first_batch_at: &mut Option<Instant>,
        agg_timeout: Duration,
        pkt_sem: &mut u8,
    ) -> std::io::Result<()> {
        let should_flush = !batch.is_empty()
            && (batch.len() >= MTU
                || agg_timeout.is_zero()
                || first_batch_at
                    .map(|started| started.elapsed() >= agg_timeout)
                    .unwrap_or(false));
        if should_flush {
            socket.send_to(batch, peer)?;
            batch.clear();
            *first_batch_at = None;
            *pkt_sem = 1;
        }
        Ok(())
    }

    fn drain_socket(
        socket: &UdpSocket,
        device: &tun::Device,
        udp_buf: &mut [u8],
    ) -> std::io::Result<()> {
        loop {
            match socket.recv_from(udp_buf) {
                Ok((0, _)) => continue,
                Ok((amount, _)) => write_tun_payloads(device, &udp_buf[..amount])?,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(err) => return Err(err),
            }
        }
    }

    fn write_tun_payloads(device: &tun::Device, mut payload: &[u8]) -> std::io::Result<()> {
        while payload.len() >= TUN_HEADER_LEN {
            let packet_len = u16::from_be_bytes([payload[0], payload[1]]) as usize;
            payload = &payload[TUN_HEADER_LEN..];
            if packet_len == 0 || packet_len > payload.len() {
                if !payload.is_empty() {
                    device.send(payload)?;
                }
                return Ok(());
            }
            device.send(&payload[..packet_len])?;
            payload = &payload[packet_len..];
        }
        Ok(())
    }

    fn parse_cidr(value: &str) -> CliResult<(Ipv4Addr, Ipv4Addr)> {
        let (addr, prefix) = value
            .split_once('/')
            .ok_or("TUN address must use CIDR form, for example 10.5.0.2/24")?;
        let addr = addr.parse::<Ipv4Addr>()?;
        let prefix = prefix.parse::<u8>()?;
        if prefix > 32 {
            return Err("CIDR prefix must be <= 32".into());
        }
        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        };
        Ok((addr, Ipv4Addr::from(mask)))
    }

    fn print_help() {
        println!(
            r#"wfb_tun

Rust UDP/TUN bridge compatible with WFB-ng's length-prefixed tunnel payloads.

Usage:
  wfb_tun [-t tun_name] [-a tun_addr] [-c peer_addr] [-u peer_port] [-l listen_port] [-T agg_timeout_ms]

Defaults:
  tun_name=wfb-tun tun_addr=10.5.0.2/24 peer=127.0.0.1:5801 listen_port=5800 agg_timeout_ms=5"#
        );
    }
}
