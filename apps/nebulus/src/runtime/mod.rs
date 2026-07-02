//! Target-specific receiver runtimes.

use std::collections::VecDeque;

mod messages;
mod route_runtime;

#[cfg(debug_assertions)]
pub(crate) mod codec_mock;

#[cfg(not(target_arch = "wasm32"))]
mod native;
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
                };
            } else {
                queue.push_back(RuntimeEvent::NativeVideo {
                    frame,
                    decode_latency_ms,
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
            | RuntimeEvent::Stopped
            | RuntimeEvent::Failed(_)
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{queue_event, BatchMetrics, RuntimeEvent};

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
}
