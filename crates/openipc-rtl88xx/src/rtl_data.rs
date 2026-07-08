// Large Realtek firmware and register-table constants live here so the HAL
// modules stay readable. This data is checked in; no reference checkout is
// needed at build time.
#![allow(dead_code)]

include!("data/rtl_reference_data.rs");
include!("data/rtl8812_tx_power_tables.rs");
include!("data/rtl8822b_reference_data.rs");
include!("data/rtl8821c_reference_data.rs");
include!("data/rtl8822c_reference_data.rs");
include!("data/rtl8822e_reference_data.rs");

#[cfg(test)]
mod tests {
    use super::RTL8812_PHY_REG;

    fn fnv1a_u32_le(values: &[u32]) -> u64 {
        values.iter().fold(0xcbf2_9ce4_8422_2325, |hash, value| {
            value.to_le_bytes().into_iter().fold(hash, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        })
    }

    #[test]
    fn rtl8812_normal_phy_table_matches_devourer() {
        // This is the normal CONFIG_BB_PHY_REG table used by both the known-working
        // WebAssembly fork and current Devourer, not the four-value manufacturing override.
        assert_eq!(RTL8812_PHY_REG.len(), 470);
        assert_eq!(
            &RTL8812_PHY_REG[..4],
            &[0x800, 0x8020_d010, 0x804, 0x0801_12e0]
        );
        assert_eq!(fnv1a_u32_le(RTL8812_PHY_REG), 0xaa31_7561_fb4b_f7f9);
    }
}
