use std::{
    collections::VecDeque,
    rc::Rc,
    sync::{Arc, Mutex, Weak},
};

use cros_codecs::{
    libva::{Display, Surface},
    video_frame::{
        gbm_video_frame::{GbmDevice, GbmUsage},
        generic_dma_video_frame::GenericDmaVideoFrame,
        ReadMapping, VideoFrame, WriteMapping,
    },
    Fourcc, Resolution,
};

use crate::VideoError;

#[derive(Debug)]
pub(crate) struct VaFrame {
    inner: Option<GenericDmaVideoFrame>,
    pool: Weak<Mutex<VecDeque<GenericDmaVideoFrame>>>,
}

impl VaFrame {
    fn inner(&self) -> &GenericDmaVideoFrame {
        self.inner
            .as_ref()
            .expect("VA frame remains present until its final drop")
    }

    fn inner_mut(&mut self) -> &mut GenericDmaVideoFrame {
        self.inner
            .as_mut()
            .expect("VA frame remains present until its final drop")
    }
}

impl Drop for VaFrame {
    fn drop(&mut self) {
        let Some(frame) = self.inner.take() else {
            return;
        };
        if let Some(pool) = self.pool.upgrade() {
            if let Ok(mut frames) = pool.lock() {
                frames.push_back(frame);
            }
        }
    }
}

impl VideoFrame for VaFrame {
    type MemDescriptor = GenericDmaVideoFrame;
    type NativeHandle = Surface<GenericDmaVideoFrame>;

    fn fourcc(&self) -> Fourcc {
        self.inner().fourcc()
    }

    fn resolution(&self) -> Resolution {
        self.inner().resolution()
    }

    fn get_plane_size(&self) -> Vec<usize> {
        self.inner().get_plane_size()
    }

    fn get_plane_pitch(&self) -> Vec<usize> {
        self.inner().get_plane_pitch()
    }

    fn map<'a>(&'a self) -> Result<Box<dyn ReadMapping<'a> + 'a>, String> {
        self.inner().map()
    }

    fn map_mut<'a>(&'a mut self) -> Result<Box<dyn WriteMapping<'a> + 'a>, String> {
        self.inner_mut().map_mut()
    }

    fn to_native_handle(&self, display: &Rc<Display>) -> Result<Self::NativeHandle, String> {
        self.inner().to_native_handle(display)
    }
}

pub(crate) struct VaFramePool {
    gbm: Arc<GbmDevice>,
    frames: Option<Arc<Mutex<VecDeque<GenericDmaVideoFrame>>>>,
    retained_frame_allowance: usize,
}

impl VaFramePool {
    pub(crate) fn new(gbm: Arc<GbmDevice>, retained_frame_allowance: usize) -> Self {
        Self {
            gbm,
            frames: None,
            retained_frame_allowance,
        }
    }

    pub(crate) fn resize(
        &mut self,
        display_resolution: Resolution,
        coded_resolution: Resolution,
        minimum_frames: usize,
    ) -> Result<(), VideoError> {
        let count = minimum_frames
            .checked_add(self.retained_frame_allowance)
            .ok_or(VideoError::InvalidOption("VA frame-pool size overflow"))?;
        let mut frames = VecDeque::with_capacity(count);
        for _ in 0..count {
            let frame = Arc::clone(&self.gbm)
                .new_frame(
                    Fourcc::from(b"NV12"),
                    display_resolution,
                    coded_resolution,
                    GbmUsage::Decode,
                )
                .and_then(|frame| frame.to_generic_dma_video_frame())
                .map_err(|message| VideoError::Backend {
                    backend: "vaapi",
                    operation: "allocate DMA output surface",
                    message,
                })?;
            frames.push_back(frame);
        }
        self.frames = Some(Arc::new(Mutex::new(frames)));
        Ok(())
    }

    pub(crate) fn allocate(&self) -> Option<VaFrame> {
        let pool = self.frames.as_ref()?;
        let inner = pool.lock().ok()?.pop_front()?;
        Some(VaFrame {
            inner: Some(inner),
            pool: Arc::downgrade(pool),
        })
    }
}
