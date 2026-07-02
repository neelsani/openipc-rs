/// Visible dimensions of a decoded frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameDimensions {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Platform-independent description of common decoded pixel formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PixelFormat {
    /// Bi-planar 8-bit YUV 4:2:0, video range.
    Nv12VideoRange,
    /// Bi-planar 8-bit YUV 4:2:0, full range.
    Nv12FullRange,
    /// 32-bit BGRA.
    Bgra8,
    /// Platform-specific format represented by its native code.
    Native(u32),
}

/// A decoded platform surface retained from the operating system or browser.
///
/// Desktop surface implementations are `Send + Sync`. Browser `VideoFrame`
/// objects and some mobile image leases are intentionally thread-local, so
/// cross-thread applications should add `Send + Sync` to their own generic
/// bounds when they require it.
pub trait DecodedSurface: 'static {
    /// Visible frame dimensions.
    fn dimensions(&self) -> FrameDimensions;

    /// Pixel format of the retained surface.
    fn pixel_format(&self) -> PixelFormat;
}
