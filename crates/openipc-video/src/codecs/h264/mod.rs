//! H.264 decoder configuration assembled from SPS and PPS NAL units.

mod config;
mod inspect;

pub use config::H264Config;
pub(crate) use inspect::{is_keyframe, nal_type};
