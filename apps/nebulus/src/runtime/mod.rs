//! Target-specific receiver runtimes.

use std::collections::VecDeque;

mod messages;
mod route_runtime;

#[cfg(debug_assertions)]
pub(crate) mod codec_mock;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod udp_input;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod web;

pub(crate) use messages::*;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::Runtime;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use web::Runtime;

fn queue_event(queue: &mut VecDeque<RuntimeEvent>, event: RuntimeEvent) {
    match event {
        RuntimeEvent::Batch(batch) => {
            if let Some(RuntimeEvent::Batch(pending)) = queue
                .iter_mut()
                .rev()
                .take_while(|event| !is_lifecycle_barrier(event))
                .find(|pending| matches!(pending, RuntimeEvent::Batch(_)))
            {
                pending.merge(*batch);
            } else {
                queue.push_back(RuntimeEvent::Batch(batch));
            }
        }
        RuntimeEvent::NativeVideo {
            frame,
            decode_latency_ms,
            ready_at,
        } => {
            if let Some(pending) = queue
                .iter_mut()
                .rev()
                .take_while(|event| !is_lifecycle_barrier(event))
                .find(|pending| matches!(pending, RuntimeEvent::NativeVideo { .. }))
            {
                *pending = RuntimeEvent::NativeVideo {
                    frame,
                    decode_latency_ms,
                    ready_at,
                };
            } else {
                queue.push_back(RuntimeEvent::NativeVideo {
                    frame,
                    decode_latency_ms,
                    ready_at,
                });
            }
        }
        event => queue.push_back(event),
    }
}

fn is_lifecycle_barrier(event: &RuntimeEvent) -> bool {
    matches!(
        event,
        RuntimeEvent::Connecting
            | RuntimeEvent::Connected { .. }
            | RuntimeEvent::Started
            | RuntimeEvent::ScanStarted { .. }
            | RuntimeEvent::ScanCompleted
            | RuntimeEvent::ScanFailed(_)
            | RuntimeEvent::Stopped
            | RuntimeEvent::Failed(_)
    )
}

#[derive(Default)]
struct ChannelScanAccumulator {
    packets: u64,
    bytes: u64,
    wfb_frames: u64,
    rssi_sum: [i64; 2],
    rssi_samples: [u64; 2],
    strongest: [u8; 2],
}

impl ChannelScanAccumulator {
    fn observe(&mut self, packet: &openipc_core::realtek::RealtekRxPacket<'_>) {
        use openipc_core::{realtek::RxPacketType, FrameLayout, WifiFrame};

        if packet.attrib.crc_err
            || packet.attrib.icv_err
            || packet.attrib.pkt_rpt_type != RxPacketType::NormalRx
        {
            return;
        }
        self.packets = self.packets.saturating_add(1);
        self.bytes = self.bytes.saturating_add(packet.data.len() as u64);
        if WifiFrame::parse(packet.data, FrameLayout::WithFcs)
            .ok()
            .and_then(|frame| frame.channel_id())
            .is_some()
        {
            self.wfb_frames = self.wfb_frames.saturating_add(1);
        }
        for path in 0..2 {
            let rssi = packet.attrib.rssi[path];
            if rssi > 0 {
                self.rssi_sum[path] += i64::from(rssi);
                self.rssi_samples[path] += 1;
                self.strongest[path] = self.strongest[path].max(rssi);
            }
        }
    }

    fn finish(self, channel: u8, dwell: std::time::Duration) -> ChannelScanResult {
        let average_rssi_dbm = std::array::from_fn(|path| {
            if self.rssi_samples[path] == 0 {
                0
            } else {
                -(self.rssi_sum[path] / self.rssi_samples[path] as i64) as i32
            }
        });
        ChannelScanResult {
            channel,
            packets: self.packets,
            bytes: self.bytes,
            wfb_frames: self.wfb_frames,
            average_rssi_dbm,
            strongest_rssi_dbm: self.strongest.map(|value| -(i32::from(value))),
            dwell_ms: dwell.as_millis().min(u128::from(u64::MAX)) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{queue_event, BatchMetrics, ChannelScanAccumulator, RuntimeEvent};

    #[test]
    fn pending_batches_are_merged_without_losing_counts() {
        let mut queue = VecDeque::new();
        queue_event(
            &mut queue,
            RuntimeEvent::Batch(Box::new(BatchMetrics {
                transfers: 1,
                transfer_bytes: 100,
                packets: 2,
                ..BatchMetrics::default()
            })),
        );
        queue_event(
            &mut queue,
            RuntimeEvent::Batch(Box::new(BatchMetrics {
                transfers: 1,
                transfer_bytes: 200,
                packets: 3,
                usb_latency_ms: 1.5,
                ..BatchMetrics::default()
            })),
        );

        let Some(RuntimeEvent::Batch(batch)) = queue.pop_front() else {
            panic!("expected one merged batch");
        };
        assert!(queue.is_empty());
        assert_eq!(batch.transfers, 2);
        assert_eq!(batch.transfer_bytes, 300);
        assert_eq!(batch.packets, 5);
        assert_eq!(batch.usb_latency_ms, 1.5);
    }

    #[test]
    fn batches_are_not_merged_across_receiver_lifecycle_events() {
        let mut queue = VecDeque::new();
        queue_event(
            &mut queue,
            RuntimeEvent::Batch(Box::new(BatchMetrics {
                transfers: 1,
                ..BatchMetrics::default()
            })),
        );
        queue_event(&mut queue, RuntimeEvent::Stopped);
        queue_event(&mut queue, RuntimeEvent::Connecting);
        queue_event(
            &mut queue,
            RuntimeEvent::Batch(Box::new(BatchMetrics {
                transfers: 2,
                ..BatchMetrics::default()
            })),
        );

        assert_eq!(queue.len(), 4);
        let RuntimeEvent::Batch(first) = queue.pop_front().unwrap() else {
            panic!("expected first receiver batch");
        };
        assert_eq!(first.transfers, 1);
        assert!(matches!(queue.pop_front(), Some(RuntimeEvent::Stopped)));
        assert!(matches!(queue.pop_front(), Some(RuntimeEvent::Connecting)));
        let RuntimeEvent::Batch(second) = queue.pop_front().unwrap() else {
            panic!("expected second receiver batch");
        };
        assert_eq!(second.transfers, 2);
    }

    #[test]
    fn channel_scan_counts_valid_wfb_frames_and_signal_paths() {
        let channel = openipc_core::ChannelId::default_video();
        let mut frame = openipc_core::ieee80211::build_wfb_header(channel, [1, 0]).to_vec();
        frame.push(0x42);
        frame.extend_from_slice(&[0; 4]);
        let attrib = openipc_core::RxPacketAttrib {
            rssi: [58, 62, 0, 0],
            ..openipc_core::RxPacketAttrib::default()
        };
        let packet = openipc_core::RealtekRxPacket {
            attrib,
            data: &frame,
        };
        let mut accumulator = ChannelScanAccumulator::default();
        accumulator.observe(&packet);
        let result = accumulator.finish(161, std::time::Duration::from_millis(150));

        assert_eq!(result.packets, 1);
        assert_eq!(result.wfb_frames, 1);
        assert_eq!(result.average_rssi_dbm, [-58, -62]);
        assert_eq!(result.strongest_rssi_dbm, [-58, -62]);
    }
}
