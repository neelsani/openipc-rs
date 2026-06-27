use std::future::Future;

use crate::types::{ChipInfo, DriverError};

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
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PhyContext {
    cut_version: u8,
    support_interface: u8,
    support_platform: u8,
    package_type: u8,
    rfe_type: u8,
}

pub(crate) fn phy_context(chip: ChipInfo) -> PhyContext {
    PhyContext {
        cut_version: chip.cut_version,
        support_interface: 0x02,
        support_platform: 0x04,
        package_type: 0,
        rfe_type: 0,
    }
}

pub(crate) async fn load_plain_pairs_async<F, Fut>(
    table: &[u32],
    mut write: F,
) -> Result<(), DriverError>
where
    F: FnMut(u32, u32) -> Fut,
    Fut: Future<Output = Result<(), DriverError>>,
{
    let mut i = 0;
    while i + 1 < table.len() {
        write(table[i], table[i + 1]).await?;
        i += 2;
    }
    Ok(())
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

fn check_positive(ctx: PhyContext, c1: u32, _c2: u32, _c3: u32, _c4: u32) -> bool {
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
