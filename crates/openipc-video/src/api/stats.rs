/// Cumulative decoder and backpressure statistics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecoderStats {
    /// Encoded access units offered to the decoder.
    pub access_units_received: u64,
    /// Access units accepted by the platform decoder.
    pub access_units_submitted: u64,
    /// Access units ignored while waiting for parameter sets or a keyframe.
    pub waiting_drops: u64,
    /// Access units dropped because the decoder queue was full.
    pub backpressure_drops: u64,
    /// Successful decoded frames received from the platform.
    pub frames_decoded: u64,
    /// Decoded frames replaced before the application consumed them.
    pub output_drops: u64,
    /// Synchronous or asynchronous platform decoder errors.
    pub decode_errors: u64,
    /// Number of decoder session configurations or reconfigurations.
    pub reconfigurations: u64,
    /// Frames currently owned by the platform decoder.
    pub frames_in_flight: usize,
    /// Most recent submit-to-output latency in microseconds.
    pub last_decode_latency_us: u64,
    /// Highest observed submit-to-output latency in microseconds.
    pub max_decode_latency_us: u64,
    /// Most recent native platform error code.
    pub last_platform_status: Option<i32>,
}
