use super::VideoCodec;

/// Error produced while configuring or driving a video backend.
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    /// Decoder options were internally inconsistent.
    #[error("invalid decoder option: {0}")]
    InvalidOption(&'static str),
    /// Input did not contain a valid Annex-B NAL unit sequence.
    #[error("invalid Annex-B access unit: {0}")]
    InvalidAnnexB(&'static str),
    /// A NAL unit cannot be represented by the target framing format.
    #[error("NAL unit is too large: {0} bytes")]
    NalUnitTooLarge(usize),
    /// Codec configuration did not match the submitted stream.
    #[error("decoder is configured for {configured}, but received {received}")]
    CodecMismatch {
        /// Active decoder codec.
        configured: VideoCodec,
        /// Submitted access-unit codec.
        received: VideoCodec,
    },
    /// The current platform does not provide the requested decoder.
    #[error("{codec} is not supported by the {backend} backend")]
    UnsupportedCodec {
        /// Requested codec.
        codec: VideoCodec,
        /// Backend identifier.
        backend: &'static str,
    },
    /// The codec exists on this platform, but no hardware decoder is advertised.
    #[error("no hardware {codec} decoder is available through {backend}")]
    HardwareDecoderUnavailable {
        /// Requested codec.
        codec: VideoCodec,
        /// Backend identifier.
        backend: &'static str,
    },
    /// A native platform call returned an error status.
    #[error("{api} failed with platform status {status}")]
    Platform {
        /// Native API that failed.
        api: &'static str,
        /// Native status code.
        status: i32,
    },
    /// A backend operation failed with a platform-specific diagnostic.
    #[error("{backend} {operation} failed: {message}")]
    Backend {
        /// Decoder backend identifier.
        backend: &'static str,
        /// Operation that failed.
        operation: &'static str,
        /// Native library diagnostic.
        message: String,
    },
}
