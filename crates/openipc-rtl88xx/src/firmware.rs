use crate::types::ChipFamily;

pub(crate) fn strip_firmware_header(family: ChipFamily, firmware: &[u8]) -> &[u8] {
    if firmware.len() < 32 {
        return firmware;
    }
    let signature = u16::from_le_bytes([firmware[0], firmware[1]]);
    let has_header = match family {
        ChipFamily::Rtl8821 => signature & 0xfff0 == 0x2100,
        ChipFamily::Rtl8814 => signature & 0xfff0 == 0x8810,
        ChipFamily::Rtl8822c | ChipFamily::Rtl8822e => signature & 0xfff0 == 0x8820,
        ChipFamily::Rtl8812 => signature & 0xfff0 == 0x9500,
    };
    if has_header {
        if family == ChipFamily::Rtl8814 && firmware.len() > 64 {
            &firmware[64..]
        } else {
            &firmware[32..]
        }
    } else {
        firmware
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn firmware_with_signature(signature: u16, len: usize) -> Vec<u8> {
        let mut firmware = vec![0xaa; len];
        let bytes = signature.to_le_bytes();
        firmware[0] = bytes[0];
        firmware[1] = bytes[1];
        firmware
    }

    #[test]
    fn leaves_short_firmware_unchanged() {
        let firmware = firmware_with_signature(0x9500, 31);

        assert_eq!(
            strip_firmware_header(ChipFamily::Rtl8812, &firmware).as_ptr(),
            firmware.as_ptr()
        );
    }

    #[test]
    fn leaves_unknown_signature_unchanged() {
        let firmware = firmware_with_signature(0x1234, 96);

        let stripped = strip_firmware_header(ChipFamily::Rtl8812, &firmware);

        assert_eq!(stripped.len(), 96);
        assert_eq!(stripped.as_ptr(), firmware.as_ptr());
    }

    #[test]
    fn strips_jaguar1_and_jaguar3_headers_at_32_bytes() {
        for (family, signature) in [
            (ChipFamily::Rtl8812, 0x9500),
            (ChipFamily::Rtl8821, 0x2100),
            (ChipFamily::Rtl8822c, 0x8820),
            (ChipFamily::Rtl8822e, 0x8820),
        ] {
            let firmware = firmware_with_signature(signature, 96);

            let stripped = strip_firmware_header(family, &firmware);

            assert_eq!(stripped.len(), 64);
            assert_eq!(stripped.as_ptr(), firmware[32..].as_ptr());
        }
    }

    #[test]
    fn strips_rtl8814_reserved_page_header_at_64_bytes_when_present() {
        let firmware = firmware_with_signature(0x8810, 96);

        let stripped = strip_firmware_header(ChipFamily::Rtl8814, &firmware);

        assert_eq!(stripped.len(), 32);
        assert_eq!(stripped.as_ptr(), firmware[64..].as_ptr());
    }

    #[test]
    fn strips_rtl8814_short_header_at_32_bytes() {
        let firmware = firmware_with_signature(0x8810, 64);

        let stripped = strip_firmware_header(ChipFamily::Rtl8814, &firmware);

        assert_eq!(stripped.len(), 32);
        assert_eq!(stripped.as_ptr(), firmware[32..].as_ptr());
    }
}
