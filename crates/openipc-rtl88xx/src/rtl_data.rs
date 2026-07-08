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
    use super::*;

    fn fnv1a_bytes(bytes: impl IntoIterator<Item = u8>) -> u64 {
        bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    fn fnv1a_u8(values: &[u8]) -> u64 {
        fnv1a_bytes(values.iter().copied())
    }

    fn fnv1a_u32_le(values: &[u32]) -> u64 {
        fnv1a_bytes(values.iter().flat_map(|value| value.to_le_bytes()))
    }

    fn fnv1a_iqk(values: &[(u16, u32, u32)]) -> u64 {
        fnv1a_bytes(values.iter().flat_map(|(register, mask, value)| {
            register
                .to_le_bytes()
                .into_iter()
                .chain(mask.to_le_bytes())
                .chain(value.to_le_bytes())
        }))
    }

    fn assert_u8(values: &[u8], expected_len: usize, expected_hash: u64) {
        assert_eq!(values.len(), expected_len);
        assert_eq!(fnv1a_u8(values), expected_hash);
    }

    fn assert_u32(values: &[u32], expected_len: usize, expected_hash: u64) {
        assert_eq!(values.len(), expected_len);
        assert_eq!(fnv1a_u32_le(values), expected_hash);
    }

    #[test]
    fn jaguar1_payload_fingerprints_match_devourer() {
        assert_u8(RTL8812_FW_NIC, 27_054, 0x96e6_95eb_1ea0_25bc);
        assert_u8(RTL8821_FW_NIC, 31_834, 0xd2c9_7be7_18b3_64ba);
        assert_u8(RTL8814_FW_NIC, 68_320, 0x1ce6_e36a_bbb7_1848);
        assert_u32(RTL8812_MAC_REG, 224, 0xbb53_4964_607d_4374);
        assert_u32(RTL8812_PHY_REG, 470, 0xaa31_7561_fb4b_f7f9);
        assert_u32(RTL8812_AGC_TAB, 668, 0x2ca7_dcba_9875_5a9b);
        assert_u32(RTL8812_RADIO_A, 864, 0x0c3b_71d0_229b_932d);
        assert_u32(RTL8812_RADIO_B, 848, 0xfaa9_cb3f_3832_07f1);
        assert_u32(RTL8821_MAC_REG, 196, 0x0589_ad90_d1b6_9e2e);
        assert_u32(RTL8821_PHY_REG, 344, 0xda1f_5053_af16_36ef);
        assert_u32(RTL8821_AGC_TAB, 504, 0x8785_5bf3_1ddf_74af);
        assert_u32(RTL8821_RADIO_A, 1_734, 0xcba5_198d_4708_c5d0);
        assert_u32(RTL8814_MAC_REG, 286, 0x98e0_91b9_d79b_0eec);
        assert_u32(RTL8814_PHY_REG, 4_622, 0xa7f7_e82a_4e26_860d);
        assert_u32(RTL8814_AGC_TAB, 6_280, 0xacba_168e_bf16_4bdc);
        assert_u32(RTL8814_RADIO_A, 4_634, 0x631a_50ff_43db_65b5);
        assert_u32(RTL8814_RADIO_B, 4_396, 0x6c05_c380_3172_7292);
        assert_u32(RTL8814_RADIO_C, 4_524, 0x6194_4f13_a82f_632b);
        assert_u32(RTL8814_RADIO_D, 4_600, 0x86de_38f7_11c9_7eba);

        // The normal CONFIG_BB_PHY_REG table starts here; the similarly
        // named manufacturing override contains only two register writes.
        assert_eq!(
            &RTL8812_PHY_REG[..4],
            &[0x800, 0x8020_d010, 0x804, 0x0801_12e0]
        );
    }

    #[test]
    fn jaguar2_payload_fingerprints_match_devourer() {
        assert_u8(RTL8822B_FW_NIC, 161_240, 0x2d76_7ba6_1ed5_dd5c);
        assert_u32(RTL8822B_MAC_REG, 250, 0xd8f8_f43d_3c87_ffb2);
        assert_u32(RTL8822B_PHY_REG, 2_984, 0xf60f_b666_4680_1026);
        assert_u32(RTL8822B_AGC_TAB, 21_368, 0xd878_6105_bdf7_827e);
        assert_u32(RTL8822B_RADIO_A, 10_638, 0xaaa9_8dbf_54e6_b1da);
        assert_u32(RTL8822B_RADIO_B, 9_234, 0xb9a5_edac_958f_bf33);
        assert_u8(RTL8821C_FW_NIC, 138_984, 0xa9aa_be84_cb67_0ff9);
        assert_u32(RTL8821C_MAC_REG, 276, 0x8b10_06ba_e761_261e);
        assert_u32(RTL8821C_PHY_REG, 3_356, 0x83ec_d389_35ef_e8b5);
        assert_u32(RTL8821C_AGC_TAB, 3_200, 0x4ee4_29d7_5afb_7e62);
        assert_u32(RTL8821C_RADIO_A, 5_424, 0x6a04_4764_cbc2_8593);
        assert_u32(RTL8821C_PHY_REG_PG, 90, 0x966f_a67e_07d2_782a);
    }

    #[test]
    fn jaguar3_payload_fingerprints_match_devourer() {
        assert_u8(RTL8822C_FW_NIC, 200_624, 0xb4a6_c110_10b3_d0b8);
        assert_u32(RTL8822C_AGC_TAB, 3_734, 0xcf16_fe75_0737_4fc9);
        assert_u32(RTL8822C_PHY_REG, 3_020, 0xdb1d_b17f_6575_9aef);
        assert_u32(RTL8822C_RADIO_A, 40_130, 0xc570_6ca3_321a_fa76);
        assert_u32(RTL8822C_RADIO_B, 40_766, 0x7a2d_70b7_9717_f793);
        assert_u32(RTL8822C_CAL_INIT, 4_928, 0x6ca8_d56b_17d9_0bf8);
        assert_eq!(RTL8822C_IQK_NCTL.len(), 1_801);
        assert_eq!(fnv1a_iqk(RTL8822C_IQK_NCTL), 0xb539_debf_fcb5_85ed);
        assert_u8(RTL8822E_FW_NIC, 199_928, 0x83b1_5d1d_3b1b_922b);
        assert_u32(RTL8822E_AGC_TAB, 14_628, 0x2837_a7c0_513d_4dd5);
        assert_u32(RTL8822E_PHY_REG, 3_082, 0x8c37_1fa1_cd1f_b464);
        assert_u32(RTL8822E_PHY_REG_PG, 276, 0x4630_57f1_761f_5d92);
        assert_u32(RTL8822E_PHY_REG_PG_TYPE5, 276, 0x8779_2650_c23a_f4ea);
        assert_u32(RTL8822E_RADIO_A, 10_622, 0x28ac_0fbd_2f80_c188);
        assert_u32(RTL8822E_RADIO_B, 12_050, 0x7f84_8643_3c23_1b30);
        assert_u32(RTL8822E_CAL_INIT, 5_222, 0x6126_b465_5c2f_4eff);
    }
}
