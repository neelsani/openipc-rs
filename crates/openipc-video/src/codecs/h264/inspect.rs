pub(crate) fn nal_type(data: &[u8]) -> Option<u8> {
    data.first().map(|byte| byte & 0x1f)
}

pub(crate) fn is_keyframe(data: &[u8]) -> bool {
    nal_type(data) == Some(5)
}
