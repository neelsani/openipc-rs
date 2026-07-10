//! Static capabilities derived from the probed Realtek silicon.

use crate::{ChannelWidth, ChipFamily, ChipInfo, TxCapabilities, TxPowerCaps};

/// Realtek driver generation used by a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipGeneration {
    /// RTL8812A/RTL8821A/RTL8814A generation.
    Jaguar1,
    /// RTL8822B/RTL8821C generation.
    Jaguar2,
    /// RTL8822C/RTL8822E generation.
    Jaguar3,
}

/// A tunable or characterized RF frequency range in MHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandRange {
    /// Inclusive lower center-frequency limit.
    pub min_mhz: u16,
    /// Inclusive upper center-frequency limit.
    pub max_mhz: u16,
}

impl BandRange {
    /// Return whether this range includes a center frequency.
    pub const fn contains(self, frequency_mhz: u16) -> bool {
        frequency_mhz >= self.min_mhz && frequency_mhz <= self.max_mhz
    }
}

/// Complete static capability report for one opened USB adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterCapabilities {
    /// Whether this adapter has a complete implementation in this driver.
    pub supported: bool,
    /// Detected silicon family.
    pub family: ChipFamily,
    /// Driver register/descriptor generation.
    pub generation: ChipGeneration,
    /// Human-readable silicon name.
    pub chip_name: &'static str,
    /// Common USB marketing aliases.
    pub marketing_names: &'static str,
    /// Devourer-compatible silicon variant tag.
    pub variant: &'static str,
    /// Hardware transport used by this crate.
    pub transport: &'static str,
    /// SYS_CFG2 dispatch identifier used by Devourer.
    pub chip_id: u8,
    /// Number of transmit RF chains.
    pub tx_chains: u8,
    /// Number of receive RF chains.
    pub rx_chains: u8,
    /// Modulation and spatial-stream capabilities.
    pub tx: TxCapabilities,
    /// Runtime transmit-power capabilities.
    pub tx_power: TxPowerCaps,
    /// Bitmask of supported widths: bits 0 through 4 mean 5/10/20/40/80 MHz.
    pub bandwidth_mask: u8,
    /// 2.4 GHz synthesizer tuning range.
    pub tune_2g4: BandRange,
    /// Extended 5 GHz synthesizer tuning range.
    pub tune_5g: BandRange,
    /// 2.4 GHz range covered by calibration/power tables.
    pub characterized_2g4: BandRange,
    /// 5 GHz range covered by calibration/power tables.
    pub characterized_5g: BandRange,
    /// Whether descriptors support a per-packet TX-power offset.
    pub per_packet_tx_power: bool,
    /// Whether 5/10 MHz baseband re-clocking is supported.
    pub narrowband: bool,
    /// Maximum crystal-cap trim code.
    pub crystal_cap_max: u8,
    /// Default crystal-cap trim code when no EFUSE value is available.
    pub crystal_cap_default: u8,
    /// Whether a lean same-band retune implementation exists.
    pub fast_retune: bool,
    /// Whether PHY status exposes useful per-chain RSSI.
    pub per_chain_rssi: bool,
    /// Whether every RX descriptor includes a hardware TSF low word.
    pub hardware_rx_timestamp: bool,
    /// Whether hardware beacon TX inserts a live egress TSF.
    pub hardware_beacon_tx_timestamp: bool,
}

impl AdapterCapabilities {
    /// Derive the static USB capability report from a hardware probe.
    pub const fn for_chip(chip: ChipInfo) -> Self {
        const WIDTH_5: u8 = 1 << 0;
        const WIDTH_10: u8 = 1 << 1;
        const WIDTH_AC: u8 = (1 << 2) | (1 << 3) | (1 << 4);

        let (generation, chip_name, marketing_names, variant, chip_id) = match chip.family {
            ChipFamily::Rtl8812 if matches!(chip.rf_type, crate::RfType::OneTOneR) => (
                ChipGeneration::Jaguar1,
                "RTL8811A",
                "RTL8811AU/RTL8811AR",
                "8811A",
                0x04,
            ),
            ChipFamily::Rtl8812 => (
                ChipGeneration::Jaguar1,
                "RTL8812A",
                "RTL8812AU/RTL8812AR",
                "8812A",
                0x04,
            ),
            ChipFamily::Rtl8814 => (
                ChipGeneration::Jaguar1,
                "RTL8814A",
                "RTL8814AU",
                "8814A",
                0x08,
            ),
            ChipFamily::Rtl8821 => (
                ChipGeneration::Jaguar1,
                "RTL8821A",
                "RTL8821AU",
                "8821A",
                0x05,
            ),
            ChipFamily::Rtl8822b => (
                ChipGeneration::Jaguar2,
                "RTL8822B",
                "RTL8822BU/RTL8812BU",
                "C8822B",
                0x0a,
            ),
            ChipFamily::Rtl8821c => (
                ChipGeneration::Jaguar2,
                "RTL8821C",
                "RTL8821CU/RTL8811CU",
                "C8821C",
                0x09,
            ),
            ChipFamily::Rtl8822c => (
                ChipGeneration::Jaguar3,
                "RTL8822C",
                "RTL8822CU/RTL8812CU",
                "C8822C",
                0x13,
            ),
            ChipFamily::Rtl8822e => (
                ChipGeneration::Jaguar3,
                "RTL8822E",
                "RTL8822EU/RTL8812EU",
                "C8822E",
                0x17,
            ),
        };
        let narrowband = !matches!(chip.family, ChipFamily::Rtl8821);
        let bandwidth_mask = WIDTH_AC | if narrowband { WIDTH_5 | WIDTH_10 } else { 0 };
        let chains = chip.total_rf_paths() as u8;
        Self {
            supported: true,
            family: chip.family,
            generation,
            chip_name,
            marketing_names,
            variant,
            transport: "usb",
            chip_id,
            tx_chains: chains,
            rx_chains: chains,
            tx: TxCapabilities::for_chip(chip),
            tx_power: TxPowerCaps::for_chip(chip),
            bandwidth_mask,
            tune_2g4: BandRange {
                min_mhz: 2412,
                max_mhz: 2484,
            },
            tune_5g: BandRange {
                min_mhz: 5080,
                max_mhz: 6165,
            },
            characterized_2g4: BandRange {
                min_mhz: 2412,
                max_mhz: 2484,
            },
            characterized_5g: BandRange {
                min_mhz: 5180,
                max_mhz: 5825,
            },
            per_packet_tx_power: matches!(generation, ChipGeneration::Jaguar2),
            narrowband,
            crystal_cap_max: if matches!(generation, ChipGeneration::Jaguar3) {
                0x7f
            } else {
                0x3f
            },
            crystal_cap_default: 0x20,
            fast_retune: true,
            per_chain_rssi: chains >= 2,
            hardware_rx_timestamp: true,
            hardware_beacon_tx_timestamp: !matches!(generation, ChipGeneration::Jaguar1),
        }
    }

    /// Return whether a channel width is supported by this silicon.
    pub const fn supports_width(self, width: ChannelWidth) -> bool {
        let bit = match width {
            ChannelWidth::Mhz5 => 1 << 0,
            ChannelWidth::Mhz10 => 1 << 1,
            ChannelWidth::Mhz20 => 1 << 2,
            ChannelWidth::Mhz40 => 1 << 3,
            ChannelWidth::Mhz80 => 1 << 4,
        };
        self.bandwidth_mask & bit != 0
    }

    /// Return whether TX-power calibration tables characterize this frequency.
    pub const fn is_characterized(self, frequency_mhz: u16) -> bool {
        self.characterized_2g4.contains(frequency_mhz)
            || self.characterized_5g.contains(frequency_mhz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RfType;

    fn chip(family: ChipFamily, rf_type: RfType) -> ChipInfo {
        ChipInfo {
            family,
            rf_type,
            cut_version: 0,
            sys_cfg: 0,
        }
    }

    #[test]
    fn capabilities_match_devourer_usb_generations() {
        let j1 = AdapterCapabilities::for_chip(chip(ChipFamily::Rtl8812, RfType::TwoTTwoR));
        assert!(j1.supports_width(ChannelWidth::Mhz5));
        assert!(!j1.hardware_beacon_tx_timestamp);
        assert_eq!(j1.rx_chains, 2);
        assert_eq!(j1.chip_id, 0x04);
        assert_eq!(j1.variant, "8812A");

        let rtl8811 = AdapterCapabilities::for_chip(chip(ChipFamily::Rtl8812, RfType::OneTOneR));
        assert_eq!(rtl8811.chip_name, "RTL8811A");
        assert_eq!(rtl8811.transport, "usb");

        let rtl8821a = AdapterCapabilities::for_chip(chip(ChipFamily::Rtl8821, RfType::OneTOneR));
        assert!(!rtl8821a.supports_width(ChannelWidth::Mhz5));

        let j2 = AdapterCapabilities::for_chip(chip(ChipFamily::Rtl8822b, RfType::TwoTTwoR));
        assert!(j2.per_packet_tx_power);
        assert!(j2.hardware_beacon_tx_timestamp);

        let j3 = AdapterCapabilities::for_chip(chip(ChipFamily::Rtl8822e, RfType::TwoTTwoR));
        assert_eq!(j3.crystal_cap_max, 0x7f);
        assert!(j3.tune_5g.contains(6165));
        assert!(!j3.is_characterized(6165));
    }
}
