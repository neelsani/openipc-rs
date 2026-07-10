//! RTL8822B physical-EFUSE power trim and PA-bias calibration.

use crate::device::RealtekDevice;
use crate::phy::RfPath;
use crate::{ChipFamily, ChipInfo, DriverError};

const PHYSICAL_ADDRESSES: [u16; 13] = [
    0x3ee, 0x3ec, 0x3e8, 0x3e4, 0x3e0, 0x3dc, 0x3eb, 0x3e7, 0x3e3, 0x3df, 0x3db, 0x3d5, 0x3d6,
];

/// Immutable trim data retained for channel changes after bring-up.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Jaguar2KfreeState {
    gain: [[u8; 2]; 6],
    has_2g: bool,
    has_5g: bool,
}

impl RealtekDevice {
    pub(crate) async fn init_kfree_8822b_async(&self, chip: ChipInfo) -> Result<(), DriverError> {
        if chip.family != ChipFamily::Rtl8822b {
            return Ok(());
        }

        let bytes = self
            .read_physical_efuse_bytes_async(&PHYSICAL_ADDRESSES)
            .await?;
        let mut state = Jaguar2KfreeState::default();
        let trim_2g = bytes[0];
        if trim_2g != 0xff {
            state.gain[0] = [trim_2g & 0x0f, trim_2g >> 4];
            state.has_2g = true;
        }
        if bytes[1] != 0xff {
            for group in 0..5 {
                state.gain[group + 1] = [bytes[group + 1], bytes[group + 6]];
            }
            state.has_5g = true;
        }
        log::info!(
            target: "openipc_rtl88xx::kfree",
            "RTL8822B KFree power trim: 2g={} 5g={} raw_2g=0x{trim_2g:02x}",
            state.has_2g,
            state.has_5g
        );

        let pa_bias_a = bytes[11];
        if pa_bias_a != 0xff {
            for path in RfPath::iter(chip.total_rf_paths().min(2)) {
                let raw = bytes[11 + path.index()] & 0x0f;
                let magnitude = i16::from(raw >> 1);
                let bias = if raw & 1 != 0 { magnitude } else { -magnitude };
                self.apply_pa_bias_8822b_async(chip, path, bias).await?;
            }
        }

        let _ = self.jaguar2_kfree.set(state);
        Ok(())
    }

    async fn apply_pa_bias_8822b_async(
        &self,
        chip: ChipInfo,
        path: RfPath,
        bias: i16,
    ) -> Result<(), DriverError> {
        // Close any table window first: RF 0x51/0x52 alias LUT contents while
        // 0xef is non-zero.
        self.set_rf_reg_async(chip, path, 0xef, 0x000f_ffff, 0)
            .await?;
        let rf51 = self.query_rf_reg_async(chip, path, 0x51).await?;
        let rf52 = self.query_rf_reg_async(chip, path, 0x52).await?;
        let mut rf3f = ((rf52 & 0x000e_0000) >> 17)
            | (((rf52 & 0x0001_8000) >> 15) << 3)
            | ((rf52 & 0x0f) << 5)
            | (((rf51 & 0x78) >> 3) << 9)
            | (((rf52 & 0x2000) >> 13) << 13);
        let pa = (i16::from(((rf3f & 0x1e00) >> 9) as u8) + bias).clamp(0, 7) as u32;
        rf3f = (rf3f & 0x000f_e1ff) | (pa << 9);

        self.set_rf_reg_async(chip, path, 0xef, 1 << 10, 1).await?;
        self.set_rf_reg_async(chip, path, 0x33, 0x000f_ffff, 0)
            .await?;
        self.set_rf_reg_async(chip, path, 0x3f, 0x000f_ffff, rf3f)
            .await?;
        for selector in [1u32, 3, 3] {
            self.set_rf_reg_async(chip, path, 0x33, 0x03, selector)
                .await?;
            self.set_rf_reg_async(chip, path, 0x3f, 0x000f_ffff, rf3f)
                .await?;
        }
        self.set_rf_reg_async(chip, path, 0xef, 1 << 10, 0).await?;
        log::info!(
            target: "openipc_rtl88xx::kfree",
            "RTL8822B PA bias path={} offset={} rf51=0x{rf51:05x} rf52=0x{rf52:05x} rf3f=0x{rf3f:05x}",
            path.index(),
            bias
        );
        Ok(())
    }

    pub(crate) async fn apply_kfree_8822b_async(
        &self,
        chip: ChipInfo,
        channel: u8,
    ) -> Result<(), DriverError> {
        if chip.family != ChipFamily::Rtl8822b {
            return Ok(());
        }
        let Some(state) = self.jaguar2_kfree.get() else {
            return Ok(());
        };
        let group = if channel <= 14 && state.has_2g {
            Some(0)
        } else if channel > 14 && state.has_5g {
            match channel {
                36..=48 => Some(1),
                52..=64 => Some(2),
                100..=120 => Some(3),
                122..=144 => Some(4),
                149..=u8::MAX => Some(5),
                _ => None,
            }
        } else {
            None
        };

        for path in RfPath::iter(chip.total_rf_paths().min(2)) {
            let data = group.map_or(0, |index| state.gain[index][path.index()]);
            self.set_rf_reg_async(chip, path, 0xde, 1 << 0, 1).await?;
            self.set_rf_reg_async(chip, path, 0xde, 1 << 4, 1).await?;
            self.set_rf_reg_async(chip, path, 0x65, 0xffff, 0x9000)
                .await?;
            self.set_rf_reg_async(chip, path, 0x55, 1 << 5, 1).await?;
            self.set_rf_reg_async(chip, path, 0x55, 1 << 19, u32::from(data & 1))
                .await?;
            self.set_rf_reg_async(chip, path, 0x55, 0x7c000, u32::from((data & 0x1f) >> 1))
                .await?;
            if group.is_none() {
                self.set_rf_reg_async(chip, path, 0xde, 1 << 0, 0).await?;
                self.set_rf_reg_async(chip, path, 0xde, 1 << 4, 1).await?;
                self.set_rf_reg_async(chip, path, 0x65, 0xffff, 0x9000)
                    .await?;
                self.set_rf_reg_async(chip, path, 0x55, 1 << 5, 0).await?;
                self.set_rf_reg_async(chip, path, 0x55, 1 << 7, 0).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_addresses_cover_vendor_kfree_cells() {
        assert!(PHYSICAL_ADDRESSES.contains(&0x3ee));
        assert!(PHYSICAL_ADDRESSES.contains(&0x3ec));
        assert!(PHYSICAL_ADDRESSES.contains(&0x3db));
        assert!(PHYSICAL_ADDRESSES.contains(&0x3d5));
        assert!(PHYSICAL_ADDRESSES.contains(&0x3d6));
    }
}
