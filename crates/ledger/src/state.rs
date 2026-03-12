use crate::{Era, LedgerError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerState {
    pub current_era: Era,
    pub tip_slot: u64,
}

impl LedgerState {
    pub fn new(current_era: Era) -> Self {
        Self {
            current_era,
            tip_slot: 0,
        }
    }

    pub fn apply_block(&mut self, block: &crate::tx::Block) -> Result<(), LedgerError> {
        if block.era != self.current_era {
            return Err(LedgerError::UnsupportedEra(block.era));
        }

        self.tip_slot = block.slot_no;
        Ok(())
    }
}
