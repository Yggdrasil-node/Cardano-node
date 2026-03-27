use yggdrasil_ledger::{BlockNo, SlotNo};

/// Controls how VRF tiebreakers are applied when comparing equal-length chains.
///
/// Reference: `VRFTiebreakerFlavor` in
/// `ouroboros-consensus/src/ouroboros-consensus-protocol/src/Ouroboros/Consensus/Protocol/Praos/Common.hs`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VrfTiebreakerFlavor {
    /// Always apply VRF tiebreaker regardless of slot distance.
    /// Used before the Conway hard fork.
    UnrestrictedVrfTiebreaker,
    /// Only apply VRF tiebreaker when the slot distance between candidates
    /// is at most `max_dist` (`maxDist` in upstream, typically `3k / f`).
    /// Introduced with Conway to prevent extreme-slot-distance preference swings.
    RestrictedVrfTiebreaker { max_dist: u64 },
}

/// A chain candidate summarizing the tip of a candidate chain for Praos
/// fork-choice comparison.
///
/// Reference: `PraosTiebreakerView` in
/// `ouroboros-consensus/src/ouroboros-consensus-protocol/src/Ouroboros/Consensus/Protocol/Praos/Common.hs`.
///
/// Fields mirror upstream naming:
/// - `ptvSlotNo` → `slot_no`
/// - `ptvIssuer` → `issuer_vkey_hash`
/// - `ptvIssueNo` → `ocert_issue_no`
/// - `ptvTieBreakVRF` → `vrf_tiebreaker`
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainCandidate {
    /// Block height at the tip.
    pub block_no: BlockNo,
    /// Slot at the tip.
    pub slot_no: SlotNo,
    /// Blake2b-224 hash of the block issuer's cold verification key.
    /// Used together with `slot_no` to determine whether two candidates
    /// share the same issuer at the same slot (OCert comparison path).
    pub issuer_vkey_hash: Option<[u8; 28]>,
    /// Operational certificate issue number (counter).
    /// When two candidates share the same issuer and slot, the higher
    /// issue number wins (protects against hot-key compromise).
    pub ocert_issue_no: Option<u64>,
    /// VRF tiebreaker output (lower wins).
    /// When `None`, the candidate does not participate in VRF-based tie
    /// resolution.
    pub vrf_tiebreaker: Option<[u8; 32]>,
}

/// Selects the preferred chain candidate following Praos ordering.
///
/// 1. Higher `block_no` wins (longer chain preferred).
/// 2. On equal `block_no`:
///    a. If both candidates have the **same issuer** at the **same slot**,
///       the higher `ocert_issue_no` wins.
///    b. Otherwise, VRF tiebreaker: lower value wins, subject to
///       `VrfTiebreakerFlavor` slot-distance restriction.
/// 3. If no tiebreaker applies, the left (incumbent) candidate wins.
///
/// Reference: `comparePraos` in
/// `ouroboros-consensus/src/ouroboros-consensus-protocol/src/Ouroboros/Consensus/Protocol/Praos/Common.hs`.
pub fn select_preferred(
    left: ChainCandidate,
    right: ChainCandidate,
    flavor: VrfTiebreakerFlavor,
) -> ChainCandidate {
    // Step 1: longer chain always wins.
    match right.block_no.0.cmp(&left.block_no.0) {
        std::cmp::Ordering::Greater => return right,
        std::cmp::Ordering::Less => return left,
        std::cmp::Ordering::Equal => {}
    }

    // Step 2a: same issuer at the same slot → higher OCert issue number.
    if let (Some(l_issuer), Some(r_issuer)) =
        (left.issuer_vkey_hash, right.issuer_vkey_hash)
    {
        if l_issuer == r_issuer && left.slot_no == right.slot_no {
            if let (Some(l_no), Some(r_no)) = (left.ocert_issue_no, right.ocert_issue_no) {
                return match r_no.cmp(&l_no) {
                    std::cmp::Ordering::Greater => right,
                    std::cmp::Ordering::Less => left,
                    std::cmp::Ordering::Equal => left, // identical → incumbent
                };
            }
        }
    }

    // Step 2b: VRF tiebreaker (lower output wins), subject to flavor.
    let vrf_armed = match flavor {
        VrfTiebreakerFlavor::UnrestrictedVrfTiebreaker => true,
        VrfTiebreakerFlavor::RestrictedVrfTiebreaker { max_dist } => {
            let l = left.slot_no.0;
            let r = right.slot_no.0;
            let dist = if l >= r { l - r } else { r - l };
            dist <= max_dist
        }
    };

    if vrf_armed {
        if let (Some(l_vrf), Some(r_vrf)) = (left.vrf_tiebreaker, right.vrf_tiebreaker) {
            return match r_vrf.cmp(&l_vrf) {
                std::cmp::Ordering::Less => right,
                std::cmp::Ordering::Greater => left,
                std::cmp::Ordering::Equal => left, // identical → incumbent
            };
        }
    }

    // No tiebreaker applied → incumbent wins.
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default flavor used in most tests (pre-Conway behavior).
    const UNRESTRICTED: VrfTiebreakerFlavor = VrfTiebreakerFlavor::UnrestrictedVrfTiebreaker;

    fn mk(block_no: u64, slot: u64, vrf: Option<[u8; 32]>) -> ChainCandidate {
        ChainCandidate {
            block_no: BlockNo(block_no),
            slot_no: SlotNo(slot),
            issuer_vkey_hash: None,
            ocert_issue_no: None,
            vrf_tiebreaker: vrf,
        }
    }

    fn mk_full(
        block_no: u64,
        slot: u64,
        issuer: Option<[u8; 28]>,
        ocert: Option<u64>,
        vrf: Option<[u8; 32]>,
    ) -> ChainCandidate {
        ChainCandidate {
            block_no: BlockNo(block_no),
            slot_no: SlotNo(slot),
            issuer_vkey_hash: issuer,
            ocert_issue_no: ocert,
            vrf_tiebreaker: vrf,
        }
    }

    // -- Block number (longest chain) --

    #[test]
    fn higher_block_no_wins() {
        let a = mk(10, 100, None);
        let b = mk(20, 200, None);
        assert_eq!(select_preferred(a, b, UNRESTRICTED), mk(20, 200, None));
    }

    #[test]
    fn lower_block_no_loses() {
        let a = mk(20, 100, None);
        let b = mk(10, 200, None);
        assert_eq!(select_preferred(a, b, UNRESTRICTED), mk(20, 100, None));
    }

    #[test]
    fn block_no_takes_priority_over_vrf() {
        let a = mk(20, 1000, Some([0xFF; 32]));
        let b = mk(10, 1, Some([0x00; 32]));
        assert_eq!(
            select_preferred(a, b, UNRESTRICTED),
            mk(20, 1000, Some([0xFF; 32]))
        );
    }

    // -- VRF tiebreaker (equal block_no, different issuers or slots) --

    #[test]
    fn equal_block_no_vrf_lower_wins() {
        let low_vrf = [0x00; 32];
        let high_vrf = [0xFF; 32];
        let a = mk(10, 50, Some(high_vrf));
        let b = mk(10, 100, Some(low_vrf));
        assert_eq!(
            select_preferred(a, b, UNRESTRICTED),
            mk(10, 100, Some(low_vrf))
        );
    }

    #[test]
    fn equal_block_no_same_slot_vrf_lower_wins() {
        let low_vrf = [0x00; 32];
        let high_vrf = [0xFF; 32];
        let a = mk(10, 50, Some(high_vrf));
        let b = mk(10, 50, Some(low_vrf));
        assert_eq!(
            select_preferred(a, b, UNRESTRICTED),
            mk(10, 50, Some(low_vrf))
        );
    }

    #[test]
    fn equal_vrf_left_wins() {
        let same_vrf = [0xAA; 32];
        let a = mk(10, 50, Some(same_vrf));
        let b = mk(10, 50, Some(same_vrf));
        let result = select_preferred(a, b, UNRESTRICTED);
        assert_eq!(result.vrf_tiebreaker, Some(same_vrf));
        // Incumbent (left) wins on identical VRF.
        assert_eq!(result.slot_no, SlotNo(50));
    }

    #[test]
    fn no_vrf_left_wins() {
        let a = mk(10, 50, None);
        let b = mk(10, 100, None);
        assert_eq!(select_preferred(a, b, UNRESTRICTED), mk(10, 50, None));
    }

    #[test]
    fn one_has_vrf_other_doesnt_left_wins() {
        let a = mk(10, 50, Some([0xFF; 32]));
        let b = mk(10, 50, None);
        assert_eq!(
            select_preferred(a, b, UNRESTRICTED),
            mk(10, 50, Some([0xFF; 32]))
        );
    }

    // -- OCert tiebreaker (same issuer, same slot) --

    #[test]
    fn same_issuer_same_slot_higher_ocert_wins() {
        let issuer = [0x01; 28];
        let a = mk_full(10, 50, Some(issuer), Some(3), Some([0x00; 32]));
        let b = mk_full(10, 50, Some(issuer), Some(7), Some([0xFF; 32]));
        // Higher OCert wins, even though `a` has a lower VRF.
        assert_eq!(select_preferred(a, b, UNRESTRICTED), b);
    }

    #[test]
    fn same_issuer_same_slot_equal_ocert_incumbent_wins() {
        let issuer = [0x02; 28];
        let a = mk_full(10, 50, Some(issuer), Some(5), Some([0xFF; 32]));
        let b = mk_full(10, 50, Some(issuer), Some(5), Some([0x00; 32]));
        // Equal OCert → incumbent (left) wins, skipping VRF.
        assert_eq!(select_preferred(a, b, UNRESTRICTED), a);
    }

    #[test]
    fn same_issuer_different_slot_uses_vrf_not_ocert() {
        let issuer = [0x03; 28];
        let a = mk_full(10, 50, Some(issuer), Some(3), Some([0xFF; 32]));
        let b = mk_full(10, 60, Some(issuer), Some(7), Some([0x00; 32]));
        // Different slot → OCert not consulted; lower VRF wins.
        assert_eq!(select_preferred(a, b, UNRESTRICTED), b);
    }

    #[test]
    fn different_issuer_same_slot_uses_vrf_not_ocert() {
        let a = mk_full(10, 50, Some([0x01; 28]), Some(3), Some([0xFF; 32]));
        let b = mk_full(10, 50, Some([0x02; 28]), Some(1), Some([0x00; 32]));
        // Different issuer → OCert not consulted; lower VRF wins.
        assert_eq!(select_preferred(a, b, UNRESTRICTED), b);
    }

    // -- RestrictedVrfTiebreaker --

    #[test]
    fn restricted_vrf_within_distance_tiebreaks() {
        let flavor = VrfTiebreakerFlavor::RestrictedVrfTiebreaker { max_dist: 100 };
        let a = mk(10, 50, Some([0xFF; 32]));
        let b = mk(10, 140, Some([0x00; 32]));
        // Distance = 90 ≤ 100 → VRF armed, lower wins.
        assert_eq!(select_preferred(a, b, flavor), b);
    }

    #[test]
    fn restricted_vrf_beyond_distance_no_tiebreak() {
        let flavor = VrfTiebreakerFlavor::RestrictedVrfTiebreaker { max_dist: 100 };
        let a = mk(10, 50, Some([0xFF; 32]));
        let b = mk(10, 200, Some([0x00; 32]));
        // Distance = 150 > 100 → VRF not armed, incumbent wins.
        assert_eq!(select_preferred(a, b, flavor), a);
    }

    #[test]
    fn restricted_vrf_exact_boundary() {
        let flavor = VrfTiebreakerFlavor::RestrictedVrfTiebreaker { max_dist: 100 };
        let a = mk(10, 50, Some([0xFF; 32]));
        let b = mk(10, 150, Some([0x00; 32]));
        // Distance = 100 == max_dist → armed (≤ check), lower VRF wins.
        assert_eq!(select_preferred(a, b, flavor), b);
    }

    #[test]
    fn restricted_same_issuer_same_slot_still_uses_ocert() {
        // OCert path does not depend on VRF flavor.
        let flavor = VrfTiebreakerFlavor::RestrictedVrfTiebreaker { max_dist: 0 };
        let issuer = [0x04; 28];
        let a = mk_full(10, 50, Some(issuer), Some(1), Some([0x00; 32]));
        let b = mk_full(10, 50, Some(issuer), Some(9), Some([0xFF; 32]));
        assert_eq!(select_preferred(a, b, flavor), b);
    }
}
