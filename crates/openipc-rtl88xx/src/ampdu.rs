use std::{fmt, str::FromStr};

/// Hardware A-MPDU transmit settings shared by all supported RTL88xx families.
///
/// A-MPDU remains opt-in. The default enabled recipe mirrors Devourer's tested
/// OpenIPC broadcast mode: TID 0, up to 16 MPDUs, density 7, no retries, and a
/// short aggregate-fill timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmpduMode {
    /// Whether descriptor aggregation and MAC pacing are enabled.
    pub enabled: bool,
    /// Data queue/TID used by aggregatable frames, in the range 0..=7.
    pub tid: u8,
    /// Maximum MPDUs in an aggregate, in the range 1..=31.
    pub max_num: u8,
    /// Minimum MPDU spacing encoded in the descriptor, in the range 0..=7.
    pub density: u8,
    /// Disable hardware retries for broadcast/FEC traffic with no BlockAck peer.
    pub no_ack: bool,
    /// Aggregate-fill timer register value; `0` retains the bring-up default.
    pub max_time: u8,
    /// Clear the HalMAC burst-mode gate where the generation exposes it.
    pub clear_burst_mode: bool,
}

impl AmpduMode {
    /// Devourer's hardware-tested A-MPDU recipe for OpenIPC broadcast traffic.
    pub const OPENIPC: Self = Self {
        enabled: true,
        tid: 0,
        max_num: 16,
        density: 7,
        no_ack: true,
        max_time: 0x20,
        clear_burst_mode: true,
    };

    /// Return a disabled mode that preserves the ordinary single-frame path.
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::OPENIPC
        }
    }

    pub(crate) fn descriptor_values(self) -> (u8, u8, u8, u8) {
        (
            self.tid & 0x07,
            self.max_num.clamp(1, 0x1f),
            self.density & 0x07,
            if self.no_ack { 0 } else { 12 },
        )
    }
}

impl Default for AmpduMode {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Error returned for a malformed `tid/max[/density[/noack[/max_time]]]` spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseAmpduModeError;

impl fmt::Display for ParseAmpduModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected tid/max[/density[/noack[/max_time]]] A-MPDU settings")
    }
}

impl std::error::Error for ParseAmpduModeError {}

impl FromStr for AmpduMode {
    type Err = ParseAmpduModeError;

    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        let mut fields = spec.split('/');
        let tid = parse_u8(fields.next().ok_or(ParseAmpduModeError)?)?;
        if tid > 7 {
            return Err(ParseAmpduModeError);
        }
        let max_num = parse_u8(fields.next().ok_or(ParseAmpduModeError)?)?;
        if max_num == 0 {
            return Err(ParseAmpduModeError);
        }
        let density = fields.next().map(parse_u8).transpose()?.unwrap_or(7) & 0x07;
        let no_ack = fields.next().map(parse_u8).transpose()? != Some(0);
        let max_time = fields.next().map(parse_u8).transpose()?.unwrap_or(0x20);
        if fields.next().is_some() {
            return Err(ParseAmpduModeError);
        }
        Ok(Self {
            enabled: true,
            tid,
            max_num: max_num & 0x1f,
            density,
            no_ack,
            max_time,
            clear_burst_mode: true,
        })
    }
}

fn parse_u8(value: &str) -> Result<u8, ParseAmpduModeError> {
    let value = value.trim();
    let parsed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse(), |hex| u8::from_str_radix(hex, 16));
    parsed.map_err(|_| ParseAmpduModeError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_devourer_ampdu_grammar() {
        assert_eq!("0/16".parse::<AmpduMode>().unwrap(), AmpduMode::OPENIPC);
        assert_eq!(
            "3/31/0/0/0x70".parse::<AmpduMode>().unwrap(),
            AmpduMode {
                enabled: true,
                tid: 3,
                max_num: 31,
                density: 0,
                no_ack: false,
                max_time: 0x70,
                clear_burst_mode: true,
            }
        );
    }

    #[test]
    fn rejects_invalid_required_fields() {
        for spec in ["", "0", "8/16", "0/0", "x/16", "0/16/7/1/20/extra"] {
            assert!(spec.parse::<AmpduMode>().is_err(), "accepted {spec:?}");
        }
    }
}
