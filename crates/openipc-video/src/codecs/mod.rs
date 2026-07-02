//! Codec framing and decoder configuration independent of any platform API.

mod access_unit;
/// Split and convert Annex-B byte streams without interpreting codec syntax.
pub mod annex_b;
/// H.264 parameter-set configuration types.
pub mod h264;
/// H.265 parameter-set configuration types.
pub mod h265;

pub use access_unit::{CodecConfigTracker, ConfigUpdate};
pub use h264::H264Config;
pub use h265::H265Config;
