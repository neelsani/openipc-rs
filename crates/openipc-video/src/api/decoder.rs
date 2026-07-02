use super::{
    CodecConfig, DecodedFrame, DecodedSurface, DecoderCapabilities, DecoderStats,
    EncodedAccessUnit, VideoError,
};

/// Result of offering an access unit to a low-latency decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// Frame was accepted by the platform decoder.
    Submitted,
    /// Parameter sets have not yet formed a complete decoder configuration.
    WaitingForConfiguration,
    /// Decoder was reconfigured and accepted this frame.
    Reconfigured,
    /// A delta frame was intentionally skipped until the next keyframe.
    WaitingForKeyframe,
    /// Decoder queue was full and the frame was dropped.
    DroppedForBackpressure,
}

/// Common behavior implemented by platform video decoders.
pub trait VideoDecoder {
    /// Native decoded surface type produced by this backend.
    type Surface: DecodedSurface;

    /// Query decoder support on the current machine.
    fn capabilities(&self) -> DecoderCapabilities;

    /// Explicitly configure or reconfigure the platform decoder.
    fn configure(&mut self, config: CodecConfig) -> Result<(), VideoError>;

    /// Submit one complete encoded Annex-B access unit.
    fn submit(&mut self, frame: EncodedAccessUnit) -> Result<SubmitOutcome, VideoError>;

    /// Take the newest decoded frame, if one is ready.
    fn latest_frame(&mut self) -> Option<DecodedFrame<Self::Surface>>;

    /// Clear decoder state and queued work.
    ///
    /// Native backends finish work when their platform API supports a
    /// synchronous drain. WebCodecs closes immediately; use
    /// `WebDecoder::flush_async` when browser output must be drained first.
    fn flush(&mut self) -> Result<(), VideoError>;

    /// Read cumulative decoder statistics.
    fn stats(&self) -> DecoderStats;
}
