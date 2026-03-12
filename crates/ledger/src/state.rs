use crate::types::Point;
use crate::{Era, LedgerError};

/// Ledger state tracking the current era and chain tip.
///
/// The `tip` is represented as a [`Point`]: `Origin` before any block is
/// applied, or `BlockPoint(slot, hash)` after at least one block.
///
/// Reference: `Ouroboros.Consensus.Ledger.Abstract` — `LedgerState`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerState {
    /// The ledger era currently in effect.
    pub current_era: Era,
    /// Chain tip as a point (slot + header hash).
    pub tip: Point,
}

impl LedgerState {
    /// Creates a new ledger state rooted at the given era with an `Origin`
    /// tip.
    pub fn new(current_era: Era) -> Self {
        Self {
            current_era,
            tip: Point::Origin,
        }
    }

    /// Applies a block to the current state when the era matches.
    ///
    /// On success the tip advances to the applied block's slot and hash.
    pub fn apply_block(&mut self, block: &crate::tx::Block) -> Result<(), LedgerError> {
        if block.era != self.current_era {
            return Err(LedgerError::UnsupportedEra(block.era));
        }

        self.tip = Point::BlockPoint(block.header.slot_no, block.header.hash);
        Ok(())
    }
}
