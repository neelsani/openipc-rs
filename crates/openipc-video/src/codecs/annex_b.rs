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

/// Allocation-free iterator over the NAL units in an Annex-B access unit.
#[derive(Debug, Clone)]
pub struct NalUnits<'a> {
    data: &'a [u8],
    next_start: Option<(usize, usize)>,
    pending: Option<NalUnit<'a>>,
}

impl<'a> Iterator for NalUnits<'a> {
    type Item = NalUnit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(unit) = self.pending.take() {
            return Some(unit);
        }
        self.next_unit()
    }
}

impl<'a> NalUnits<'a> {
    fn next_unit(&mut self) -> Option<NalUnit<'a>> {
        loop {
            let (start_offset, start_code_len) = self.next_start.take()?;
            let payload_start = start_offset + start_code_len;
            let next = find_start_code(self.data, payload_start);
            let payload_end = next.map_or(self.data.len(), |(offset, _)| offset);
            self.next_start = next;
            let data = trim_trailing_zero_bytes(&self.data[payload_start..payload_end]);
            if !data.is_empty() {
                return Some(NalUnit {
                    data,
                    offset: start_offset,
                    start_code_len,
                });
            }
        }
    }
}

/// Iterate over non-empty Annex-B NAL units without allocating a temporary list.
pub fn nal_units_iter(data: &[u8]) -> Result<NalUnits<'_>, VideoError> {
    let Some(first) = find_start_code(data, 0) else {
        return Err(VideoError::InvalidAnnexB("no start code"));
    };
    if data[..first.0].iter().any(|byte| *byte != 0) {
        return Err(VideoError::InvalidAnnexB(
            "non-zero bytes precede the first start code",
        ));
    }

    let mut units = NalUnits {
        data,
        next_start: Some(first),
        pending: None,
    };
    let first_unit = units
        .next_unit()
        .ok_or(VideoError::InvalidAnnexB("access unit has no NAL data"))?;
    units.pending = Some(first_unit);
    Ok(units)
}

/// Split an Annex-B access unit into non-empty NAL units.
pub fn nal_units(data: &[u8]) -> Result<Vec<NalUnit<'_>>, VideoError> {
    Ok(nal_units_iter(data)?.collect())
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

/// Convert four-byte Annex-B start codes to lengths without copying payload data.
///
/// Returns `false` without modifying `data` when the access unit contains
/// three-byte start codes, empty units, leading padding, or trailing-zero
/// padding that would require resizing. Call [`to_length_prefixed`] as the
/// general fallback in that case.
pub fn to_length_prefixed_in_place(data: &mut [u8]) -> Result<bool, VideoError> {
    {
        let base = data.as_ptr() as usize;
        let mut previous_end = None;
        for unit in nal_units_iter(data)? {
            let data_offset = unit.data.as_ptr() as usize - base;
            if unit.start_code_len != 4
                || data_offset != unit.offset + 4
                || previous_end.is_some_and(|end| end != unit.offset)
                || (previous_end.is_none() && unit.offset != 0)
            {
                return Ok(false);
            }
            let _ = u32::try_from(unit.data.len())
                .map_err(|_| VideoError::NalUnitTooLarge(unit.data.len()))?;
            previous_end = Some(data_offset + unit.data.len());
        }
        if previous_end != Some(data.len()) {
            return Ok(false);
        }
    }

    let mut start = 0;
    loop {
        let payload_start = start + 4;
        let next = find_start_code(data, payload_start);
        let payload_end = next.map_or(data.len(), |(offset, _)| offset);
        let payload_len = payload_end - payload_start;
        let payload_len =
            u32::try_from(payload_len).map_err(|_| VideoError::NalUnitTooLarge(payload_len))?;
        data[start..payload_start].copy_from_slice(&payload_len.to_be_bytes());
        let Some((next_start, _)) = next else {
            break;
        };
        start = next_start;
    }
    Ok(true)
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
    use super::{nal_units, nal_units_iter, to_length_prefixed, to_length_prefixed_in_place};

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
    fn converts_four_byte_start_codes_in_place() {
        let mut data = [0, 0, 0, 1, 0x65, 1, 2, 0, 0, 0, 1, 0x41, 3];
        assert!(to_length_prefixed_in_place(&mut data).unwrap());
        assert_eq!(data, [0, 0, 0, 3, 0x65, 1, 2, 0, 0, 0, 2, 0x41, 3]);
    }

    #[test]
    fn in_place_conversion_leaves_resizing_inputs_untouched() {
        for source in [
            vec![0, 0, 1, 0x65, 1],
            vec![0, 0, 0, 1, 0x65, 1, 0],
            vec![0, 0, 0, 1, 0x65, 1, 0, 0, 0, 1],
        ] {
            let mut data = source.clone();
            assert!(!to_length_prefixed_in_place(&mut data).unwrap());
            assert_eq!(data, source);
        }
    }

    #[test]
    fn rejects_non_annex_b_data() {
        assert!(nal_units(&[1, 2, 3, 4]).is_err());
    }

    #[test]
    fn iterator_skips_empty_units_without_allocating_a_list() {
        let data = [0, 0, 1, 0, 0, 1, 0x65, 1, 0, 0, 1];
        let units = nal_units_iter(&data).unwrap().collect::<Vec<_>>();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].data, [0x65, 1]);
    }
}
