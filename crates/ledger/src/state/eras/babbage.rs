//! Babbage-era block application on `LedgerState`.
//!
//! Babbage adds reference inputs, inline datums, reference scripts, and
//! collateral_return to Alonzo's foundation. Phase-1 validation grows
//! to include reference-input-aware required-witness collection,
//! script-well-formedness on both witness scripts and reference
//! scripts, and supplemental-datum coverage extended over reference
//! inputs.
//!
//! Reference:
//! `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/{Bbody,Ledger,Utxow,Utxo,Utxos}.hs`
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Synthesis: per-rule Babbage apply-path
//! across upstream `Cardano.Ledger.Babbage.Rules.{Bbody,Ledger,Ledgers,Utxow,Utxo,Utxos,Deleg,Pool,Cert,Certs,NewEpoch,Epoch,Mir,PPUP}.hs`
//! (reference inputs, inline datums, reference scripts, native-
//! asset value preservation are the new Babbage surface). Yggdrasil
//! aggregates per-rule logic in one file per-era; upstream splits
//! per-rule.

use std::collections::HashSet;

use super::super::LedgerState;
use super::super::accumulate_mir_from_certs;
use super::super::apply_certificates_and_withdrawals_with_future;
use super::super::phase1_validation::{
    collect_reference_script_hashes, sum_redeemer_ex_units_from_bytes, validate_alonzo_plus_tx,
    validate_auxiliary_data, validate_block_ex_units, validate_native_scripts_if_present,
    validate_no_extraneous_script_witnesses, validate_output_network_ids,
    validate_per_redeemer_ex_units_from_bytes, validate_required_script_witnesses,
    validate_tx_body_network_id, validate_withdrawal_network_ids, validate_witnesses_if_present,
};
use crate::eras::babbage::BabbageTxBody;
use crate::eras::shelley::ShelleyTxIn;
use crate::utxo::MultiEraTxOut;
use crate::{CborDecode, LedgerError};

impl LedgerState {
    pub(in crate::state) fn apply_babbage_block(
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
            BabbageTxBody,
            crate::eras::babbage::BabbageTxOutputRawSizes,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<bool>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = BabbageTxBody::from_cbor_bytes(&tx.body)?;
                let output_sizes =
                    crate::eras::babbage::extract_babbage_tx_output_raw_sizes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    output_sizes,
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
                .map(|(_, _, _, _, wb, _, _)| wb.as_deref())
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
        for (tx_id, tx_size, body, output_sizes, witness_bytes, aux_data, is_valid) in &decoded {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            // Babbage UTXOW: validateScriptsWellFormed.
            if let Some(eval) = evaluator {
                let protocol_version = self
                    .protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version);
                if let Some(wb) = witness_bytes.as_deref() {
                    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;
                    crate::witnesses::validate_script_witnesses_well_formed(
                        &ws,
                        eval,
                        protocol_version,
                    )?;
                }
                let produced_outputs = if tx_is_valid {
                    body.outputs.as_slice()
                } else {
                    &[]
                };
                crate::witnesses::validate_reference_scripts_well_formed(
                    produced_outputs,
                    body.collateral_return.as_ref(),
                    eval,
                    protocol_version,
                )?;
            }
            if let Some(ref_inputs) = &body.reference_inputs {
                staged.validate_reference_inputs(ref_inputs)?;
                // Babbage allows overlapping spending and reference inputs;
                // disjointness is enforced only in Conway.
            }
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
                Some(&staged),
                body.reference_inputs.as_deref(),
                Some(&body.inputs),
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
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                let coll_ret = body
                    .collateral_return
                    .as_ref()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()));
                validate_alonzo_plus_tx(
                    params,
                    &staged,
                    *tx_size,
                    body.fee,
                    &outputs,
                    Some(&output_sizes.outputs),
                    body.collateral.as_deref(),
                    total_eu.as_ref(),
                    coll_ret.as_ref(),
                    output_sizes.collateral_return,
                    body.total_collateral,
                    total_eu.is_some(),
                    0,
                    true,
                )?;
                // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                validate_per_redeemer_ex_units_from_bytes(witness_bytes.as_deref(), params)?;
            }
            // Network validation (Babbage UTXO rule: WrongNetwork + WrongNetworkInTxBody)
            if let Some(expected_net) = self.expected_network_id {
                let mut outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                // Upstream allSizedOutputsTxBodyF includes collateral_return.
                if let Some(cr) = &body.collateral_return {
                    outputs.push(MultiEraTxOut::Babbage(cr.clone()));
                }
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
            // Native script validation (Babbage)
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
                body.reference_inputs.as_deref(),
                Some(&body.inputs),
            )?;
            let babbage_blk_ref_scripts =
                collect_reference_script_hashes(&staged, body.reference_inputs.as_deref());
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                if babbage_blk_ref_scripts.is_empty() {
                    None
                } else {
                    Some(&babbage_blk_ref_scripts)
                },
            )?;
            // Unspendable UTxO check (Babbage block — no datum on Plutus-locked input).
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
                None, // Babbage: no PlutusV3
            )?;
            // Supplemental datum check (Babbage — includes reference inputs).
            {
                let mut tx_outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                if let Some(collateral_return) = &body.collateral_return {
                    tx_outputs.push(MultiEraTxOut::Babbage(collateral_return.clone()));
                }
                let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = body
                    .reference_inputs
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|txin| staged.get(txin).map(|txout| (txin.clone(), txout.clone())))
                    .collect();
                crate::plutus_validation::validate_supplemental_datums(
                    witness_bytes.as_deref(),
                    &staged,
                    &body.inputs,
                    &tx_outputs,
                    &ref_utxos,
                )?;
            }
            // ExtraRedeemer check (Babbage block — Phase-1 UTXOW).
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
                    body.reference_inputs.as_deref(),
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
                    body.reference_inputs.as_deref(),
                )?;
            }
            let run_phase2 = || -> Result<(), LedgerError> {
                // Plutus script validation (Babbage)
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
                            .map(|o| MultiEraTxOut::Babbage(o.clone()))
                            .collect(),
                        validity_start: body.validity_interval_start,
                        ttl: body.ttl,
                        required_signers: body.required_signers.clone().unwrap_or_default(),
                        mint: body.mint.clone().unwrap_or_default(),
                        withdrawals: body.withdrawals.clone().unwrap_or_default(),
                        reference_inputs: body.reference_inputs.clone().unwrap_or_default(),
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
                    self.stability_window,
                    self.mir_validation_context(slot, true).as_ref(),
                )?;
                staged.apply_babbage_tx_withdrawals(
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
                crate::utxo::apply_collateral_only(
                    &mut staged,
                    tx_id.0,
                    body.collateral.as_deref(),
                    body.collateral_return.as_ref(),
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
        for (_tx_id, _tx_size, body, _output_sizes, _witness_bytes, _aux_data, is_valid) in &decoded
        {
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
