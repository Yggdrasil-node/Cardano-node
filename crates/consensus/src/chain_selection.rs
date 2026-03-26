use yggdrasil_ledger::{BlockNo, SlotNo};

/// A chain candidate summarizing the tip of a candidate chain.
///
/// Reference: `ChainCandidate` is a simplified local concept — the official
/// node selects chains by comparing `BlockNo` (longer chain wins), with
/// slot-based VRF tiebreakers as a secondary criterion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainCandidate {
    /// Block height at the tip.
    pub block_no: BlockNo,
    /// Slot at the tip.
    pub slot_no: SlotNo,
    /// Optional VRF tiebreaker (lower wins).  When `None`, the candidate
    /// does not participate in VRF-based tie resolution.
    pub vrf_tiebreaker: Option<[u8; 32]>,
}

/// Selects the preferred chain candidate following Praos ordering:
///
/// 1. Higher `block_no` wins.
/// 2. On equal `block_no`, slot-number comparison (lower slot wins — the
///    block was produced earlier).
/// 3. On equal `block_no` **and** `slot_no`, VRF tiebreaker (lower hash wins).
///
/// Reference: the upstream `ChainOrder` instance for `PraosTiebreakerView`
/// drives selection; we capture the essence here without the full KES/DSIGN
/// ceremony.
pub fn select_preferred(left: ChainCandidate, right: ChainCandidate) -> ChainCandidate {
    match right.block_no.0.cmp(&left.block_no.0) {
        std::cmp::Ordering::Greater => right,
        std::cmp::Ordering::Less => left,
        std::cmp::Ordering::Equal => {
            // Equal block height — prefer the earlier slot.
            match left.slot_no.0.cmp(&right.slot_no.0) {
                std::cmp::Ordering::Less => left,
                std::cmp::Ordering::Greater => right,
                std::cmp::Ordering::Equal => {
                    // Equal slot — VRF tiebreaker (lower wins).
                    match (left.vrf_tiebreaker, right.vrf_tiebreaker) {
                        (Some(l), Some(r)) if r < l => right,
                        _ => left,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(block_no: u64, slot: u64, vrf: Option<[u8; 32]>) -> ChainCandidate {
        ChainCandidate {
            block_no: BlockNo(block_no),
            slot_no: SlotNo(slot),
            vrf_tiebreaker: vrf,
        }
    }

    #[test]
    fn higher_block_no_wins() {
        let a = mk(10, 100, None);
        let b = mk(20, 200, None);
        assert_eq!(select_preferred(a, b), mk(20, 200, None));
    }

    #[test]
    fn lower_block_no_loses() {
        let a = mk(20, 100, None);
        let b = mk(10, 200, None);
        assert_eq!(select_preferred(a, b), mk(20, 100, None));
    }

    #[test]
    fn equal_block_no_earlier_slot_wins() {
        let a = mk(10, 50, None);
        let b = mk(10, 100, None);
        assert_eq!(select_preferred(a, b), mk(10, 50, None));
    }

    #[test]
    fn equal_block_no_later_slot_loses() {
        let a = mk(10, 100, None);
        let b = mk(10, 50, None);
        assert_eq!(select_preferred(a, b), mk(10, 50, None));
    }

    #[test]
    fn equal_block_and_slot_vrf_lower_wins() {
        let low_vrf = [0x00; 32];
        let high_vrf = [0xFF; 32];
        let a = mk(10, 50, Some(high_vrf));
        let b = mk(10, 50, Some(low_vrf));
        assert_eq!(select_preferred(a, b), mk(10, 50, Some(low_vrf)));
    }

    #[test]
    fn equal_block_and_slot_vrf_left_preferred_when_equal() {
        let same_vrf = [0xAA; 32];
        let a = mk(10, 50, Some(same_vrf));
        let b = mk(10, 50, Some(same_vrf));
        // Left wins (fall through)
        let result = select_preferred(a, b);
        assert_eq!(result.vrf_tiebreaker, Some(same_vrf));
    }

    #[test]
    fn equal_block_and_slot_no_vrf_left_wins() {
        let a = mk(10, 50, None);
        let b = mk(10, 50, None);
        assert_eq!(select_preferred(a, b), mk(10, 50, None));
    }

    #[test]
    fn one_has_vrf_other_doesnt_left_wins() {
        let a = mk(10, 50, Some([0xFF; 32]));
        let b = mk(10, 50, None);
        // (Some(l), None) → left wins (doesn't match the `(Some, Some) if r < l` arm)
        assert_eq!(select_preferred(a, b), mk(10, 50, Some([0xFF; 32])));
    }

    #[test]
    fn block_no_takes_priority_over_slot() {
        // Higher block_no wins even with worse slot
        let a = mk(20, 1000, None);
        let b = mk(10, 1, None);
        assert_eq!(select_preferred(a, b), mk(20, 1000, None));
    }
}
