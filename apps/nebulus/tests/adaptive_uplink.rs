use crypto_box::SecretKey;
use openipc_core::wfb::WfbEvent;
use openipc_core::{
    AdaptiveLink, ChannelId, RadioPort, WfbKeypair, WfbReceiver, WfbTransmitter, WfbTxKeypair,
    ADAPTIVE_LINK_GS_PORT, ADAPTIVE_LINK_VTX_PORT,
};
use openipc_uplink::{parse_tunnel_payload, NetworkConfig, UserspaceNetwork};

fn recover_tunnel_packet_from_one_shard(tunnel_packet: &[u8]) -> Vec<Vec<u8>> {
    let ground_secret = SecretKey::from([3; 32]);
    let vtx_secret = SecretKey::from([9; 32]);
    let tx_keypair = WfbTxKeypair {
        tx_secretkey: ground_secret.to_bytes(),
        rx_publickey: vtx_secret.public_key().to_bytes(),
    };
    let rx_keypair = WfbKeypair {
        rx_secretkey: vtx_secret.to_bytes(),
        tx_publickey: ground_secret.public_key().to_bytes(),
    };
    let channel = ChannelId::from_link_port(0x7505d6, RadioPort::TunnelTx);
    let mut transmitter = WfbTransmitter::new(channel, tx_keypair, 0, 1, 5).unwrap();
    let mut receiver = WfbReceiver::new(channel, rx_keypair, 0);
    receiver
        .push_forwarder_packet(transmitter.session_forwarder_packet())
        .unwrap();

    let packets = transmitter
        .forwarder_packets_for_payload(tunnel_packet, 0)
        .unwrap();
    assert_eq!(packets.len(), 5);
    packets
        .last()
        .into_iter()
        .flat_map(|packet| receiver.push_forwarder_packet(packet).unwrap())
        .filter_map(|event| match event {
            WfbEvent::Payload(payload) => Some(payload.payload),
            WfbEvent::Session(_) => None,
        })
        .collect()
}

#[test]
fn adaptive_feedback_crosses_userspace_udp_and_wfb() {
    let mut adaptive = AdaptiveLink::new();
    adaptive.set_keyframe_request_messages(0);
    adaptive.record_rx(1_000, 72, 68, 31, 27);
    adaptive.record_fec(1_000, 100, 3, 0);
    let feedback = adaptive.feedback_udp_payload(1_000);

    let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
    network
        .send_udp(ADAPTIVE_LINK_GS_PORT, ADAPTIVE_LINK_VTX_PORT, &feedback)
        .unwrap();
    network.poll(1_000);
    let tunnel_packets = network.drain_outbound().collect::<Vec<_>>();
    assert_eq!(tunnel_packets.len(), 1);

    let ip = parse_tunnel_payload(&tunnel_packets[0]).unwrap();
    let ip_header_len = usize::from(ip[0] & 0x0f) * 4;
    let udp = &ip[ip_header_len..];
    assert_eq!(u16::from_be_bytes([udp[0], udp[1]]), ADAPTIVE_LINK_GS_PORT);
    assert_eq!(u16::from_be_bytes([udp[2], udp[3]]), ADAPTIVE_LINK_VTX_PORT);
    assert_eq!(&udp[8..], feedback);

    // Adaptive feedback uses 1:5 FEC. Recover from only the final shard to
    // prove that losing the other four copies still reaches the VTX intact.
    let recovered = recover_tunnel_packet_from_one_shard(&tunnel_packets[0]);
    assert_eq!(recovered, [tunnel_packets[0].clone()]);

    let metrics = network.metrics();
    assert_eq!(metrics.udp_datagrams_queued, 1);
    assert_eq!(metrics.tunnel_packets_sent, 1);
}

#[test]
fn tun_packet_crosses_shared_network_and_wfb_unchanged() {
    let raw_ip_packet = [
        0x45, 0, 0, 20, 0, 7, 0, 0, 64, 1, 0, 0, 10, 5, 0, 3, 10, 5, 0, 10,
    ];
    let mut network = UserspaceNetwork::new(NetworkConfig::default()).unwrap();
    network
        .queue_outbound_ip_packet_owned(raw_ip_packet.to_vec())
        .unwrap();
    let tunnel_packets = network.drain_outbound().collect::<Vec<_>>();
    assert_eq!(tunnel_packets.len(), 1);
    assert_eq!(
        parse_tunnel_payload(&tunnel_packets[0]).unwrap(),
        raw_ip_packet
    );

    let recovered = recover_tunnel_packet_from_one_shard(&tunnel_packets[0]);
    assert_eq!(recovered, [tunnel_packets[0].clone()]);
    let metrics = network.metrics();
    assert_eq!(metrics.raw_ip_packets_queued, 1);
    assert_eq!(metrics.raw_ip_bytes_queued, raw_ip_packet.len() as u64);
    assert_eq!(metrics.tunnel_packets_sent, 1);
}
