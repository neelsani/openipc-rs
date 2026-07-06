use std::{hint::black_box, time::Instant};

use openipc_core::wfb::MAX_FEC_PAYLOAD;
use openipc_core::{
    ieee80211::build_wfb_header, parse_rx_aggregate, ChannelId, Codec, FecCode, FrameLayout,
    PayloadRouteId, PlainAssembler, ReceiverBatchOptions, ReceiverRuntime, RtpDepacketizer,
    WifiFrame,
};

const BLOCK_SIZE: usize = 3_996;

fn main() {
    bench_wifi_parse();
    bench_realtek_aggregate();
    bench_fec();
    bench_wfb_assembler();
    bench_rtp_single_nalu();
    bench_rtp_fragmented();
    bench_receiver_direct();
}

fn bench_wfb_assembler() {
    let mut assembler = PlainAssembler::new(8, 12).unwrap();
    let payload_size = 1_200usize;
    let mut fragment = vec![0; MAX_FEC_PAYLOAD];
    fragment[1..3].copy_from_slice(&(payload_size as u16).to_be_bytes());
    fragment[3..3 + payload_size].fill(0x5a);
    let mut block = 0u64;
    measure("wfb_8_12_primary_block", 25_000, || {
        let mut bytes = 0usize;
        for index in 0..8u64 {
            let outputs = assembler
                .push_decrypted_fragment((block << 8) | index, &fragment)
                .unwrap();
            bytes += outputs
                .iter()
                .map(|output| output.payload.len())
                .sum::<usize>();
        }
        block = block.wrapping_add(1);
        bytes
    });
}

fn measure(name: &str, iterations: usize, mut operation: impl FnMut() -> usize) {
    for _ in 0..iterations.min(2_000) {
        black_box(operation());
    }

    let mut samples = [0.0; 9];
    for sample in &mut samples {
        let started = Instant::now();
        let mut checksum = 0usize;
        for _ in 0..iterations {
            checksum = checksum.wrapping_add(black_box(operation()));
        }
        black_box(checksum);
        *sample = started.elapsed().as_nanos() as f64 / iterations as f64;
    }
    samples.sort_by(f64::total_cmp);
    println!("{name:<28} {:>10.2} ns/op", samples[samples.len() / 2]);
}

fn bench_wifi_parse() {
    let channel = ChannelId::default_video();
    let mut frame = Vec::from(build_wfb_header(channel, [0, 0]));
    frame.resize(1_500, 0x5a);
    frame.extend_from_slice(&[0; 4]);
    measure("wifi_parse", 1_000_000, || {
        let parsed = WifiFrame::parse(black_box(&frame), FrameLayout::WithFcs).unwrap();
        parsed.payload().len() ^ parsed.channel_id().unwrap().raw() as usize
    });
}

fn bench_realtek_aggregate() {
    let payload_len = 1_500usize;
    let mut aggregate = Vec::new();
    for sequence in 0..8u16 {
        let mut descriptor = [0u8; 24];
        let d0 = payload_len as u32;
        descriptor[..4].copy_from_slice(&d0.to_le_bytes());
        descriptor[8..12].copy_from_slice(&(sequence as u32).to_le_bytes());
        aggregate.extend_from_slice(&descriptor);
        aggregate.resize(aggregate.len() + payload_len, sequence as u8);
        aggregate.resize((aggregate.len() + 7) & !7, 0);
    }
    measure("realtek_aggregate_8x1500", 100_000, || {
        let packets = parse_rx_aggregate(black_box(&aggregate)).unwrap();
        packets.iter().map(|packet| packet.data.len()).sum()
    });
}

fn bench_fec() {
    let fec = FecCode::new(8, 12).unwrap();
    let primary: Vec<Vec<u8>> = (0..8)
        .map(|fragment| {
            (0..BLOCK_SIZE)
                .map(|offset| (fragment * 31 + offset * 17) as u8)
                .collect()
        })
        .collect();
    let parity = fec.encode(&primary, BLOCK_SIZE).unwrap();
    let mut fragments: Vec<u8> = primary.iter().chain(&parity).flatten().copied().collect();
    let mut present = vec![true; 12];
    measure("fec_8_12_one_missing", 100_000, || {
        present[7] = false;
        fec.recover_primary_into(&mut fragments, &mut present, BLOCK_SIZE)
            .unwrap()
    });
    measure("fec_8_12_four_missing", 100_000, || {
        present[4..8].fill(false);
        fec.recover_primary_into(&mut fragments, &mut present, BLOCK_SIZE)
            .unwrap()
    });
}

fn bench_rtp_single_nalu() {
    let mut depacketizer = configured_h264_depacketizer();
    let mut packet = rtp_packet(3, 3_000, true, 96, &[0x65; 1_200]);
    measure("rtp_h264_single_1200", 100_000, || {
        advance_rtp(&mut packet, 1, 3_000);
        let frame = depacketizer.push(&packet).unwrap().unwrap();
        debug_assert_eq!(frame.codec, Codec::H264);
        frame.data.len()
    });
}

fn bench_rtp_fragmented() {
    let mut depacketizer = configured_h264_depacketizer();
    let initial_timestamp = 99u32.wrapping_sub(3_000);
    let mut start = rtp_packet(u16::MAX - 1, initial_timestamp, false, 96, &[0x7c, 0x85]);
    start.extend(std::iter::repeat_n(0x31, 1_398));
    let mut middle = rtp_packet(u16::MAX, initial_timestamp, false, 96, &[0x7c, 0x05]);
    middle.extend(std::iter::repeat_n(0x32, 1_398));
    let mut end = rtp_packet(0, initial_timestamp, true, 96, &[0x7c, 0x45]);
    end.extend(std::iter::repeat_n(0x33, 1_198));
    measure("rtp_h264_fu_4200", 50_000, || {
        advance_rtp(&mut start, 3, 3_000);
        advance_rtp(&mut middle, 3, 3_000);
        advance_rtp(&mut end, 3, 3_000);
        assert!(depacketizer.push(&start).unwrap().is_none());
        assert!(depacketizer.push(&middle).unwrap().is_none());
        match depacketizer.push(&end).unwrap() {
            Some(frame) => frame.data.len(),
            None => panic!("fragment benchmark lost frame: {:?}", depacketizer.status()),
        }
    });
}

fn bench_receiver_direct() {
    let route = PayloadRouteId::new(1);
    let channel = ChannelId::default_video();
    let mut receiver =
        ReceiverRuntime::with_direct_video_route(FrameLayout::WithFcs, route, channel, 0);
    let runtime = receiver.video_runtime();
    let options = ReceiverBatchOptions::default();
    for (sequence, payload) in [
        (1, &[0x67, 0x42, 0, 0x1f][..]),
        (2, &[0x68, 0xce, 0x06, 0xe2][..]),
    ] {
        receiver
            .push_direct_payload(
                runtime,
                u64::from(sequence),
                &rtp_packet(sequence, 10, true, 96, payload),
                &options,
            )
            .unwrap();
    }
    let mut packet = rtp_packet(3, 3_000, true, 96, &[0x65; 1_200]);
    let mut packet_seq = 3u64;
    measure("receiver_direct_h264", 100_000, || {
        advance_rtp(&mut packet, 1, 3_000);
        packet_seq = packet_seq.wrapping_add(1);
        let batch = receiver
            .push_direct_payload(runtime, packet_seq, &packet, &options)
            .unwrap();
        batch.frames.first().map_or(0, |frame| frame.data.len())
    });
}

fn configured_h264_depacketizer() -> RtpDepacketizer {
    let mut depacketizer = RtpDepacketizer::new();
    depacketizer
        .push(&rtp_packet(1, 10, true, 96, &[0x67, 0x42, 0, 0x1f]))
        .unwrap();
    depacketizer
        .push(&rtp_packet(2, 10, true, 96, &[0x68, 0xce, 0x06, 0xe2]))
        .unwrap();
    depacketizer
}

fn rtp_packet(
    sequence: u16,
    timestamp: u32,
    marker: bool,
    payload_type: u8,
    payload: &[u8],
) -> Vec<u8> {
    let mut packet = Vec::with_capacity(12 + payload.len());
    packet.push(0x80);
    packet.push((u8::from(marker) << 7) | payload_type);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&0x1020_3040u32.to_be_bytes());
    packet.extend_from_slice(payload);
    packet
}

fn advance_rtp(packet: &mut [u8], sequence_delta: u16, timestamp_delta: u32) {
    let sequence = u16::from_be_bytes([packet[2], packet[3]]).wrapping_add(sequence_delta);
    packet[2..4].copy_from_slice(&sequence.to_be_bytes());
    let timestamp =
        u32::from_be_bytes(packet[4..8].try_into().unwrap()).wrapping_add(timestamp_delta);
    packet[4..8].copy_from_slice(&timestamp.to_be_bytes());
}
