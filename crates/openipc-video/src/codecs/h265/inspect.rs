pub(crate) fn nal_type(data: &[u8]) -> Option<u8> {
    data.first().map(|byte| (byte >> 1) & 0x3f)
}

pub(crate) fn is_keyframe(data: &[u8]) -> bool {
    matches!(nal_type(data), Some(16..=21))
}
