//! Conway-era block application on `LedgerState`.
//!
//! Conway adds the governance pipeline (votes, proposals, treasury
//! withdrawals, constitutional updates), DRep delegation, and the
//! reference-input / spending-input disjointness rule (Babbage allowed
//! overlap). Phase-1 validation runs the largest set of cross-cutting
//! checks of any era.
//!
//! Reference:
//! `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/{Bbody,Ledger,Utxow,Utxo,Utxos,Gov,GovCert,Cert,Certs,Deleg,Pool,NewEpoch,Epoch,Tickf,Mempool,HardFork,Enact,Ratify}.hs`

use std::collections::HashSet;

use super::super::LedgerState;
use super::super::apply_certificates_and_withdrawals_with_future;
use super::super::phase1_validation::{
    collect_reference_script_hashes, sum_redeemer_ex_units_from_bytes, validate_alonzo_plus_tx,
    validate_auxiliary_data, validate_block_ex_units, validate_native_scripts_if_present,
    validate_no_extraneous_script_witnesses, validate_output_network_ids,
    validate_per_redeemer_ex_units_from_bytes, validate_required_script_witnesses,
    validate_tx_body_network_id, validate_withdrawal_network_ids, validate_witnesses_if_present,
};
use crate::eras::conway::ConwayTxBody;
use crate::eras::shelley::ShelleyTxIn;
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::{CborDecode, LedgerError};

use super::super::{
    apply_conway_votes, collect_conway_unregistered_drep_voters,
    conway_governance_state_after_certificates, conway_post_pv10, disjoint_ref_inputs_enforced,
    remove_conway_drep_votes, touch_drep_activity_for_certs, update_dormant_drep_expiries,
    validate_conway_current_treasury_value, validate_conway_proposals,
    validate_conway_vote_targets, validate_conway_voter_permissions, validate_conway_voters,
    validate_unelected_committee_voters, validate_withdrawals_delegated,
};

impl LedgerState {
    pub(in crate::state) fn apply_conway_block(
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
            ConwayTxBody,
            crate::eras::babbage::BabbageTxOutputRawSizes,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<bool>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ConwayTxBody::from_cbor_bytes(&tx.body)?;
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

        // Conway BBODY rule: block-level reference-script size limit.
        // Reference: `Cardano.Ledger.Conway.Rules.Bbody` — `BodyRefScriptsSizeTooBig`.
        //
        // At PV <= 10: sum of `txNonDistinctRefScriptsSize` per tx over the
        // pre-block UTxO (static).
        // At PV > 10 (`hardforkConwayRefactorTotalRefScriptsSize`): fold
        // through txs with a running UTxO that accumulates each tx's outputs
        // (valid tx → regular outputs, invalid tx → collateral return) before
        // measuring the next tx's ref-script size.  Spent inputs are NOT
        // removed.  The current tx is measured against the running UTxO
        // BEFORE its own outputs are added.
        // Reference: `Cardano.Ledger.Conway.Rules.Bbody` — `totalRefScriptSizeInBlock`.
        {
            let pv = self
                .protocol_params
                .as_ref()
                .and_then(|p| p.protocol_version);
            let use_running_utxo = conway_post_pv10(pv);
            let mut block_ref_total: usize = 0;
            if use_running_utxo {
                // PV > 10: fold with a running UTxO overlay that accumulates
                // newly produced outputs from preceding txs.
                let mut overlay: std::collections::HashMap<ShelleyTxIn, MultiEraTxOut> =
                    std::collections::HashMap::new();
                for (tx_id, _, body, _, _, _, is_valid) in &decoded {
                    // Measure ref-script size from ORIGINAL utxo + overlay
                    // (overlay entries take precedence conceptually but won't
                    // collide with existing entries since they use fresh TxIds).
                    let mut tx_ref_size: usize = 0;
                    for input in body
                        .inputs
                        .iter()
                        .chain(body.reference_inputs.as_deref().unwrap_or(&[]).iter())
                    {
                        // Check overlay first, then original UTxO.
                        let txout = overlay
                            .get(input)
                            .or_else(|| self.multi_era_utxo.get(input));
                        if let Some(out) = txout {
                            if let Some(sr) = out.script_ref() {
                                tx_ref_size = tx_ref_size.saturating_add(sr.0.binary_size());
                            }
                        }
                    }
                    block_ref_total = block_ref_total.saturating_add(tx_ref_size);
                    // Add this tx's outputs to overlay for the NEXT tx.
                    let tx_is_valid = is_valid.unwrap_or(true);
                    if tx_is_valid {
                        for (idx, out) in body.outputs.iter().enumerate() {
                            let txin = ShelleyTxIn {
                                transaction_id: tx_id.0,
                                index: idx as u16,
                            };
                            overlay.insert(txin, MultiEraTxOut::Babbage(out.clone()));
                        }
                    } else if let Some(collateral_return) = &body.collateral_return {
                        // Invalid tx: add collateral return output (upstream `collOuts`).
                        // Upstream `mkCollateralTxIn`: index = length(outputs).
                        let txin = ShelleyTxIn {
                            transaction_id: tx_id.0,
                            index: body.outputs.len() as u16,
                        };
                        overlay.insert(txin, MultiEraTxOut::Babbage(collateral_return.clone()));
                    }
                }
            } else {
                // PV <= 10: use pre-block UTxO (static) for all txs.
                for (_, _, body, _, _, _, _) in &decoded {
                    block_ref_total = block_ref_total.saturating_add(
                        self.multi_era_utxo
                            .total_ref_scripts_size(&body.inputs, body.reference_inputs.as_deref()),
                    );
                }
            }
            if block_ref_total > crate::utxo::MAX_REF_SCRIPT_SIZE_PER_BLOCK {
                return Err(LedgerError::BodyRefScriptsSizeTooBig {
                    actual: block_ref_total,
                    max_allowed: crate::utxo::MAX_REF_SCRIPT_SIZE_PER_BLOCK,
                });
            }
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
        let mut staged_governance_actions = self.governance_actions.clone();
        let mut staged_utxos_donation: u64 = 0;
        let mut staged_num_dormant = self.num_dormant_epochs;
        let drep_activity = self
            .protocol_params
            .as_ref()
            .and_then(|pp| pp.drep_activity)
            .unwrap_or(0);
        let current_treasury = self.accounting.treasury;
        let cert_ctx = self.certificate_validation_context();
        for (tx_id, tx_size, body, output_sizes, witness_bytes, aux_data, is_valid) in &decoded {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            // Conway UTXOW: validateScriptsWellFormed.
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
                if disjoint_ref_inputs_enforced(
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                ) {
                    MultiEraUtxo::validate_reference_input_disjointness(&body.inputs, ref_inputs)?;
                }
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
            if let Some(voting_procedures) = &body.voting_procedures {
                crate::witnesses::required_script_hashes_from_voting_procedures(
                    voting_procedures,
                    &mut required_scripts,
                );
            }
            if let Some(proposal_procedures) = &body.proposal_procedures {
                crate::witnesses::required_script_hashes_from_proposal_procedures(
                    proposal_procedures,
                    &mut required_scripts,
                );
            }
            crate::plutus_validation::validate_script_data_hash(
                body.script_data_hash,
                witness_bytes.as_deref(),
                self.protocol_params.as_ref(),
                true,
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
                let ref_scripts_size =
                    staged.total_ref_scripts_size(&body.inputs, body.reference_inputs.as_deref());
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
                    ref_scripts_size,
                    true,
                )?;
                // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                validate_per_redeemer_ex_units_from_bytes(witness_bytes.as_deref(), params)?;
            }
            // Network validation (Conway UTXO rule: WrongNetwork + WrongNetworkInTxBody)
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
            if let Some(voting_procedures) = &body.voting_procedures {
                crate::witnesses::required_vkey_hashes_from_voting_procedures(
                    voting_procedures,
                    &mut required,
                );
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // Native script validation (Conway)
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
            if let Some(voting_procedures) = &body.voting_procedures {
                crate::witnesses::required_script_hashes_from_voting_procedures(
                    voting_procedures,
                    &mut required_scripts,
                );
            }
            if let Some(proposal_procedures) = &body.proposal_procedures {
                crate::witnesses::required_script_hashes_from_proposal_procedures(
                    proposal_procedures,
                    &mut required_scripts,
                );
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
            let conway_blk_ref_scripts =
                collect_reference_script_hashes(&staged, body.reference_inputs.as_deref());
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                if conway_blk_ref_scripts.is_empty() {
                    None
                } else {
                    Some(&conway_blk_ref_scripts)
                },
            )?;
            // Unspendable UTxO check (Conway block — no datum on Plutus-locked input).
            // CIP-0069: collect PlutusV3 script hashes for V3 datum exemption.
            let conway_blk_v3_hashes = {
                let ws_bytes = witness_bytes.as_deref();
                let ws_decoded =
                    ws_bytes.map(crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes);
                let ws_ref = match &ws_decoded {
                    Some(Ok(w)) => Some(w),
                    _ => None,
                };
                crate::plutus_validation::collect_v3_script_hashes(
                    ws_ref,
                    Some(&staged),
                    body.reference_inputs.as_deref(),
                )
            };
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
                if conway_blk_v3_hashes.is_empty() {
                    None
                } else {
                    Some(&conway_blk_v3_hashes)
                },
            )?;
            // Supplemental datum check (Conway — includes reference inputs).
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
            // ExtraRedeemer check (Conway block — Phase-1 UTXOW).
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
                let sorted_voters: Vec<crate::eras::conway::Voter> = body
                    .voting_procedures
                    .as_ref()
                    .map(|vp| {
                        let mut vs: Vec<_> = vp.procedures.keys().cloned().collect();
                        vs.sort();
                        vs
                    })
                    .unwrap_or_default();
                let proposal_slice: Vec<crate::eras::conway::ProposalProcedure> =
                    body.proposal_procedures.as_deref().unwrap_or(&[]).to_vec();
                crate::plutus_validation::validate_no_extra_redeemers(
                    witness_bytes.as_deref(),
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &sorted_voters,
                    &proposal_slice,
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
                    &sorted_voters,
                    &proposal_slice,
                    body.reference_inputs.as_deref(),
                )?;
            }
            let run_phase2 = || -> Result<(), LedgerError> {
                // Plutus script validation (Conway)
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
                    let sorted_voters: Vec<crate::eras::conway::Voter> = body
                        .voting_procedures
                        .as_ref()
                        .map(|v| v.procedures.keys().cloned().collect())
                        .unwrap_or_default();
                    let proposal_slice = body.proposal_procedures.as_deref().unwrap_or(&[]);
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
                        current_treasury_value: body.current_treasury_value,
                        treasury_donation: body.treasury_donation,
                        voting_procedures: body.voting_procedures.clone(),
                        proposal_procedures: proposal_slice.to_vec(),
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
                        &sorted_voters,
                        proposal_slice,
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
                // Conway LEDGER rule: total reference script size limit
                // (upstream runs inside IsValid True branch).
                staged
                    .validate_tx_ref_scripts_size(&body.inputs, body.reference_inputs.as_deref())?;
                // Conway LEDGER rule: treasury value consistency
                // (upstream `validateTreasuryValue`, inside IsValid True branch).
                validate_conway_current_treasury_value(
                    body.current_treasury_value,
                    current_treasury,
                )?;
                // Conway LEDGER rule: withdrawal credentials must be delegated
                // to a DRep (post-bootstrap only, uses pre-CERTS state).
                validate_withdrawals_delegated(
                    body.withdrawals.as_ref(),
                    &staged_stake_credentials,
                    cert_ctx.bootstrap_phase,
                )?;
                let unregistered_drep_voters =
                    collect_conway_unregistered_drep_voters(body.certificates.as_deref());

                // Upstream `updateDormantDRepExpiries` — bump all DRep
                // expiries and reset dormant counter when tx has proposals.
                update_dormant_drep_expiries(
                    body.proposal_procedures
                        .as_ref()
                        .is_some_and(|p| !p.is_empty()),
                    &mut staged_drep_state,
                    &mut staged_num_dormant,
                    self.current_epoch,
                    drep_activity,
                );

                if body.voting_procedures.is_some()
                    || body.proposal_procedures.is_some()
                    || !unregistered_drep_voters.is_empty()
                {
                    let (
                        governance_pool_state,
                        governance_stake_credentials,
                        governance_committee_state,
                        governance_drep_state,
                    ) = conway_governance_state_after_certificates(
                        &staged_pool_state,
                        &staged_stake_credentials,
                        &staged_committee_state,
                        &staged_drep_state,
                        &staged_reward_accounts,
                        &staged_deposit_pot,
                        &staged_gen_delegs,
                        &staged_governance_actions,
                        &cert_ctx,
                        body.certificates.as_deref(),
                    )?;

                    let mut governance_actions_for_tx = staged_governance_actions.clone();

                    if let Some(voting_procedures) = &body.voting_procedures {
                        // Upstream: UnelectedCommitteeVoters check runs first
                        // (hardforkConwayDisallowUnelectedCommitteeFromVoting).
                        validate_unelected_committee_voters(
                            voting_procedures,
                            &governance_committee_state,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                        )?;
                        validate_conway_voters(
                            voting_procedures,
                            &governance_pool_state,
                            &governance_committee_state,
                            &governance_drep_state,
                        )?;
                    }

                    if let Some(proposal_procedures) = &body.proposal_procedures {
                        validate_conway_proposals(
                            *tx_id,
                            proposal_procedures,
                            self.current_epoch,
                            &mut governance_actions_for_tx,
                            &governance_stake_credentials,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.gov_action_deposit),
                            self.expected_network_id,
                            self.protocol_params.as_ref(),
                            &self.enact_state,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.gov_action_lifetime),
                        )?;
                    }

                    if let Some(voting_procedures) = &body.voting_procedures {
                        validate_conway_vote_targets(
                            voting_procedures,
                            &governance_actions_for_tx,
                        )?;
                        validate_conway_voter_permissions(
                            self.current_epoch,
                            voting_procedures,
                            &governance_actions_for_tx,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                        )?;
                    }

                    staged_governance_actions = governance_actions_for_tx;
                    if let Some(voting_procedures) = &body.voting_procedures {
                        apply_conway_votes(
                            voting_procedures,
                            &mut staged_governance_actions,
                            &mut staged_drep_state,
                            self.current_epoch,
                            staged_num_dormant,
                            cert_ctx.bootstrap_phase,
                        );
                    }
                    remove_conway_drep_votes(
                        &unregistered_drep_voters,
                        &mut staged_governance_actions,
                    );
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
                    None, // Conway: MIR certs rejected as UnsupportedCertificate
                )?;
                // Track DRep activity for registration and update certificates.
                touch_drep_activity_for_certs(
                    body.certificates.as_deref(),
                    &mut staged_drep_state,
                    self.current_epoch,
                    staged_num_dormant,
                    cert_ctx.bootstrap_phase,
                );
                // Conway UTXO rule: totalTxDeposits includes both certificate
                // deposits and proposal procedure deposits.
                // Reference: Cardano.Ledger.Conway.TxInfo — totalTxDeposits.
                let proposal_deposits: u64 = body
                    .proposal_procedures
                    .as_ref()
                    .map(|ps| ps.iter().map(|p| p.deposit).fold(0u64, u64::saturating_add))
                    .unwrap_or(0);
                // Track proposal deposits in the deposit pot (upstream oblProposal).
                staged_deposit_pot.add_proposal_deposit(proposal_deposits);
                let total_deposits = cert_adj.total_deposits.saturating_add(proposal_deposits);
                staged.apply_conway_tx_withdrawals(
                    tx_id.0,
                    body,
                    slot,
                    cert_adj.withdrawal_total,
                    total_deposits,
                    cert_adj.total_refunds,
                )?;
                // Accumulate treasury donation (Conway UTXOS rule).
                // Reference: Cardano.Ledger.Conway.Rules.Utxo — validateZeroDonation.
                if let Some(donation) = body.treasury_donation {
                    if donation == 0 {
                        return Err(LedgerError::ZeroDonation);
                    }
                    staged_utxos_donation = staged_utxos_donation.saturating_add(donation);
                }
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
        self.governance_actions = staged_governance_actions;
        self.utxos_donation = self.utxos_donation.saturating_add(staged_utxos_donation);
        self.num_dormant_epochs = staged_num_dormant;
        Ok(())
    }
}
