use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{Arc, Mutex},
};

use smoltcp::{
    iface::{Config as InterfaceConfig, Interface, SocketHandle, SocketSet},
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    socket::{tcp, udp},
    time::Instant,
    wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint},
};

use crate::{
    frame_ip_packet, parse_tunnel_packets,
    stream::{StreamState, VirtualTcpStream, STREAM_QUEUE_CAPACITY},
    TunnelFramingError, MAX_TUNNEL_PACKET_LEN,
};

const DEFAULT_MTU: usize = 1_500;
const DEFAULT_TCP_BUFFER_SIZE: usize = 128 * 1024;
const FIRST_EPHEMERAL_PORT: u16 = 49_152;
const UDP_TX_PACKET_CAPACITY: usize = 16;
const IPV4_UDP_HEADER_LEN: usize = 20 + 8;
const DEVICE_PACKET_QUEUE_CAPACITY: usize = 256;

/// Addressing and buffer policy for the userspace WFB tunnel network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkConfig {
    pub local_address: [u8; 4],
    pub remote_address: [u8; 4],
    pub prefix_length: u8,
    pub mtu: usize,
    pub tcp_buffer_size: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            local_address: [10, 5, 0, 1],
            remote_address: [10, 5, 0, 10],
            prefix_length: 24,
            mtu: DEFAULT_MTU,
            tcp_buffer_size: DEFAULT_TCP_BUFFER_SIZE,
        }
    }
}

/// Cumulative userspace network counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetworkMetrics {
    pub tunnel_packets_received: u64,
    pub tunnel_bytes_received: u64,
    pub tunnel_packets_sent: u64,
    pub tunnel_bytes_sent: u64,
    pub malformed_tunnel_packets: u64,
    pub tcp_connections_opened: u64,
    pub tcp_connection_failures: u64,
    pub tcp_connections_active: usize,
    pub udp_datagrams_queued: u64,
    pub udp_bytes_queued: u64,
    pub udp_send_failures: u64,
    pub raw_ip_packets_queued: u64,
    pub raw_ip_bytes_queued: u64,
    pub raw_ip_send_failures: u64,
    pub inbound_queue_full: u64,
    pub outbound_queue_full: u64,
}

/// Failure produced while configuring or driving the userspace network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkError {
    InvalidConfig(&'static str),
    TunnelFraming(TunnelFramingError),
    TcpConnect(String),
    UdpPayloadTooLarge {
        length: usize,
        maximum: usize,
    },
    UdpBind {
        port: u16,
        error: String,
    },
    UdpSend {
        local_port: u16,
        remote_port: u16,
        error: String,
    },
    IpPacketTooLarge {
        length: usize,
        mtu: usize,
    },
    PacketQueueFull {
        direction: &'static str,
        capacity: usize,
    },
}

impl fmt::Display for NetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => formatter.write_str(message),
            Self::TunnelFraming(error) => error.fmt(formatter),
            Self::TcpConnect(error) => write!(formatter, "userspace TCP connect failed: {error}"),
            Self::UdpPayloadTooLarge { length, maximum } => write!(
                formatter,
                "userspace UDP payload is too large: {length} bytes (maximum {maximum})"
            ),
            Self::UdpBind { port, error } => {
                write!(
                    formatter,
                    "userspace UDP bind on port {port} failed: {error}"
                )
            }
            Self::UdpSend {
                local_port,
                remote_port,
                error,
            } => write!(
                formatter,
                "userspace UDP {local_port} -> {remote_port} send failed: {error}"
            ),
            Self::IpPacketTooLarge { length, mtu } => write!(
                formatter,
                "outbound IP packet is too large: {length} bytes (tunnel MTU {mtu})"
            ),
            Self::PacketQueueFull {
                direction,
                capacity,
            } => write!(
                formatter,
                "userspace network {direction} packet queue is full ({capacity} packets)"
            ),
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<TunnelFramingError> for NetworkError {
    fn from(error: TunnelFramingError) -> Self {
        Self::TunnelFraming(error)
    }
}

struct Connection {
    handle: SocketHandle,
    state: Arc<Mutex<StreamState>>,
    was_active: bool,
}

/// IPv4/UDP/TCP stack and raw-IP queue for OpenIPC WFB tunnel payloads.
///
/// Call [`ingest_tunnel_payload`](Self::ingest_tunnel_payload) for every
/// recovered port `0x20` payload, call [`poll`](Self::poll) regularly, and send
/// every payload returned by [`drain_outbound`](Self::drain_outbound) through a
/// WFB transmitter on port `0xa0`.
pub struct UserspaceNetwork {
    config: NetworkConfig,
    device: TunnelDevice,
    interface: Interface,
    sockets: SocketSet<'static>,
    connections: HashMap<u64, Connection>,
    udp_sockets: HashMap<u16, SocketHandle>,
    next_connection_id: u64,
    next_ephemeral_port: u16,
    metrics: NetworkMetrics,
}

impl UserspaceNetwork {
    /// Create a userspace network with OpenIPC-compatible addressing.
    pub fn new(config: NetworkConfig) -> Result<Self, NetworkError> {
        if config.mtu < 576 {
            return Err(NetworkError::InvalidConfig(
                "tunnel MTU must be at least 576",
            ));
        }
        if config.mtu > MAX_TUNNEL_PACKET_LEN {
            return Err(NetworkError::InvalidConfig(
                "tunnel MTU must fit the 16-bit OpenIPC length prefix",
            ));
        }
        if config.tcp_buffer_size == 0 {
            return Err(NetworkError::InvalidConfig(
                "TCP buffer size must be greater than zero",
            ));
        }
        if config.prefix_length > 32 {
            return Err(NetworkError::InvalidConfig(
                "IPv4 prefix length must not exceed 32",
            ));
        }
        let mut device = TunnelDevice::new(config.mtu, DEVICE_PACKET_QUEUE_CAPACITY);
        let interface_config = InterfaceConfig::new(HardwareAddress::Ip);
        let mut interface = Interface::new(interface_config, &mut device, Instant::from_millis(0));
        interface.update_ip_addrs(|addresses| {
            addresses
                .push(IpCidr::new(
                    IpAddress::v4(
                        config.local_address[0],
                        config.local_address[1],
                        config.local_address[2],
                        config.local_address[3],
                    ),
                    config.prefix_length,
                ))
                .expect("one interface address fits in smoltcp storage");
        });
        Ok(Self {
            config,
            device,
            interface,
            sockets: SocketSet::new(Vec::new()),
            connections: HashMap::new(),
            udp_sockets: HashMap::new(),
            next_connection_id: 1,
            next_ephemeral_port: FIRST_EPHEMERAL_PORT,
            metrics: NetworkMetrics::default(),
        })
    }

    /// Open a TCP stream to the configured VTX address.
    pub fn connect_tcp(&mut self, remote_port: u16) -> Result<VirtualTcpStream, NetworkError> {
        let receive = tcp::SocketBuffer::new(vec![0; self.config.tcp_buffer_size]);
        let transmit = tcp::SocketBuffer::new(vec![0; self.config.tcp_buffer_size]);
        let mut socket = tcp::Socket::new(receive, transmit);
        socket.set_nagle_enabled(false);
        socket.set_ack_delay(None);
        socket.set_timeout(Some(smoltcp::time::Duration::from_secs(30)));
        let local_port = self.allocate_ephemeral_port();
        socket
            .connect(
                self.interface.context(),
                (
                    IpAddress::v4(
                        self.config.remote_address[0],
                        self.config.remote_address[1],
                        self.config.remote_address[2],
                        self.config.remote_address[3],
                    ),
                    remote_port,
                ),
                local_port,
            )
            .map_err(|error| NetworkError::TcpConnect(error.to_string()))?;
        let handle = self.sockets.add(socket);
        let state = Arc::new(Mutex::new(StreamState::default()));
        let id = self.next_connection_id;
        self.next_connection_id = self.next_connection_id.wrapping_add(1).max(1);
        self.connections.insert(
            id,
            Connection {
                handle,
                state: Arc::clone(&state),
                was_active: false,
            },
        );
        self.metrics.tcp_connections_opened += 1;
        self.metrics.tcp_connections_active = self.connections.len();
        Ok(VirtualTcpStream::new(state))
    }

    /// Queue a UDP datagram to the configured remote VTX address.
    ///
    /// The datagram is converted into IPv4 by smoltcp on the next [`poll`](Self::poll).
    /// The resulting IP packet is returned by [`drain_outbound`](Self::drain_outbound)
    /// with OpenIPC's two-byte tunnel length prefix.
    pub fn send_udp(
        &mut self,
        local_port: u16,
        remote_port: u16,
        payload: &[u8],
    ) -> Result<(), NetworkError> {
        let maximum = self.config.mtu.saturating_sub(IPV4_UDP_HEADER_LEN);
        if payload.len() > maximum {
            self.metrics.udp_send_failures += 1;
            return Err(NetworkError::UdpPayloadTooLarge {
                length: payload.len(),
                maximum,
            });
        }

        let remote = IpEndpoint::new(
            IpAddress::v4(
                self.config.remote_address[0],
                self.config.remote_address[1],
                self.config.remote_address[2],
                self.config.remote_address[3],
            ),
            remote_port,
        );
        let handle = match self.udp_socket(local_port) {
            Ok(handle) => handle,
            Err(error) => {
                self.metrics.udp_send_failures += 1;
                return Err(error);
            }
        };
        let result = self
            .sockets
            .get_mut::<udp::Socket>(handle)
            .send_slice(payload, remote)
            .map_err(|error| NetworkError::UdpSend {
                local_port,
                remote_port,
                error: error.to_string(),
            });
        match result {
            Ok(()) => {
                self.metrics.udp_datagrams_queued += 1;
                self.metrics.udp_bytes_queued += payload.len() as u64;
                Ok(())
            }
            Err(error) => {
                self.metrics.udp_send_failures += 1;
                Err(error)
            }
        }
    }

    /// Queue a complete IP packet produced by an external network stack.
    ///
    /// This is the boundary for native TUN/Wintun/VpnService traffic. The OS
    /// has already built the IP and transport headers, so the packet bypasses
    /// smoltcp socket processing but shares its outbound queue, tunnel framing,
    /// ordering, metrics, and WFB transmitter.
    pub fn queue_outbound_ip_packet(&mut self, packet: &[u8]) -> Result<(), NetworkError> {
        self.queue_outbound_ip_packet_owned(packet.to_vec())
    }

    /// Queue an owned complete IP packet without copying it into the queue.
    ///
    /// Native TUN readers should prefer this method because they already own a
    /// freshly read packet buffer.
    pub fn queue_outbound_ip_packet_owned(&mut self, packet: Vec<u8>) -> Result<(), NetworkError> {
        if packet.is_empty() {
            self.metrics.raw_ip_send_failures += 1;
            return Err(NetworkError::TunnelFraming(TunnelFramingError::EmptyPacket));
        }
        if packet.len() > self.config.mtu {
            self.metrics.raw_ip_send_failures += 1;
            return Err(NetworkError::IpPacketTooLarge {
                length: packet.len(),
                mtu: self.config.mtu,
            });
        }
        if self.device.outbound.len() >= self.device.queue_capacity {
            self.metrics.raw_ip_send_failures += 1;
            self.metrics.outbound_queue_full += 1;
            return Err(NetworkError::PacketQueueFull {
                direction: "outbound",
                capacity: self.device.queue_capacity,
            });
        }
        let packet_len = packet.len();
        self.device.outbound.push_back(packet);
        self.metrics.raw_ip_packets_queued += 1;
        self.metrics.raw_ip_bytes_queued += packet_len as u64;
        Ok(())
    }

    /// Queue a recovered WFB tunnel payload for IPv4/TCP processing.
    pub fn ingest_tunnel_payload(&mut self, payload: &[u8]) -> Result<(), NetworkError> {
        let packets = match parse_tunnel_packets(payload).collect::<Result<Vec<_>, _>>() {
            Ok(packets) => packets,
            Err(error) => {
                self.metrics.malformed_tunnel_packets += 1;
                return Err(error.into());
            }
        };
        if self.device.inbound.len().saturating_add(packets.len()) > self.device.queue_capacity {
            self.metrics.inbound_queue_full += 1;
            return Err(NetworkError::PacketQueueFull {
                direction: "inbound",
                capacity: self.device.queue_capacity,
            });
        }
        for packet in packets {
            self.metrics.tunnel_packets_received += 1;
            self.metrics.tunnel_bytes_received += packet.len() as u64;
            self.device.inbound.push_back(packet.to_vec());
        }
        Ok(())
    }

    /// Advance TCP state and move bytes between sockets and virtual streams.
    pub fn poll(&mut self, now_ms: u64) {
        self.move_application_writes();
        let timestamp = Instant::from_millis(now_ms.min(i64::MAX as u64) as i64);
        let _ = self
            .interface
            .poll(timestamp, &mut self.device, &mut self.sockets);
        self.prune_abandoned_connections();
        self.move_network_reads();
        let _ = self
            .interface
            .poll(timestamp, &mut self.device, &mut self.sockets);
    }

    /// Return framed IP packets that must be transmitted on WFB tunnel TX.
    pub fn drain_outbound(&mut self) -> impl Iterator<Item = Vec<u8>> + '_ {
        self.drain_outbound_limited(usize::MAX)
    }

    /// Return at most `limit` framed outbound IP packets, retaining the rest.
    pub fn drain_outbound_limited(&mut self, limit: usize) -> impl Iterator<Item = Vec<u8>> + '_ {
        let amount = limit.min(self.device.outbound.len());
        self.device.outbound.drain(..amount).filter_map(|packet| {
            let framed = frame_ip_packet(&packet).ok()?;
            self.metrics.tunnel_packets_sent += 1;
            self.metrics.tunnel_bytes_sent += packet.len() as u64;
            Some(framed)
        })
    }

    /// Number of smoltcp-produced IP packets waiting for the uplink scheduler.
    pub fn outbound_queue_len(&self) -> usize {
        self.device.outbound.len()
    }

    /// Current cumulative network counters.
    pub fn metrics(&self) -> NetworkMetrics {
        self.metrics
    }

    fn allocate_ephemeral_port(&mut self) -> u16 {
        let selected = self.next_ephemeral_port;
        self.next_ephemeral_port = if selected == u16::MAX {
            FIRST_EPHEMERAL_PORT
        } else {
            selected + 1
        };
        selected
    }

    fn udp_socket(&mut self, local_port: u16) -> Result<SocketHandle, NetworkError> {
        if let Some(handle) = self.udp_sockets.get(&local_port) {
            return Ok(*handle);
        }

        let receive = udp::PacketBuffer::new(Vec::new(), Vec::new());
        let transmit = udp::PacketBuffer::new(
            vec![udp::PacketMetadata::EMPTY; UDP_TX_PACKET_CAPACITY],
            vec![0; self.config.mtu * UDP_TX_PACKET_CAPACITY],
        );
        let mut socket = udp::Socket::new(receive, transmit);
        socket
            .bind(local_port)
            .map_err(|error| NetworkError::UdpBind {
                port: local_port,
                error: error.to_string(),
            })?;
        let handle = self.sockets.add(socket);
        self.udp_sockets.insert(local_port, handle);
        Ok(handle)
    }

    fn move_application_writes(&mut self) {
        for connection in self.connections.values_mut() {
            let socket = self.sockets.get_mut::<tcp::Socket>(connection.handle);
            let Ok(mut state) = connection.state.lock() else {
                socket.abort();
                continue;
            };
            if state.local_closed && state.pending_send.is_empty() && socket.may_send() {
                socket.close();
            }
            while socket.can_send() && !state.pending_send.is_empty() {
                let amount = {
                    let (first, second) = state.pending_send.as_slices();
                    let source = if first.is_empty() { second } else { first };
                    match socket.send_slice(source) {
                        Ok(amount) => amount,
                        Err(error) => {
                            state.error = Some(error.to_string());
                            0
                        }
                    }
                };
                if amount == 0 {
                    break;
                }
                state.pending_send.drain(..amount);
                if let Some(waker) = state.write_waker.take() {
                    waker.wake();
                }
            }
            if state.pending_send.is_empty() {
                if let Some(waker) = state.flush_waker.take() {
                    waker.wake();
                }
            }
        }
    }

    fn move_network_reads(&mut self) {
        for connection in self.connections.values_mut() {
            let socket = self.sockets.get_mut::<tcp::Socket>(connection.handle);
            let Ok(mut state) = connection.state.lock() else {
                socket.abort();
                continue;
            };
            if socket.state() == tcp::State::Established && !connection.was_active {
                connection.was_active = true;
                state.connected = true;
                state.remote_closed = false;
                state.wake_all();
            }
            while socket.can_recv() && state.received.len() < STREAM_QUEUE_CAPACITY {
                let available = STREAM_QUEUE_CAPACITY - state.received.len();
                let result = socket.recv(|bytes| {
                    let amount = bytes.len().min(available);
                    state.received.extend(&bytes[..amount]);
                    (amount, amount)
                });
                match result {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Some(waker) = state.read_waker.take() {
                            waker.wake();
                        }
                    }
                    Err(error) => {
                        state.error = Some(error.to_string());
                        state.wake_all();
                        break;
                    }
                }
            }
            if !socket.may_recv() && (connection.was_active || !socket.is_open()) {
                if !connection.was_active {
                    self.metrics.tcp_connection_failures += 1;
                    state.error = Some("remote host rejected or timed out the connection".into());
                }
                state.remote_closed = true;
                state.wake_all();
            }
        }
    }

    fn prune_abandoned_connections(&mut self) {
        let abandoned = self
            .connections
            .iter()
            .filter_map(|(id, connection)| {
                (Arc::strong_count(&connection.state) == 1).then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in abandoned {
            if let Some(connection) = self.connections.remove(&id) {
                self.sockets.remove(connection.handle);
            }
        }
        self.metrics.tcp_connections_active = self.connections.len();
    }
}

struct TunnelDevice {
    mtu: usize,
    queue_capacity: usize,
    inbound: VecDeque<Vec<u8>>,
    outbound: VecDeque<Vec<u8>>,
}

impl TunnelDevice {
    fn new(mtu: usize, queue_capacity: usize) -> Self {
        Self {
            mtu,
            queue_capacity,
            inbound: VecDeque::new(),
            outbound: VecDeque::new(),
        }
    }
}

struct TunnelRxToken(Vec<u8>);

impl RxToken for TunnelRxToken {
    fn consume<R, F>(self, function: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        function(&self.0)
    }
}

struct TunnelTxToken<'a>(&'a mut VecDeque<Vec<u8>>);

impl TxToken for TunnelTxToken<'_> {
    fn consume<R, F>(self, length: usize, function: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut packet = vec![0; length];
        let result = function(&mut packet);
        self.0.push_back(packet);
        result
    }
}

impl Device for TunnelDevice {
    type RxToken<'a> = TunnelRxToken;
    type TxToken<'a> = TunnelTxToken<'a>;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if self.outbound.len() >= self.queue_capacity {
            return None;
        }
        let packet = self.inbound.pop_front()?;
        Some((TunnelRxToken(packet), TunnelTxToken(&mut self.outbound)))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        (self.outbound.len() < self.queue_capacity).then_some(TunnelTxToken(&mut self.outbound))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ip;
        capabilities.max_transmission_unit = self.mtu;
        capabilities.max_burst_size = Some(64);
        capabilities
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Poll};

    use smoltcp::socket::tcp;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    use super::{NetworkConfig, NetworkError, UserspaceNetwork, DEFAULT_MTU};

    fn ipv4_payload(tunnel_packet: &[u8]) -> &[u8] {
        crate::parse_tunnel_payload(tunnel_packet).unwrap()
    }

    fn internet_checksum_sum(bytes: &[u8]) -> u16 {
        let mut sum = 0u32;
        for chunk in bytes.chunks_exact(2) {
            sum += u32::from(u16::from_be_bytes([chunk[0], chunk[1]]));
        }
        if let Some(last) = bytes.chunks_exact(2).remainder().first() {
            sum += u32::from(*last) << 8;
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        sum as u16
    }

    fn exchange(left: &mut UserspaceNetwork, right: &mut UserspaceNetwork) {
        let packets = left.drain_outbound().collect::<Vec<_>>();
        for packet in packets {
            right.ingest_tunnel_payload(&packet).unwrap();
        }
    }

    #[test]
    fn tcp_connect_emits_ipv4_syn_in_tunnel_framing() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        let stream = network.connect_tcp(22).unwrap();
        assert!(!stream.is_connected());
        network.poll(1);
        let packets = network.drain_outbound().collect::<Vec<_>>();
        assert_eq!(packets.len(), 1);
        assert_eq!(
            u16::from_be_bytes([packets[0][0], packets[0][1]]) as usize,
            packets[0].len() - 2
        );
        assert_eq!(packets[0][2] >> 4, 4);
    }

    #[test]
    fn network_rejects_mtu_larger_than_tunnel_prefix() {
        let config = NetworkConfig {
            mtu: crate::MAX_TUNNEL_PACKET_LEN + 1,
            ..NetworkConfig::default()
        };
        assert!(matches!(
            UserspaceNetwork::new(config),
            Err(NetworkError::InvalidConfig(
                "tunnel MTU must fit the 16-bit OpenIPC length prefix"
            ))
        ));
    }

    #[test]
    fn udp_send_emits_compatible_length_prefixed_ipv4_packet() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        let payload = b"adaptive-link feedback";
        network.send_udp(54_321, 9_999, payload).unwrap();
        network.poll(1_000);

        let packets = network.drain_outbound().collect::<Vec<_>>();
        assert_eq!(packets.len(), 1);
        let ip = ipv4_payload(&packets[0]);
        let header_len = usize::from(ip[0] & 0x0f) * 4;
        assert_eq!(ip[0] >> 4, 4);
        assert_eq!(header_len, 20);
        assert_eq!(usize::from(u16::from_be_bytes([ip[2], ip[3]])), ip.len());
        assert_eq!(ip[9], 17);
        assert_eq!(&ip[12..16], &[10, 5, 0, 1]);
        assert_eq!(&ip[16..20], &[10, 5, 0, 10]);
        assert_eq!(internet_checksum_sum(&ip[..header_len]), u16::MAX);

        let udp = &ip[header_len..];
        assert_eq!(u16::from_be_bytes([udp[0], udp[1]]), 54_321);
        assert_eq!(u16::from_be_bytes([udp[2], udp[3]]), 9_999);
        assert_eq!(usize::from(u16::from_be_bytes([udp[4], udp[5]])), udp.len());
        assert_ne!(u16::from_be_bytes([udp[6], udp[7]]), 0);
        assert_eq!(&udp[8..], payload);
        let mut udp_checksum_input = Vec::with_capacity(12 + udp.len());
        udp_checksum_input.extend_from_slice(&ip[12..20]);
        udp_checksum_input.extend_from_slice(&[0, ip[9]]);
        udp_checksum_input.extend_from_slice(&(udp.len() as u16).to_be_bytes());
        udp_checksum_input.extend_from_slice(udp);
        assert_eq!(internet_checksum_sum(&udp_checksum_input), u16::MAX);

        let metrics = network.metrics();
        assert_eq!(metrics.udp_datagrams_queued, 1);
        assert_eq!(metrics.udp_bytes_queued, payload.len() as u64);
        assert_eq!(metrics.udp_send_failures, 0);
        assert_eq!(metrics.tunnel_packets_sent, 1);
    }

    #[test]
    fn udp_sender_reuses_port_and_preserves_datagram_order() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        network.send_udp(54_321, 9_999, b"first").unwrap();
        network.send_udp(54_321, 9_999, b"second").unwrap();
        assert_eq!(network.udp_sockets.len(), 1);

        network.poll(2_000);
        let packets = network.drain_outbound().collect::<Vec<_>>();
        let payloads = packets
            .iter()
            .map(|packet| {
                let ip = ipv4_payload(packet);
                let header_len = usize::from(ip[0] & 0x0f) * 4;
                &ip[header_len + 8..]
            })
            .collect::<Vec<_>>();
        assert_eq!(payloads, [b"first".as_slice(), b"second".as_slice()]);
    }

    #[test]
    fn raw_ip_packet_shares_outbound_framing_and_preserves_bytes() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        let packet = [
            0x45, 0, 0, 20, 0, 1, 0, 0, 64, 1, 0, 0, 10, 5, 0, 3, 10, 5, 0, 10,
        ];
        network.queue_outbound_ip_packet(&packet).unwrap();

        let framed = network.drain_outbound().collect::<Vec<_>>();
        assert_eq!(framed.len(), 1);
        assert_eq!(ipv4_payload(&framed[0]), packet);
        let metrics = network.metrics();
        assert_eq!(metrics.raw_ip_packets_queued, 1);
        assert_eq!(metrics.raw_ip_bytes_queued, packet.len() as u64);
        assert_eq!(metrics.raw_ip_send_failures, 0);
        assert_eq!(metrics.tunnel_packets_sent, 1);
    }

    #[test]
    fn smoltcp_output_precedes_later_tun_packet_in_shared_queue() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        network.send_udp(54_321, 9_999, b"adaptive").unwrap();
        network.poll(1_000);
        let tun_packet = [
            0x45, 0, 0, 20, 0, 2, 0, 0, 64, 1, 0, 0, 10, 5, 0, 3, 10, 5, 0, 10,
        ];
        network.queue_outbound_ip_packet(&tun_packet).unwrap();

        let framed = network.drain_outbound().collect::<Vec<_>>();
        assert_eq!(framed.len(), 2);
        assert_eq!(ipv4_payload(&framed[0])[9], 17, "adaptive UDP stays first");
        assert_eq!(ipv4_payload(&framed[1]), tun_packet);
    }

    #[test]
    fn raw_ip_packet_rejects_empty_and_oversized_input() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        assert_eq!(
            network.queue_outbound_ip_packet(&[]),
            Err(NetworkError::TunnelFraming(
                crate::TunnelFramingError::EmptyPacket
            ))
        );
        let oversized = vec![0; DEFAULT_MTU + 1];
        assert_eq!(
            network.queue_outbound_ip_packet(&oversized),
            Err(NetworkError::IpPacketTooLarge {
                length: DEFAULT_MTU + 1,
                mtu: DEFAULT_MTU,
            })
        );
        assert_eq!(network.metrics().raw_ip_send_failures, 2);
        assert_eq!(network.drain_outbound().count(), 0);
    }

    #[test]
    fn raw_ip_queue_is_bounded_before_packet_ownership_is_lost() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        let packet = [
            0x45, 0, 0, 20, 0, 1, 0, 0, 64, 1, 0, 0, 10, 5, 0, 3, 10, 5, 0, 10,
        ];
        for _ in 0..super::DEVICE_PACKET_QUEUE_CAPACITY {
            network.queue_outbound_ip_packet(&packet).unwrap();
        }
        assert_eq!(
            network.queue_outbound_ip_packet(&packet),
            Err(NetworkError::PacketQueueFull {
                direction: "outbound",
                capacity: super::DEVICE_PACKET_QUEUE_CAPACITY,
            })
        );
        assert_eq!(network.metrics().outbound_queue_full, 1);
        assert_eq!(
            network.drain_outbound().count(),
            super::DEVICE_PACKET_QUEUE_CAPACITY
        );
    }

    #[test]
    fn udp_sender_rejects_payload_larger_than_tunnel_mtu() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        let maximum = DEFAULT_MTU - 28;
        let payload = vec![0; maximum + 1];
        assert_eq!(
            network.send_udp(54_321, 9_999, &payload),
            Err(NetworkError::UdpPayloadTooLarge {
                length: maximum + 1,
                maximum,
            })
        );
        assert_eq!(network.metrics().udp_send_failures, 1);
        network.poll(3_000);
        assert_eq!(network.drain_outbound().count(), 0);
    }

    #[test]
    fn malformed_tunnel_payload_is_counted() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        assert!(network.ingest_tunnel_payload(&[0, 5, 1]).is_err());
        assert_eq!(network.metrics().malformed_tunnel_packets, 1);
    }

    #[test]
    fn aggregated_tunnel_payload_queues_every_ip_packet() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        let mut aggregate = crate::frame_ip_packet(&[0x45, 1, 2]).unwrap();
        aggregate.extend(crate::frame_ip_packet(&[0x45, 3, 4]).unwrap());
        network.ingest_tunnel_payload(&aggregate).unwrap();
        assert_eq!(network.metrics().tunnel_packets_received, 2);
        assert_eq!(network.device.inbound.len(), 2);
    }

    #[test]
    fn dropped_virtual_stream_releases_socket_buffers() {
        let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        let stream = network.connect_tcp(22).unwrap();
        assert_eq!(network.metrics().tcp_connections_active, 1);
        drop(stream);
        network.poll(1);
        assert_eq!(network.metrics().tcp_connections_active, 0);
        assert!(network.connections.is_empty());
    }

    #[test]
    fn virtual_stream_exchanges_bytes_with_a_smoltcp_peer() {
        let mut client = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
        let mut server = UserspaceNetwork::new(NetworkConfig {
            local_address: [10, 5, 0, 10],
            remote_address: [10, 5, 0, 1],
            ..NetworkConfig::default()
        })
        .unwrap();
        let socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; 4_096]),
            tcp::SocketBuffer::new(vec![0; 4_096]),
        );
        let server_handle = server.sockets.add(socket);
        server
            .sockets
            .get_mut::<tcp::Socket>(server_handle)
            .listen(22)
            .unwrap();

        let mut stream = client.connect_tcp(22).unwrap();
        for now in 0..20 {
            client.poll(now);
            exchange(&mut client, &mut server);
            server.poll(now);
            exchange(&mut server, &mut client);
        }
        assert!(stream.is_connected());
        assert!(stream.state.lock().unwrap().error.is_none());
        assert!(!stream.state.lock().unwrap().remote_closed);

        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        assert!(matches!(
            Pin::new(&mut stream).poll_write(&mut context, b"hello"),
            Poll::Ready(Ok(5))
        ));

        let mut request = Vec::new();
        for now in 20..80 {
            client.poll(now);
            exchange(&mut client, &mut server);
            server.poll(now);
            {
                let socket = server.sockets.get_mut::<tcp::Socket>(server_handle);
                if socket.can_recv() {
                    socket
                        .recv(|bytes| {
                            request.extend_from_slice(bytes);
                            (bytes.len(), ())
                        })
                        .unwrap();
                }
                if request == b"hello" && socket.can_send() && socket.send_queue() == 0 {
                    socket.send_slice(b"world").unwrap();
                }
            }
            exchange(&mut server, &mut client);
        }
        assert_eq!(request, b"hello");

        client.poll(81);
        let mut response = [0; 5];
        let mut output = ReadBuf::new(&mut response);
        assert!(matches!(
            Pin::new(&mut stream).poll_read(&mut context, &mut output),
            Poll::Ready(Ok(()))
        ));
        assert_eq!(output.filled(), b"world");
    }
}
