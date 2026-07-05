use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use openipc_core::realtek::{RxPacketType, DEFAULT_RX_TRANSFER_SIZE};
use openipc_core::{
    parse_rx_aggregate_with_kind, PayloadPipeline, PayloadPipelineEvent, RxDescriptorKind,
};
use openipc_rtl88xx::ChipFamily;

#[path = "../common.rs"]
mod common;

use common::{
    channel_id_from_parts, frame_layout, load_rx_keypair, next_arg, open_radio,
    parse_common_radio_option, parse_u16, parse_u32, parse_u64, parse_u8, CliResult,
    RadioDeviceConfig,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> CliResult<()> {
    let config = RxConfig::parse(std::env::args().skip(1))?;
    run_rx(config)
}

#[derive(Debug, Clone)]
struct RxConfig {
    key_path: PathBuf,
    link_id: u32,
    radio_port: u8,
    minimum_epoch: u64,
    client: SocketAddr,
    log_interval: Duration,
    max_transfers: Option<u64>,
    rx_urbs: usize,
    radio_device: RadioDeviceConfig,
}

impl RxConfig {
    fn parse(args: impl Iterator<Item = String>) -> CliResult<Self> {
        let mut config = Self {
            key_path: PathBuf::from("rx.key"),
            link_id: 0,
            radio_port: 0,
            minimum_epoch: 0,
            client: "127.0.0.1:5600".parse()?,
            log_interval: Duration::from_millis(1000),
            max_transfers: None,
            rx_urbs: 4,
            radio_device: RadioDeviceConfig::default(),
        };

        let mut client_addr = "127.0.0.1".to_owned();
        let mut client_port = 5600u16;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            if parse_common_radio_option(&arg, &mut args, &mut config.radio_device)? {
                continue;
            }
            match arg.as_str() {
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "-K" | "--key" => config.key_path = PathBuf::from(next_arg(&mut args, "-K")?),
                "-c" => client_addr = next_arg(&mut args, "-c")?,
                "-u" => client_port = parse_u16(&next_arg(&mut args, "-u")?)?,
                "-U" => {
                    return Err(
                        "Unix socket output is not implemented in the Rust userland RX".into(),
                    )
                }
                "-p" => config.radio_port = parse_u8(&next_arg(&mut args, "-p")?)?,
                "-i" => config.link_id = parse_u32(&next_arg(&mut args, "-i")?)? & 0x00ff_ffff,
                "-e" => config.minimum_epoch = parse_u64(&next_arg(&mut args, "-e")?)?,
                "-l" => {
                    config.log_interval =
                        Duration::from_millis(parse_u64(&next_arg(&mut args, "-l")?)?);
                }
                "-R" | "-s" => {
                    let _ = next_arg(&mut args, &arg)?;
                }
                "--max-transfers" => {
                    config.max_transfers =
                        Some(parse_u64(&next_arg(&mut args, "--max-transfers")?)?);
                }
                "--rx-urbs" => {
                    config.rx_urbs = parse_u64(&next_arg(&mut args, "--rx-urbs")?)? as usize;
                    if config.rx_urbs == 0 {
                        return Err("--rx-urbs must be greater than zero".into());
                    }
                }
                "-f" | "-a" => {
                    return Err(
                        "forwarder/aggregator mode is not implemented; this Rust RX uses the Realtek USB adapter directly"
                            .into(),
                    );
                }
                other if other.starts_with('-') => {
                    return Err(format!("unknown option: {other}").into())
                }
                other => {
                    eprintln!("ignoring kernel interface argument '{other}' in userland USB mode")
                }
            }
        }
        config.client = format!("{client_addr}:{client_port}").parse()?;
        Ok(config)
    }
}

#[derive(Default)]
struct RxStats {
    transfers: u64,
    rx_packets: u64,
    payloads: u64,
    bytes: u64,
    sessions: u64,
    dropped: u64,
    ignored: u64,
    parse_errors: u64,
}

fn run_rx(config: RxConfig) -> CliResult<()> {
    let opened = open_radio(&config.radio_device)?;
    let mut ep_in = opened.device.bulk_in_endpoint()?;
    let channel = channel_id_from_parts(config.link_id, config.radio_port);
    let keypair = load_rx_keypair(&config.key_path)?;
    let mut pipeline =
        PayloadPipeline::with_keypair(channel, frame_layout(), keypair, config.minimum_epoch)?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let mut stats = RxStats::default();
    let mut last_log = Instant::now();
    let descriptor = rx_descriptor_kind(opened.chip_family);

    eprintln!(
        "wfb_rx: channel=0x{:08x} output={} key={}",
        channel.raw(),
        config.client,
        config.key_path.display()
    );

    while ep_in.pending() < config.rx_urbs {
        let buffer = ep_in.allocate(DEFAULT_RX_TRANSFER_SIZE);
        ep_in.submit(buffer);
    }

    loop {
        if let Some(max) = config.max_transfers {
            if stats.transfers >= max {
                break;
            }
        }

        let Some(completion) = ep_in.wait_next_complete(Duration::from_millis(1000)) else {
            log_stats(&stats);
            continue;
        };
        let actual_len = completion.actual_len;
        if let Err(err) = completion.status {
            eprintln!("bulk IN transfer failed: {err}");
            ep_in.submit(completion.buffer);
            continue;
        }

        let bytes = &completion.buffer[..actual_len];
        process_transfer(
            bytes,
            descriptor,
            &mut pipeline,
            &socket,
            config.client,
            &mut stats,
        )?;
        ep_in.submit(completion.buffer);

        if last_log.elapsed() >= config.log_interval {
            log_stats(&stats);
            last_log = Instant::now();
        }
    }

    log_stats(&stats);
    Ok(())
}

fn process_transfer(
    bytes: &[u8],
    descriptor: RxDescriptorKind,
    pipeline: &mut PayloadPipeline,
    socket: &UdpSocket,
    client: SocketAddr,
    stats: &mut RxStats,
) -> CliResult<()> {
    stats.transfers += 1;
    let packets = match parse_rx_aggregate_with_kind(bytes, descriptor) {
        Ok(packets) => packets,
        Err(err) => {
            stats.parse_errors += 1;
            eprintln!("RX aggregate parse failed: {err}");
            return Ok(());
        }
    };
    stats.rx_packets += packets.len() as u64;
    for packet in packets {
        if packet.attrib.pkt_rpt_type != RxPacketType::NormalRx {
            stats.dropped += 1;
            continue;
        }
        if !pipeline.accepts_80211_frame(packet.data) {
            stats.ignored += 1;
            continue;
        }
        for event in pipeline.push_80211_frame(packet.data)? {
            match event {
                PayloadPipelineEvent::IgnoredFrame => stats.ignored += 1,
                PayloadPipelineEvent::SessionEstablished {
                    epoch,
                    fec_k,
                    fec_n,
                } => {
                    stats.sessions += 1;
                    eprintln!("WFB session established epoch={epoch} fec={fec_k}/{fec_n}");
                }
                PayloadPipelineEvent::Payload(payload) => {
                    socket.send_to(&payload.data, client)?;
                    stats.payloads += 1;
                    stats.bytes += payload.data.len() as u64;
                }
            }
        }
    }
    Ok(())
}

fn rx_descriptor_kind(chip_family: ChipFamily) -> RxDescriptorKind {
    match chip_family {
        ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => RxDescriptorKind::Jaguar2,
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => RxDescriptorKind::Jaguar3,
        ChipFamily::Rtl8812 | ChipFamily::Rtl8814 | ChipFamily::Rtl8821 => {
            RxDescriptorKind::Jaguar1
        }
    }
}

fn log_stats(stats: &RxStats) {
    eprintln!(
        "transfers={} rx_packets={} payloads={} bytes={} sessions={} dropped={} ignored={} parse_errors={}",
        stats.transfers,
        stats.rx_packets,
        stats.payloads,
        stats.bytes,
        stats.sessions,
        stats.dropped,
        stats.ignored,
        stats.parse_errors
    );
}

fn print_help() {
    println!(
        r#"wfb_rx

Rust userland WFB receiver using openipc-rtl88xx instead of pcap/kernel monitor mode.

Usage:
  wfb_rx [-K rx.key] [-c addr] [-u port] [-p radio_port] [-i link_id] [radio options]

Defaults:
  key=rx.key output=127.0.0.1:5600 link_id=0 radio_port=0

Radio options:
  {}"#,
        common::usage_common_radio()
    );
}
