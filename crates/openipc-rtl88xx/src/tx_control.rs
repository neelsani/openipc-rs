//! Runtime transmit capabilities, power controls, and submission statistics.

use crate::{ChipFamily, ChipInfo};

/// Static capabilities of a chipset's runtime TX-power controls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TxPowerCaps {
    /// Whether runtime TX-power control is implemented.
    pub supported: bool,
    /// Maximum hardware TXAGC index.
    pub index_max: u8,
    /// Quarter-dB represented by one hardware index step.
    pub step_qdb: u8,
    /// Whether the nominal step size has been confirmed on-air.
    pub step_measured: bool,
    /// Lowest representable relative offset, in quarter-dB.
    pub offset_min_qdb: i16,
    /// Highest representable relative offset, in quarter-dB.
    pub offset_max_qdb: i16,
}

impl TxPowerCaps {
    /// Return the runtime power capabilities for a probed chipset.
    pub const fn for_chip(chip: ChipInfo) -> Self {
        let jaguar3 = chip.family.is_jaguar3();
        Self {
            supported: true,
            index_max: if jaguar3 { 127 } else { 63 },
            step_qdb: if jaguar3 { 1 } else { 2 },
            step_measured: match chip.family {
                ChipFamily::Rtl8812 | ChipFamily::Rtl8814 => true,
                ChipFamily::Rtl8822b | ChipFamily::Rtl8821c => true,
                ChipFamily::Rtl8822c => true,
                // Devourer found nonlinear behavior on 8822E and no reliable
                // 5 GHz response on 8821A, so retain the honest distinction.
                ChipFamily::Rtl8821 | ChipFamily::Rtl8822e => false,
            },
            offset_min_qdb: if jaguar3 { -127 } else { -126 },
            offset_max_qdb: if jaguar3 { 127 } else { 126 },
        }
    }
}

/// Quantize a relative quarter-dB request to a chipset's hardware step.
///
/// Ties round away from zero, matching Devourer's controller-facing API.
pub fn quantize_tx_power_offset_qdb(requested_qdb: i16, caps: TxPowerCaps) -> (i16, i16) {
    if !caps.supported || caps.step_qdb == 0 {
        return (0, 0);
    }
    let requested = requested_qdb.clamp(caps.offset_min_qdb, caps.offset_max_qdb);
    let step = i32::from(caps.step_qdb);
    let requested = i32::from(requested);
    let steps = if requested >= 0 {
        (requested + step / 2) / step
    } else {
        (requested - step / 2) / step
    };
    let applied = (steps * step).clamp(
        i32::from(caps.offset_min_qdb),
        i32::from(caps.offset_max_qdb),
    );
    (applied as i16, steps as i16)
}

/// Snapshot of the active TX-power controls and representative rate indexes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TxPowerState {
    /// Whether the chipset supports this report.
    pub valid: bool,
    /// Flat absolute index, or `None` for the calibrated per-rate table.
    pub flat_index: Option<u8>,
    /// Applied relative offset in quarter-dB.
    pub offset_qdb: i16,
    /// Applied relative offset in hardware steps.
    pub offset_steps: i16,
    /// At least one rate saturated at zero during the latest apply.
    pub saturated_low: bool,
    /// At least one rate saturated at the hardware maximum during the latest apply.
    pub saturated_high: bool,
    /// Representative path-A CCK 1 Mbps index, when available.
    pub cck_index: Option<u8>,
    /// Representative path-A OFDM 6 Mbps index, when available.
    pub ofdm_index: Option<u8>,
    /// Representative path-A HT MCS7 index, when available.
    pub mcs7_index: Option<u8>,
    /// Whether representative indexes came from readable hardware registers.
    pub hardware_readback: bool,
}

/// Modulation capabilities callers must respect when constructing descriptors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TxCapabilities {
    /// Whether capabilities are known for this chipset.
    pub supported: bool,
    /// Number of available spatial streams.
    pub spatial_streams: u8,
    /// Whether STBC may be transmitted safely.
    pub stbc: bool,
    /// Whether LDPC may be transmitted.
    pub ldpc: bool,
    /// Whether short guard intervals may be transmitted.
    pub short_gi: bool,
    /// Maximum transmit channel width in MHz.
    pub max_bandwidth_mhz: u8,
}

impl TxCapabilities {
    /// Derive conservative capabilities when only the family is available.
    pub const fn for_family(family: ChipFamily) -> Self {
        let streams = match family {
            ChipFamily::Rtl8821 | ChipFamily::Rtl8821c => 1,
            ChipFamily::Rtl8814 => 4,
            ChipFamily::Rtl8812
            | ChipFamily::Rtl8822b
            | ChipFamily::Rtl8822c
            | ChipFamily::Rtl8822e => 2,
        };
        Self {
            supported: true,
            spatial_streams: streams,
            stbc: streams >= 2,
            ldpc: true,
            short_gi: true,
            max_bandwidth_mhz: 80,
        }
    }

    /// Derive transmit capabilities from the probed RF path count.
    pub const fn for_chip(chip: ChipInfo) -> Self {
        let streams = chip.total_rf_paths() as u8;
        Self {
            supported: true,
            spatial_streams: streams,
            stbc: streams >= 2,
            ldpc: true,
            short_gi: true,
            max_bandwidth_mhz: 80,
        }
    }
}

/// Driver-side TX submission health used to distinguish congestion from failure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TxStats {
    /// Frames handed to a bulk-OUT transfer.
    pub submitted: u64,
    /// Frames whose final transfer completion failed.
    pub failed: u64,
    /// Whether the most recent failure represented timeout/backpressure.
    pub last_was_timeout: bool,
    /// Stable textual class for the most recent error.
    pub last_error: Option<TxErrorKind>,
}

/// Platform-independent class of a failed USB TX completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxErrorKind {
    /// Transfer timed out or was cancelled by a timeout.
    Timeout,
    /// Endpoint was stalled.
    Stall,
    /// Device disconnected.
    Disconnected,
    /// Other transport failure.
    Other,
}

/// Convert a requested Jaguar2 per-frame dB delta to `TXPWR_OFSET`.
///
/// Hardware exposes only `{0, -3, -7, -11, +3, +6}` dB. Equal-distance
/// ties retain the earlier table entry, matching Devourer.
pub fn jaguar2_packet_power_step(db: i8) -> u8 {
    const LUT: [(i8, u8); 6] = [(0, 0), (-3, 1), (-7, 2), (-11, 3), (3, 4), (6, 5)];
    LUT.into_iter()
        .min_by_key(|(candidate, _)| i16::from(db).abs_diff(i16::from(*candidate)))
        .map_or(0, |(_, step)| step)
}

/// Return the nominal dB delta represented by a Jaguar2 power step.
pub const fn jaguar2_packet_power_db(step: u8) -> i8 {
    match step {
        1 => -3,
        2 => -7,
        3 => -11,
        4 => 3,
        5 => 6,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TxPowerControl {
    pub flat_index: Option<u8>,
    pub offset_qdb: i16,
    pub offset_steps: i16,
    pub saturated_low: bool,
    pub saturated_high: bool,
    pub cck_index: Option<u8>,
    pub ofdm_index: Option<u8>,
    pub mcs7_index: Option<u8>,
    pub hardware_readback: bool,
}

impl TxPowerControl {
    pub(crate) fn apply(self, baseline: i16, maximum: u8) -> (u8, bool, bool) {
        let baseline = self.flat_index.map_or(baseline, i16::from);
        let requested = baseline + self.offset_steps;
        (
            requested.clamp(0, i16::from(maximum)) as u8,
            requested < 0,
            requested > i16::from(maximum),
        )
    }

    pub(crate) fn public(self, valid: bool) -> TxPowerState {
        TxPowerState {
            valid,
            flat_index: self.flat_index,
            offset_qdb: self.offset_qdb,
            offset_steps: self.offset_steps,
            saturated_low: self.saturated_low,
            saturated_high: self.saturated_high,
            cck_index: self.cck_index,
            ofdm_index: self.ofdm_index,
            mcs7_index: self.mcs7_index,
            hardware_readback: self.hardware_readback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_db_quantization_matches_devourer() {
        let caps = TxPowerCaps {
            supported: true,
            step_qdb: 2,
            offset_min_qdb: -126,
            offset_max_qdb: 126,
            ..TxPowerCaps::default()
        };
        assert_eq!(quantize_tx_power_offset_qdb(1, caps), (2, 1));
        assert_eq!(quantize_tx_power_offset_qdb(-1, caps), (-2, -1));
        assert_eq!(quantize_tx_power_offset_qdb(127, caps), (126, 63));
    }

    #[test]
    fn jaguar2_packet_power_lut_matches_devourer() {
        assert_eq!(jaguar2_packet_power_step(0), 0);
        assert_eq!(jaguar2_packet_power_step(-5), 1);
        assert_eq!(jaguar2_packet_power_step(-9), 2);
        assert_eq!(jaguar2_packet_power_step(5), 5);
        assert_eq!(jaguar2_packet_power_db(3), -11);
    }
}
