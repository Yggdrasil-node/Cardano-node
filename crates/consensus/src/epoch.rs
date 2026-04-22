use yggdrasil_ledger::{EpochNo, SlotNo};

/// Number of slots per epoch.
///
/// Reference: `Cardano.Slotting.EpochInfo` — `EpochSize`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EpochSize(pub u64);

/// Era-aware epoch schedule covering the Byron→Shelley hard-fork.
///
/// Cardano mainnet, preprod, and preview have a Byron prefix with a
/// fixed slots-per-epoch (21,600) followed by a Shelley-and-later
/// region with a different slots-per-epoch (e.g. 432,000 on
/// mainnet/preprod, 86,400 on preview).  The Byron→Shelley boundary
/// occurs at a network-specific absolute slot, and the first
/// post-Byron epoch number is also network-specific (208 on mainnet,
/// 4 on preprod, 0 on preview).
///
/// When `byron_shelley_transition` is `Some((boundary_slot, first_shelley_epoch))`,
/// slot-to-epoch math uses Byron pacing for `slot < boundary_slot` and
/// Shelley pacing afterwards.  When it is `None`, the schedule
/// degenerates to a simple fixed-length epoch using `slots_per_epoch`,
/// matching the legacy `EpochSize` behavior.
///
/// Reference: `Cardano.Slotting.EpochInfo` plus the hard-fork-history
/// summary in `Ouroboros.Consensus.HardFork.History`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EpochSchedule {
    /// Slots per epoch in the post-Byron (Shelley+) region.
    pub slots_per_epoch: EpochSize,
    /// Slots per epoch in the Byron region.  Defaults to 21,600 on the
    /// public networks.
    pub byron_slots_per_epoch: u64,
    /// Optional Byron→Shelley transition: `(boundary_slot, first_shelley_epoch)`.
    /// `boundary_slot` is the absolute slot of the first Shelley block
    /// (== `byron_epochs * byron_slots_per_epoch`).
    pub byron_shelley_transition: Option<(u64, u64)>,
}

impl EpochSchedule {
    /// Construct a fixed-length schedule (no Byron prefix).
    pub fn fixed(slots_per_epoch: EpochSize) -> Self {
        Self {
            slots_per_epoch,
            byron_slots_per_epoch: 21_600,
            byron_shelley_transition: None,
        }
    }

    /// Construct an era-aware schedule with the given Byron prefix.
    pub fn with_byron_prefix(
        slots_per_epoch: EpochSize,
        byron_slots_per_epoch: u64,
        boundary_slot: u64,
        first_shelley_epoch: u64,
    ) -> Self {
        Self {
            slots_per_epoch,
            byron_slots_per_epoch,
            byron_shelley_transition: Some((boundary_slot, first_shelley_epoch)),
        }
    }

    /// Map an absolute slot to its containing epoch.
    pub fn slot_to_epoch(&self, slot: SlotNo) -> EpochNo {
        match self.byron_shelley_transition {
            Some((boundary_slot, first_shelley_epoch)) if slot.0 >= boundary_slot => {
                let post = slot.0 - boundary_slot;
                EpochNo(first_shelley_epoch + post / self.slots_per_epoch.0)
            }
            Some((_, _)) => EpochNo(slot.0 / self.byron_slots_per_epoch),
            None => slot_to_epoch(slot, self.slots_per_epoch),
        }
    }

    /// True when `slot` is in a different epoch than `prev_slot`.
    pub fn is_new_epoch(&self, prev_slot: Option<SlotNo>, slot: SlotNo) -> bool {
        match prev_slot {
            None => true,
            Some(ps) => self.slot_to_epoch(ps) != self.slot_to_epoch(slot),
        }
    }

    /// Shelley-region epoch size (used by reward formula / pool-perf
    /// expected blocks computations, which are Shelley-only).
    pub fn shelley_epoch_size(&self) -> EpochSize {
        self.slots_per_epoch
    }
}

impl From<EpochSize> for EpochSchedule {
    fn from(size: EpochSize) -> Self {
        Self::fixed(size)
    }
}

/// Converts a slot to its containing epoch given a fixed epoch length.
///
/// Reference: `epochInfoEpoch` applied to a simple fixed-length epoch info.
pub fn slot_to_epoch(slot: SlotNo, epoch_size: EpochSize) -> EpochNo {
    EpochNo(slot.0 / epoch_size.0)
}

/// Returns the first slot of the given epoch.
///
/// Reference: `epochInfoFirst` applied to a simple fixed-length epoch info.
pub fn epoch_first_slot(epoch: EpochNo, epoch_size: EpochSize) -> SlotNo {
    SlotNo(epoch.0 * epoch_size.0)
}

/// Determines whether a slot falls in a new epoch relative to an optional
/// previous slot.
///
/// Reference: `isNewEpoch` in `Ouroboros.Consensus.Protocol.Ledger.Util`.
pub fn is_new_epoch(prev_slot: Option<SlotNo>, slot: SlotNo, epoch_size: EpochSize) -> bool {
    match prev_slot {
        None => true,
        Some(ps) => slot_to_epoch(ps, epoch_size) != slot_to_epoch(slot, epoch_size),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const E100: EpochSize = EpochSize(100);
    const MAINNET: EpochSize = EpochSize(432_000);

    #[test]
    fn slot_to_epoch_basic() {
        assert_eq!(slot_to_epoch(SlotNo(0), E100), EpochNo(0));
        assert_eq!(slot_to_epoch(SlotNo(99), E100), EpochNo(0));
        assert_eq!(slot_to_epoch(SlotNo(100), E100), EpochNo(1));
        assert_eq!(slot_to_epoch(SlotNo(250), E100), EpochNo(2));
    }

    #[test]
    fn slot_to_epoch_mainnet() {
        assert_eq!(slot_to_epoch(SlotNo(0), MAINNET), EpochNo(0));
        assert_eq!(slot_to_epoch(SlotNo(431_999), MAINNET), EpochNo(0));
        assert_eq!(slot_to_epoch(SlotNo(432_000), MAINNET), EpochNo(1));
    }

    #[test]
    fn epoch_first_slot_basic() {
        assert_eq!(epoch_first_slot(EpochNo(0), E100), SlotNo(0));
        assert_eq!(epoch_first_slot(EpochNo(1), E100), SlotNo(100));
        assert_eq!(epoch_first_slot(EpochNo(5), E100), SlotNo(500));
    }

    #[test]
    fn epoch_first_slot_mainnet() {
        assert_eq!(epoch_first_slot(EpochNo(100), MAINNET), SlotNo(43_200_000));
    }

    #[test]
    fn roundtrip_slot_epoch_slot() {
        // First slot of each epoch round-trips
        for e in 0..10 {
            let slot = epoch_first_slot(EpochNo(e), E100);
            assert_eq!(slot_to_epoch(slot, E100), EpochNo(e));
        }
    }

    #[test]
    fn is_new_epoch_none_prev() {
        assert!(is_new_epoch(None, SlotNo(0), E100));
        assert!(is_new_epoch(None, SlotNo(500), E100));
    }

    #[test]
    fn is_new_epoch_same_epoch() {
        assert!(!is_new_epoch(Some(SlotNo(10)), SlotNo(50), E100));
        assert!(!is_new_epoch(Some(SlotNo(0)), SlotNo(99), E100));
    }

    #[test]
    fn is_new_epoch_different_epoch() {
        assert!(is_new_epoch(Some(SlotNo(99)), SlotNo(100), E100));
        assert!(is_new_epoch(Some(SlotNo(50)), SlotNo(200), E100));
    }

    #[test]
    fn is_new_epoch_boundary_exact() {
        // Last slot of epoch 0 vs first slot of epoch 1
        assert!(is_new_epoch(Some(SlotNo(99)), SlotNo(100), E100));
        // Both at boundary of same epoch
        assert!(!is_new_epoch(Some(SlotNo(100)), SlotNo(100), E100));
    }
}
