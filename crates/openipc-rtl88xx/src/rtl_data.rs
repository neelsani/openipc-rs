// Large Realtek firmware and register-table constants live here so the HAL
// modules stay readable. This data is checked in; no reference checkout is
// needed at build time.
#![allow(dead_code)]

include!("data/rtl_reference_data.rs");
include!("data/rtl8812_tx_power_tables.rs");
include!("data/rtl8822b_reference_data.rs");
include!("data/rtl8822c_reference_data.rs");
include!("data/rtl8822e_reference_data.rs");
