//! WiFi channel/frequency conversion and sweep-list parsing.

/// Map a 2.4 GHz or extended 5 GHz center frequency in MHz to a WiFi channel.
pub const fn frequency_to_channel(frequency_mhz: u16) -> Option<u8> {
    if frequency_mhz == 2484 {
        return Some(14);
    }
    if frequency_mhz >= 2412 && frequency_mhz <= 2472 {
        return Some(((frequency_mhz - 2407) / 5) as u8);
    }
    if frequency_mhz >= 5000 && frequency_mhz <= 6265 {
        return Some(((frequency_mhz - 5000) / 5) as u8);
    }
    None
}

/// Map a WiFi channel to its center frequency in MHz.
pub const fn channel_to_frequency(channel: u8) -> Option<u16> {
    if channel == 0 {
        None
    } else if channel == 14 {
        Some(2484)
    } else if channel <= 13 {
        Some(2407 + 5 * channel as u16)
    } else {
        Some(5000 + 5 * channel as u16)
    }
}

/// Expand a Devourer-compatible channel or frequency sweep specification.
///
/// Tokens are comma separated and may be individual channels (`1,6,11`),
/// inclusive channel ranges (`36-48/4`), or MHz ranges (`5170-5250/5`).
/// Invalid and out-of-band bins are skipped, matching Devourer's diagnostic
/// sweep grammar.
pub fn parse_channel_sweep(spec: &str) -> Vec<u8> {
    let mut channels = Vec::new();
    for raw_token in spec.split(',') {
        let mut token = raw_token.trim();
        if token.is_empty() {
            continue;
        }
        let mut step = 1u32;
        if let Some((range, raw_step)) = token.split_once('/') {
            token = range;
            step = parse_integer(raw_step).unwrap_or(1).max(1);
        }
        let Some((raw_start, raw_end)) = token.split_once('-') else {
            if let Some(channel) = parse_integer(token)
                .and_then(|value| u8::try_from(value).ok())
                .filter(|channel| *channel > 0)
            {
                channels.push(channel);
            }
            continue;
        };
        let (Some(start), Some(end)) = (parse_integer(raw_start), parse_integer(raw_end)) else {
            continue;
        };
        if start == 0 || end < start {
            continue;
        }
        let frequencies = start >= 1_000;
        if frequencies {
            step = step.max(5);
        }
        let mut value = start;
        while value <= end {
            let channel = if frequencies {
                u16::try_from(value).ok().and_then(frequency_to_channel)
            } else {
                u8::try_from(value).ok().filter(|channel| *channel > 0)
            };
            if let Some(channel) = channel {
                channels.push(channel);
            }
            let Some(next) = value.checked_add(step) else {
                break;
            };
            value = next;
        }
    }
    channels
}

fn parse_integer(value: &str) -> Option<u32> {
    let value = value.trim();
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(
            || value.parse().ok(),
            |hex| u32::from_str_radix(hex, 16).ok(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_wifi_channels_and_frequencies_like_devourer() {
        assert_eq!(frequency_to_channel(2412), Some(1));
        assert_eq!(frequency_to_channel(2484), Some(14));
        assert_eq!(frequency_to_channel(5180), Some(36));
        assert_eq!(frequency_to_channel(6165), Some(233));
        assert_eq!(frequency_to_channel(6265), Some(253));
        assert_eq!(frequency_to_channel(6270), None);
        assert_eq!(frequency_to_channel(4900), None);
        assert_eq!(channel_to_frequency(1), Some(2412));
        assert_eq!(channel_to_frequency(14), Some(2484));
        assert_eq!(channel_to_frequency(36), Some(5180));
        assert_eq!(channel_to_frequency(233), Some(6165));
        assert_eq!(channel_to_frequency(253), Some(6265));
    }

    #[test]
    fn expands_devourer_sweep_grammar() {
        assert_eq!(parse_channel_sweep("1,6,11"), [1, 6, 11]);
        assert_eq!(parse_channel_sweep("36-48/4"), [36, 40, 44, 48]);
        assert_eq!(parse_channel_sweep("0x24-0x30/0x4"), [36, 40, 44, 48]);
        assert_eq!(parse_channel_sweep("5170-5250/20"), [34, 38, 42, 46, 50]);
        assert!(parse_channel_sweep("bad,0,500-100").is_empty());
    }
}
