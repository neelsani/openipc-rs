use std::future::Future;

use crate::async_efuse::EfuseInfo;
use crate::types::{ChipFamily, ChipInfo, DriverError};

pub(crate) fn bit_shift(mask: u32) -> u32 {
    mask.trailing_zeros()
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RfPath {
    A,
    B,
    C,
    D,
}

impl RfPath {
    pub(crate) fn iter(count: usize) -> impl Iterator<Item = Self> {
        [Self::A, Self::B, Self::C, Self::D].into_iter().take(count)
    }

    pub(crate) const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::A),
            1 => Some(Self::B),
            2 => Some(Self::C),
            3 => Some(Self::D),
            _ => None,
        }
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::C => 2,
            Self::D => 3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PhyContext {
    condition_mode: PhyConditionMode,
    cut_version: u8,
    support_interface: u8,
    support_platform: u8,
    package_type: u8,
    rfe_type: u8,
    board_type: u8,
    type_gpa: u16,
    type_apa: u16,
    type_glna: u16,
    type_alna: u16,
}

#[derive(Debug, Clone, Copy)]
enum PhyConditionMode {
    BoardAmplifier,
    RfeType,
}

pub(crate) fn phy_context(chip: ChipInfo, efuse: EfuseInfo) -> PhyContext {
    PhyContext {
        condition_mode: match chip.family {
            ChipFamily::Rtl8812 => PhyConditionMode::BoardAmplifier,
            ChipFamily::Rtl8814
            | ChipFamily::Rtl8821
            | ChipFamily::Rtl8822c
            | ChipFamily::Rtl8822e => PhyConditionMode::RfeType,
        },
        cut_version: chip.cut_version,
        support_interface: 0x02,
        support_platform: 0x04,
        package_type: 0,
        rfe_type: efuse.rfe_type,
        board_type: efuse.board_type,
        type_gpa: efuse.type_gpa,
        type_apa: efuse.type_apa,
        type_glna: efuse.type_glna,
        type_alna: efuse.type_alna,
    }
}

pub(crate) async fn load_phy_table_async<F, Fut>(
    table: &[u32],
    ctx: PhyContext,
    mut write: F,
) -> Result<(), DriverError>
where
    F: FnMut(u32, u32) -> Fut,
    Fut: Future<Output = Result<(), DriverError>>,
{
    const BIT_POSITIVE: u32 = 1 << 31;
    const BIT_NEGATIVE: u32 = 1 << 30;
    const C_COND_MASK: u32 = (1 << 29) | (1 << 28);
    const C_COND_SHIFT: u32 = 28;
    const COND_ELSE: u8 = 2;
    const COND_ENDIF: u8 = 3;

    let mut is_matched = true;
    let mut is_skipped = false;
    let mut pre_v1 = 0;
    let mut pre_v2 = 0;
    let mut i = 0;
    while i + 1 < table.len() {
        let v1 = table[i];
        let v2 = table[i + 1];
        if v1 & (BIT_POSITIVE | BIT_NEGATIVE) != 0 {
            if v1 & BIT_POSITIVE != 0 {
                let cond = ((v1 & C_COND_MASK) >> C_COND_SHIFT) as u8;
                if cond == COND_ENDIF {
                    is_matched = true;
                    is_skipped = false;
                } else if cond == COND_ELSE {
                    is_matched = !is_skipped;
                } else {
                    pre_v1 = v1;
                    pre_v2 = v2;
                }
            } else if !is_skipped && check_positive(ctx, pre_v1, pre_v2, v1, v2) {
                is_matched = true;
                is_skipped = true;
            } else {
                is_matched = false;
            }
        } else if is_matched {
            write(v1, v2).await?;
        }
        i += 2;
    }
    Ok(())
}

fn check_positive(ctx: PhyContext, c1: u32, c2: u32, _c3: u32, c4: u32) -> bool {
    match ctx.condition_mode {
        PhyConditionMode::BoardAmplifier => check_positive_board_amplifier(ctx, c1, c2, c4),
        PhyConditionMode::RfeType => check_positive_rfe_type(ctx, c1),
    }
}

fn check_positive_rfe_type(ctx: PhyContext, c1: u32) -> bool {
    let cut_for_para = if ctx.cut_version == 0 {
        15
    } else {
        ctx.cut_version
    };
    let pkg_for_para = if ctx.package_type == 0 {
        15
    } else {
        ctx.package_type
    };
    let driver1 = ((cut_for_para as u32) << 24)
        | (((ctx.support_interface as u32) & 0xf0) << 16)
        | ((ctx.support_platform as u32) << 16)
        | ((pkg_for_para as u32) << 12)
        | (((ctx.support_interface as u32) & 0x0f) << 8)
        | ctx.rfe_type as u32;

    if (c1 & 0x0f00_0000) != 0 && (c1 & 0x0f00_0000) != (driver1 & 0x0f00_0000) {
        return false;
    }
    if (c1 & 0x0000_f000) != 0 && (c1 & 0x0000_f000) != (driver1 & 0x0000_f000) {
        return false;
    }
    if (c1 & 0x0000_0f00) != 0 && (c1 & 0x0000_0f00) != (driver1 & 0x0000_0f00) {
        return false;
    }

    c1 & 0xff == driver1 & 0xff
}

fn check_positive_board_amplifier(ctx: PhyContext, c1: u32, c2: u32, c4: u32) -> bool {
    let cut_for_para = if ctx.cut_version == 0 {
        15
    } else {
        ctx.cut_version
    };
    let pkg_for_para = if ctx.package_type == 0 {
        15
    } else {
        ctx.package_type
    };
    let driver1 = ((cut_for_para as u32) << 24)
        | (((ctx.support_interface as u32) & 0xf0) << 16)
        | ((pkg_for_para as u32) << 12)
        | (((ctx.support_interface as u32) & 0x0f) << 8)
        | ctx.board_type as u32;
    let driver2 = (ctx.type_glna as u32 & 0x00ff)
        | ((ctx.type_gpa as u32 & 0x00ff) << 8)
        | ((ctx.type_alna as u32 & 0x00ff) << 16)
        | ((ctx.type_apa as u32 & 0x00ff) << 24);
    let driver4 = ((ctx.type_glna as u32 & 0xff00) >> 8)
        | (ctx.type_gpa as u32 & 0xff00)
        | ((ctx.type_alna as u32 & 0xff00) << 8)
        | ((ctx.type_apa as u32 & 0xff00) << 16);

    if (c1 & 0x0f00_0000) != 0 && (c1 & 0x0f00_0000) != (driver1 & 0x0f00_0000) {
        return false;
    }
    if (c1 & 0x0000_f000) != 0 && (c1 & 0x0000_f000) != (driver1 & 0x0000_f000) {
        return false;
    }

    let cond1 = c1 & 0x00ff_0fff;
    let driver1 = driver1 & 0x00ff_0fff;
    if (cond1 & driver1) != cond1 {
        return false;
    }

    if cond1 & 0x0f == 0 {
        return true;
    }

    let mut bit_mask = 0;
    if cond1 & 0x01 != 0 {
        bit_mask |= 0x0000_00ff;
    }
    if cond1 & 0x02 != 0 {
        bit_mask |= 0x0000_ff00;
    }
    if cond1 & 0x04 != 0 {
        bit_mask |= 0x00ff_0000;
    }
    if cond1 & 0x08 != 0 {
        bit_mask |= 0xff00_0000;
    }

    (c2 & bit_mask) == (driver2 & bit_mask) && (c4 & bit_mask) == (driver4 & bit_mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(board_type: u8) -> PhyContext {
        PhyContext {
            condition_mode: PhyConditionMode::BoardAmplifier,
            cut_version: 1,
            support_interface: 0x02,
            support_platform: 0x04,
            package_type: 0,
            rfe_type: 0,
            board_type,
            type_gpa: 0,
            type_apa: 0,
            type_glna: 0,
            type_alna: 0,
        }
    }

    fn rfe_ctx(rfe_type: u8) -> PhyContext {
        PhyContext {
            condition_mode: PhyConditionMode::RfeType,
            cut_version: 1,
            support_interface: 0x02,
            support_platform: 0x04,
            package_type: 0,
            rfe_type,
            board_type: 0,
            type_gpa: 0,
            type_apa: 0,
            type_glna: 0,
            type_alna: 0,
        }
    }

    #[test]
    fn matches_aviateur_usb_condition_without_board_type() {
        assert!(check_positive(ctx(0), 0x8000_0200, 0, 0x4000_0000, 0));
    }

    #[test]
    fn uses_board_type_for_condition_low_byte() {
        let mut context = ctx(0x01);
        context.type_glna = 0x02;
        assert!(check_positive(
            context,
            0x8000_0001,
            0x0000_0002,
            0x4000_0000,
            0
        ));
        assert!(!check_positive(
            ctx(0),
            0x8000_0001,
            0x0000_0002,
            0x4000_0000,
            0
        ));
    }

    #[test]
    fn rejects_non_usb_hci_condition() {
        assert!(!check_positive(ctx(0), 0x8000_0400, 0, 0x4000_0000, 0));
    }

    #[test]
    fn rfe_mode_keeps_newer_8814_table_matching() {
        assert!(check_positive(rfe_ctx(1), 0x8000_0001, 0, 0x4000_0000, 0));
        assert!(!check_positive(rfe_ctx(2), 0x8000_0001, 0, 0x4000_0000, 0));
    }
}
