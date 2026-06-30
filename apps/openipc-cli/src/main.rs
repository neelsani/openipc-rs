use std::env;
use std::fs;
use std::io::{self, Write};
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nusb::transfer::{Bulk, Out};
use openipc_core::channel::DEFAULT_LINK_ID;
use openipc_core::realtek::{parse_rx_aggregate, RxPacketType};
use openipc_core::realtek::{RxPacketAttrib, DEFAULT_RX_TRANSFER_SIZE};
use openipc_core::{
    AdaptiveLinkSender, ChannelId, FecCounters, FrameLayout, PayloadRouteId, RadioPort,
    ReceiverBatchOptions, ReceiverRuntime, WfbKeypair, WfbTxKeypair,
};
use openipc_rtl88xx::{
    list_devices, list_supported_devices, ChannelWidth, ChipFamily, DriverOptions,
    Firmware8814Mode, MonitorOptions, RadioConfig, RealtekDevice, RealtekTxOptions,
};

const VIDEO_ROUTE_ID: PayloadRouteId = PayloadRouteId::new(1);
const DEFAULT_KEY_SLOT: u64 = 0;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None | Some("help") | Some("--help") | Some("-h") => {
            print_help();
        }
        Some("list") => {
            for dev in list_devices()? {
                let marker = if dev.supported { "*" } else { " " };
                let product = dev.product.as_deref().unwrap_or("unknown product");
                println!(
                    "{marker} {:04x}:{:04x} bus={} addr={} ports={:?} {}",
                    dev.vendor_id,
                    dev.product_id,
                    dev.bus_id,
                    dev.device_address,
                    dev.port_chain,
                    product
                );
            }
        }
        Some("list-supported") => {
            for dev in list_supported_devices()? {
                let product = dev.product.as_deref().unwrap_or("unknown product");
                println!(
                    "{:04x}:{:04x} bus={} addr={} ports={:?} {}",
                    dev.vendor_id,
                    dev.product_id,
                    dev.bus_id,
                    dev.device_address,
                    dev.port_chain,
                    product
                );
            }
        }
        Some("probe") => {
            let mut options = DriverOptions::from_env();
            options.initialize_hardware = false;
            parse_driver_options(args, &mut options)?;
            let dev = RealtekDevice::open_first(options)?;
            let chip = dev.probe_chip()?;
            println!(
                "claimed Realtek interface; chip={} rf_paths={} cut={} speed={:?} bulk_in=0x{:02x} bulk_out=0x{:02x}",
                chip.family.name(),
                chip.total_rf_paths(),
                chip.cut_version,
                dev.device_speed(),
                dev.bulk_in_ep,
                dev.bulk_out_ep
            );
            match dev.read_u8(0x0000) {
                Ok(value) => println!("register 0x0000 = 0x{value:02x}"),
                Err(err) => println!("register probe failed: {err}"),
            }
        }
        Some("parse-aggregate") => {
            let path = args
                .next()
                .map(PathBuf::from)
                .ok_or("parse-aggregate requires a binary transfer file")?;
            let bytes = fs::read(&path)?;
            let packets = parse_rx_aggregate(&bytes)?;
            println!("{} packets", packets.len());
            for (idx, packet) in packets.iter().enumerate() {
                println!(
                    "#{idx}: len={} seq={} rate={} crc={} icv={}",
                    packet.data.len(),
                    packet.attrib.seq_num,
                    packet.attrib.data_rate,
                    packet.attrib.crc_err,
                    packet.attrib.icv_err
                );
            }
        }
        Some("decode-aggregate") => {
            let path = args
                .next()
                .map(PathBuf::from)
                .ok_or("decode-aggregate requires a binary transfer file")?;
            let config = RecvConfig::parse(args)?;
            let bytes = fs::read(&path)?;
            let mut receiver = config.receiver_runtime()?;
            let mut sinks = config.sinks()?;
            let mut stats = StreamStats::default();
            process_rx_transfer(
                &bytes,
                &mut receiver,
                &mut sinks,
                &mut stats,
                None,
                0,
                config.monitor_options.accept_bad_fcs,
            )?;
            eprintln!("{}", stats.summary());
        }
        Some("recv") => {
            let config = RecvConfig::parse(args)?;
            run_recv(config)?;
        }
        Some(other) => {
            return Err(format!("unknown command: {other}").into());
        }
    }
    Ok(())
}

fn print_help() {
    println!(
        r#"openipc-rs

commands:
  list                         list USB devices visible to nusb
  list-supported               list recognized Realtek rtl88xx adapters
  probe [usb options]          open, claim, and identify an adapter
  parse-aggregate <file>       parse a binary Realtek RX bulk transfer
  decode-aggregate <file> ...  decrypt/decode one captured RX transfer
  recv ...                     initialize monitor mode and receive from an adapter

usb options:
  --vid <hex|dec>              target USB vendor id
  --pid <hex|dec>              target USB product id
  --tx-ep <hex|dec>            override selected bulk-OUT endpoint
  --skip-reset                 do not USB-reset the adapter before claiming it

recv/decode options:
  --key <gs.key>               WFB keypair file: rx secret key + tx public key
  --out <file|->               write Annex-B H.264/H.265 frames; '-' means stdout
  --rtp-udp <host:port>        mirror recovered RTP packets to UDP
  --channel-id <id>            channel id as decimal or 0x-prefixed hex
  --epoch <n>                  minimum accepted WFB session epoch
  --max-transfers <n>          stop after n USB transfers
  --rx-urbs <n>                number of pending bulk IN reads, default 4
  --no-init                    skip Realtek monitor-mode initialization
  --rf-channel <n>             WiFi channel for monitor mode, default 36
  --rf-width <20|40|80>        channel width, default 20
  --rf-offset <n>              secondary-channel offset, default 0
  --accept-bad-fcs             ask the chip to pass CRC/ICV-bad frames
  --skip-txpwr                 skip TX-power table writes during channel set
  --force-iqk                  run IQK on RTL8814 as well as RTL8812
  --disable-iqk                skip IQK even where normally armed
  --fwdl-8814 <kernel|rtw88>   RTL8814 firmware download path
  --fwdl-8814-chunk <n>        RTL8814 kernel firmware chunk size, 64..4096
  --tx-legacy-8812-desc        use legacy 8812 TX descriptor shape on RTL8814
  --adaptive-link              send adaptive-link feedback on tunnel TX port 0xa0
  --alink-key <tx.key>         uplink keypair; defaults to --key
  --alink-epoch <n>            uplink WFB session epoch, default 0
  --alink-fec <k:n>            uplink FEC parameters, default 1:5
  --alink-tx-power <0..63>     force adaptive-link uplink TXAGC index

devourer-compatible env:
  DEVOURER_VID DEVOURER_PID DEVOURER_SKIP_RESET DEVOURER_TX_EP
  DEVOURER_SKIP_TXPWR DEVOURER_FORCE_IQK DEVOURER_DISABLE_IQK
  DEVOURER_8814_FWDL DEVOURER_8814_FWDL_CHUNK
  DEVOURER_TX_LEGACY_8812_DESC"#
    );
}

#[derive(Debug, Clone)]
struct RecvConfig {
    key_path: PathBuf,
    out_path: Option<PathBuf>,
    stdout_out: bool,
    rtp_udp: Option<SocketAddr>,
    channel_id: ChannelId,
    minimum_epoch: u64,
    max_transfers: Option<u64>,
    rx_urbs: usize,
    initialize_hardware: bool,
    driver_options: DriverOptions,
    monitor_options: MonitorOptions,
    tx_legacy_8812_descriptor: bool,
    radio: RadioConfig,
    adaptive_link: bool,
    alink_key_path: Option<PathBuf>,
    alink_epoch: u64,
    alink_fec_k: usize,
    alink_fec_n: usize,
    alink_tx_power: Option<u8>,
}

impl RecvConfig {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut key_path = None;
        let mut out_path = None;
        let mut stdout_out = false;
        let mut rtp_udp = None;
        let mut channel_id =
            ChannelId::from_link_port(DEFAULT_LINK_ID, openipc_core::RadioPort::Video);
        let mut minimum_epoch = 0u64;
        let mut max_transfers = None;
        let mut rx_urbs = 4usize;
        let mut initialize_hardware = true;
        let mut driver_options = DriverOptions::from_env();
        let mut monitor_options = MonitorOptions::from_env();
        let mut tx_legacy_8812_descriptor = env::var_os("DEVOURER_TX_LEGACY_8812_DESC").is_some();
        let mut radio = RadioConfig::default();
        let mut adaptive_link = false;
        let mut alink_key_path = None;
        let mut alink_epoch = 0u64;
        let mut alink_fec_k = 1usize;
        let mut alink_fec_n = 5usize;
        let mut alink_tx_power = None;

        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--key" => key_path = Some(PathBuf::from(next_arg(&mut args, "--key")?)),
                "--out" => {
                    let value = next_arg(&mut args, "--out")?;
                    if value == "-" {
                        stdout_out = true;
                    } else {
                        out_path = Some(PathBuf::from(value));
                    }
                }
                "--rtp-udp" => {
                    rtp_udp = Some(next_arg(&mut args, "--rtp-udp")?.parse()?);
                }
                "--channel-id" => {
                    channel_id = ChannelId::new(parse_u32(&next_arg(&mut args, "--channel-id")?)?);
                }
                "--epoch" => {
                    minimum_epoch = parse_u64(&next_arg(&mut args, "--epoch")?)?;
                }
                "--max-transfers" => {
                    max_transfers = Some(parse_u64(&next_arg(&mut args, "--max-transfers")?)?);
                }
                "--rx-urbs" => {
                    rx_urbs = parse_u64(&next_arg(&mut args, "--rx-urbs")?)? as usize;
                    if rx_urbs == 0 {
                        return Err("--rx-urbs must be greater than zero".into());
                    }
                }
                "--skip-reset" => driver_options.skip_reset = true,
                "--vid" => {
                    driver_options.target_vendor_id =
                        Some(parse_u16(&next_arg(&mut args, "--vid")?)?);
                }
                "--pid" => {
                    driver_options.target_product_id =
                        Some(parse_u16(&next_arg(&mut args, "--pid")?)?);
                }
                "--tx-ep" => {
                    driver_options.tx_endpoint_override =
                        Some(parse_u8(&next_arg(&mut args, "--tx-ep")?)?);
                }
                "--no-init" => initialize_hardware = false,
                "--accept-bad-fcs" => monitor_options.accept_bad_fcs = true,
                "--skip-txpwr" => monitor_options.skip_tx_power = true,
                "--force-iqk" => monitor_options.force_iqk = true,
                "--disable-iqk" => monitor_options.disable_iqk = true,
                "--fwdl-8814" => {
                    let mode = next_arg(&mut args, "--fwdl-8814")?;
                    monitor_options.firmware_8814_mode = Firmware8814Mode::from_env_value(&mode)
                        .ok_or_else(|| {
                            format!(
                                "unsupported --fwdl-8814 value {mode}; expected kernel or rtw88"
                            )
                        })?;
                }
                "--fwdl-8814-chunk" => {
                    monitor_options.firmware_8814_chunk =
                        Some(parse_u64(&next_arg(&mut args, "--fwdl-8814-chunk")?)? as usize);
                }
                "--tx-legacy-8812-desc" => tx_legacy_8812_descriptor = true,
                "--rf-channel" => {
                    radio.channel = parse_u8(&next_arg(&mut args, "--rf-channel")?)?;
                }
                "--rf-width" => {
                    radio.channel_width = parse_channel_width(&next_arg(&mut args, "--rf-width")?)?;
                }
                "--rf-offset" => {
                    radio.channel_offset = parse_u8(&next_arg(&mut args, "--rf-offset")?)?;
                }
                "--adaptive-link" => adaptive_link = true,
                "--alink-key" => {
                    alink_key_path = Some(PathBuf::from(next_arg(&mut args, "--alink-key")?));
                }
                "--alink-epoch" => {
                    alink_epoch = parse_u64(&next_arg(&mut args, "--alink-epoch")?)?;
                }
                "--alink-fec" => {
                    let value = next_arg(&mut args, "--alink-fec")?;
                    let (k, n) = parse_fec_pair(&value)?;
                    alink_fec_k = k;
                    alink_fec_n = n;
                }
                "--alink-tx-power" => {
                    alink_tx_power = Some(parse_u8(&next_arg(&mut args, "--alink-tx-power")?)?);
                }
                other => return Err(format!("unknown recv/decode option: {other}").into()),
            }
        }

        Ok(Self {
            key_path: key_path.ok_or("--key <gs.key> is required")?,
            out_path,
            stdout_out,
            rtp_udp,
            channel_id,
            minimum_epoch,
            max_transfers,
            rx_urbs,
            initialize_hardware,
            driver_options,
            monitor_options,
            tx_legacy_8812_descriptor,
            radio,
            adaptive_link,
            alink_key_path,
            alink_epoch,
            alink_fec_k,
            alink_fec_n,
            alink_tx_power,
        })
    }

    fn receiver_runtime(&self) -> Result<ReceiverRuntime, Box<dyn std::error::Error>> {
        let keypair = WfbKeypair::from_bytes(&fs::read(&self.key_path)?)?;
        Ok(ReceiverRuntime::with_keyed_video_route(
            FrameLayout::WithFcs,
            VIDEO_ROUTE_ID,
            self.channel_id,
            DEFAULT_KEY_SLOT,
            keypair,
            self.minimum_epoch,
        )?)
    }

    fn sinks(&self) -> Result<StreamSinks, Box<dyn std::error::Error>> {
        let video: Box<dyn Write> = if self.stdout_out {
            Box::new(io::stdout())
        } else if let Some(path) = &self.out_path {
            Box::new(fs::File::create(path)?)
        } else {
            Box::new(io::sink())
        };

        let rtp = if let Some(dest) = self.rtp_udp {
            Some((UdpSocket::bind("0.0.0.0:0")?, dest))
        } else {
            None
        };

        Ok(StreamSinks { video, rtp })
    }
}

struct StreamSinks {
    video: Box<dyn Write>,
    rtp: Option<(UdpSocket, SocketAddr)>,
}

#[derive(Default)]
struct StreamStats {
    transfers: u64,
    rx_packets: u64,
    accepted_wifi_frames: u64,
    rtp_packets: u64,
    video_frames: u64,
    ignored_frames: u64,
    bad_transfers: u64,
    adaptive_tx_frames: u64,
    adaptive_tx_errors: u64,
}

impl StreamStats {
    fn summary(&self) -> String {
        format!(
            "transfers={} rx_packets={} wifi_frames={} rtp_packets={} video_frames={} ignored={} bad_transfers={} alink_tx_frames={} alink_tx_errors={}",
            self.transfers,
            self.rx_packets,
            self.accepted_wifi_frames,
            self.rtp_packets,
            self.video_frames,
            self.ignored_frames,
            self.bad_transfers,
            self.adaptive_tx_frames,
            self.adaptive_tx_errors
        )
    }
}

struct AdaptiveRuntime {
    sender: AdaptiveLinkSender,
    last_counters: FecCounters,
    tx_options: RealtekTxOptions,
}

impl AdaptiveRuntime {
    fn record_rx(&mut self, now_ms: u64, attrib: &RxPacketAttrib) {
        self.sender.record_rx_paths(now_ms, attrib.rssi, attrib.snr);
    }

    fn record_pipeline(&mut self, now_ms: u64, counters: FecCounters) {
        let total = counters
            .total_packets
            .saturating_sub(self.last_counters.total_packets);
        let recovered = counters
            .recovered_packets
            .saturating_sub(self.last_counters.recovered_packets);
        let lost = counters
            .lost_packets
            .saturating_sub(self.last_counters.lost_packets);
        self.last_counters = counters;
        self.sender.record_fec(
            now_ms,
            total.min(u32::MAX as u64) as u32,
            recovered.min(u32::MAX as u64) as u32,
            lost.min(u32::MAX as u64) as u32,
        );
    }

    fn tick(
        &mut self,
        now_ms: u64,
        ep_out: &mut nusb::Endpoint<Bulk, Out>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let frames = self.sender.tick(now_ms)?;
        let count = frames.len();
        for frame in frames {
            RealtekDevice::send_packet_on(ep_out, &frame, self.tx_options)?;
        }
        Ok(count)
    }
}

impl RecvConfig {
    fn adaptive_runtime(
        &self,
        counters: FecCounters,
        chip_family: ChipFamily,
    ) -> Result<AdaptiveRuntime, Box<dyn std::error::Error>> {
        let key_path = self.alink_key_path.as_ref().unwrap_or(&self.key_path);
        let keypair = WfbTxKeypair::from_bytes(&fs::read(key_path)?)?;
        let link_id = self.channel_id.raw() >> 8;
        Ok(AdaptiveRuntime {
            sender: AdaptiveLinkSender::new(
                link_id,
                keypair,
                self.alink_epoch,
                self.alink_fec_k,
                self.alink_fec_n,
            )?,
            last_counters: counters,
            tx_options: RealtekTxOptions {
                current_channel: self.radio.channel,
                is_8814a: chip_family == ChipFamily::Rtl8814,
                legacy_8812_descriptor: self.tx_legacy_8812_descriptor,
                ..RealtekTxOptions::default()
            },
        })
    }
}

fn run_recv(config: RecvConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut driver_options = config.driver_options;
    driver_options.initialize_hardware = config.initialize_hardware;
    let device = RealtekDevice::open_first(driver_options)?;
    let chip = device.probe_chip()?;
    eprintln!(
        "claimed Realtek adapter speed={:?} bulk_in=0x{:02x} bulk_out=0x{:02x}",
        device.device_speed(),
        device.bulk_in_ep,
        device.bulk_out_ep
    );

    if config.initialize_hardware {
        let report =
            device.initialize_monitor_with_options(config.radio, config.monitor_options)?;
        eprintln!(
            "Realtek init: chip={} rf_paths={} cut={} status={:?} firmware_downloaded={}",
            report.chip.family.name(),
            report.chip.total_rf_paths(),
            report.chip.cut_version,
            report.status,
            report.firmware_downloaded
        );
    } else {
        eprintln!("Realtek init skipped by --no-init");
    }

    let mut ep_in = device.bulk_in_endpoint()?;
    let mut receiver = config.receiver_runtime()?;
    let mut sinks = config.sinks()?;
    let mut stats = StreamStats::default();
    let mut ep_out = if config.adaptive_link {
        Some(device.bulk_out_endpoint()?)
    } else {
        None
    };
    let mut adaptive = if config.adaptive_link {
        if let Some(power) = config.alink_tx_power {
            device.set_tx_power_override(config.radio.channel, power)?;
            eprintln!("adaptive link TX power override applied: txagc={power}");
        }
        eprintln!(
            "adaptive link enabled: uplink_channel=0x{:08x} fec={}:{}",
            ChannelId::from_link_port(config.channel_id.raw() >> 8, RadioPort::TunnelTx).raw(),
            config.alink_fec_k,
            config.alink_fec_n
        );
        Some(config.adaptive_runtime(receiver.video_fec_counters(), chip.family)?)
    } else {
        None
    };

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
            let now_ms = unix_time_ms();
            tick_adaptive(&mut adaptive, ep_out.as_mut(), now_ms, &mut stats);
            eprintln!("{}", stats.summary());
            continue;
        };

        let actual_len = completion.actual_len;
        if let Err(err) = completion.status {
            eprintln!("bulk IN transfer failed: {err}");
            ep_in.submit(completion.buffer);
            continue;
        }

        {
            let bytes = &completion.buffer[..actual_len];
            let now_ms = unix_time_ms();
            process_rx_transfer(
                bytes,
                &mut receiver,
                &mut sinks,
                &mut stats,
                adaptive.as_mut(),
                now_ms,
                config.monitor_options.accept_bad_fcs,
            )?;
            if let Some(runtime) = adaptive.as_mut() {
                runtime.record_pipeline(now_ms, receiver.video_fec_counters());
            }
            tick_adaptive(&mut adaptive, ep_out.as_mut(), now_ms, &mut stats);
        }
        ep_in.submit(completion.buffer);
    }

    sinks.video.flush()?;
    eprintln!("{}", stats.summary());
    Ok(())
}

fn process_rx_transfer(
    bytes: &[u8],
    receiver: &mut ReceiverRuntime,
    sinks: &mut StreamSinks,
    stats: &mut StreamStats,
    mut adaptive: Option<&mut AdaptiveRuntime>,
    now_ms: u64,
    accept_bad_fcs: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    stats.transfers += 1;
    let packets = match parse_rx_aggregate(bytes) {
        Ok(packets) => packets,
        Err(err) => {
            stats.bad_transfers += 1;
            eprintln!("RX aggregate parse failed: {err}");
            return Ok(());
        }
    };
    stats.rx_packets += packets.len() as u64;

    for packet in &packets {
        if packet.attrib.pkt_rpt_type != RxPacketType::NormalRx {
            continue;
        }
        if !accept_bad_fcs && (packet.attrib.crc_err || packet.attrib.icv_err) {
            continue;
        }
        if receiver.accepts_video_frame(packet.data) {
            if let Some(runtime) = adaptive.as_deref_mut() {
                runtime.record_rx(now_ms, &packet.attrib);
            }
        }
    }

    let raw_payload_routes = if sinks.rtp.is_some() {
        vec![VIDEO_ROUTE_ID]
    } else {
        Vec::new()
    };
    let batch = receiver.push_rx_packets(
        packets,
        &ReceiverBatchOptions {
            accept_corrupted: accept_bad_fcs,
            raw_payload_routes,
        },
    );

    stats.accepted_wifi_frames += batch.counters.wfb_payloads as u64;
    stats.rtp_packets += batch.counters.rtp_packets as u64;
    stats.video_frames += batch.counters.video_frames as u64;
    stats.ignored_frames += batch.counters.ignored_frames as u64;
    for payload in batch.raw_payloads {
        if let Some((socket, dest)) = &sinks.rtp {
            socket.send_to(&payload.data, dest)?;
        }
    }
    for frame in batch.frames {
        sinks.video.write_all(&frame.data)?;
    }
    Ok(())
}

fn tick_adaptive(
    adaptive: &mut Option<AdaptiveRuntime>,
    ep_out: Option<&mut nusb::Endpoint<Bulk, Out>>,
    now_ms: u64,
    stats: &mut StreamStats,
) {
    let (Some(runtime), Some(ep_out)) = (adaptive.as_mut(), ep_out) else {
        return;
    };
    match runtime.tick(now_ms, ep_out) {
        Ok(frames) => stats.adaptive_tx_frames += frames as u64,
        Err(err) => {
            stats.adaptive_tx_errors += 1;
            eprintln!("adaptive link TX failed: {err}");
        }
    }
}

fn next_arg(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    option: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value").into())
}

fn parse_driver_options(
    args: impl Iterator<Item = String>,
    options: &mut DriverOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--skip-reset" => options.skip_reset = true,
            "--vid" => options.target_vendor_id = Some(parse_u16(&next_arg(&mut args, "--vid")?)?),
            "--pid" => options.target_product_id = Some(parse_u16(&next_arg(&mut args, "--pid")?)?),
            "--tx-ep" => {
                options.tx_endpoint_override = Some(parse_u8(&next_arg(&mut args, "--tx-ep")?)?)
            }
            other => return Err(format!("unknown probe option: {other}").into()),
        }
    }
    Ok(())
}

fn parse_u32(value: &str) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(
        if let Some(hex) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            u32::from_str_radix(hex, 16)?
        } else {
            value.parse()?
        },
    )
}

fn parse_u64(value: &str) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(
        if let Some(hex) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            u64::from_str_radix(hex, 16)?
        } else {
            value.parse()?
        },
    )
}

fn parse_u16(value: &str) -> Result<u16, Box<dyn std::error::Error>> {
    let parsed = parse_u32(value)?;
    Ok(u16::try_from(parsed).map_err(|_| format!("{value} is outside u16 range"))?)
}

fn parse_u8(value: &str) -> Result<u8, Box<dyn std::error::Error>> {
    let parsed = parse_u32(value)?;
    Ok(u8::try_from(parsed).map_err(|_| format!("{value} is outside u8 range"))?)
}

fn parse_fec_pair(value: &str) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let (k, n) = value
        .split_once(':')
        .ok_or("--alink-fec must use k:n format, for example 1:5")?;
    let k = parse_u64(k)? as usize;
    let n = parse_u64(n)? as usize;
    if k == 0 || n == 0 || k > n || n > 255 {
        return Err("--alink-fec requires 0 < k <= n <= 255".into());
    }
    Ok((k, n))
}

fn parse_channel_width(value: &str) -> Result<ChannelWidth, Box<dyn std::error::Error>> {
    match value {
        "20" => Ok(ChannelWidth::Mhz20),
        "40" => Ok(ChannelWidth::Mhz40),
        "80" => Ok(ChannelWidth::Mhz80),
        _ => Err(format!("unsupported channel width {value}; expected 20, 40, or 80").into()),
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
