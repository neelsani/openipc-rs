/// Maximum bytes in one Realtek USB TX aggregate, matching Devourer's buffer.
pub const MAX_TX_AGGREGATE_BYTES: usize = 20_480;

/// Generation-specific limits used to pack one USB bulk-OUT transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxAggregateLimits {
    /// Hardware TX descriptor size in bytes.
    pub descriptor_size: usize,
    /// USB bulk endpoint maximum-packet size.
    pub bulk_size: usize,
    /// Maximum aggregate transfer size.
    pub max_bytes: usize,
    /// Maximum number of frames represented by the first descriptor.
    pub max_frames: usize,
    /// Jaguar1 descriptor-start limit per bulk window; zero disables it.
    pub descriptors_per_bulk: usize,
    /// Whether the first block normally carries an 8-byte packet-offset pad.
    pub first_reserve: bool,
}

/// One descriptor and frame block inside a packed USB transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxAggregateBlock {
    /// Byte offset of the TX descriptor in the transfer.
    pub offset: usize,
    /// Descriptor, optional first-frame pad, and frame length.
    pub length: usize,
}

/// Pure layout plan for one Realtek USB TX aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TxAggregatePlan {
    /// Accepted prefix of input frames in transfer order.
    pub blocks: Vec<TxAggregateBlock>,
    /// Final unpadded USB transfer length.
    pub total: usize,
    /// Whether the first descriptor advertises one 8-byte packet-offset unit.
    pub first_shim: bool,
}

impl TxAggregatePlan {
    /// Number of input frames accepted into this transfer.
    pub fn frame_count(&self) -> usize {
        self.blocks.len()
    }
}

/// Greedily plan the accepted prefix of frames for one bulk-OUT transfer.
///
/// This is a direct Rust expression of Devourer's `plan_tx_agg`: every block
/// begins on an 8-byte boundary, Jaguar1 observes its OQT descriptor-start
/// limit, and the first-frame reserve flips to avoid an exact USB MPS length.
pub fn plan_tx_aggregate(frame_lengths: &[usize], limits: TxAggregateLimits) -> TxAggregatePlan {
    let mut plan = TxAggregatePlan {
        first_shim: limits.first_reserve,
        ..TxAggregatePlan::default()
    };
    if frame_lengths.is_empty() || limits.bulk_size == 0 || limits.max_frames == 0 {
        return plan;
    }

    let capacity = if limits.first_reserve {
        limits.max_bytes
    } else {
        limits.max_bytes.saturating_sub(8)
    };
    let mut tail = 0usize;
    let mut next = 0usize;
    let mut descriptor_count = 0usize;
    let mut bulk_end = limits.bulk_size;

    for &frame_length in frame_lengths {
        let first_pad = usize::from(plan.blocks.is_empty() && plan.first_shim) * 8;
        let block_length = limits
            .descriptor_size
            .saturating_add(frame_length)
            .saturating_add(first_pad);
        if next.saturating_add(block_length) > capacity {
            break;
        }
        plan.blocks.push(TxAggregateBlock {
            offset: next,
            length: block_length,
        });
        tail = next + block_length;
        next = round_up_8(tail);
        if plan.blocks.len() >= limits.max_frames {
            break;
        }
        if next < bulk_end {
            descriptor_count += 1;
            if limits.descriptors_per_bulk != 0 && descriptor_count >= limits.descriptors_per_bulk {
                break;
            }
        } else {
            descriptor_count = 0;
            bulk_end = (next / limits.bulk_size + 1) * limits.bulk_size;
        }
    }
    plan.total = tail;

    if plan.total != 0 && plan.total.is_multiple_of(limits.bulk_size) {
        plan.first_shim = !plan.first_shim;
        if limits.first_reserve {
            plan.blocks[0].length -= 8;
            for block in &mut plan.blocks[1..] {
                block.offset -= 8;
            }
            plan.total -= 8;
        } else {
            plan.blocks[0].length += 8;
            for block in &mut plan.blocks[1..] {
                block.offset += 8;
            }
            plan.total += 8;
        }
    }
    plan
}

const fn round_up_8(value: usize) -> usize {
    (value + 7) & !7
}

#[cfg(test)]
mod tests {
    use super::*;

    fn halmac_hs() -> TxAggregateLimits {
        TxAggregateLimits {
            descriptor_size: 48,
            bulk_size: 512,
            max_bytes: MAX_TX_AGGREGATE_BYTES,
            max_frames: 255,
            descriptors_per_bulk: 3,
            first_reserve: false,
        }
    }

    #[test]
    fn matches_devourer_basic_layouts() {
        let plan = plan_tx_aggregate(&[1500, 1500, 1500], halmac_hs());
        assert_eq!(plan.frame_count(), 3);
        assert!(!plan.first_shim);
        assert_eq!(
            plan.blocks[0],
            TxAggregateBlock {
                offset: 0,
                length: 1548
            }
        );
        assert_eq!(plan.blocks[1].offset, 1552);
        assert_eq!(plan.total, plan.blocks[2].offset + plan.blocks[2].length);
        assert!(!plan.total.is_multiple_of(512));

        let mut jaguar1 = halmac_hs();
        jaguar1.descriptor_size = 40;
        jaguar1.first_reserve = true;
        let plan = plan_tx_aggregate(&[1500, 1500, 1500], jaguar1);
        assert!(plan.first_shim);
        assert_eq!(plan.blocks[0].length, 1548);
        assert_eq!(plan.blocks[1].length, 1540);
    }

    #[test]
    fn flips_first_shim_at_bulk_boundary() {
        let inserted = plan_tx_aggregate(&[464], halmac_hs());
        assert!(inserted.first_shim);
        assert_eq!(inserted.total, 520);

        let mut jaguar1 = halmac_hs();
        jaguar1.descriptor_size = 40;
        jaguar1.first_reserve = true;
        let removed = plan_tx_aggregate(&[464], jaguar1);
        assert!(!removed.first_shim);
        assert_eq!(removed.total, 504);

        for first_reserve in [false, true] {
            let mut limits = halmac_hs();
            limits.first_reserve = first_reserve;
            for length in 1..1600 {
                let plan = plan_tx_aggregate(&[length; 4], limits);
                assert!(plan.total == 0 || !plan.total.is_multiple_of(512));
            }
        }
    }

    #[test]
    fn applies_oqt_and_flat_frame_caps() {
        let mut jaguar1 = halmac_hs();
        jaguar1.descriptor_size = 40;
        jaguar1.descriptors_per_bulk = 1;
        jaguar1.first_reserve = true;
        assert_eq!(plan_tx_aggregate(&[100; 4], jaguar1).frame_count(), 1);
        assert_eq!(plan_tx_aggregate(&[1500; 4], jaguar1).frame_count(), 4);

        let mut halmac = halmac_hs();
        halmac.max_frames = 3;
        assert_eq!(plan_tx_aggregate(&[40; 8], halmac).frame_count(), 3);
    }

    #[test]
    fn applies_byte_and_frame_caps() {
        let mut limits = halmac_hs();
        limits.max_frames = 2;
        assert_eq!(plan_tx_aggregate(&[1000; 3], limits).frame_count(), 2);

        limits = halmac_hs();
        limits.max_bytes = 2200;
        assert_eq!(plan_tx_aggregate(&[1000; 3], limits).frame_count(), 2);

        limits.max_bytes = 512;
        assert_eq!(plan_tx_aggregate(&[4000], limits).frame_count(), 0);
    }
}
