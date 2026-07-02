use std::{
    collections::VecDeque,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::NonNull,
    sync::{Arc, Mutex},
    time::Instant,
};

use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMTime, CMTimeFlags};
use objc2_core_video::{CVImageBuffer, CVPixelBuffer};
use objc2_video_toolbox::VTDecodeInfoFlags;

use crate::{
    runtime::{LatestFrameMailbox, StatsHandle},
    DecodedFrame, VideoTimestamp,
};

use super::MacOsVideoFrame;

#[derive(Clone)]
pub(crate) struct CallbackState {
    frames: LatestFrameMailbox<DecodedFrame<MacOsVideoFrame>>,
    stats: StatsHandle,
    pending: Arc<Mutex<VecDeque<(VideoTimestamp, Instant)>>>,
}

impl CallbackState {
    pub(crate) fn new(
        frames: LatestFrameMailbox<DecodedFrame<MacOsVideoFrame>>,
        stats: StatsHandle,
    ) -> Self {
        Self {
            frames,
            stats,
            pending: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub(crate) fn submitted(&self, timestamp: VideoTimestamp) {
        self.pending
            .lock()
            .expect("VideoToolbox pending-frame mutex poisoned")
            .push_back((timestamp, Instant::now()));
        self.stats.update(|stats| {
            stats.access_units_submitted += 1;
            stats.frames_in_flight += 1;
        });
    }

    pub(crate) fn rejected(&self, timestamp: VideoTimestamp, status: i32) {
        let was_pending = self.remove_pending(timestamp).is_some();
        self.stats.update(|stats| {
            if was_pending {
                stats.frames_in_flight = stats.frames_in_flight.saturating_sub(1);
            }
            stats.decode_errors += 1;
            stats.last_platform_status = Some(status);
        });
    }

    pub(crate) fn output(
        &self,
        status: i32,
        info_flags: u32,
        pixel_buffer: Option<CFRetained<CVPixelBuffer>>,
        timestamp: VideoTimestamp,
        duration: Option<VideoTimestamp>,
    ) {
        let Some(submitted_at) = self.remove_pending(timestamp) else {
            self.stats.update(|stats| {
                stats.decode_errors += 1;
                stats.last_platform_status = Some(status);
            });
            return;
        };
        let latency_us = u64::try_from(submitted_at.elapsed().as_micros()).unwrap_or(u64::MAX);

        self.stats.update(|stats| {
            stats.frames_in_flight = stats.frames_in_flight.saturating_sub(1);
            stats.last_platform_status = (status != 0).then_some(status);
            if status != 0 || pixel_buffer.is_none() {
                stats.decode_errors += 1;
            }
            stats.last_decode_latency_us = latency_us;
            stats.max_decode_latency_us = stats.max_decode_latency_us.max(latency_us);
        });
        if status != 0 {
            return;
        }
        let Some(pixel_buffer) = pixel_buffer else {
            return;
        };
        let frame = DecodedFrame {
            surface: MacOsVideoFrame::new(pixel_buffer, info_flags),
            timestamp,
            duration,
        };
        let replaced = self.frames.replace(frame);
        self.stats.update(|stats| {
            stats.frames_decoded += 1;
            if replaced {
                stats.output_drops += 1;
            }
        });
    }

    fn remove_pending(&self, timestamp: VideoTimestamp) -> Option<Instant> {
        let mut pending = self
            .pending
            .lock()
            .expect("VideoToolbox pending-frame mutex poisoned");
        let position = pending
            .iter()
            .position(|(candidate, _)| *candidate == timestamp)?;
        pending.remove(position).map(|(_, instant)| instant)
    }
}

pub(crate) unsafe extern "C-unwind" fn decompression_output_callback(
    output_refcon: *mut std::ffi::c_void,
    _source_refcon: *mut std::ffi::c_void,
    status: i32,
    info_flags: VTDecodeInfoFlags,
    image_buffer: *mut CVImageBuffer,
    presentation_time: CMTime,
    presentation_duration: CMTime,
) {
    // No Rust panic may cross the VideoToolbox callback boundary.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(context) = NonNull::new(output_refcon.cast::<CallbackState>()) else {
            return;
        };
        // SAFETY: The session owns the boxed callback context until it has
        // waited for all asynchronous callbacks and invalidated the session.
        let context = unsafe { context.as_ref() };
        let timestamp = timestamp_from_cm_time(presentation_time).unwrap_or_default();
        let duration = timestamp_from_cm_time(presentation_duration);
        let pixel_buffer = NonNull::new(image_buffer).map(|buffer| {
            // SAFETY: VideoToolbox lends a valid CVImageBuffer for the callback
            // duration. Retaining it allows the app to consume it afterward.
            unsafe { CFRetained::retain(buffer) }
        });
        context.output(status, info_flags.0, pixel_buffer, timestamp, duration);
    }));
}

fn timestamp_from_cm_time(time: CMTime) -> Option<VideoTimestamp> {
    (time.flags.contains(CMTimeFlags::Valid) && time.timescale > 0)
        .then_some(VideoTimestamp::new(time.value, time.timescale))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::CallbackState;
    use crate::{runtime::StatsHandle, VideoTimestamp};

    #[test]
    fn unknown_callback_does_not_consume_another_pending_frame() {
        let stats = StatsHandle::default();
        let state = CallbackState::new(Default::default(), stats.clone());
        let submitted = VideoTimestamp::from_rtp(10);
        state.submitted(submitted);

        state.output(-1, 0, None, VideoTimestamp::from_rtp(11), None);

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.frames_in_flight, 1);
        assert_eq!(snapshot.decode_errors, 1);
        state.rejected(submitted, -2);
        assert_eq!(stats.snapshot().frames_in_flight, 0);
    }
}
