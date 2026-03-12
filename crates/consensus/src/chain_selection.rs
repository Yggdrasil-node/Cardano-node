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
