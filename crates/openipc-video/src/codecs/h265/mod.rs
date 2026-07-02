//! H.265 decoder configuration assembled from VPS, SPS, and PPS NAL units.

mod config;
mod inspect;

pub use config::H265Config;
pub(crate) use inspect::{is_keyframe, nal_type};
