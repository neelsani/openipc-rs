use std::{
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
    time::Duration,
};

use socket2::{Domain, Protocol, Socket, Type};

pub(super) const MAX_UDP_DATAGRAM_SIZE: usize = 65_535;
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(20);
const REQUESTED_RECEIVE_BUFFER_SIZE: usize = 4 * 1024 * 1024;

/// Native UDP socket configured for bounded-latency RTP reception.
pub(super) struct UdpRtpInput {
    socket: UdpSocket,
    local_address: SocketAddr,
    receive_buffer_size: Option<usize>,
}

impl UdpRtpInput {
    pub(super) fn bind(address: &str, port: u16) -> Result<Self, String> {
        let ip = address
            .trim()
            .parse::<IpAddr>()
            .map_err(|error| format!("invalid UDP bind address {address:?}: {error}"))?;
        if port == 0 {
            return Err("UDP RTP port must be between 1 and 65535".to_owned());
        }

        Self::bind_socket(SocketAddr::new(ip, port))
    }

    fn bind_socket(requested: SocketAddr) -> Result<Self, String> {
        let socket = Socket::new(
            Domain::for_address(requested),
            Type::DGRAM,
            Some(Protocol::UDP),
        )
        .map_err(|error| format!("create UDP RTP socket failed: {error}"))?;
        socket
            .set_reuse_address(true)
            .map_err(|error| format!("configure UDP RTP socket reuse failed: {error}"))?;
        let _ = socket.set_recv_buffer_size(REQUESTED_RECEIVE_BUFFER_SIZE);
        let receive_buffer_size = socket.recv_buffer_size().ok();
        socket
            .bind(&requested.into())
            .map_err(|error| format!("bind UDP RTP listener {requested} failed: {error}"))?;

        let socket = UdpSocket::from(socket);
        socket
            .set_read_timeout(Some(RECEIVE_TIMEOUT))
            .map_err(|error| format!("configure UDP RTP receive timeout failed: {error}"))?;
        let local_address = socket
            .local_addr()
            .map_err(|error| format!("read UDP RTP listener address failed: {error}"))?;

        Ok(Self {
            socket,
            local_address,
            receive_buffer_size,
        })
    }

    pub(super) const fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    pub(super) const fn receive_buffer_size(&self) -> Option<usize> {
        self.receive_buffer_size
    }

    pub(super) fn receive(&self, buffer: &mut [u8]) -> Result<Option<(usize, SocketAddr)>, String> {
        match self.socket.recv_from(buffer) {
            Ok(received) => Ok(Some(received)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(format!("UDP RTP receive failed: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::UdpSocket;

    use openipc_core::{
        ChannelId, Codec, FrameLayout, PayloadRouteId, ReceiverBatchOptions, ReceiverRuntime,
    };

    use super::{UdpRtpInput, MAX_UDP_DATAGRAM_SIZE};

    #[test]
    fn udp_datagram_enters_the_direct_video_pipeline() {
        let input = UdpRtpInput::bind_socket("127.0.0.1:0".parse().unwrap()).unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut packet = vec![0x80, 0x80 | openipc_core::rtp::RTP_PAYLOAD_TYPE_H264];
        packet.extend_from_slice(&1u16.to_be_bytes());
        packet.extend_from_slice(&90_000u32.to_be_bytes());
        packet.extend_from_slice(&0x1122_3344u32.to_be_bytes());
        packet.push(24);
        for nalu in [
            &[0x67, 0x42, 0x00, 0x1e, 0xab][..],
            &[0x68, 0xce, 0x06, 0xe2][..],
            &[0x65, 0x88, 0x84, 0x21][..],
        ] {
            packet.extend_from_slice(&(nalu.len() as u16).to_be_bytes());
            packet.extend_from_slice(nalu);
        }
        sender.send_to(&packet, input.local_address()).unwrap();

        let mut buffer = vec![0; MAX_UDP_DATAGRAM_SIZE];
        let (length, peer) = input.receive(&mut buffer).unwrap().unwrap();

        assert_eq!(&buffer[..length], packet);
        assert_eq!(peer, sender.local_addr().unwrap());

        let mut receiver = ReceiverRuntime::with_direct_video_route(
            FrameLayout::WithFcs,
            PayloadRouteId::new(1),
            ChannelId::default_video(),
            0,
        );
        let batch = receiver
            .push_direct_payload(
                receiver.video_runtime(),
                1,
                &buffer[..length],
                &ReceiverBatchOptions::default(),
            )
            .unwrap();
        assert_eq!(batch.frames.len(), 1);
        assert_eq!(batch.frames[0].codec, Codec::H264);
        assert!(batch.frames[0].is_keyframe);
    }
}
