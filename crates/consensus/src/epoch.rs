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
