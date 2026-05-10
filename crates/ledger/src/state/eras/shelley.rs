//! Shelley-era block application on `LedgerState`.
//!
//! Mirrors upstream `Cardano.Ledger.Shelley.Rules.Bbody` /
//! `Cardano.Ledger.Shelley.Rules.Ledger` /
//! `Cardano.Ledger.Shelley.Rules.Utxow`: Shelley advances the legacy
//! `ShelleyUtxo` (authoritative for Shelley blocks), processes
//! certificates, withdrawals, witness validation (vkey, native multi-sig,
//! script), MIR genesis quorum, network-id checks, and PPUP proposal
//! collection. There is no Plutus, no reference inputs, and no
//! governance actions at Shelley.
//!
//! Reference:
//! `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/{Bbody,Ledger,Utxow,Utxo,Pool,Deleg,Cert}.hs`
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Aggregates the Shelley apply-path
//! across upstream `Cardano.Ledger.Shelley.Rules.Bbody`,
//! `Rules.Ledger`, `Rules.Utxow`, `Rules.Utxo`, `Rules.Deleg`,
//! `Rules.Pool`, `Rules.PoolReap`, `Rules.NewEpoch`, `Rules.Epoch`,
//! `Rules.Mir`, `Rules.PPUP`, `Rules.Ratify`, plus the witness +
//! MIR-genesis-quorum + network-id + PPUP proposal collection
//! checks. Yggdrasil's per-era apply file is a synthesis that
//! orchestrates these per-rule helpers in one place; upstream
//! splits per-rule into separate Rules/*.hs files.

use std::collections::HashSet;

use super::super::LedgerState;
use super::super::accumulate_mir_from_certs;
use super::super::apply_certificates_and_withdrawals_with_future;
use super::super::phase1_validation::{
    validate_auxiliary_data, validate_native_scripts_if_present,
    validate_no_extraneous_script_witnesses, validate_output_network_ids, validate_pre_alonzo_tx,
    validate_required_script_witnesses, validate_withdrawal_network_ids,
    validate_witnesses_if_present,
};
use crate::eras::shelley::ShelleyTxBody;
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::{CborDecode, LedgerError};

impl LedgerState {
    /// Apply a Shelley block.
    ///
    /// Validates phase-1 invariants per tx (size, fee, min-UTxO, witness
    /// coverage, network IDs, native scripts, MIR genesis quorum), then
    /// applies certificates, withdrawals, and the UTxO transition
    /// atomically. PPUP proposal collection and MIR certificate
    /// accumulation happen in a second pass after the UTxO commit.
    pub(in crate::state) fn apply_shelley_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(
            crate::types::TxId,
            usize,
            ShelleyTxBody,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ShelleyTxBody::from_cbor_bytes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    tx.witnesses.clone(),
                    tx.auxiliary_data.clone(),
                ))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // Atomic: clone the Shelley UTxO, apply all txs, then commit.
        // The legacy shelley_utxo is the authoritative source for Shelley
        // blocks (preserves backward compatibility with tests that seed
        // via utxo_mut()).
        let mut staged = self.shelley_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let mut staged_gen_delegs = self.gen_delegs.clone();
        let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
        let cert_ctx = self.certificate_validation_context();
        // Pre-compute genesis delegate key hash set for MIR quorum validation
        // (uses pre-block gen_delegs per upstream UTXOW rule).
        let gen_delg_set = crate::witnesses::gen_delg_hash_set(&self.gen_delegs);
        for (tx_index, (tx_id, tx_size, body, witness_bytes, aux_data)) in
            decoded.iter().enumerate()
        {
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
            }
            // Network validation (Shelley UTXO rule: WrongNetwork / WrongNetworkWithdrawal)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
            }
            // Witness validation: compute required VKey hashes and check
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_shelley(
                &body.inputs,
                &staged,
                &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            // Upstream propWits: proposer genesis key hashes.
            if let Some(update) = &body.update {
                crate::witnesses::required_vkey_hashes_from_ppup(
                    update,
                    &self.gen_delegs,
                    &mut required,
                );
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // MIR genesis quorum check (validateMIRInsufficientGenesisSigs).
            crate::witnesses::validate_mir_genesis_quorum_if_present(
                body.certificates.as_deref(),
                &gen_delg_set,
                self.genesis_update_quorum,
                witness_bytes.as_deref(),
            )?;
            // Native (MultiSig) script validation (Shelley UTXOW —
            // validateFailedNativeScripts / validateMissingScripts /
            // extraneousScriptWitnessesUTXOW).
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_shelley(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            let native_satisfied = validate_native_scripts_if_present(
                witness_bytes.as_deref(),
                &required_scripts,
                slot,
            )?;
            // Shelley has no reference inputs and no Plutus; an empty
            // MultiEraUtxo is sufficient for required-witness collection.
            let empty_utxo = MultiEraUtxo::new();
            validate_required_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                &native_satisfied,
                &empty_utxo,
                None,
                None,
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                None, // Shelley: no reference inputs
            )?;
            let cert_adj = apply_certificates_and_withdrawals_with_future(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &mut staged_future_gen_delegs,
                &self.governance_actions,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
                slot,
                tx_index as u64,
                self.stability_window,
                self.mir_validation_context(slot, false).as_ref(),
            )?;
            staged.apply_tx_with_withdrawals(
                tx_id.0,
                body,
                slot,
                cert_adj.withdrawal_total,
                cert_adj.total_deposits,
                cert_adj.total_refunds,
            )?;
        }
        self.shelley_utxo = staged;
        self.multi_era_utxo = MultiEraUtxo::from_shelley_utxo(&self.shelley_utxo);
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        self.future_gen_delegs = staged_future_gen_delegs;
        // Collect protocol parameter update proposals (PPUP rule) and
        // accumulate MIR certificates (Shelley through Babbage only).
        let ppup_ctx = self.ppup_slot_context(slot);
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data) in &decoded {
            if let Some(ref update) = body.update {
                self.validate_ppup_proposal(update, ppup_ctx.as_ref())?;
                self.collect_pparam_proposals(update);
            }
            accumulate_mir_from_certs(
                &mut self.instantaneous_rewards,
                body.certificates.as_deref(),
            );
        }
        Ok(())
    }
}
