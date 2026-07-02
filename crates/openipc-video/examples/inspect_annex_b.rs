use std::{env, fs};

use openipc_video::{codecs::annex_b, CodecConfigTracker, ConfigUpdate, VideoCodec};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let codec = match args.next().as_deref() {
        Some("h264") => VideoCodec::H264,
        Some("h265") | Some("hevc") => VideoCodec::H265,
        _ => return Err("usage: inspect_annex_b <h264|h265> <access-unit-file>".into()),
    };
    let path = args
        .next()
        .ok_or("usage: inspect_annex_b <h264|h265> <access-unit-file>")?;
    let data = fs::read(path)?;
    let units = annex_b::nal_units(&data)?;
    let mut tracker = CodecConfigTracker::default();
    let config = tracker.observe(codec, &data)?;

    println!(
        "codec={codec} bytes={} nal_units={}",
        data.len(),
        units.len()
    );
    println!(
        "configuration={}",
        match config {
            ConfigUpdate::Incomplete => "incomplete",
            ConfigUpdate::Unchanged => "unchanged",
            ConfigUpdate::Changed(_) => "complete",
        }
    );
    Ok(())
}
