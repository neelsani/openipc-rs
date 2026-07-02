use crate::{
    backends::test_fixtures::{H264_KEYFRAME, H265_KEYFRAME},
    DecodedSurface, DecoderOptions, EncodedAccessUnit, LinuxDecoder, PixelFormat, VideoCodec,
    VideoDecoder, VideoTimestamp,
};

#[test]
#[ignore = "requires a Linux VA-API device and permission to open its DRM render node"]
fn decodes_h264_to_a_dma_frame() {
    decode_fixture(VideoCodec::H264, H264_KEYFRAME);
}

#[test]
#[ignore = "requires a Linux VA-API device and permission to open its DRM render node"]
fn decodes_h265_to_a_dma_frame() {
    decode_fixture(VideoCodec::H265, H265_KEYFRAME);
}

fn decode_fixture(codec: VideoCodec, data: &[u8]) {
    let mut decoder = LinuxDecoder::new(DecoderOptions::default()).unwrap();
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
            frame
                .surface
                .with_mapped_planes(|planes| {
                    assert_eq!(planes.len(), 2);
                    assert!(planes.iter().all(|plane| !plane.is_empty()));
                })
                .unwrap();
            return;
        }
    }
    panic!("VA-API produced no frame; stats={:?}", decoder.stats());
}
