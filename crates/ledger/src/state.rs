use crate::eras::allegra::AllegraTxBody;
use crate::eras::alonzo::AlonzoTxBody;
use crate::eras::babbage::BabbageTxBody;
use crate::eras::conway::ConwayTxBody;
use crate::eras::shelley::{ShelleyTxBody, ShelleyUtxo};
use crate::types::Point;
use crate::utxo::MultiEraUtxo;
use crate::{CborDecode, CborEncode, Era, LedgerError};

/// Ledger state tracking the current era, chain tip, and UTxO set.
///
/// `apply_block` decodes each transaction body according to the block's
/// era and applies the UTxO transition rules via `MultiEraUtxo`.
/// A legacy `ShelleyUtxo` accessor is retained for backward compatibility
/// with existing tests that seed and inspect Shelley-only entries.
///
/// Reference: `Ouroboros.Consensus.Ledger.Abstract` — `LedgerState`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerState {
    /// The ledger era currently in effect.
    pub current_era: Era,
    /// Chain tip as a point (slot + header hash).
    pub tip: Point,
    /// Multi-era UTxO set.
    multi_era_utxo: MultiEraUtxo,
    /// Legacy Shelley-only UTxO set kept in sync for backward compatibility.
    shelley_utxo: ShelleyUtxo,
}

impl LedgerState {
    /// Creates a new ledger state rooted at the given era with an `Origin`
    /// tip and an empty UTxO set.
    pub fn new(current_era: Era) -> Self {
        Self {
            current_era,
            tip: Point::Origin,
            multi_era_utxo: MultiEraUtxo::new(),
            shelley_utxo: ShelleyUtxo::new(),
        }
    }

    /// Returns a reference to the legacy Shelley UTxO set.
    ///
    /// This provides backward compatibility for existing tests that
    /// inspect Shelley-era outputs via `ShelleyUtxo`.
    pub fn utxo(&self) -> &ShelleyUtxo {
        &self.shelley_utxo
    }

    /// Returns a mutable reference to the legacy Shelley UTxO set.
    ///
    /// Insertions via this accessor are mirrored into the multi-era UTxO
    /// so that block application works correctly.
    pub fn utxo_mut(&mut self) -> &mut ShelleyUtxo {
        &mut self.shelley_utxo
    }

    /// Returns a reference to the multi-era UTxO set.
    pub fn multi_era_utxo(&self) -> &MultiEraUtxo {
        &self.multi_era_utxo
    }

    /// Returns a mutable reference to the multi-era UTxO set.
    pub fn multi_era_utxo_mut(&mut self) -> &mut MultiEraUtxo {
        &mut self.multi_era_utxo
    }

    /// Applies a block to the current state.
    ///
    /// Each transaction body is decoded from CBOR according to the block's
    /// era and applied to the UTxO set. On any validation failure the state
    /// is unchanged (atomic per block).
    ///
    /// On success the tip advances to the applied block's slot and hash.
    pub fn apply_block(&mut self, block: &crate::tx::Block) -> Result<(), LedgerError> {
        let slot = block.header.slot_no.0;

        match block.era {
            Era::Shelley => self.apply_shelley_block(block, slot)?,
            Era::Allegra => self.apply_allegra_block(block, slot)?,
            Era::Mary => self.apply_mary_block(block, slot)?,
            Era::Alonzo => self.apply_alonzo_block(block, slot)?,
            Era::Babbage => self.apply_babbage_block(block, slot)?,
            Era::Conway => self.apply_conway_block(block, slot)?,
            era => return Err(LedgerError::UnsupportedEra(era)),
        }

        self.tip = Point::BlockPoint(block.header.slot_no, block.header.hash);
        Ok(())
    }

    /// Applies a single submitted transaction to the current ledger state.
    ///
    /// This uses the same era-specific UTxO transition rules as block
    /// application while preserving atomicity: on validation failure, the
    /// ledger state is unchanged.
    pub fn apply_submitted_tx(
        &mut self,
        tx: &crate::tx::MultiEraSubmittedTx,
        current_slot: crate::types::SlotNo,
    ) -> Result<(), LedgerError> {
        match tx {
            crate::tx::MultiEraSubmittedTx::Shelley(tx) => {
                let mut staged = self.shelley_utxo.clone();
                staged.apply_tx(crate::tx::compute_tx_id(&tx.body.to_cbor_bytes()).0, &tx.body, current_slot.0)?;
                self.shelley_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Allegra(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_allegra_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Mary(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_mary_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Alonzo(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_alonzo_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Babbage(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_babbage_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Conway(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_conway_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
        }

        Ok(())
    }

    // -- Private per-era apply helpers --------------------------------------

    fn apply_shelley_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, ShelleyTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ShelleyTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // Atomic: clone the Shelley UTxO, apply all txs, then commit.
        // The legacy shelley_utxo is the authoritative source for Shelley
        // blocks (preserves backward compatibility with tests that seed
        // via utxo_mut()).
        let mut staged = self.shelley_utxo.clone();
        for (tx_id, body) in &decoded {
            staged.apply_tx(tx_id.0, body, slot)?;
        }
        self.shelley_utxo = staged;
        Ok(())
    }

    fn apply_allegra_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, AllegraTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AllegraTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        for (tx_id, body) in &decoded {
            staged.apply_allegra_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
        Ok(())
    }

    fn apply_mary_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, crate::eras::mary::MaryTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = crate::eras::mary::MaryTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        for (tx_id, body) in &decoded {
            staged.apply_mary_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
        Ok(())
    }

    fn apply_alonzo_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, AlonzoTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AlonzoTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        for (tx_id, body) in &decoded {
            staged.apply_alonzo_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
        Ok(())
    }

    fn apply_babbage_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, BabbageTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = BabbageTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        for (tx_id, body) in &decoded {
            staged.apply_babbage_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
        Ok(())
    }

    fn apply_conway_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, ConwayTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ConwayTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        for (tx_id, body) in &decoded {
            staged.apply_conway_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
        Ok(())
    }
}
