use yggdrasil_ledger::{EpochNo, SlotNo};

/// Number of slots per epoch.
///
/// Reference: `Cardano.Slotting.EpochInfo` — `EpochSize`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EpochSize(pub u64);

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
