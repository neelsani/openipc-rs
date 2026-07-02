use crate::VideoError;

/// One NAL unit borrowed from an Annex-B byte stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NalUnit<'a> {
    /// NAL bytes without the Annex-B start code.
    pub data: &'a [u8],
    /// Byte offset of the start code in the source access unit.
    pub offset: usize,
    /// Length of the three- or four-byte start code.
    pub start_code_len: usize,
}

/// Split an Annex-B access unit into non-empty NAL units.
pub fn nal_units(data: &[u8]) -> Result<Vec<NalUnit<'_>>, VideoError> {
    let Some((first_offset, first_len)) = find_start_code(data, 0) else {
        return Err(VideoError::InvalidAnnexB("no start code"));
    };
    if data[..first_offset].iter().any(|byte| *byte != 0) {
        return Err(VideoError::InvalidAnnexB(
            "non-zero bytes precede the first start code",
        ));
    }

    let mut units = Vec::new();
    let mut start_offset = first_offset;
    let mut start_len = first_len;
    loop {
        let payload_start = start_offset + start_len;
        let next = find_start_code(data, payload_start);
        let payload_end = next.map_or(data.len(), |(offset, _)| offset);
        let payload = trim_trailing_zero_bytes(&data[payload_start..payload_end]);
        if !payload.is_empty() {
            units.push(NalUnit {
                data: payload,
                offset: start_offset,
                start_code_len: start_len,
            });
        }
        let Some((next_offset, next_len)) = next else {
            break;
        };
        start_offset = next_offset;
        start_len = next_len;
    }

    if units.is_empty() {
        return Err(VideoError::InvalidAnnexB("access unit has no NAL data"));
    }
    Ok(units)
}

/// Convert Annex-B NAL units to four-byte big-endian length prefixes.
pub fn to_length_prefixed(data: &[u8]) -> Result<Vec<u8>, VideoError> {
    let units = nal_units(data)?;
    let payload_len = units.iter().try_fold(0usize, |total, unit| {
        let _ = u32::try_from(unit.data.len())
            .map_err(|_| VideoError::NalUnitTooLarge(unit.data.len()))?;
        total
            .checked_add(4 + unit.data.len())
            .ok_or(VideoError::NalUnitTooLarge(unit.data.len()))
    })?;
    let mut output = Vec::with_capacity(payload_len);
    for unit in units {
        let len = u32::try_from(unit.data.len())
            .map_err(|_| VideoError::NalUnitTooLarge(unit.data.len()))?;
        output.extend_from_slice(&len.to_be_bytes());
        output.extend_from_slice(unit.data);
    }
    Ok(output)
}

fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    if data.len() < 3 || from > data.len().saturating_sub(3) {
        return None;
    }
    for offset in from..=data.len() - 3 {
        if data[offset] != 0 || data[offset + 1] != 0 {
            continue;
        }
        if data.get(offset + 2) == Some(&1) {
            return Some((offset, 3));
        }
        if data.get(offset + 2) == Some(&0) && data.get(offset + 3) == Some(&1) {
            return Some((offset, 4));
        }
    }
    None
}

fn trim_trailing_zero_bytes(mut data: &[u8]) -> &[u8] {
    while data.last() == Some(&0) {
        data = &data[..data.len() - 1];
    }
    data
}

#[cfg(test)]
mod tests {
    use super::{nal_units, to_length_prefixed};

    #[test]
    fn splits_three_and_four_byte_start_codes() {
        let data = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3];
        let units = nal_units(&data).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].data, [0x67, 1, 2]);
        assert_eq!(units[1].data, [0x68, 3]);
    }

    #[test]
    fn converts_to_four_byte_lengths() {
        let data = [0, 0, 1, 0x65, 1, 2, 0, 0, 0, 1, 0x41, 3];
        let converted = to_length_prefixed(&data).unwrap();
        assert_eq!(converted, [0, 0, 0, 3, 0x65, 1, 2, 0, 0, 0, 2, 0x41, 3]);
    }

    #[test]
    fn rejects_non_annex_b_data() {
        assert!(nal_units(&[1, 2, 3, 4]).is_err());
    }
}
