use crate::eras::shelley::{ShelleyTxBody, ShelleyUtxo};
use crate::types::Point;
use crate::{CborDecode, Era, LedgerError};

/// Ledger state tracking the current era, chain tip, and UTxO set.
///
/// For Shelley-era blocks the `apply_block` method decodes each transaction
/// body and applies the UTxO transition rules via `ShelleyUtxo::apply_tx`.
/// Non-Shelley eras advance the tip only (UTxO validation is not yet
/// implemented for other eras).
///
/// Reference: `Ouroboros.Consensus.Ledger.Abstract` — `LedgerState`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerState {
    /// The ledger era currently in effect.
    pub current_era: Era,
    /// Chain tip as a point (slot + header hash).
    pub tip: Point,
    /// Shelley-era UTxO set; only populated when applying Shelley blocks.
    utxo: ShelleyUtxo,
}

impl LedgerState {
    /// Creates a new ledger state rooted at the given era with an `Origin`
    /// tip and an empty UTxO set.
    pub fn new(current_era: Era) -> Self {
        Self {
            current_era,
            tip: Point::Origin,
            utxo: ShelleyUtxo::new(),
        }
    }

    /// Returns a reference to the current Shelley UTxO set.
    pub fn utxo(&self) -> &ShelleyUtxo {
        &self.utxo
    }

    /// Returns a mutable reference to the current Shelley UTxO set.
    ///
    /// This is useful for seeding the initial UTxO before block application
    /// begins (e.g. from a snapshot or genesis distribution).
    pub fn utxo_mut(&mut self) -> &mut ShelleyUtxo {
        &mut self.utxo
    }

    /// Applies a block to the current state when the era matches.
    ///
    /// For Shelley-era blocks, each transaction body is decoded from CBOR
    /// and applied to the UTxO set. On any UTxO validation failure the
    /// state is unchanged.
    ///
    /// On success the tip advances to the applied block's slot and hash.
    pub fn apply_block(&mut self, block: &crate::tx::Block) -> Result<(), LedgerError> {
        if block.era != self.current_era {
            return Err(LedgerError::UnsupportedEra(block.era));
        }

        let slot = block.header.slot_no.0;

        if block.era == Era::Shelley && !block.transactions.is_empty() {
            // Decode and validate all transactions before mutating state,
            // so a failure in the middle does not leave a partial update.
            let decoded: Vec<(crate::types::TxId, ShelleyTxBody)> = block
                .transactions
                .iter()
                .map(|tx| {
                    let body = ShelleyTxBody::from_cbor_bytes(&tx.body)?;
                    Ok((tx.id, body))
                })
                .collect::<Result<Vec<_>, LedgerError>>()?;

            // Apply all transactions; clone the UTxO to preserve atomicity.
            let mut staged = self.utxo.clone();
            for (tx_id, body) in &decoded {
                staged.apply_tx(tx_id.0, body, slot)?;
            }
            self.utxo = staged;
        }

        self.tip = Point::BlockPoint(block.header.slot_no, block.header.hash);
        Ok(())
    }
}
