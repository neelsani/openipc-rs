use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Mutex},
};

use openipc_core::{
    radiotap::TxRadioParams,
    wfb::{WfbError, MAX_PAYLOAD_SIZE},
    ChannelId, RadioPort, WfbTransmitter, WfbTxKeypair,
};

use crate::{frame_ip_packet, NetworkConfig, NetworkError, TunnelFramingError, UserspaceNetwork};

const DEFAULT_CONTROL_QUEUE_CAPACITY: usize = 128;
const DEFAULT_TUNNEL_QUEUE_CAPACITY: usize = 128;
const DEFAULT_SESSION_INTERVAL_MS: u64 = 1_000;
const DEFAULT_RETRY_BACKOFF_MS: u64 = 2;
const DEFAULT_FRAME_RETRIES: u8 = 2;

/// Priority class assigned to an outbound tunnel packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UplinkTrafficClass {
    /// Latency-sensitive packets emitted by userspace UDP/TCP, including
    /// adaptive-link feedback and SSH.
    Control,
    /// Packets read from an optional operating-system TUN interface.
    Tunnel,
}

impl fmt::Display for UplinkTrafficClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Control => "control",
            Self::Tunnel => "TUN",
        })
    }
}

/// Bounded scheduling and retry policy for [`UplinkEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UplinkEngineConfig {
    /// Maximum smoltcp-produced IP packets retained by the priority scheduler.
    pub control_queue_capacity: usize,
    /// Maximum operating-system TUN packets retained by the scheduler.
    pub tunnel_queue_capacity: usize,
    /// Largest collection of length-prefixed IP packets placed in one WFB payload.
    pub max_aggregate_bytes: usize,
    /// Number of retries after the first failed USB submission.
    pub max_frame_retries: u8,
    /// Base delay before retrying a failed frame. Each subsequent retry uses a
    /// linear multiple of this delay.
    pub retry_backoff_ms: u64,
    /// Interval at which a successfully delivered WFB session packet is refreshed.
    pub session_interval_ms: u64,
}

impl Default for UplinkEngineConfig {
    fn default() -> Self {
        Self {
            control_queue_capacity: DEFAULT_CONTROL_QUEUE_CAPACITY,
            tunnel_queue_capacity: DEFAULT_TUNNEL_QUEUE_CAPACITY,
            max_aggregate_bytes: MAX_PAYLOAD_SIZE,
            max_frame_retries: DEFAULT_FRAME_RETRIES,
            retry_backoff_ms: DEFAULT_RETRY_BACKOFF_MS,
            session_interval_ms: DEFAULT_SESSION_INTERVAL_MS,
        }
    }
}

/// Stable class for a failed platform TX completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxFailureKind {
    /// USB transfer timed out or was cancelled by a timeout.
    Timeout,
    /// The bulk-OUT endpoint stalled.
    Stall,
    /// A successful completion reported fewer bytes than were submitted.
    ShortWrite,
    /// The USB adapter disconnected.
    Disconnected,
    /// The platform TX queue or worker stopped.
    QueueClosed,
    /// Another transport error occurred.
    Other,
}

/// Result reported by a platform sink for one submitted radio frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxOutcome {
    /// The complete USB frame, including any required terminating transfer,
    /// reached a successful completion.
    Completed,
    /// The frame was not delivered and may be retried within the configured budget.
    Retryable(TxFailureKind),
    /// The frame cannot be retried on the current transport.
    Fatal(TxFailureKind),
}

/// One immutable radio frame presented to a platform TX sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxFrame {
    ticket: u64,
    bytes: Vec<u8>,
    attempt: u8,
}

impl TxFrame {
    /// Identifier that must be returned with the frame's completion.
    pub const fn ticket(&self) -> u64 {
        self.ticket
    }

    /// Radiotap + 802.11 + encrypted WFB bytes to inject.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Zero-based submission attempt number.
    pub const fn attempt(&self) -> u8 {
        self.attempt
    }
}

/// Atomically admitted collection of WFB radio frames.
///
/// A sink must either accept every frame and call [`UplinkEngine::mark_submitted`]
/// or accept none of them. This prevents a queue boundary from splitting a
/// session/data/FEC group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxBatch {
    id: u64,
    class: UplinkTrafficClass,
    frames: Vec<TxFrame>,
    ip_packets: usize,
    payload_bytes: usize,
}

impl TxBatch {
    /// Stable batch identifier used by [`UplinkEngine::mark_submitted`].
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Highest-priority packet class represented by this aggregate.
    pub const fn class(&self) -> UplinkTrafficClass {
        self.class
    }

    /// Complete set of frames that must be admitted together.
    pub fn frames(&self) -> &[TxFrame] {
        &self.frames
    }

    /// Number of IP packets aggregated into the WFB payload.
    pub const fn ip_packets(&self) -> usize {
        self.ip_packets
    }

    /// Number of length-prefixed tunnel bytes before WFB encryption/FEC.
    pub const fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }
}

/// Cumulative scheduler, queue, and completion counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UplinkEngineMetrics {
    pub control_packets_queued: u64,
    pub tunnel_packets_queued: u64,
    pub control_queue_full: u64,
    pub tunnel_queue_full: u64,
    pub aggregates_created: u64,
    pub aggregates_completed: u64,
    pub aggregates_failed: u64,
    pub ip_packets_aggregated: u64,
    pub aggregate_payload_bytes: u64,
    pub aggregate_bytes_completed: u64,
    pub batches_submitted: u64,
    pub frames_submitted: u64,
    pub frames_completed: u64,
    pub frames_failed: u64,
    pub frames_retried: u64,
    pub frames_dropped: u64,
    pub short_writes: u64,
    pub stalls: u64,
    pub timeouts: u64,
    pub fatal_failures: u64,
}

/// Failure returned by the shared uplink scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UplinkEngineError {
    InvalidConfig(&'static str),
    Network(String),
    Wfb(WfbError),
    QueueFull {
        class: UplinkTrafficClass,
        capacity: usize,
    },
    BatchMismatch,
    CompletionMismatch(u64),
}

impl fmt::Display for UplinkEngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => formatter.write_str(message),
            Self::Network(message) => write!(formatter, "uplink network failed: {message}"),
            Self::Wfb(error) => write!(formatter, "uplink WFB failed: {error}"),
            Self::QueueFull { class, capacity } => {
                write!(
                    formatter,
                    "{class} uplink queue is full ({capacity} packets)"
                )
            }
            Self::BatchMismatch => formatter.write_str("uplink TX batch no longer matches"),
            Self::CompletionMismatch(ticket) => {
                write!(formatter, "unexpected uplink TX completion ticket {ticket}")
            }
        }
    }
}

impl std::error::Error for UplinkEngineError {}

impl From<NetworkError> for UplinkEngineError {
    fn from(error: NetworkError) -> Self {
        Self::Network(error.to_string())
    }
}

impl From<WfbError> for UplinkEngineError {
    fn from(error: WfbError) -> Self {
        Self::Wfb(error)
    }
}

impl From<TunnelFramingError> for UplinkEngineError {
    fn from(error: TunnelFramingError) -> Self {
        Self::Network(error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameStatus {
    ReadyAt(u64),
    InFlight,
    Completed,
    Dropped,
}

struct ScheduledFrame {
    frame: TxFrame,
    status: FrameStatus,
    session: bool,
}

struct ActiveBatch {
    id: u64,
    class: UplinkTrafficClass,
    frames: Vec<ScheduledFrame>,
    ip_packets: usize,
    payload_bytes: usize,
}

/// Shared userspace-IP, tunnel aggregation, WFB, and retry scheduler.
///
/// Platform code owns the final USB sink. It asks for a ready batch, admits the
/// complete batch, and reports each asynchronous completion by ticket. Only one
/// batch is active at a time, bounding memory and limiting the amount of stale
/// TUN traffic that can sit in front of newly generated control packets.
pub struct UplinkEngine {
    config: UplinkEngineConfig,
    network: Arc<Mutex<UserspaceNetwork>>,
    transmitter: WfbTransmitter,
    tx_params: TxRadioParams,
    control: VecDeque<Vec<u8>>,
    tunnel: VecDeque<Vec<u8>>,
    active: Option<ActiveBatch>,
    next_batch_id: u64,
    next_ticket: u64,
    last_session_ms: Option<u64>,
    metrics: UplinkEngineMetrics,
}

impl UplinkEngine {
    /// Create an OpenIPC tunnel uplink using default network and scheduler policy.
    pub fn new(
        link_id: u32,
        keypair: WfbTxKeypair,
        epoch: u64,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<Self, UplinkEngineError> {
        Self::with_config(
            link_id,
            keypair,
            epoch,
            fec_k,
            fec_n,
            NetworkConfig::default(),
            UplinkEngineConfig::default(),
        )
    }

    /// Create an uplink with explicit network and scheduler policy.
    #[allow(clippy::too_many_arguments)]
    pub fn with_config(
        link_id: u32,
        keypair: WfbTxKeypair,
        epoch: u64,
        fec_k: usize,
        fec_n: usize,
        network_config: NetworkConfig,
        config: UplinkEngineConfig,
    ) -> Result<Self, UplinkEngineError> {
        if config.control_queue_capacity == 0 || config.tunnel_queue_capacity == 0 {
            return Err(UplinkEngineError::InvalidConfig(
                "uplink queue capacities must be greater than zero",
            ));
        }
        if config.max_aggregate_bytes < 2 || config.max_aggregate_bytes > MAX_PAYLOAD_SIZE {
            return Err(UplinkEngineError::InvalidConfig(
                "uplink aggregate size must fit one WFB payload",
            ));
        }
        if config.session_interval_ms == 0 {
            return Err(UplinkEngineError::InvalidConfig(
                "uplink session interval must be greater than zero",
            ));
        }
        let network = UserspaceNetwork::new(network_config)?;
        let transmitter = WfbTransmitter::new(
            ChannelId::from_link_port(link_id, RadioPort::TunnelTx),
            keypair,
            epoch,
            fec_k,
            fec_n,
        )?;
        Ok(Self {
            config,
            network: Arc::new(Mutex::new(network)),
            transmitter,
            tx_params: TxRadioParams::openipc_uplink_default(),
            control: VecDeque::new(),
            tunnel: VecDeque::new(),
            active: None,
            next_batch_id: 1,
            next_ticket: 1,
            last_session_ms: None,
            metrics: UplinkEngineMetrics::default(),
        })
    }

    /// Shared network handle used by SSH and legacy VTX-control clients.
    pub fn network(&self) -> Arc<Mutex<UserspaceNetwork>> {
        Arc::clone(&self.network)
    }

    /// Override radiotap parameters for generated WFB frames.
    pub fn set_tx_params(&mut self, params: TxRadioParams) {
        self.tx_params = params;
    }

    /// Queue a userspace UDP datagram. smoltcp owns its IPv4 and UDP headers.
    pub fn send_udp(
        &mut self,
        local_port: u16,
        remote_port: u16,
        payload: &[u8],
    ) -> Result<(), UplinkEngineError> {
        self.network
            .lock()
            .map_err(|_| UplinkEngineError::Network("state lock poisoned".to_owned()))?
            .send_udp(local_port, remote_port, payload)?;
        Ok(())
    }

    /// Feed a recovered WFB tunnel-RX payload into userspace TCP/UDP.
    pub fn ingest_downlink(&mut self, payload: &[u8]) -> Result<(), UplinkEngineError> {
        self.network
            .lock()
            .map_err(|_| UplinkEngineError::Network("state lock poisoned".to_owned()))?
            .ingest_tunnel_payload(payload)?;
        Ok(())
    }

    /// Number of additional operating-system TUN packets that can be accepted.
    pub fn tunnel_capacity_remaining(&self) -> usize {
        self.config
            .tunnel_queue_capacity
            .saturating_sub(self.tunnel.len())
    }

    /// Queue one complete IP packet read from a platform TUN interface.
    pub fn enqueue_tunnel_packet(&mut self, packet: Vec<u8>) -> Result<(), UplinkEngineError> {
        if self.tunnel.len() >= self.config.tunnel_queue_capacity {
            self.metrics.tunnel_queue_full = self.metrics.tunnel_queue_full.saturating_add(1);
            return Err(UplinkEngineError::QueueFull {
                class: UplinkTrafficClass::Tunnel,
                capacity: self.config.tunnel_queue_capacity,
            });
        }
        let framed = frame_ip_packet(&packet)?;
        if framed.len() > self.config.max_aggregate_bytes {
            return Err(UplinkEngineError::InvalidConfig(
                "TUN packet exceeds configured WFB aggregate size",
            ));
        }
        self.tunnel.push_back(framed);
        self.metrics.tunnel_packets_queued = self.metrics.tunnel_packets_queued.saturating_add(1);
        Ok(())
    }

    /// Advance userspace TCP timers and move available control output into the
    /// bounded priority queue without consuming packets that do not fit.
    pub fn advance(&mut self, monotonic_ms: u64) -> Result<(), UplinkEngineError> {
        let room = self
            .config
            .control_queue_capacity
            .saturating_sub(self.control.len());
        let packets = {
            let mut network = self
                .network
                .lock()
                .map_err(|_| UplinkEngineError::Network("state lock poisoned".to_owned()))?;
            network.poll(monotonic_ms);
            network.drain_outbound_limited(room).collect::<Vec<_>>()
        };
        self.metrics.control_packets_queued = self
            .metrics
            .control_packets_queued
            .saturating_add(packets.len() as u64);
        self.control.extend(packets);
        if room == 0 {
            let waiting = self
                .network
                .lock()
                .map_err(|_| UplinkEngineError::Network("state lock poisoned".to_owned()))?
                .outbound_queue_len();
            if waiting != 0 {
                self.metrics.control_queue_full = self.metrics.control_queue_full.saturating_add(1);
            }
        }
        if self.active.is_none() {
            self.prepare_batch(monotonic_ms)?;
        }
        Ok(())
    }

    /// Return the next complete frame group only when the sink can admit every frame.
    pub fn ready_batch(
        &mut self,
        monotonic_ms: u64,
        available_frame_slots: usize,
    ) -> Result<Option<TxBatch>, UplinkEngineError> {
        self.advance(monotonic_ms)?;
        let Some(active) = self.active.as_ref() else {
            return Ok(None);
        };
        if active
            .frames
            .iter()
            .any(|frame| frame.status == FrameStatus::InFlight)
        {
            return Ok(None);
        }
        let frames = active
            .frames
            .iter()
            .filter(|frame| {
                matches!(frame.status, FrameStatus::ReadyAt(ready) if ready <= monotonic_ms)
            })
            .map(|frame| frame.frame.clone())
            .collect::<Vec<_>>();
        if frames.is_empty() || frames.len() > available_frame_slots {
            return Ok(None);
        }
        Ok(Some(TxBatch {
            id: active.id,
            class: active.class,
            frames,
            ip_packets: active.ip_packets,
            payload_bytes: active.payload_bytes,
        }))
    }

    /// Commit a complete batch after a platform sink atomically accepted it.
    pub fn mark_submitted(&mut self, batch: &TxBatch) -> Result<(), UplinkEngineError> {
        let active = self
            .active
            .as_mut()
            .filter(|active| active.id == batch.id)
            .ok_or(UplinkEngineError::BatchMismatch)?;
        for submitted in &batch.frames {
            let frame = active
                .frames
                .iter_mut()
                .find(|frame| frame.frame.ticket == submitted.ticket)
                .filter(|frame| matches!(frame.status, FrameStatus::ReadyAt(_)))
                .ok_or(UplinkEngineError::BatchMismatch)?;
            frame.status = FrameStatus::InFlight;
        }
        self.metrics.batches_submitted = self.metrics.batches_submitted.saturating_add(1);
        self.metrics.frames_submitted = self
            .metrics
            .frames_submitted
            .saturating_add(batch.frames.len() as u64);
        Ok(())
    }

    /// Record the real platform completion for one submitted frame.
    pub fn report_completion(
        &mut self,
        ticket: u64,
        outcome: TxOutcome,
        monotonic_ms: u64,
    ) -> Result<(), UplinkEngineError> {
        let active = self
            .active
            .as_mut()
            .ok_or(UplinkEngineError::CompletionMismatch(ticket))?;
        let frame = active
            .frames
            .iter_mut()
            .find(|frame| frame.frame.ticket == ticket)
            .filter(|frame| frame.status == FrameStatus::InFlight)
            .ok_or(UplinkEngineError::CompletionMismatch(ticket))?;

        match outcome {
            TxOutcome::Completed => {
                frame.status = FrameStatus::Completed;
                self.metrics.frames_completed = self.metrics.frames_completed.saturating_add(1);
                if frame.session {
                    self.last_session_ms = Some(monotonic_ms);
                }
            }
            TxOutcome::Retryable(kind) => {
                Self::record_failure(&mut self.metrics, kind, false);
                if frame.frame.attempt < self.config.max_frame_retries {
                    frame.frame.attempt += 1;
                    let delay = self
                        .config
                        .retry_backoff_ms
                        .saturating_mul(u64::from(frame.frame.attempt));
                    frame.status = FrameStatus::ReadyAt(monotonic_ms.saturating_add(delay));
                    self.metrics.frames_retried = self.metrics.frames_retried.saturating_add(1);
                } else {
                    frame.status = FrameStatus::Dropped;
                    self.metrics.frames_dropped = self.metrics.frames_dropped.saturating_add(1);
                }
            }
            TxOutcome::Fatal(kind) => {
                Self::record_failure(&mut self.metrics, kind, true);
                frame.status = FrameStatus::Dropped;
                self.metrics.frames_dropped = self.metrics.frames_dropped.saturating_add(1);
            }
        }

        let terminal = active
            .frames
            .iter()
            .all(|frame| matches!(frame.status, FrameStatus::Completed | FrameStatus::Dropped));
        let dropped = terminal
            && active
                .frames
                .iter()
                .any(|frame| frame.status == FrameStatus::Dropped);
        let payload_bytes = active.payload_bytes as u64;
        if terminal {
            if dropped {
                self.metrics.aggregates_failed = self.metrics.aggregates_failed.saturating_add(1);
            } else {
                self.metrics.aggregates_completed =
                    self.metrics.aggregates_completed.saturating_add(1);
                self.metrics.aggregate_bytes_completed = self
                    .metrics
                    .aggregate_bytes_completed
                    .saturating_add(payload_bytes);
            }
            self.active = None;
        }
        Ok(())
    }

    /// Produce frames for a caller that owns delivery outside this API.
    ///
    /// This compatibility helper treats handing the frames to the caller as the
    /// completion boundary. Completion-aware applications should use
    /// [`ready_batch`](Self::ready_batch), [`mark_submitted`](Self::mark_submitted),
    /// and [`report_completion`](Self::report_completion) directly.
    pub fn take_ready_frames(
        &mut self,
        monotonic_ms: u64,
    ) -> Result<Vec<Vec<u8>>, UplinkEngineError> {
        let Some(batch) = self.ready_batch(monotonic_ms, usize::MAX)? else {
            return Ok(Vec::new());
        };
        self.mark_submitted(&batch)?;
        let frames = batch
            .frames
            .iter()
            .map(|frame| frame.bytes.clone())
            .collect::<Vec<_>>();
        for frame in &batch.frames {
            self.report_completion(frame.ticket, TxOutcome::Completed, monotonic_ms)?;
        }
        Ok(frames)
    }

    /// Current scheduler counters.
    pub const fn metrics(&self) -> UplinkEngineMetrics {
        self.metrics
    }

    /// Current queue lengths as `(control, TUN)`.
    pub fn queue_lengths(&self) -> (usize, usize) {
        (self.control.len(), self.tunnel.len())
    }

    fn prepare_batch(&mut self, monotonic_ms: u64) -> Result<(), UplinkEngineError> {
        let mut selected = Vec::new();
        let mut payload = Vec::new();
        self.take_fitting_packets(UplinkTrafficClass::Control, &mut payload, &mut selected);
        let control_packets = selected.len();
        self.take_fitting_packets(UplinkTrafficClass::Tunnel, &mut payload, &mut selected);
        if selected.is_empty() {
            return Ok(());
        }

        let mut radio_frames = Vec::new();
        let session_due = self.last_session_ms.is_none_or(|last| {
            monotonic_ms.saturating_sub(last) >= self.config.session_interval_ms
        });
        if session_due {
            radio_frames.push((self.transmitter.session_radio_packet(self.tx_params), true));
        }
        match self
            .transmitter
            .radio_packets_for_payload(&payload, self.tx_params)
        {
            Ok(frames) => radio_frames.extend(frames.into_iter().map(|frame| (frame, false))),
            Err(error) => {
                self.restore_selected(selected);
                return Err(error.into());
            }
        }
        if self.transmitter.has_open_fec_block() {
            let fillers = self.transmitter.close_fec_block()?;
            radio_frames.extend(fillers.into_iter().map(|packet| {
                (
                    self.transmitter
                        .radio_packet_for_forwarder_packet(&packet, self.tx_params),
                    false,
                )
            }));
        }

        let id = self.next_batch_id;
        self.next_batch_id = self.next_batch_id.wrapping_add(1).max(1);
        let frames = radio_frames
            .into_iter()
            .map(|(bytes, session)| {
                let ticket = self.next_ticket;
                self.next_ticket = self.next_ticket.wrapping_add(1).max(1);
                ScheduledFrame {
                    frame: TxFrame {
                        ticket,
                        bytes,
                        attempt: 0,
                    },
                    status: FrameStatus::ReadyAt(monotonic_ms),
                    session,
                }
            })
            .collect();
        self.metrics.aggregates_created = self.metrics.aggregates_created.saturating_add(1);
        self.metrics.ip_packets_aggregated = self
            .metrics
            .ip_packets_aggregated
            .saturating_add(selected.len() as u64);
        self.metrics.aggregate_payload_bytes = self
            .metrics
            .aggregate_payload_bytes
            .saturating_add(payload.len() as u64);
        self.active = Some(ActiveBatch {
            id,
            class: if control_packets != 0 {
                UplinkTrafficClass::Control
            } else {
                UplinkTrafficClass::Tunnel
            },
            frames,
            ip_packets: selected.len(),
            payload_bytes: payload.len(),
        });
        Ok(())
    }

    fn take_fitting_packets(
        &mut self,
        class: UplinkTrafficClass,
        payload: &mut Vec<u8>,
        selected: &mut Vec<(UplinkTrafficClass, Vec<u8>)>,
    ) {
        let queue = match class {
            UplinkTrafficClass::Control => &mut self.control,
            UplinkTrafficClass::Tunnel => &mut self.tunnel,
        };
        while let Some(packet) = queue.front() {
            if payload.len().saturating_add(packet.len()) > self.config.max_aggregate_bytes {
                break;
            }
            let packet = queue.pop_front().expect("front packet exists");
            payload.extend_from_slice(&packet);
            selected.push((class, packet));
        }
    }

    fn restore_selected(&mut self, selected: Vec<(UplinkTrafficClass, Vec<u8>)>) {
        for (class, packet) in selected.into_iter().rev() {
            match class {
                UplinkTrafficClass::Control => self.control.push_front(packet),
                UplinkTrafficClass::Tunnel => self.tunnel.push_front(packet),
            }
        }
    }

    fn record_failure(metrics: &mut UplinkEngineMetrics, kind: TxFailureKind, fatal: bool) {
        metrics.frames_failed = metrics.frames_failed.saturating_add(1);
        if fatal {
            metrics.fatal_failures = metrics.fatal_failures.saturating_add(1);
        }
        match kind {
            TxFailureKind::Timeout => metrics.timeouts = metrics.timeouts.saturating_add(1),
            TxFailureKind::Stall => metrics.stalls = metrics.stalls.saturating_add(1),
            TxFailureKind::ShortWrite => {
                metrics.short_writes = metrics.short_writes.saturating_add(1)
            }
            TxFailureKind::Disconnected | TxFailureKind::QueueClosed | TxFailureKind::Other => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crypto_box::SecretKey;
    use openipc_core::{
        wfb::{WfbEvent, WfbKeypair, WfbReceiver},
        WfbTxKeypair,
    };

    use super::*;
    use crate::{parse_tunnel_packets, parse_tunnel_payload};

    fn linked_keys() -> (WfbTxKeypair, WfbKeypair) {
        let ground = SecretKey::from([3; 32]);
        let air = SecretKey::from([9; 32]);
        (
            WfbTxKeypair {
                tx_secretkey: ground.to_bytes(),
                rx_publickey: air.public_key().to_bytes(),
            },
            WfbKeypair {
                rx_secretkey: air.to_bytes(),
                tx_publickey: ground.public_key().to_bytes(),
            },
        )
    }

    fn engine(config: UplinkEngineConfig) -> (UplinkEngine, WfbReceiver) {
        let (tx, rx) = linked_keys();
        let channel = ChannelId::from_link_port(0x7505d6, RadioPort::TunnelTx);
        (
            UplinkEngine::with_config(0x7505d6, tx, 0, 1, 5, NetworkConfig::default(), config)
                .unwrap(),
            WfbReceiver::new(channel, rx, 0),
        )
    }

    fn complete_batch(engine: &mut UplinkEngine, batch: &TxBatch, now: u64) {
        engine.mark_submitted(batch).unwrap();
        for frame in batch.frames() {
            engine
                .report_completion(frame.ticket(), TxOutcome::Completed, now)
                .unwrap();
        }
    }

    fn recover_payload(receiver: &mut WfbReceiver, batch: &TxBatch) -> Vec<u8> {
        let mut payload = None;
        for frame in batch.frames() {
            let wifi_offset = frame.bytes()[2] as usize;
            let forwarder = &frame.bytes()[wifi_offset + 24..];
            for event in receiver.push_forwarder_packet(forwarder).unwrap() {
                if let WfbEvent::Payload(recovered) = event {
                    payload = Some(recovered.payload);
                }
            }
        }
        payload.unwrap()
    }

    #[test]
    fn aggregates_control_before_tunnel_without_reordering_each_class() {
        let (mut engine, mut receiver) = engine(UplinkEngineConfig::default());
        engine.enqueue_tunnel_packet(vec![0x45, 0, 0, 20]).unwrap();
        engine.send_udp(54_321, 9_999, b"control").unwrap();

        let batch = engine.ready_batch(1, 6).unwrap().unwrap();
        assert_eq!(batch.class(), UplinkTrafficClass::Control);
        assert_eq!(batch.ip_packets(), 2);
        assert_eq!(batch.frames().len(), 6);
        let payload = recover_payload(&mut receiver, &batch);
        let packets = parse_tunnel_packets(&payload)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0][9], 17, "smoltcp UDP is scheduled first");
        assert_eq!(packets[1], &[0x45, 0, 0, 20]);
        complete_batch(&mut engine, &batch, 2);
    }

    #[test]
    fn simultaneous_ssh_adaptive_and_tun_traffic_share_one_priority_batch() {
        let (mut engine, mut receiver) = engine(UplinkEngineConfig::default());
        let network = engine.network();
        let _ssh = network.lock().unwrap().connect_tcp(22).unwrap();
        engine.send_udp(54_321, 9_999, b"adaptive").unwrap();
        engine.enqueue_tunnel_packet(vec![0x45, 0, 0, 20]).unwrap();

        let batch = engine.ready_batch(1, 6).unwrap().unwrap();
        assert_eq!(batch.class(), UplinkTrafficClass::Control);
        assert_eq!(batch.ip_packets(), 3);
        let payload = recover_payload(&mut receiver, &batch);
        let packets = parse_tunnel_packets(&payload)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(packets[0][9], 6, "SSH TCP SYN is control traffic");
        assert_eq!(packets[1][9], 17, "adaptive UDP follows TCP in stack order");
        assert_eq!(packets[2], &[0x45, 0, 0, 20], "TUN traffic stays last");
    }

    #[test]
    fn capacity_check_keeps_complete_fec_group_ready() {
        let (mut engine, _) = engine(UplinkEngineConfig::default());
        engine.send_udp(54_321, 9_999, b"feedback").unwrap();
        assert!(engine.ready_batch(1, 5).unwrap().is_none());
        let first = engine.ready_batch(1, 6).unwrap().unwrap();
        let second = engine.ready_batch(1, 6).unwrap().unwrap();
        assert_eq!(
            first, second,
            "unadmitted frames remain owned by the engine"
        );
        assert_eq!(first.frames().len(), 6);
    }

    #[test]
    fn bounded_tunnel_queue_rejects_before_consuming_another_packet() {
        let config = UplinkEngineConfig {
            tunnel_queue_capacity: 2,
            ..UplinkEngineConfig::default()
        };
        let (mut engine, _) = engine(config);
        engine.enqueue_tunnel_packet(vec![0x45, 1]).unwrap();
        engine.enqueue_tunnel_packet(vec![0x45, 2]).unwrap();
        assert_eq!(engine.tunnel_capacity_remaining(), 0);
        assert_eq!(
            engine.enqueue_tunnel_packet(vec![0x45, 3]),
            Err(UplinkEngineError::QueueFull {
                class: UplinkTrafficClass::Tunnel,
                capacity: 2,
            })
        );
        assert_eq!(engine.queue_lengths(), (0, 2));
        assert_eq!(engine.metrics().tunnel_queue_full, 1);
    }

    #[test]
    fn failed_frame_is_retained_and_retried_with_same_bytes() {
        let config = UplinkEngineConfig {
            retry_backoff_ms: 3,
            max_frame_retries: 1,
            ..UplinkEngineConfig::default()
        };
        let (mut engine, _) = engine(config);
        engine.send_udp(54_321, 9_999, b"feedback").unwrap();
        let batch = engine.ready_batch(10, 6).unwrap().unwrap();
        let failed = batch.frames()[2].clone();
        engine.mark_submitted(&batch).unwrap();
        for frame in batch.frames() {
            let outcome = if frame.ticket() == failed.ticket() {
                TxOutcome::Retryable(TxFailureKind::Stall)
            } else {
                TxOutcome::Completed
            };
            engine
                .report_completion(frame.ticket(), outcome, 10)
                .unwrap();
        }
        assert!(engine.ready_batch(12, 1).unwrap().is_none());
        let retry = engine.ready_batch(13, 1).unwrap().unwrap();
        assert_eq!(retry.frames().len(), 1);
        assert_eq!(retry.frames()[0].ticket(), failed.ticket());
        assert_eq!(retry.frames()[0].bytes(), failed.bytes());
        assert_eq!(retry.frames()[0].attempt(), 1);
        complete_batch(&mut engine, &retry, 14);
        assert_eq!(engine.metrics().frames_retried, 1);
        assert_eq!(engine.metrics().frames_dropped, 0);
    }

    #[test]
    fn same_tick_small_packets_share_one_wfb_payload() {
        let (mut engine, mut receiver) = engine(UplinkEngineConfig::default());
        engine.enqueue_tunnel_packet(vec![0x45, 0, 0, 20]).unwrap();
        engine.enqueue_tunnel_packet(vec![0x45, 0, 0, 21]).unwrap();
        let batch = engine.ready_batch(1, 6).unwrap().unwrap();
        assert_eq!(batch.ip_packets(), 2);
        let recovered = recover_payload(&mut receiver, &batch);
        let packets = parse_tunnel_packets(&recovered)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(packets, [&[0x45, 0, 0, 20][..], &[0x45, 0, 0, 21][..]]);
    }

    #[test]
    fn compatibility_take_marks_frames_completed() {
        let (mut engine, _) = engine(UplinkEngineConfig::default());
        engine.send_udp(54_321, 9_999, b"feedback").unwrap();
        assert_eq!(engine.take_ready_frames(1).unwrap().len(), 6);
        assert_eq!(engine.metrics().frames_completed, 6);
        assert!(engine.take_ready_frames(2).unwrap().is_empty());
    }

    #[test]
    fn control_backpressure_retains_network_output_until_scheduler_has_room() {
        let config = UplinkEngineConfig {
            control_queue_capacity: 1,
            ..UplinkEngineConfig::default()
        };
        let (mut engine, _) = engine(config);
        engine.send_udp(54_321, 9_999, b"first").unwrap();
        engine.send_udp(54_321, 9_999, b"second").unwrap();

        let first = engine.ready_batch(1, 6).unwrap().unwrap();
        assert_eq!(first.ip_packets(), 1);
        assert_eq!(engine.engine_network_outbound_len(), 1);
        complete_batch(&mut engine, &first, 2);

        let second = engine.ready_batch(3, 6).unwrap().unwrap();
        assert_eq!(second.ip_packets(), 1);
        assert_eq!(engine.engine_network_outbound_len(), 0);
        assert_eq!(engine.metrics().control_packets_queued, 2);
    }

    #[test]
    fn partial_multi_source_fec_block_is_closed_before_atomic_admission() {
        let (tx, rx) = linked_keys();
        let channel = ChannelId::from_link_port(0x7505d6, RadioPort::TunnelTx);
        let mut engine = UplinkEngine::with_config(
            0x7505d6,
            tx,
            0,
            2,
            3,
            NetworkConfig::default(),
            UplinkEngineConfig::default(),
        )
        .unwrap();
        let mut receiver = WfbReceiver::new(channel, rx, 0);
        engine.send_udp(54_321, 9_999, b"one payload").unwrap();

        assert!(engine.ready_batch(1, 3).unwrap().is_none());
        let batch = engine.ready_batch(1, 4).unwrap().unwrap();
        assert_eq!(batch.frames().len(), 4, "session plus complete 2:3 block");
        let recovered = recover_payload(&mut receiver, &batch);
        assert_eq!(parse_tunnel_payload(&recovered).unwrap()[9], 17);
    }

    #[test]
    fn short_write_exhausts_retry_budget_without_losing_completion_state() {
        let config = UplinkEngineConfig {
            max_frame_retries: 1,
            retry_backoff_ms: 0,
            ..UplinkEngineConfig::default()
        };
        let (mut engine, _) = engine(config);
        engine.send_udp(54_321, 9_999, b"feedback").unwrap();
        let first = engine.ready_batch(1, 6).unwrap().unwrap();
        let failed_ticket = first.frames()[0].ticket();
        engine.mark_submitted(&first).unwrap();
        for frame in first.frames() {
            engine
                .report_completion(
                    frame.ticket(),
                    if frame.ticket() == failed_ticket {
                        TxOutcome::Retryable(TxFailureKind::ShortWrite)
                    } else {
                        TxOutcome::Completed
                    },
                    1,
                )
                .unwrap();
        }
        let retry = engine.ready_batch(1, 1).unwrap().unwrap();
        engine.mark_submitted(&retry).unwrap();
        engine
            .report_completion(
                failed_ticket,
                TxOutcome::Retryable(TxFailureKind::ShortWrite),
                1,
            )
            .unwrap();
        assert_eq!(engine.metrics().short_writes, 2);
        assert_eq!(engine.metrics().frames_retried, 1);
        assert_eq!(engine.metrics().frames_dropped, 1);
    }

    #[test]
    fn fatal_disconnect_is_not_retried() {
        let (mut engine, _) = engine(UplinkEngineConfig::default());
        engine.send_udp(54_321, 9_999, b"feedback").unwrap();
        let batch = engine.ready_batch(1, 6).unwrap().unwrap();
        engine.mark_submitted(&batch).unwrap();
        for frame in batch.frames() {
            engine
                .report_completion(
                    frame.ticket(),
                    TxOutcome::Fatal(TxFailureKind::Disconnected),
                    1,
                )
                .unwrap();
        }
        assert_eq!(engine.metrics().fatal_failures, 6);
        assert_eq!(engine.metrics().frames_retried, 0);
        assert_eq!(engine.metrics().frames_dropped, 6);
        assert!(engine.ready_batch(2, 6).unwrap().is_none());
    }

    #[test]
    fn generated_udp_keeps_valid_tunnel_framing() {
        let (mut engine, mut receiver) = engine(UplinkEngineConfig::default());
        engine.send_udp(54_321, 9_999, b"feedback").unwrap();
        let batch = engine.ready_batch(1, 6).unwrap().unwrap();
        let payload = recover_payload(&mut receiver, &batch);
        assert_eq!(parse_tunnel_payload(&payload).unwrap()[0] >> 4, 4);
    }

    impl UplinkEngine {
        fn engine_network_outbound_len(&self) -> usize {
            self.network.lock().unwrap().outbound_queue_len()
        }
    }
}
