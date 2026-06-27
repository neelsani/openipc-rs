use crate::types::ChipFamily;

pub(crate) fn strip_firmware_header(family: ChipFamily, firmware: &[u8]) -> &[u8] {
    if firmware.len() < 32 {
        return firmware;
    }
    let signature = u16::from_le_bytes([firmware[0], firmware[1]]);
    let has_header = match family {
        ChipFamily::Rtl8821 => signature & 0xfff0 == 0x2100,
        ChipFamily::Rtl8814 => signature & 0xfff0 == 0x8810,
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
