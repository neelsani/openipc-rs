use super::VideoCodec;

/// Runtime support reported for one codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecCapability {
    /// Codec being described.
    pub codec: VideoCodec,
    /// The platform can create a decoder for this codec.
    pub supported: bool,
    /// A hardware decoder is advertised for this codec.
    pub hardware_accelerated: bool,
    /// Whether `hardware_accelerated` is a definite platform report.
    ///
    /// Android API 26 and WebCodecs can express a preference but cannot expose
    /// a reliable hardware/software classification, so this is false there.
    pub hardware_acceleration_known: bool,
}

/// Capabilities exposed by a decoder backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderCapabilities {
    /// Stable backend identifier such as `videotoolbox`.
    pub backend: &'static str,
    /// Codec support advertised by the current machine.
    pub codecs: Vec<CodecCapability>,
    /// Decoded frames can remain in GPU-compatible platform memory.
    pub native_surfaces: bool,
}

impl DecoderCapabilities {
    /// Find support information for `codec`.
    pub fn codec(&self, codec: VideoCodec) -> Option<CodecCapability> {
        self.codecs
            .iter()
            .copied()
            .find(|entry| entry.codec == codec)
    }
}
