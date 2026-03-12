#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Epoch(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Slot(pub u64);

pub fn slot_to_epoch(slot: Slot, epoch_length: u64) -> Epoch {
    Epoch(slot.0 / epoch_length)
}
