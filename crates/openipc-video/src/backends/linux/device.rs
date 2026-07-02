use std::{env, path::PathBuf, rc::Rc, sync::Arc};

use cros_codecs::{
    libva::{Display, VAEntrypoint, VAProfile},
    video_frame::gbm_video_frame::GbmDevice,
};

use crate::{CodecCapability, DecoderCapabilities, VideoCodec, VideoError};

pub(crate) struct VaDevice {
    pub(crate) path: PathBuf,
    pub(crate) display: Rc<Display>,
    pub(crate) gbm: Arc<GbmDevice>,
    pub(crate) capabilities: DecoderCapabilities,
    pub(crate) vendor: Option<String>,
}

impl VaDevice {
    pub(crate) fn open(path: Option<PathBuf>) -> Result<Self, VideoError> {
        let candidates = match path {
            Some(path) => vec![path],
            None => candidate_render_nodes(),
        };
        let mut failures = Vec::new();
        for path in candidates {
            let display = match Display::open_drm_display(&path) {
                Ok(display) => display,
                Err(error) => {
                    failures.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            let gbm = match GbmDevice::open(&path) {
                Ok(gbm) => gbm,
                Err(error) => {
                    failures.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            let capabilities = capabilities_for_display(&display);
            let vendor = display.query_vendor_string().ok();
            return Ok(Self {
                path,
                display,
                gbm,
                capabilities,
                vendor,
            });
        }

        Err(VideoError::Backend {
            backend: "vaapi",
            operation: "open DRM render node",
            message: if failures.is_empty() {
                "no /dev/dri/renderD* device is available".to_owned()
            } else {
                failures.join("; ")
            },
        })
    }
}

pub(crate) fn probe_capabilities() -> DecoderCapabilities {
    VaDevice::open(None)
        .map(|device| device.capabilities)
        .unwrap_or_else(|_| unsupported_capabilities())
}

fn candidate_render_nodes() -> Vec<PathBuf> {
    if let Some(path) = env::var_os("OPENIPC_VAAPI_DEVICE") {
        return vec![PathBuf::from(path)];
    }
    (128..144)
        .map(|index| PathBuf::from(format!("/dev/dri/renderD{index}")))
        .collect()
}

fn capabilities_for_display(display: &Display) -> DecoderCapabilities {
    let profiles = display.query_config_profiles().unwrap_or_default();
    let supports = |candidates: &[VAProfile::Type]| {
        candidates.iter().any(|profile| {
            profiles.contains(profile)
                && display
                    .query_config_entrypoints(*profile)
                    .is_ok_and(|entrypoints| entrypoints.contains(&VAEntrypoint::VAEntrypointVLD))
        })
    };
    let h264 = supports(&[
        VAProfile::VAProfileH264ConstrainedBaseline,
        VAProfile::VAProfileH264Main,
        VAProfile::VAProfileH264High,
    ]);
    // The shared Linux surface pool is NV12, so advertise the 8-bit Main
    // profile only. Main10 requires a separate P010 allocation path.
    let h265 = supports(&[VAProfile::VAProfileHEVCMain]);
    DecoderCapabilities {
        backend: "vaapi",
        codecs: vec![
            CodecCapability {
                codec: VideoCodec::H264,
                supported: h264,
                hardware_accelerated: h264,
                hardware_acceleration_known: true,
            },
            CodecCapability {
                codec: VideoCodec::H265,
                supported: h265,
                hardware_accelerated: h265,
                hardware_acceleration_known: true,
            },
        ],
        native_surfaces: true,
    }
}

fn unsupported_capabilities() -> DecoderCapabilities {
    DecoderCapabilities {
        backend: "vaapi",
        codecs: vec![
            CodecCapability {
                codec: VideoCodec::H264,
                supported: false,
                hardware_accelerated: false,
                hardware_acceleration_known: true,
            },
            CodecCapability {
                codec: VideoCodec::H265,
                supported: false,
                hardware_accelerated: false,
                hardware_acceleration_known: true,
            },
        ],
        native_surfaces: true,
    }
}
