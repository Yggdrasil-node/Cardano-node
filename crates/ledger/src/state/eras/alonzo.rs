//! Alonzo-era block application on `LedgerState`.
//!
//! Alonzo introduces Plutus phase-2 evaluation: scripts execute in the
//! CEK machine with a fixed budget (ExUnits = CPU + memory). Block-level
//! and per-tx ExUnits limits, script-data hash binding, redeemer
//! coverage (no extras / no missing), datum-hash requirement on
//! script-locked outputs, and the `is_valid` bifurcation (validating
//! txs apply their state changes; invalid txs only consume collateral)
//! all enter at this era.
//!
//! Reference:
//! `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/{Bbody,Ledger,Utxow,Utxo,Utxos,Pool,Deleg,Cert}.hs`
//! `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Plutus/TxInfo.hs`
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Synthesis: per-rule Alonzo apply-path
//! across upstream `Cardano.Ledger.Alonzo.Rules.{Bbody,Ledger,Ledgers,Utxow,Utxo,Utxos,Deleg,Pool,Cert,Certs,NewEpoch,Epoch,Mir,PPUP}.hs`
//! (Plutus phase-2 evaluation, ExUnits, redeemer / datum binding,
//! is_valid bifurcation are the new Alonzo surface). Yggdrasil
//! aggregates per-rule logic in one file per-era; upstream splits
//! per-rule.

use std::collections::HashSet;

use super::super::LedgerState;
use super::super::accumulate_mir_from_certs;
use super::super::apply_certificates_and_withdrawals_with_future;
use super::super::phase1_validation::{
    sum_redeemer_ex_units_from_bytes, validate_alonzo_plus_tx, validate_auxiliary_data,
    validate_block_ex_units, validate_native_scripts_if_present,
    validate_no_extraneous_script_witnesses, validate_output_network_ids,
    validate_per_redeemer_ex_units_from_bytes, validate_required_script_witnesses,
    validate_tx_body_network_id, validate_withdrawal_network_ids, validate_witnesses_if_present,
};
use crate::eras::alonzo::AlonzoTxBody;
use crate::utxo::MultiEraTxOut;
use crate::{CborDecode, LedgerError};

impl LedgerState {
    pub(in crate::state) fn apply_alonzo_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
        evaluator: Option<&dyn crate::plutus_validation::PlutusEvaluator>,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(
            crate::types::TxId,
            usize,
            AlonzoTxBody,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<bool>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AlonzoTxBody::from_cbor_bytes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    tx.witnesses.clone(),
                    tx.auxiliary_data.clone(),
                    tx.is_valid,
                ))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // BBODY rule: block-level ExUnits limit.
        {
            let wb_refs: Vec<Option<&[u8]>> = decoded
                .iter()
                .map(|(_, _, _, wb, _, _)| wb.as_deref())
                .collect();
            validate_block_ex_units(self.protocol_params.as_ref(), &wb_refs)?;
        }

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let mut staged_gen_delegs = self.gen_delegs.clone();
        let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
        let cert_ctx = self.certificate_validation_context();
        let gen_delg_set = crate::witnesses::gen_delg_hash_set(&self.gen_delegs);
        for (tx_index, (tx_id, tx_size, body, witness_bytes, aux_data, is_valid)) in
            decoded.iter().enumerate()
        {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
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
            if let Some(mint) = &body.mint {
                crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
            }
            crate::plutus_validation::validate_script_data_hash(
                body.script_data_hash,
                witness_bytes.as_deref(),
                self.protocol_params.as_ref(),
                false,
                None,
                None,
                None,
                Some(&required_scripts),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                    .collect();
                validate_alonzo_plus_tx(
                    params,
                    &staged,
                    *tx_size,
                    body.fee,
                    &outputs,
                    None,
                    body.collateral.as_deref(),
                    total_eu.as_ref(),
                    None,
                    None,
                    None,
                    total_eu.is_some(),
                    0,
                    false,
                )?;
                // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                validate_per_redeemer_ex_units_from_bytes(witness_bytes.as_deref(), params)?;
            }
            // Network validation (Alonzo UTXO rule: WrongNetwork + WrongNetworkInTxBody)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
                validate_tx_body_network_id(expected_net, body.network_id)?;
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
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
            if let Some(signers) = &body.required_signers {
                for signer in signers {
                    required.insert(*signer);
                }
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
            // Native script validation (Alonzo)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
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
            if let Some(mint) = &body.mint {
                crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
            }
            let native_satisfied = validate_native_scripts_if_present(
                witness_bytes.as_deref(),
                &required_scripts,
                slot,
            )?;
            validate_required_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                &native_satisfied,
                &staged,
                None,
                None,
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                None, // Alonzo block: no reference inputs
            )?;
            // Unspendable UTxO check (Alonzo block — no datum on Plutus-locked input).
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
                None, // Alonzo: no PlutusV3
            )?;
            // Output-side datum hash check: Alonzo outputs to script
            // addresses must carry datum_hash.
            // Reference: Cardano.Ledger.Alonzo.Rules.Utxo —
            //   validateOutputMissingDatumHashForScriptOutputs.
            crate::plutus_validation::validate_outputs_missing_datum_hash_alonzo(&body.outputs)?;
            // Supplemental datum check (Alonzo — no reference inputs).
            {
                let tx_outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                    .collect();
                crate::plutus_validation::validate_supplemental_datums(
                    witness_bytes.as_deref(),
                    &staged,
                    &body.inputs,
                    &tx_outputs,
                    &[], // no reference inputs in Alonzo
                )?;
            }
            // ExtraRedeemer check (Alonzo block — Phase-1 UTXOW).
            // Upstream: hasExactSetOfRedeemers in alonzoUtxowTransition runs
            // unconditionally before UTXOS is_valid dispatching.
            {
                let mut sorted_inputs = body.inputs.clone();
                sorted_inputs.sort();
                let sorted_policies: Vec<[u8; 28]> = body
                    .mint
                    .as_ref()
                    .map(|m| m.keys().copied().collect())
                    .unwrap_or_default();
                let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                let sorted_rewards: Vec<Vec<u8>> = body
                    .withdrawals
                    .as_ref()
                    .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                    .unwrap_or_default();
                crate::plutus_validation::validate_no_extra_redeemers(
                    witness_bytes.as_deref(),
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &[],
                    &[],
                    None,
                )?;
                crate::plutus_validation::validate_no_missing_redeemers(
                    witness_bytes.as_deref(),
                    &required_scripts,
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &[],
                    &[],
                    None,
                )?;
            }
            // ── is_valid bifurcation (Phase-2 / collateral-only) ──
            let run_phase2 = || -> Result<(), LedgerError> {
                // Plutus script validation (Alonzo)
                {
                    let mut sorted_inputs = body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx_id.0,
                        fee: body.fee,
                        outputs: body
                            .outputs
                            .iter()
                            .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                            .collect(),
                        validity_start: body.validity_interval_start,
                        ttl: body.ttl,
                        required_signers: body.required_signers.clone().unwrap_or_default(),
                        mint: body.mint.clone().unwrap_or_default(),
                        withdrawals: body.withdrawals.clone().unwrap_or_default(),
                        protocol_version: self
                            .protocol_params
                            .as_ref()
                            .and_then(|p| p.protocol_version),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator,
                        witness_bytes.as_deref(),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        &tx_ctx,
                        self.protocol_params
                            .as_ref()
                            .and_then(|p| p.cost_models.as_ref()),
                    )
                }
            };
            if tx_is_valid {
                match run_phase2() {
                    Ok(()) => {}
                    Err(LedgerError::PlutusScriptFailed { hash, reason })
                        if evaluator.is_some() =>
                    {
                        return Err(LedgerError::ValidationTagMismatch {
                            claimed: true,
                            actual: false,
                            reason: super::super::phase2_failure_reason(&hash, &reason),
                        });
                    }
                    Err(e) => return Err(e),
                }
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
                    self.mir_validation_context(slot, true).as_ref(),
                )?;
                staged.apply_alonzo_tx_withdrawals(
                    tx_id.0,
                    body,
                    slot,
                    cert_adj.withdrawal_total,
                    cert_adj.total_deposits,
                    cert_adj.total_refunds,
                )?;
            } else {
                if evaluator.is_some() {
                    match run_phase2() {
                        Ok(()) => {
                            return Err(LedgerError::ValidationTagMismatch {
                                claimed: false,
                                actual: true,
                                reason: "phase-2 unexpectedly succeeded".to_string(),
                            });
                        }
                        Err(LedgerError::PlutusScriptFailed { .. }) => {}
                        Err(e) => return Err(e),
                    }
                }
                // is_valid = false: collateral-only transition.
                // Alonzo has no collateral_return, so only consume collateral inputs.
                crate::utxo::apply_collateral_only(
                    &mut staged,
                    tx_id.0,
                    body.collateral.as_deref(),
                    None,
                    body.outputs.len(),
                );
            }
        }
        self.multi_era_utxo = staged;
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
        // Skip is_valid=false transactions — upstream alonzoEvalScriptsTxInvalid
        // returns `pure pup` (no PPUP) and does not run DELEGS (no MIR).
        let ppup_ctx = self.ppup_slot_context(slot);
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data, is_valid) in &decoded {
            if is_valid.unwrap_or(true) {
                if let Some(ref update) = body.update {
                    self.validate_ppup_proposal(update, ppup_ctx.as_ref())?;
                    self.collect_pparam_proposals(update);
                }
                accumulate_mir_from_certs(
                    &mut self.instantaneous_rewards,
                    body.certificates.as_deref(),
                );
            }
        }
        Ok(())
    }
}
