use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;

use crate::{
    backends::test_fixtures::{H264_KEYFRAME, H265_KEYFRAME},
    DecodedSurface, DecoderOptions, EncodedAccessUnit, PixelFormat, VideoCodec, VideoDecoder,
    VideoTimestamp, WindowsDecoder,
};

#[test]
#[ignore = "requires a Windows D3D11 hardware video decoder"]
fn decodes_h264_to_a_d3d11_texture() {
    decode_fixture(VideoCodec::H264, H264_KEYFRAME);
}

#[test]
#[ignore = "requires a Windows D3D11 hardware video decoder"]
fn decodes_h265_to_a_d3d11_texture() {
    decode_fixture(VideoCodec::H265, H265_KEYFRAME);
}

fn decode_fixture(codec: VideoCodec, data: &[u8]) {
    let mut decoder = WindowsDecoder::new(DecoderOptions::default()).unwrap();
    for timestamp in 0..3 {
        decoder
            .submit(EncodedAccessUnit::new(
                codec,
                data.to_vec(),
                VideoTimestamp::from_rtp(timestamp),
                true,
            ))
            .unwrap();
        if let Some(frame) = decoder.latest_frame() {
            assert_eq!(frame.dimensions().width, 128);
            assert_eq!(frame.dimensions().height, 128);
            assert_eq!(frame.surface.pixel_format(), PixelFormat::Nv12VideoRange);
            assert_eq!(frame.surface.texture_desc().Format, DXGI_FORMAT_NV12);
            return;
        }
    }
    panic!(
        "Media Foundation produced no frame; stats={:?}",
        decoder.stats()
    );
}
