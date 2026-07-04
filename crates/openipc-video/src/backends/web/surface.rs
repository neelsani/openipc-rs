use crate::{DecodedSurface, FrameDimensions, PixelFormat};

/// Retained browser decoder output backed by a WebCodecs `VideoFrame`.
///
/// The frame is closed when this value is dropped. Clone it before handing it
/// to JavaScript code that needs to retain the frame independently.
#[derive(Debug)]
pub struct WebVideoFrame {
    frame: web_codecs::VideoFrame,
    dimensions: FrameDimensions,
}

impl WebVideoFrame {
    pub(crate) fn new(frame: web_codecs::VideoFrame) -> Self {
        let dimensions = frame.dimensions();
        Self {
            frame,
            dimensions: FrameDimensions {
                width: dimensions.width,
                height: dimensions.height,
            },
        }
    }

    /// Take ownership of a transferable browser `VideoFrame`.
    ///
    /// This is used by applications that decode in a dedicated Web Worker and
    /// transfer the native frame back to their presentation thread. The frame
    /// is closed automatically when this surface is dropped.
    pub fn from_video_frame(frame: web_sys::VideoFrame) -> Self {
        Self::new(web_codecs::VideoFrame::from(frame))
    }

    /// Borrow the WebCodecs frame for direct canvas or WebGPU use.
    pub fn video_frame(&self) -> &web_sys::VideoFrame {
        &self.frame
    }

    /// Clone the browser handle for JavaScript or another rendering owner.
    ///
    /// The returned frame must eventually be closed by its new owner.
    pub fn clone_video_frame(&self) -> web_sys::VideoFrame {
        web_sys::VideoFrame::from(self.frame.clone())
    }
}

impl DecodedSurface for WebVideoFrame {
    fn dimensions(&self) -> FrameDimensions {
        self.dimensions
    }

    fn pixel_format(&self) -> PixelFormat {
        match self.frame.format() {
            Some(web_sys::VideoPixelFormat::Nv12) => PixelFormat::Nv12VideoRange,
            Some(web_sys::VideoPixelFormat::Bgra | web_sys::VideoPixelFormat::Bgrx) => {
                PixelFormat::Bgra8
            }
            Some(format) => PixelFormat::Native(format as u32),
            None => PixelFormat::Native(0),
        }
    }
}
