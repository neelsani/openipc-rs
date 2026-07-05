use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use nusb::transfer::{Bulk, In, Out};
use openipc_core::{
    ieee80211::build_wfb_header_with_frame_type, radiotap::build_radiotap_header,
    try_parse_tx_mode_str, wfb::WFB_PACKET_FEC_ONLY, ChannelId, TxMode, TxRadioParams,
    WfbTransmitter, WfbTxKeypair, FRAME_TYPE_DATA,
};
use openipc_rtl88xx::{ChipFamily, Jaguar3PowerTrackingState, RealtekDevice, RealtekTxOptions};

#[path = "../common.rs"]
mod common;
#[path = "../tx_cmd_proto.rs"]
mod tx_cmd_proto;

use common::{
    channel_id_from_parts, load_tx_keypair, next_arg, open_radio, open_radios,
    parse_common_radio_option, parse_frame_type, parse_tx_mode_flags, parse_u16, parse_u32,
    parse_u64, parse_u8, radio_params_from_mode, tx_options, CliResult, RadioDeviceConfig,
};
use tx_cmd_proto::{CommandRequest, CommandResponse, FecSettings, RadioSettings};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> CliResult<()> {
    let config = TxConfig::parse(std::env::args().skip(1))?;
    run_tx(config)
}

#[derive(Debug, Clone)]
struct TxConfig {
    key_path: PathBuf,
    fec: FecSettings,
    udp_port: u16,
    link_id: u32,
    radio_port: u8,
    epoch: u64,
    frame_type: u8,
    radio_settings: RadioSettings,
    control_port: Option<u16>,
    log_interval: Duration,
    session_interval: Duration,
    fec_delay: Duration,
    fec_timeout: Option<Duration>,
    debug_port: Option<u16>,
    inject_retries: u32,
    inject_retry_delay: Duration,
    mirror: bool,
    max_packets: Option<u64>,
    tx_power: Option<u8>,
    mcs_sweep: Vec<(String, TxMode)>,
    mcs_step_interval: Duration,
    thermal_poll_interval: Option<Duration>,
    thermal_poll_configured: bool,
    radio_device: RadioDeviceConfig,
    output_names: Vec<String>,
}

impl TxConfig {
    fn parse(args: impl Iterator<Item = String>) -> CliResult<Self> {
        let mut config = Self {
            key_path: PathBuf::from("tx.key"),
            fec: FecSettings { k: 8, n: 12 },
            udp_port: 5600,
            link_id: 0,
            radio_port: 0,
            epoch: 0,
            frame_type: FRAME_TYPE_DATA,
            radio_settings: RadioSettings {
                stbc: 0,
                ldpc: false,
                short_gi: false,
                bandwidth: 20,
                mcs_index: 1,
                vht_mode: false,
                vht_nss: 1,
            },
            control_port: None,
            log_interval: Duration::from_millis(1000),
            session_interval: Duration::from_millis(1000),
            fec_delay: Duration::ZERO,
            fec_timeout: None,
            debug_port: None,
            inject_retries: 0,
            inject_retry_delay: Duration::from_micros(5000),
            mirror: false,
            max_packets: None,
            tx_power: None,
            mcs_sweep: Vec::new(),
            mcs_step_interval: Duration::from_millis(2_000),
            thermal_poll_interval: None,
            thermal_poll_configured: false,
            radio_device: RadioDeviceConfig::default(),
            output_names: Vec::new(),
        };

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
                "-k" => config.fec.k = parse_u8(&next_arg(&mut args, "-k")?)?,
                "-n" => config.fec.n = parse_u8(&next_arg(&mut args, "-n")?)?,
                "-u" => config.udp_port = parse_u16(&next_arg(&mut args, "-u")?)?,
                "-U" => {
                    return Err(
                        "Unix socket input is not implemented in the Rust userland TX".into(),
                    )
                }
                "-p" => config.radio_port = parse_u8(&next_arg(&mut args, "-p")?)?,
                "-i" => config.link_id = parse_u32(&next_arg(&mut args, "-i")?)? & 0x00ff_ffff,
                "-e" => config.epoch = parse_u64(&next_arg(&mut args, "-e")?)?,
                "-f" => config.frame_type = parse_frame_type(&next_arg(&mut args, "-f")?)?,
                "-B" => {
                    config.radio_settings.bandwidth = parse_u8(&next_arg(&mut args, "-B")?)?;
                    if config.radio_settings.bandwidth >= 80 {
                        config.radio_settings.vht_mode = true;
                    }
                }
                "-G" => {
                    let value = next_arg(&mut args, "-G")?;
                    config.radio_settings.short_gi =
                        value.starts_with('s') || value.starts_with('S');
                }
                "-S" => config.radio_settings.stbc = parse_u8(&next_arg(&mut args, "-S")?)?,
                "-L" => config.radio_settings.ldpc = parse_u8(&next_arg(&mut args, "-L")?)? != 0,
                "-M" => config.radio_settings.mcs_index = parse_u8(&next_arg(&mut args, "-M")?)?,
                "-N" => config.radio_settings.vht_nss = parse_u8(&next_arg(&mut args, "-N")?)?,
                "-V" => config.radio_settings.vht_mode = true,
                "-C" | "--control-port" => {
                    config.control_port = Some(parse_u16(&next_arg(&mut args, "-C")?)?);
                }
                "-F" => {
                    config.fec_delay =
                        Duration::from_micros(parse_u64(&next_arg(&mut args, "-F")?)?);
                }
                "-T" => {
                    let timeout_ms = parse_u64(&next_arg(&mut args, "-T")?)?;
                    config.fec_timeout =
                        (timeout_ms > 0).then_some(Duration::from_millis(timeout_ms));
                }
                "-D" => config.debug_port = Some(parse_u16(&next_arg(&mut args, "-D")?)?),
                "-J" => config.inject_retries = parse_u32(&next_arg(&mut args, "-J")?)?,
                "-E" => {
                    config.inject_retry_delay =
                        Duration::from_micros(parse_u64(&next_arg(&mut args, "-E")?)?);
                }
                "-m" => config.mirror = true,
                "-l" => {
                    config.log_interval =
                        Duration::from_millis(parse_u64(&next_arg(&mut args, "-l")?)?);
                }
                "--session-interval" => {
                    config.session_interval = Duration::from_millis(parse_u64(&next_arg(
                        &mut args,
                        "--session-interval",
                    )?)?);
                }
                "--max-packets" => {
                    config.max_packets = Some(parse_u64(&next_arg(&mut args, "--max-packets")?)?);
                }
                "--tx-power" => {
                    config.tx_power = Some(parse_u8(&next_arg(&mut args, "--tx-power")?)?)
                }
                "--mcs-sweep" => {
                    config.mcs_sweep = parse_mcs_sweep(&next_arg(&mut args, "--mcs-sweep")?)?;
                }
                "--mcs-step-ms" => {
                    let millis = parse_u64(&next_arg(&mut args, "--mcs-step-ms")?)?;
                    if millis == 0 {
                        return Err("--mcs-step-ms must be greater than zero".into());
                    }
                    config.mcs_step_interval = Duration::from_millis(millis);
                }
                "--thermal-poll-ms" => {
                    let millis = parse_u64(&next_arg(&mut args, "--thermal-poll-ms")?)?;
                    config.thermal_poll_interval =
                        (millis > 0).then_some(Duration::from_millis(millis));
                    config.thermal_poll_configured = true;
                }
                "-R" | "-s" | "-P" => {
                    let _ = next_arg(&mut args, &arg)?;
                }
                "-Q" => {}
                "-d" | "-I" => {
                    return Err(
                        "distributor/injector mode is not implemented; this Rust TX injects with the Realtek USB adapter directly"
                            .into(),
                    );
                }
                other if other.starts_with('-') => {
                    return Err(format!("unknown option: {other}").into())
                }
                other => config.output_names.push(other.to_owned()),
            }
        }
        if !config.fec.valid() {
            return Err("invalid FEC settings; require 1 <= k <= n <= 255".into());
        }
        if !config.mcs_sweep.is_empty() && !config.thermal_poll_configured {
            config.thermal_poll_interval = Some(Duration::from_secs(1));
        }
        Ok(config)
    }
}

fn parse_mcs_sweep(spec: &str) -> CliResult<Vec<(String, TxMode)>> {
    let mut modes = Vec::new();
    for raw in spec.split(',') {
        let label = raw.trim();
        if label.is_empty() {
            continue;
        }
        let mode = try_parse_tx_mode_str(label)
            .ok_or_else(|| format!("invalid TX mode in --mcs-sweep: {label}"))?;
        modes.push((label.to_owned(), mode));
    }
    if modes.is_empty() {
        return Err("--mcs-sweep requires at least one TX mode".into());
    }
    Ok(modes)
}

#[derive(Default)]
struct TxStats {
    input_packets: u64,
    input_bytes: u64,
    radio_packets: u64,
    radio_bytes: u64,
    session_packets: u64,
    control_packets: u64,
    control_errors: u64,
    fec_timeouts: u64,
    retry_attempts: u64,
    output_failures: u64,
}

struct TxState {
    keypair: WfbTxKeypair,
    channel: ChannelId,
    epoch: u64,
    fec: FecSettings,
    radio_settings: RadioSettings,
    frame_type: u8,
    transmitter: WfbTransmitter,
    source_fragments_in_block: u8,
    sequence_control: u16,
    mcs_sweep: Vec<(String, TxMode)>,
    mcs_step_interval: Duration,
    mcs_sweep_index: usize,
    next_mcs_step: Option<Duration>,
    swept_tx_mode: Option<TxMode>,
}

impl TxState {
    fn new(config: &TxConfig, keypair: WfbTxKeypair) -> CliResult<Self> {
        let channel = channel_id_from_parts(config.link_id, config.radio_port);
        let transmitter = WfbTransmitter::new(
            channel,
            keypair,
            config.epoch,
            usize::from(config.fec.k),
            usize::from(config.fec.n),
        )?;
        Ok(Self {
            keypair,
            channel,
            epoch: config.epoch,
            fec: config.fec,
            radio_settings: config.radio_settings,
            frame_type: config.frame_type,
            transmitter,
            source_fragments_in_block: 0,
            sequence_control: 0,
            mcs_sweep: config.mcs_sweep.clone(),
            mcs_step_interval: config.mcs_step_interval,
            mcs_sweep_index: 0,
            next_mcs_step: None,
            swept_tx_mode: None,
        })
    }

    fn reset_fec(&mut self, fec: FecSettings) -> CliResult<()> {
        if !fec.valid() {
            return Err("invalid FEC settings".into());
        }
        self.fec = fec;
        self.transmitter = WfbTransmitter::new(
            self.channel,
            self.keypair,
            self.epoch,
            usize::from(fec.k),
            usize::from(fec.n),
        )?;
        self.source_fragments_in_block = 0;
        Ok(())
    }

    fn tx_mode(&self) -> TxMode {
        if let Some(mode) = self.swept_tx_mode {
            return mode;
        }
        parse_tx_mode_flags(
            u16::from(self.radio_settings.bandwidth),
            self.radio_settings.short_gi,
            self.radio_settings.stbc != 0,
            self.radio_settings.ldpc,
            self.radio_settings.mcs_index,
            self.radio_settings.vht_nss,
            self.radio_settings.vht_mode,
        )
    }

    fn tick_mcs_sweep(&mut self, elapsed: Duration) -> Option<(String, TxMode)> {
        if self.mcs_sweep.is_empty()
            || self
                .next_mcs_step
                .is_some_and(|deadline| elapsed < deadline)
        {
            return None;
        }
        if self.next_mcs_step.is_some() {
            self.mcs_sweep_index = (self.mcs_sweep_index + 1) % self.mcs_sweep.len();
        }
        let (label, mode) = &self.mcs_sweep[self.mcs_sweep_index];
        self.swept_tx_mode = Some(*mode);
        self.next_mcs_step = elapsed.checked_add(self.mcs_step_interval);
        Some((label.clone(), *mode))
    }

    fn has_open_fec_block(&self) -> bool {
        self.source_fragments_in_block != 0
    }

    fn forwarder_packets_for_payload(
        &mut self,
        payload: &[u8],
        flags: u8,
    ) -> CliResult<Vec<Vec<u8>>> {
        if flags & WFB_PACKET_FEC_ONLY != 0 && !self.has_open_fec_block() {
            return Ok(Vec::new());
        }
        let packets = self
            .transmitter
            .forwarder_packets_for_payload(payload, flags)?;
        if !packets.is_empty() {
            self.source_fragments_in_block = self.source_fragments_in_block.saturating_add(1);
            if self.source_fragments_in_block >= self.fec.k {
                self.source_fragments_in_block = 0;
            }
        }
        Ok(packets)
    }

    fn forwarder_packets_for_fec_only(&mut self) -> CliResult<Vec<Vec<u8>>> {
        self.forwarder_packets_for_payload(&[], WFB_PACKET_FEC_ONLY)
    }

    fn radio_packet_for_forwarder_packet(
        &mut self,
        forwarder_packet: &[u8],
        params: TxRadioParams,
    ) -> Vec<u8> {
        let mut out = build_radiotap_header(params);
        let seq = self.sequence_control.to_le_bytes();
        out.extend_from_slice(&build_wfb_header_with_frame_type(
            self.channel,
            seq,
            params.frame_type,
        ));
        out.extend_from_slice(forwarder_packet);
        self.sequence_control = self.sequence_control.wrapping_add(16);
        out
    }
}

struct UsbOutput {
    device: RealtekDevice,
    chip_family: ChipFamily,
    ep_in: Option<nusb::Endpoint<Bulk, In>>,
    ep_out: nusb::Endpoint<Bulk, Out>,
    tx_options: RealtekTxOptions,
    last_jaguar3_tick: Instant,
    last_thermal_poll: Instant,
    jaguar3_power_tracking: Jaguar3PowerTrackingState,
}

struct DebugOutput {
    socket: UdpSocket,
    base_port: u16,
    output_count: usize,
}

enum TxOutput {
    Usb(Vec<UsbOutput>),
    Debug(DebugOutput),
}

impl TxOutput {
    fn output_count(&self) -> usize {
        match self {
            Self::Usb(outputs) => outputs.len(),
            Self::Debug(output) => output.output_count,
        }
    }

    fn service(&mut self, thermal_poll_interval: Option<Duration>) {
        let Self::Usb(outputs) = self else {
            return;
        };
        for output in outputs {
            if let Some(ep_in) = output.ep_in.as_mut() {
                while let Some(completion) = ep_in.wait_next_complete(Duration::ZERO) {
                    ep_in.submit(completion.buffer);
                }
            }
            if output.chip_family.is_jaguar3()
                && output.last_jaguar3_tick.elapsed() >= Duration::from_secs(2)
            {
                output.last_jaguar3_tick = Instant::now();
                if let Err(error) = output.device.run_jaguar3_coex_keepalive() {
                    eprintln!("Jaguar3 coex keepalive failed: {error}");
                }
                if let Err(error) = output
                    .device
                    .tick_jaguar3_power_tracking(&mut output.jaguar3_power_tracking)
                {
                    eprintln!("Jaguar3 thermal tracking failed: {error}");
                }
            }
            if let Some(interval) = thermal_poll_interval {
                if output.last_thermal_poll.elapsed() >= interval {
                    output.last_thermal_poll = Instant::now();
                    match output.device.read_thermal_status() {
                        Ok(status) => eprintln!(
                            "<wfb-rs-thermal>raw={} baseline={} delta={} valid={}",
                            status.raw,
                            status.baseline,
                            status.delta,
                            u8::from(status.valid)
                        ),
                        Err(error) => eprintln!("thermal polling failed: {error}"),
                    }
                }
            }
        }
    }

    fn shutdown(&mut self) {
        let Self::Usb(outputs) = self else {
            return;
        };
        for output in outputs {
            if let Err(error) = output.device.shutdown_monitor() {
                eprintln!("Realtek monitor shutdown failed: {error}");
            }
        }
    }
}

fn open_tx_output(config: &TxConfig) -> CliResult<TxOutput> {
    if let Some(base_port) = config.debug_port {
        if config.tx_power.is_some() {
            eprintln!("ignoring --tx-power in UDP debug mode");
        }
        let output_count = config.output_names.len().max(1);
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        eprintln!(
            "wfb_tx: UDP debug output to 127.0.0.1:{}..{} outputs={}",
            base_port,
            base_port.saturating_add(output_count.saturating_sub(1) as u16),
            output_count
        );
        return Ok(TxOutput::Debug(DebugOutput {
            socket,
            base_port,
            output_count,
        }));
    }

    let opened = if config.mirror {
        let radios = open_radios(&config.radio_device)?;
        if radios.len() == 1 {
            eprintln!("mirror mode requested but only one matching Realtek adapter was opened");
        }
        radios
    } else {
        vec![open_radio(&config.radio_device)?]
    };

    let mut outputs = Vec::with_capacity(opened.len());
    for opened in opened {
        if let Some(power) = config.tx_power {
            opened
                .device
                .set_tx_power_override(config.radio_device.radio.channel, power)?;
            eprintln!("TX power override applied: txagc={power}");
        }
        let mut ep_in = if opened.chip_family.is_jaguar3() {
            opened.device.prepare_transmit_only()?;
            Some(opened.device.bulk_in_endpoint()?)
        } else {
            None
        };
        if let Some(endpoint) = ep_in.as_mut() {
            while endpoint.pending() < 2 {
                endpoint.submit(endpoint.allocate(16 * 1024));
            }
        }
        let ep_out = opened.device.bulk_out_endpoint()?;
        let tx_options = tx_options(&config.radio_device, opened.chip_family);
        outputs.push(UsbOutput {
            device: opened.device,
            chip_family: opened.chip_family,
            ep_in,
            ep_out,
            tx_options,
            last_jaguar3_tick: Instant::now(),
            last_thermal_poll: Instant::now(),
            jaguar3_power_tracking: Jaguar3PowerTrackingState::default(),
        });
    }

    Ok(TxOutput::Usb(outputs))
}

fn run_tx(config: TxConfig) -> CliResult<()> {
    let mut output = open_tx_output(&config)?;
    let keypair = load_tx_keypair(&config.key_path)?;
    let mut state = TxState::new(&config, keypair)?;
    let data_socket = UdpSocket::bind(("0.0.0.0", config.udp_port))?;
    data_socket.set_nonblocking(true)?;
    let control_socket = match config.control_port {
        Some(port) => {
            let socket = UdpSocket::bind(("127.0.0.1", port))?;
            socket.set_nonblocking(true)?;
            Some(socket)
        }
        None => None,
    };
    let mut stats = TxStats::default();
    let mut last_log = Instant::now();
    let mut last_session = Instant::now()
        .checked_sub(config.session_interval)
        .unwrap_or_else(Instant::now);
    let mut next_fec_flush = config.fec_timeout.map(|timeout| {
        Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now)
    });
    let started_at = Instant::now();

    eprintln!(
        "wfb_tx: listen=0.0.0.0:{} channel=0x{:08x} key={} fec={}/{} control={:?} outputs={} mirror={} debug={:?}",
        config.udp_port,
        state.channel.raw(),
        config.key_path.display(),
        config.fec.k,
        config.fec.n,
        config.control_port,
        output.output_count(),
        config.mirror,
        config.debug_port
    );

    let mut buf = vec![0u8; 2048];
    loop {
        output.service(config.thermal_poll_interval);
        if let Some((label, _mode)) = state.tick_mcs_sweep(started_at.elapsed()) {
            eprintln!(
                "<wfb-rs-mcs-sweep>mcs={} t_ms={}",
                label,
                started_at.elapsed().as_millis()
            );
        }
        if let Some(max) = config.max_packets {
            if stats.input_packets >= max {
                break;
            }
        }

        if last_session.elapsed() >= config.session_interval {
            send_session(&mut state, &mut output, &config, &mut stats)?;
            last_session = Instant::now();
        }

        if let Some(socket) = &control_socket {
            handle_control(socket, &mut state, &mut output, &config, &mut stats)?;
        }

        if let Some(deadline) = next_fec_flush {
            if Instant::now() >= deadline {
                let packets = state.forwarder_packets_for_fec_only()?;
                if !packets.is_empty() {
                    stats.fec_timeouts += 1;
                    emit_forwarder_packets(&mut state, &mut output, &config, &mut stats, packets)?;
                }
                next_fec_flush = config.fec_timeout.map(|timeout| {
                    Instant::now()
                        .checked_add(timeout)
                        .unwrap_or_else(Instant::now)
                });
            }
        }

        match data_socket.recv_from(&mut buf) {
            Ok((amount, _peer)) => {
                let payload = &buf[..amount];
                let packets = state.forwarder_packets_for_payload(payload, 0)?;
                emit_forwarder_packets(&mut state, &mut output, &config, &mut stats, packets)?;
                stats.input_packets += 1;
                stats.input_bytes += amount as u64;
                next_fec_flush = config.fec_timeout.map(|timeout| {
                    Instant::now()
                        .checked_add(timeout)
                        .unwrap_or_else(Instant::now)
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                // Do not add a millisecond of avoidable source-side queueing.
                // USB and socket work are already rate-limited by the device;
                // yielding lets a waiting packet be picked up immediately.
                std::thread::yield_now();
            }
            Err(err) => return Err(err.into()),
        }

        if last_log.elapsed() >= config.log_interval {
            log_stats(&stats);
            last_log = Instant::now();
        }
    }

    log_stats(&stats);
    output.shutdown();
    Ok(())
}

fn send_session(
    state: &mut TxState,
    output: &mut TxOutput,
    config: &TxConfig,
    stats: &mut TxStats,
) -> CliResult<()> {
    let packet = state.transmitter.session_forwarder_packet().to_vec();
    emit_forwarder_packet(state, output, config, stats, &packet)?;
    stats.session_packets += 1;
    Ok(())
}

fn handle_control(
    socket: &UdpSocket,
    state: &mut TxState,
    output: &mut TxOutput,
    config: &TxConfig,
    stats: &mut TxStats,
) -> CliResult<()> {
    let mut buf = [0u8; 128];
    loop {
        let (amount, peer) = match socket.recv_from(&mut buf) {
            Ok(value) => value,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        stats.control_packets += 1;
        let Some(request) = CommandRequest::parse(&buf[..amount]) else {
            stats.control_errors += 1;
            continue;
        };
        let response = apply_control_request(request, state, output, config, stats);
        socket.send_to(&response.encode(), peer)?;
    }
}

fn apply_control_request(
    request: CommandRequest,
    state: &mut TxState,
    output: &mut TxOutput,
    config: &TxConfig,
    stats: &mut TxStats,
) -> CommandResponse {
    match request {
        CommandRequest::SetFec { req_id_be, fec } => {
            if !fec.valid() {
                return CommandResponse::Ack {
                    req_id_be,
                    errno: 22,
                };
            }
            let errno = match close_fec_block_for_reconfig(state, output, config, stats) {
                Ok(()) => {
                    if state.reset_fec(fec).is_err() {
                        return CommandResponse::Ack {
                            req_id_be,
                            errno: 22,
                        };
                    }
                    for _ in 0..usize::from(fec.n.saturating_sub(fec.k) + 1) {
                        if send_session(state, output, config, stats).is_err() {
                            return CommandResponse::Ack {
                                req_id_be,
                                errno: 5,
                            };
                        }
                    }
                    eprintln!("session restarted with FEC {}/{}", fec.k, fec.n);
                    0
                }
                Err(_) => 5,
            };
            CommandResponse::Ack { req_id_be, errno }
        }
        CommandRequest::SetRadio { req_id_be, radio } => {
            state.radio_settings = radio;
            eprintln!(
                "radio updated stbc={} ldpc={} short_gi={} bandwidth={} mcs={} vht={} nss={}",
                radio.stbc,
                u8::from(radio.ldpc),
                u8::from(radio.short_gi),
                radio.bandwidth,
                radio.mcs_index,
                u8::from(radio.vht_mode),
                radio.vht_nss
            );
            CommandResponse::Ack {
                req_id_be,
                errno: 0,
            }
        }
        CommandRequest::GetFec { req_id_be } => CommandResponse::Fec {
            req_id_be,
            errno: 0,
            fec: state.fec,
        },
        CommandRequest::GetRadio { req_id_be } => CommandResponse::Radio {
            req_id_be,
            errno: 0,
            radio: state.radio_settings,
        },
    }
}

fn close_fec_block_for_reconfig(
    state: &mut TxState,
    output: &mut TxOutput,
    config: &TxConfig,
    stats: &mut TxStats,
) -> CliResult<()> {
    while state.has_open_fec_block() {
        let packets = state.forwarder_packets_for_fec_only()?;
        emit_forwarder_packets(state, output, config, stats, packets)?;
    }
    Ok(())
}

fn emit_forwarder_packets(
    state: &mut TxState,
    output: &mut TxOutput,
    config: &TxConfig,
    stats: &mut TxStats,
    packets: Vec<Vec<u8>>,
) -> CliResult<()> {
    for (idx, packet) in packets.into_iter().enumerate() {
        if idx > 0 && !config.fec_delay.is_zero() {
            std::thread::sleep(config.fec_delay);
        }
        emit_forwarder_packet(state, output, config, stats, &packet)?;
    }
    Ok(())
}

fn emit_forwarder_packet(
    state: &mut TxState,
    output: &mut TxOutput,
    config: &TxConfig,
    stats: &mut TxStats,
    packet: &[u8],
) -> CliResult<()> {
    match output {
        TxOutput::Usb(outputs) => {
            let params = radio_params_from_mode(state.tx_mode(), state.frame_type);
            let radio_packet = state.radio_packet_for_forwarder_packet(packet, params);
            send_radio_packet(outputs, &radio_packet, config, stats)?;
        }
        TxOutput::Debug(debug) => {
            send_debug_packet(debug, packet, config, stats)?;
        }
    }
    Ok(())
}

fn send_radio_packet(
    outputs: &mut [UsbOutput],
    radio_packet: &[u8],
    config: &TxConfig,
    stats: &mut TxStats,
) -> CliResult<()> {
    let output_count = if config.mirror { outputs.len() } else { 1 };
    let mut successes = 0usize;
    let mut first_error = None;

    for output in outputs.iter_mut().take(output_count) {
        match send_radio_packet_with_retry(output, radio_packet, config, stats) {
            Ok(written) => {
                successes += 1;
                stats.radio_packets += 1;
                stats.radio_bytes += written as u64;
            }
            Err(err) => {
                stats.output_failures += 1;
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }
    }

    if successes == 0 {
        return Err(first_error.unwrap_or_else(|| "no Realtek TX outputs are available".into()));
    }
    Ok(())
}

fn send_radio_packet_with_retry(
    output: &mut UsbOutput,
    radio_packet: &[u8],
    config: &TxConfig,
    stats: &mut TxStats,
) -> CliResult<usize> {
    let mut attempt = 0u32;
    loop {
        match RealtekDevice::send_packet_on(&mut output.ep_out, radio_packet, output.tx_options) {
            Ok(written) => return Ok(written),
            Err(err) if attempt < config.inject_retries => {
                attempt += 1;
                stats.retry_attempts += 1;
                if !config.inject_retry_delay.is_zero() {
                    std::thread::sleep(config.inject_retry_delay);
                }
                eprintln!(
                    "radio injection failed, retrying attempt {}/{}: {err}",
                    attempt, config.inject_retries
                );
            }
            Err(err) => return Err(err.into()),
        }
    }
}

fn send_debug_packet(
    output: &mut DebugOutput,
    packet: &[u8],
    config: &TxConfig,
    stats: &mut TxStats,
) -> CliResult<()> {
    let count = if config.mirror {
        output.output_count
    } else {
        1
    };
    for idx in 0..count {
        let port = output
            .base_port
            .checked_add(u16::try_from(idx).map_err(|_| "too many debug outputs")?)
            .ok_or("debug output port overflow")?;
        let mut frame = Vec::with_capacity(17 + packet.len());
        frame.extend_from_slice(&debug_forward_header(idx));
        frame.extend_from_slice(packet);
        let dest = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        output.socket.send_to(&frame, dest)?;
        stats.radio_packets += 1;
        stats.radio_bytes += frame.len() as u64;
    }
    Ok(())
}

fn debug_forward_header(output_idx: usize) -> [u8; 17] {
    let mut header = [0u8; 17];
    header[0] = output_idx as u8;
    header[1..5].fill(0xff);
    header[5..9].fill(i8::MIN as u8);
    header[9..13].fill(i8::MAX as u8);
    header[1] = output_idx as u8;
    header[5] = (-42i8) as u8;
    header[9] = (-70i8) as u8;
    header[13..15].copy_from_slice(&4321u16.to_be_bytes());
    header[15] = 1;
    header[16] = 20;
    header
}

fn log_stats(stats: &TxStats) {
    eprintln!(
        "input_packets={} input_bytes={} radio_packets={} radio_bytes={} sessions={} control={} control_errors={} fec_timeouts={} retries={} output_failures={}",
        stats.input_packets,
        stats.input_bytes,
        stats.radio_packets,
        stats.radio_bytes,
        stats.session_packets,
        stats.control_packets,
        stats.control_errors,
        stats.fec_timeouts,
        stats.retry_attempts,
        stats.output_failures
    );
}

fn print_help() {
    println!(
        r#"wfb_tx

Rust userland WFB transmitter using openipc-rtl88xx instead of kernel frame injection.

Usage:
  wfb_tx [-K tx.key] [-k RS_K] [-n RS_N] [-u udp_port] [-p radio_port] [-i link_id] [radio mode] [radio device options]

Common options:
  -K, --key <tx.key>          transmitter keypair, default tx.key
  -k <n> -n <n>               FEC source/total fragments, default 8/12
  -u <port>                   UDP input port, default 5600
  -p <port>                   WFB radio port, default 0
  -i <link_id>                24-bit WFB link id, default 0
  -e <epoch>                  WFB session epoch, default 0
  -f data|rts                 802.11 frame type, default data
  -C, --control-port <port>   enable wfb_tx_cmd control socket
  -F <usec>                   delay before each parity FEC fragment
  -T <msec>                   close partial FEC blocks after idle timeout
  -D <port>                   UDP debug output to 127.0.0.1:<port> instead of USB
  -J <count>                  retry failed radio injections this many times
  -E <usec>                   delay between injection retries, default 5000
  -m                          mirror each packet to every opened output
  --tx-power <0..127>         override TXAGC index (max 63 on Jaguar1)
  --mcs-sweep <modes>         cycle comma-separated modes, e.g. MCS0,MCS2,MCS4
  --mcs-step-ms <ms>          MCS sweep dwell time, default 2000
  --thermal-poll-ms <ms>      emit thermal samples; 0 disables polling

Radio mode:
  -B <20|40|80> -G short|long -S <stbc> -L <0|1> -M <mcs> -N <nss> -V

Radio device options:
  {}"#,
        common::usage_common_radio()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use openipc_core::TxModeKind;

    #[test]
    fn parses_devourer_style_mcs_sweep() {
        let modes = parse_mcs_sweep("MCS0, MCS4/SGI, VHT1SS_MCS7/40").unwrap();
        assert_eq!(modes.len(), 3);
        assert_eq!(modes[0].0, "MCS0");
        assert_eq!(modes[0].1.kind, TxModeKind::Ht);
        assert!(modes[1].1.short_gi);
        assert_eq!(modes[2].1.kind, TxModeKind::Vht);
        assert_eq!(modes[2].1.vht_mcs, 7);
    }

    #[test]
    fn rejects_invalid_mcs_sweep_entries() {
        assert!(parse_mcs_sweep("").is_err());
        assert!(parse_mcs_sweep("MCS0,MCS99").is_err());
        assert!(parse_mcs_sweep("MCS0/FAST").is_err());
    }
}
