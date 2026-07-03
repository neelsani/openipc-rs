use std::collections::{BTreeMap, VecDeque};

pub(crate) const METRIC_WINDOW_SECONDS: f64 = 15.0;

/// Current lifecycle state of the receiver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ReceiverState {
    #[default]
    Idle,
    Connecting,
    Ready,
    Receiving,
    Stopping,
    Failed,
}

impl ReceiverState {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Connecting => "CONNECTING",
            Self::Ready => "READY",
            Self::Receiving => "RECEIVING",
            Self::Stopping => "STOPPING",
            Self::Failed => "ERROR",
        }
    }
}

/// Severity attached to one application log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub(crate) const fn priority(self) -> u8 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
        }
    }

    pub(crate) const fn from_log(level: log::Level) -> Self {
        match level {
            log::Level::Trace => Self::Trace,
            log::Level::Debug => Self::Debug,
            log::Level::Info => Self::Info,
            log::Level::Warn => Self::Warn,
            log::Level::Error => Self::Error,
        }
    }
}

/// Timestamped diagnostic line displayed by the app.
#[derive(Debug, Clone)]
pub(crate) struct LogEntry {
    pub(crate) sequence: u64,
    pub(crate) elapsed_seconds: f64,
    pub(crate) level: LogLevel,
    pub(crate) target: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RouteStats {
    pub(crate) packets: u64,
    pub(crate) bytes: u64,
    pub(crate) last_bytes: usize,
    pub(crate) errors: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AudioStats {
    pub(crate) enabled: bool,
    pub(crate) supported: bool,
    pub(crate) decoder_name: String,
    pub(crate) packets: u64,
    pub(crate) bytes: u64,
    pub(crate) decoded_frames: u64,
    pub(crate) errors: u64,
    pub(crate) queued_ms: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RecordingState {
    #[default]
    Idle,
    Armed,
    Recording,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RecordingStatus {
    pub(crate) state: RecordingState,
    pub(crate) path: String,
    pub(crate) codec: String,
    pub(crate) bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EnvironmentDetails {
    pub(crate) platform: String,
    pub(crate) architecture: String,
    pub(crate) runtime: String,
    pub(crate) renderer: String,
    pub(crate) logical_processors: String,
    pub(crate) user_agent: String,
    pub(crate) decoder_backend: String,
    pub(crate) h264: String,
    pub(crate) h265: String,
    pub(crate) native_surfaces: bool,
    pub(crate) maximum_observed_resolution: Option<[u32; 2]>,
    pub(crate) maximum_observed_fps: f64,
}

impl EnvironmentDetails {
    pub(crate) fn detect() -> Self {
        #[cfg(target_arch = "wasm32")]
        let (logical_processors, user_agent) = web_sys::window()
            .map(|window| {
                let navigator = window.navigator();
                (
                    navigator.hardware_concurrency().to_string(),
                    navigator
                        .user_agent()
                        .unwrap_or_else(|_| "Unavailable".to_owned()),
                )
            })
            .unwrap_or_else(|| ("Unavailable".to_owned(), "Unavailable".to_owned()));
        #[cfg(not(target_arch = "wasm32"))]
        let (logical_processors, user_agent) = (
            std::thread::available_parallelism().map_or_else(
                |_| "Unavailable".to_owned(),
                |count| count.get().to_string(),
            ),
            "Native application".to_owned(),
        );
        Self {
            platform: std::env::consts::OS.to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            runtime: if cfg!(target_arch = "wasm32") {
                "Browser / WebAssembly"
            } else if cfg!(target_os = "android") {
                "Android NativeActivity"
            } else {
                "Native eframe"
            }
            .to_owned(),
            renderer: if cfg!(target_arch = "wasm32") {
                "WebGL"
            } else {
                "wgpu"
            }
            .to_owned(),
            logical_processors,
            user_agent,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VpnStatus {
    pub(crate) active: bool,
    pub(crate) interface_name: String,
    pub(crate) downlink_packets: u64,
    pub(crate) downlink_bytes: u64,
    pub(crate) uplink_packets: u64,
    pub(crate) uplink_bytes: u64,
    pub(crate) errors: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LatencySummary {
    pub(crate) last: f64,
    pub(crate) average: f64,
    pub(crate) p95: f64,
    pub(crate) maximum: f64,
    pub(crate) samples: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LatencySeries {
    samples: VecDeque<f64>,
}

impl LatencySeries {
    pub(crate) fn observe(&mut self, value: f64) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        if self.samples.len() == 240 {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    pub(crate) fn summary(&self) -> LatencySummary {
        let Some(last) = self.samples.back().copied() else {
            return LatencySummary::default();
        };
        let mut sorted = self.samples.iter().copied().collect::<Vec<_>>();
        sorted.sort_by(f64::total_cmp);
        let average = sorted.iter().sum::<f64>() / sorted.len() as f64;
        let p95_index = ((sorted.len() - 1) as f64 * 0.95).round() as usize;
        LatencySummary {
            last,
            average,
            p95: sorted[p95_index],
            maximum: sorted.last().copied().unwrap_or_default(),
            samples: sorted.len(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DiagnosticsState {
    pub(crate) counters: openipc_core::ReceiverBatchCounters,
    pub(crate) rtp: openipc_core::RtpDepacketizerStatus,
    pub(crate) reorder: openipc_core::RtpReorderStatus,
    pub(crate) stages: BTreeMap<&'static str, LatencySeries>,
}

impl DiagnosticsState {
    pub(crate) fn observe(&mut self, stage: &'static str, milliseconds: f64) {
        self.stages.entry(stage).or_default().observe(milliseconds);
    }
}

/// A bounded time series used by live telemetry plots.
#[derive(Debug, Clone)]
pub(crate) struct MetricSeries {
    values: VecDeque<[f64; 2]>,
    capacity: usize,
}

impl MetricSeries {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub(crate) fn push(&mut self, time: f64, value: f64) {
        let oldest_time = time - METRIC_WINDOW_SECONDS;
        while self
            .values
            .front()
            .is_some_and(|point| point[0] < oldest_time)
        {
            self.values.pop_front();
        }
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back([time, value]);
    }

    pub(crate) fn points(&self) -> impl Iterator<Item = [f64; 2]> + '_ {
        self.values.iter().copied()
    }

    pub(crate) fn clear(&mut self) {
        self.values.clear();
    }

    pub(crate) fn latest_time(&self) -> Option<f64> {
        self.values.back().map(|point| point[0])
    }
}

impl Default for MetricSeries {
    fn default() -> Self {
        Self::new(600)
    }
}

/// Aggregated live receiver and decoder statistics.
#[derive(Debug, Clone, Default)]
pub(crate) struct LiveMetrics {
    pub(crate) usb_bytes: u64,
    pub(crate) usb_transfers: u64,
    pub(crate) wifi_packets: u64,
    pub(crate) rtp_packets: u64,
    pub(crate) encoded_frames: u64,
    pub(crate) decoded_frames: u64,
    pub(crate) render_frames: u64,
    pub(crate) fec_total_packets: u64,
    pub(crate) recovered_packets: u64,
    pub(crate) lost_packets: u64,
    pub(crate) decoder_drops: u64,
    pub(crate) decoder_errors: u64,
    pub(crate) bitrate_bps: f64,
    pub(crate) receive_fps: f64,
    pub(crate) decode_fps: f64,
    pub(crate) render_fps: f64,
    pub(crate) rssi: [i32; 2],
    pub(crate) snr: [i32; 2],
    pub(crate) link_score: [i32; 2],
    pub(crate) usb_latency_ms: f64,
    pub(crate) pipeline_latency_ms: f64,
    pub(crate) batch_latency_ms: f64,
    pub(crate) video_submit_path_ms: f64,
    pub(crate) decode_latency_ms: f64,
    pub(crate) presentation_queue_latency_ms: f64,
    pub(crate) local_processing_latency_ms: f64,
    pub(crate) resolution: Option<[u32; 2]>,
    pub(crate) decoder_name: String,
}

impl LiveMetrics {
    pub(crate) fn decoder_label(&self) -> &str {
        if self.decoder_name.is_empty() {
            "Idle"
        } else {
            &self.decoder_name
        }
    }
}

/// Time-series history displayed in the Metrics tab.
#[derive(Debug, Default)]
pub(crate) struct MetricHistory {
    pub(crate) link_score: MetricSeries,
    pub(crate) fec_recovery: MetricSeries,
    pub(crate) loss: MetricSeries,
    pub(crate) bitrate: MetricSeries,
    pub(crate) receive_fps: MetricSeries,
    pub(crate) local_processing_ms: MetricSeries,
}

impl MetricHistory {
    pub(crate) fn latest_time(&self) -> f64 {
        self.link_score.latest_time().unwrap_or(0.0)
    }

    pub(crate) fn clear(&mut self) {
        self.link_score.clear();
        self.fec_recovery.clear();
        self.loss.clear();
        self.bitrate.clear();
        self.receive_fps.clear();
        self.local_processing_ms.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_series_keeps_only_the_rolling_time_window() {
        let mut series = MetricSeries::new(1_000);
        series.push(0.0, 1.0);
        series.push(14.0, 2.0);
        series.push(16.0, 3.0);

        assert_eq!(
            series.points().collect::<Vec<_>>(),
            vec![[14.0, 2.0], [16.0, 3.0]]
        );
        assert_eq!(series.latest_time(), Some(16.0));
    }

    #[test]
    fn latency_summary_reports_average_percentile_and_maximum() {
        let mut series = LatencySeries::default();
        for value in [1.0, 2.0, 3.0, 10.0] {
            series.observe(value);
        }
        let summary = series.summary();
        assert_eq!(summary.last, 10.0);
        assert_eq!(summary.average, 4.0);
        assert_eq!(summary.p95, 10.0);
        assert_eq!(summary.maximum, 10.0);
        assert_eq!(summary.samples, 4);
    }
}
