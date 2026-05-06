//! Byron-era block application on `LedgerState`.
//!
//! Mirrors upstream `Cardano.Chain.Block.Validation` /
//! `Cardano.Chain.UTxO.UTxO` rules: Byron block application advances
//! only the multi-era UTxO; there are no certificates, no governance,
//! no rewards, no protocol-parameter updates at this layer (Byron
//! protocol-parameter updates went through the legacy update mechanism
//! and are tracked separately).
//!
//! Pre-computed `Tx.id` is used (not re-derived from the decoded
//! structure) because Byron tx_ids hash the on-wire annotated CBOR
//! bytes, and re-encoding can produce a different byte sequence (e.g.
//! definite vs indefinite arrays) which would yield a wrong tx_id and
//! cause every spend of that output to fail with `InputNotFound`.
//!
//! Reference: `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/`
//! and `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node/`.

use super::super::LedgerState;
use crate::eras::byron::ByronTx;
use crate::{CborDecode, LedgerError};

impl LedgerState {
    /// Apply a Byron block — advances the multi-era UTxO only.
    ///
    /// Decodes each `Tx.body` (CBOR-encoded `ByronTx`) back into typed
    /// form, then applies them atomically: clones the multi-era UTxO,
    /// applies all txs, commits on success.
    pub(in crate::state) fn apply_byron_block(
        &mut self,
        block: &crate::tx::Block,
        _slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        // Decode each Tx.body (which is CBOR-encoded ByronTx) back into typed form.
        let decoded: Vec<ByronTx> = block
            .transactions
            .iter()
            .map(|tx| ByronTx::from_cbor_bytes(&tx.body))
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // Atomic: clone the multi-era UTxO, apply all txs, then commit.
        //
        // Use the pre-computed `Tx.id` (derived from the on-wire CBOR
        // bytes by `multi_era_block_to_block`) rather than re-deriving
        // from the decoded structure: Byron tx_ids are over the
        // annotated wire bytes, and re-encoding can produce a different
        // byte sequence (e.g. definite vs indefinite arrays) which
        // would yield a wrong tx_id and cause every spend of that
        // output to fail with `InputNotFound`.
        let mut staged = self.multi_era_utxo.clone();
        for (tx, byron_tx) in block.transactions.iter().zip(decoded.iter()) {
            staged.apply_byron_tx_with_id(tx.id.0, byron_tx)?;
        }
        self.multi_era_utxo = staged;
        Ok(())
    }
}
