use std::{thread, time::Duration};

use crate::{
    backends::test_fixtures::{H264_KEYFRAME, H265_KEYFRAME},
    DecodedSurface, DecoderOptions, EncodedAccessUnit, VideoCodec, VideoDecoder, VideoTimestamp,
};

use super::AndroidDecoder;

#[test]
#[ignore = "requires an Android device with MediaCodec and AHardwareBuffer video output"]
fn decodes_h264_to_a_hardware_buffer() {
    decode_fixture(VideoCodec::H264, H264_KEYFRAME);
}

#[test]
#[ignore = "requires an Android device with MediaCodec and AHardwareBuffer video output"]
fn decodes_h265_to_a_hardware_buffer() {
    decode_fixture(VideoCodec::H265, H265_KEYFRAME);
}

fn decode_fixture(codec: VideoCodec, data: &[u8]) {
    let mut decoder = AndroidDecoder::new(DecoderOptions::default()).unwrap();
    decoder
        .submit(EncodedAccessUnit::new(
            codec,
            data.to_vec(),
            VideoTimestamp::from_rtp(0),
            true,
        ))
        .unwrap();
    for _ in 0..100 {
        if let Some(frame) = decoder.latest_frame() {
            assert_eq!(frame.surface.dimensions().width, 128);
            assert_eq!(frame.surface.dimensions().height, 128);
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("MediaCodec produced no frame; stats={:?}", decoder.stats());
}
