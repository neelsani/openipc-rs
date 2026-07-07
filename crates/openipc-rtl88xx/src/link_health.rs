//! Classification of weak, interfered, and near-field-saturated links.

/// High-level diagnosis of the current receive window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LinkVerdict {
    /// No decodable frames were observed.
    #[default]
    NoSignal,
    /// Strong RSSI with degraded EVM indicates receiver overload/self-jamming.
    Saturated,
    /// Elevated false alarms and a dirty constellation indicate interference.
    Interference,
    /// Low RSSI and SNR indicate a genuine range limit.
    Weak,
    /// Frames decode but without comfortable margin.
    Marginal,
    /// Link metrics are in a comfortable operating region.
    Healthy,
}

/// One window of raw receive metrics in Realtek/Devourer units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkHealthInput {
    /// Number of decoded frames in the window.
    pub frames: u32,
    /// Peak path-A PWDB (`dBm ≈ raw - 110`).
    pub rssi_raw: i16,
    /// Mean path-A SNR in half-dB.
    pub snr_raw: i16,
    /// Mean path-A EVM in half-dB; more negative is better.
    pub evm_raw: i16,
    /// Whether EVM is present.
    pub evm_valid: bool,
    /// Whether frame-free FA/CCA counters are present.
    pub energy_valid: bool,
    /// OFDM false alarms during the window.
    pub fa_ofdm: u32,
    /// OFDM channel-busy count during the window.
    pub cca_ofdm: u32,
    /// Whether an IGI sample is present.
    pub igi_valid: bool,
    /// Current initial-gain index.
    pub igi: u8,
    /// Lowest expected IGI for this generation.
    pub igi_min: u8,
    /// Highest expected IGI for this generation.
    pub igi_max: u8,
}

/// Calibrated raw-unit boundaries used by the classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkHealthThresholds {
    /// PWDB at or above which the signal is considered strong.
    pub rssi_strong: i16,
    /// PWDB at or below which the signal is considered weak.
    pub rssi_weak: i16,
    /// EVM worse than this is a dirty constellation.
    pub evm_poor: i16,
    /// EVM better than this is clean.
    pub evm_good: i16,
    /// SNR below this is poor.
    pub snr_low: i16,
    /// SNR at or above this is comfortable.
    pub snr_good: i16,
    /// False-alarm count above which the channel is noisy.
    pub fa_high: u32,
}

impl Default for LinkHealthThresholds {
    fn default() -> Self {
        Self {
            rssi_strong: 66,
            rssi_weak: 38,
            evm_poor: -47,
            evm_good: -49,
            snr_low: 16,
            snr_good: 30,
            fa_high: 300,
        }
    }
}

/// Human-readable diagnosis plus converted display values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkHealth {
    /// Classified link state.
    pub verdict: LinkVerdict,
    /// Short stable label suitable for telemetry and logs.
    pub label: &'static str,
    /// Explanation of the likely mechanism.
    pub cause: &'static str,
    /// Recommended corrective action.
    pub remedy: &'static str,
    /// Converted RSSI in dBm.
    pub rssi_dbm: i16,
    /// Converted SNR in dB.
    pub snr_db: f32,
    /// Converted EVM in dB, or zero when unavailable.
    pub evm_db: f32,
    /// Whether gain has reached its lower rail.
    pub igi_at_floor: bool,
    /// Whether gain has reached its upper rail.
    pub igi_at_ceiling: bool,
}

/// Classify a link-health window using Devourer's measured decision tree.
pub fn classify_link_health(
    input: LinkHealthInput,
    thresholds: LinkHealthThresholds,
) -> LinkHealth {
    let mut result = LinkHealth {
        verdict: LinkVerdict::NoSignal,
        label: "NO_SIGNAL",
        cause: "no frames decoded this window",
        remedy: "check transmitter state, channel, bandwidth, key, and link ID",
        rssi_dbm: input.rssi_raw - 110,
        snr_db: f32::from(input.snr_raw) / 2.0,
        evm_db: if input.evm_valid {
            f32::from(input.evm_raw) / 2.0
        } else {
            0.0
        },
        igi_at_floor: input.igi_valid && input.igi <= input.igi_min,
        igi_at_ceiling: input.igi_valid && input.igi >= input.igi_max,
    };
    if input.frames == 0 {
        return result;
    }

    let strong = input.rssi_raw >= thresholds.rssi_strong;
    let weak = input.rssi_raw <= thresholds.rssi_weak;
    let snr_poor = input.snr_raw < thresholds.snr_low;
    let snr_good = input.snr_raw >= thresholds.snr_good;
    let evm_poor = input.evm_valid && input.evm_raw > thresholds.evm_poor;
    let evm_good = !input.evm_valid || input.evm_raw < thresholds.evm_good;
    let dirty = evm_poor || (!input.evm_valid && snr_poor);
    let noisy = input.energy_valid && input.fa_ofdm > thresholds.fa_high;

    let (verdict, label, cause, remedy) = if strong && dirty {
        (
            LinkVerdict::Saturated,
            "SATURATED",
            "strong RSSI with poor EVM indicates front-end overload or reflected self-interference",
            "reduce TX power, add attenuation, or increase distance",
        )
    } else if !strong && dirty && noisy {
        (
            LinkVerdict::Interference,
            "INTERFERENCE",
            "false alarms and constellation degradation indicate external or co-channel energy",
            "change channel or notch a confirmed narrowband interferer",
        )
    } else if weak && !snr_good {
        (
            LinkVerdict::Weak,
            "WEAK",
            "low RSSI and SNR indicate a range or sensitivity limit",
            "increase TX power, improve antenna alignment, or reduce distance",
        )
    } else if snr_good && evm_good {
        (
            LinkVerdict::Healthy,
            "HEALTHY",
            if strong {
                "good SNR and EVM, with RSSI near the top of the linear range"
            } else {
                "good SNR and EVM with RSSI in the linear range"
            },
            if strong {
                "leave headroom before adding power on a short link"
            } else {
                "none"
            },
        )
    } else {
        (
            LinkVerdict::Marginal,
            "MARGINAL",
            "frames decode, but SNR or EVM lacks comfortable margin",
            "watch the trend; back off power first when RSSI is high",
        )
    };
    result.verdict = verdict;
    result.label = label;
    result.cause = cause;
    result.remedy = remedy;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(input: LinkHealthInput) -> LinkVerdict {
        classify_link_health(input, LinkHealthThresholds::default()).verdict
    }

    #[test]
    fn matches_devourer_health_boundaries() {
        assert_eq!(classify(LinkHealthInput::default()), LinkVerdict::NoSignal);
        assert_eq!(
            classify(LinkHealthInput {
                frames: 50,
                rssi_raw: 75,
                snr_raw: 36,
                evm_raw: -40,
                evm_valid: true,
                ..LinkHealthInput::default()
            }),
            LinkVerdict::Saturated
        );
        assert_eq!(
            classify(LinkHealthInput {
                frames: 50,
                rssi_raw: 55,
                snr_raw: 12,
                evm_raw: -40,
                evm_valid: true,
                energy_valid: true,
                fa_ofdm: 500,
                ..LinkHealthInput::default()
            }),
            LinkVerdict::Interference
        );
        assert_eq!(
            classify(LinkHealthInput {
                frames: 50,
                rssi_raw: 35,
                snr_raw: 12,
                ..LinkHealthInput::default()
            }),
            LinkVerdict::Weak
        );
    }
}
