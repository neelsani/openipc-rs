//! Adapter-health evidence and classification for failing Realtek dongles.

/// Realtek EEPROM identifier expected at the start of a programmed EFUSE map.
pub const REALTEK_EEPROM_ID: u16 = 0x8129;

/// Result of repeated fresh physical EFUSE-map reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EfuseStability {
    /// Whether this chipset supports the probe in its current state.
    pub supported: bool,
    /// Number of physical reads completed.
    pub reads: u16,
    /// Reads whose bytes differed from the first read.
    pub mismatched_reads: u16,
    /// Reads whose EEPROM identifier was not `0x8129`.
    pub invalid_id_reads: u16,
    /// EEPROM identifier from the final completed read.
    pub eeprom_id: u16,
    /// Number of logical-map bytes compared.
    pub map_len: u16,
    /// First mismatching byte offset, when any read differed.
    pub first_mismatch_offset: Option<u16>,
}

/// Summarize several already-read EFUSE maps using Devourer's comparison rules.
pub fn compare_efuse_maps<'a>(maps: impl IntoIterator<Item = &'a [u8]>) -> EfuseStability {
    let maps: Vec<_> = maps.into_iter().filter(|map| map.len() >= 2).collect();
    let Some(reference) = maps.first().copied() else {
        return EfuseStability::default();
    };
    let map_len = reference.len().min(u16::MAX as usize);
    let mut result = EfuseStability {
        supported: true,
        map_len: map_len as u16,
        ..EfuseStability::default()
    };
    for map in maps {
        if map.len() < map_len {
            break;
        }
        result.reads = result.reads.saturating_add(1);
        result.eeprom_id = u16::from_le_bytes([map[0], map[1]]);
        if result.eeprom_id != REALTEK_EEPROM_ID {
            result.invalid_id_reads = result.invalid_id_reads.saturating_add(1);
        }
        if map[..map_len] != reference[..map_len] {
            result.mismatched_reads = result.mismatched_reads.saturating_add(1);
            if result.first_mismatch_offset.is_none() {
                result.first_mismatch_offset = reference[..map_len]
                    .iter()
                    .zip(&map[..map_len])
                    .position(|(left, right)| left != right)
                    .and_then(|offset| u16::try_from(offset).ok());
            }
        }
    }
    result
}

/// Firmware download/boot result from the latest initialization.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FirmwareBootStatus {
    /// Whether this chipset reports firmware stages.
    pub supported: bool,
    /// Whether download was attempted.
    pub attempted: bool,
    /// Whether firmware checksum completion was observed.
    pub checksum_ok: bool,
    /// Whether the MCU reported firmware ready.
    pub ready: bool,
}

/// Overall adapter diagnosis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AdapterVerdict {
    /// No conclusive probes were supplied.
    #[default]
    Unknown,
    /// All supplied probes passed.
    Healthy,
    /// A soft anomaly should be rechecked.
    Suspect,
    /// Hard evidence indicates a failing adapter.
    Failing,
}

/// Bitmask explaining an [`AdapterVerdict`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdapterHealthReasons(pub u32);

impl AdapterHealthReasons {
    /// Bring-up did not complete.
    pub const INIT_FAILED: u32 = 1 << 0;
    /// Fresh EFUSE reads disagreed.
    pub const EFUSE_UNSTABLE: u32 = 1 << 1;
    /// EFUSE was stable but had an invalid nonblank identifier.
    pub const EFUSE_ID_INVALID: u32 = 1 << 2;
    /// EFUSE map appeared blank.
    pub const EFUSE_BLANK: u32 = 1 << 3;
    /// Firmware download did not reach ready.
    pub const FIRMWARE_BOOT_FAILED: u32 = 1 << 4;
    /// No clean RX was seen despite known traffic.
    pub const RX_DEAF_TO_TRAFFIC: u32 = 1 << 5;
    /// No clean RX was seen without a traffic guarantee.
    pub const RX_SILENT: u32 = 1 << 6;

    /// Test whether a reason bit is present.
    pub const fn contains(self, reason: u32) -> bool {
        self.0 & reason != 0
    }
}

/// Evidence supplied to the pure adapter-health classifier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdapterHealthInput {
    /// Whether driver initialization completed.
    pub init_completed: bool,
    /// Repeated physical EFUSE-read evidence.
    pub efuse: EfuseStability,
    /// Firmware boot evidence.
    pub firmware: FirmwareBootStatus,
    /// Whether an RX smoke interval was run.
    pub rx_checked: bool,
    /// Whether traffic was guaranteed during the RX interval.
    pub rx_traffic_expected: bool,
    /// FCS-clean frames observed.
    pub rx_frames_ok: u32,
    /// Corrupted frames observed, retained for diagnostics.
    pub rx_frames_crc: u32,
}

/// Classify adapter evidence using Devourer's dying-dongle rules.
pub fn classify_adapter_health(
    input: AdapterHealthInput,
) -> (AdapterVerdict, AdapterHealthReasons) {
    let mut reasons = 0u32;
    if !input.init_completed {
        reasons |= AdapterHealthReasons::INIT_FAILED;
        if input.firmware.supported && input.firmware.attempted && !input.firmware.ready {
            reasons |= AdapterHealthReasons::FIRMWARE_BOOT_FAILED;
        }
        return (AdapterVerdict::Failing, AdapterHealthReasons(reasons));
    }

    let mut checked = false;
    let mut efuse_hard = false;
    let mut efuse_soft = false;
    if input.efuse.supported && input.efuse.reads > 0 {
        checked = true;
        let blank = input.efuse.mismatched_reads == 0
            && input.efuse.invalid_id_reads == input.efuse.reads
            && input.efuse.eeprom_id == u16::MAX;
        if input.efuse.mismatched_reads > 0 {
            reasons |= AdapterHealthReasons::EFUSE_UNSTABLE;
            efuse_hard = true;
        } else if blank {
            reasons |= AdapterHealthReasons::EFUSE_BLANK;
            efuse_soft = true;
        } else if input.efuse.invalid_id_reads > 0 {
            reasons |= AdapterHealthReasons::EFUSE_ID_INVALID;
            efuse_soft = true;
        }
    }

    let mut firmware_bad = false;
    if input.firmware.supported && input.firmware.attempted {
        checked = true;
        if !input.firmware.ready {
            reasons |= AdapterHealthReasons::FIRMWARE_BOOT_FAILED;
            firmware_bad = true;
        }
    }

    let mut rx_hard = false;
    let mut rx_soft = false;
    if input.rx_checked {
        checked = true;
        if input.rx_frames_ok == 0 {
            if input.rx_traffic_expected {
                reasons |= AdapterHealthReasons::RX_DEAF_TO_TRAFFIC;
                rx_hard = true;
            } else {
                reasons |= AdapterHealthReasons::RX_SILENT;
                rx_soft = true;
            }
        }
    }

    let verdict = if !checked {
        AdapterVerdict::Unknown
    } else if efuse_hard || rx_hard || (firmware_bad && efuse_soft) {
        AdapterVerdict::Failing
    } else if efuse_soft || firmware_bad || rx_soft {
        AdapterVerdict::Suspect
    } else {
        AdapterVerdict::Healthy
    };
    (verdict, AdapterHealthReasons(reasons))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unstable_efuse_is_failing() {
        let mut first = vec![0xff; 8];
        first[..2].copy_from_slice(&REALTEK_EEPROM_ID.to_le_bytes());
        let mut second = first.clone();
        second[5] ^= 1;
        let efuse = compare_efuse_maps([first.as_slice(), second.as_slice()]);
        let (verdict, reasons) = classify_adapter_health(AdapterHealthInput {
            init_completed: true,
            efuse,
            ..AdapterHealthInput::default()
        });
        assert_eq!(verdict, AdapterVerdict::Failing);
        assert!(reasons.contains(AdapterHealthReasons::EFUSE_UNSTABLE));
        assert_eq!(efuse.first_mismatch_offset, Some(5));
    }

    #[test]
    fn silent_rx_without_promised_traffic_is_only_suspect() {
        let (verdict, _) = classify_adapter_health(AdapterHealthInput {
            init_completed: true,
            rx_checked: true,
            ..AdapterHealthInput::default()
        });
        assert_eq!(verdict, AdapterVerdict::Suspect);
    }
}
