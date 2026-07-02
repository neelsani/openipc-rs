#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        env, fs, thread,
        time::{Duration, Instant},
    };

    use openipc_video::{
        DecoderOptions, EncodedAccessUnit, PlatformDecoder, VideoCodec, VideoDecoder,
        VideoTimestamp,
    };

    let mut args = env::args().skip(1);
    let codec =
        match args.next().as_deref() {
            Some("h264") => VideoCodec::H264,
            Some("h265") | Some("hevc") => VideoCodec::H265,
            _ => return Err(
                "usage: decode_access_unit <h264|h265> <annex-b-access-unit> [--allow-software]"
                    .into(),
            ),
        };
    let path = args
        .next()
        .ok_or("usage: decode_access_unit <h264|h265> <annex-b-access-unit> [--allow-software]")?;
    let allow_software = args.any(|argument| argument == "--allow-software");
    let data = fs::read(path)?;
    let capabilities = PlatformDecoder::probe_capabilities();
    println!("capabilities={capabilities:?}");
    let mut decoder = PlatformDecoder::new(DecoderOptions {
        require_hardware: !allow_software,
        ..DecoderOptions::default()
    })?;
    let outcome = decoder.submit(EncodedAccessUnit::new(
        codec,
        data,
        VideoTimestamp::from_rtp(0),
        true,
    ))?;
    println!("submit={outcome:?}");

    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if let Some(frame) = decoder.latest_frame() {
            let dimensions = frame.dimensions();
            println!(
                "decoded={}x{} pixel_format={:?}",
                dimensions.width,
                dimensions.height,
                openipc_video::DecodedSurface::pixel_format(&frame.surface),
            );
            #[cfg(target_os = "macos")]
            println!("iosurface={}", frame.surface.is_io_surface_backed());
            #[cfg(target_os = "linux")]
            println!(
                "drm_fourcc={:#010x} pitches={:?}",
                frame.surface.drm_fourcc(),
                frame.surface.plane_pitches()
            );
            #[cfg(target_os = "windows")]
            println!(
                "d3d11_subresource={} texture={:?}",
                frame.surface.subresource_index(),
                frame.surface.texture()
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(2));
    }
    Err(format!("no decoded frame; stats={:?}", decoder.stats()).into())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn main() {
    eprintln!("decode_access_unit supports macOS, Linux, and Windows");
}
