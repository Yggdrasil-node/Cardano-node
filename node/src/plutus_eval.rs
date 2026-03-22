//! CEK-machine `PlutusEvaluator` implementation for the node.
//!
//! Bridges [`yggdrasil_ledger::plutus_validation::PlutusEvaluator`] to the
//! actual [`yggdrasil_plutus`] CEK machine.
//!
//! ## Argument application
//!
//! Cardano Plutus scripts are curried functions:
//! - Spending validator:   `datum -> redeemer -> context -> result`
//! - All other validators: `redeemer -> context -> result`
//!
//! For PlutusV1/V2 the result is discarded — any non-error outcome is
//! accepted. For PlutusV3 the result must be `Constant(Bool(true))`.
//!
//! ## ScriptContext (current limitation)
//!
//! A full `ScriptContext` / `TxInfo` construction requires access to the
//! full transaction body (inputs, outputs, fee, validity range, etc.), which
//! is not yet threaded through `PlutusScriptEval`. Until that milestone,
//! the context is approximated as a version-aware placeholder.
//! For PlutusV1/V2 this remains `Constr(0, [tx_info_placeholder, purpose_data])`.
//! For PlutusV3 it now follows the upstream three-field shape
//! `Constr(0, [tx_info_placeholder, redeemer, script_info])`.
//! The `TxInfo` payload is still a stub, but the purpose/script-info payloads
//! now track the upstream constructor families instead of a single local shape.
//!
//! Full ScriptContext construction is tracked as a future milestone in
//! `crates/ledger/src/AGENTS.md`.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core>
//! Reference: <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/PlutusScripts.hs>

use yggdrasil_ledger::{
    CborEncode,
    DCert,
    LedgerError,
    plutus::PlutusData,
    plutus_validation::{PlutusEvaluator, PlutusScriptEval, PlutusVersion, ScriptPurpose},
    StakeCredential,
};
use yggdrasil_plutus::{
    decode_script_bytes,
    types::{Constant, Term},
    CostModel, ExBudget, MachineError, Value,
};

// ---------------------------------------------------------------------------
// CekPlutusEvaluator
// ---------------------------------------------------------------------------

/// A [`PlutusEvaluator`] backed by the `yggdrasil-plutus` CEK machine.
///
/// Decodes each script from its on-chain Flat bytes, applies datum (if
/// spending), redeemer, and a placeholder ScriptContext, then evaluates
/// within the budget declared by the transaction.
#[derive(Clone, Debug, Default)]
pub struct CekPlutusEvaluator {
    /// Cost model to use. Defaults to `CostModel::default()`.
    pub cost_model: CostModel,
}

impl CekPlutusEvaluator {
    /// Create an evaluator with the default cost model.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an evaluator with a custom cost model.
    pub fn with_cost_model(cost_model: CostModel) -> Self {
        Self { cost_model }
    }
}

impl PlutusEvaluator for CekPlutusEvaluator {
    fn evaluate(&self, eval: &PlutusScriptEval) -> Result<(), LedgerError> {
        // 1. Decode the on-chain script bytes (Flat / CBOR-unwrap).
        let program = decode_script_bytes(&eval.script_bytes).map_err(|e| {
            LedgerError::PlutusScriptDecodeError {
                hash: eval.script_hash,
                reason: e.to_string(),
            }
        })?;

        // 2. Build Term::Constant wrappers for datum, redeemer, and context.
        let redeemer_term = data_term(eval.redeemer.clone());
        // Placeholder context: Constr(0, [tx_info_placeholder, purpose_data]).
        // This still does not encode a real TxInfo, but it preserves the
        // outer ScriptContext shape and the resolved purpose payload.
        let context_term = Term::Constant(Constant::Data(script_context_data(eval)));

        // 3. Apply arguments in the order specified by the Plutus script ABI.
        //    spending validator: script datum redeemer context
        //    all others:         script redeemer context
        let applied = match &eval.datum {
            Some(datum) => Term::Apply(
                Box::new(Term::Apply(
                    Box::new(Term::Apply(
                        Box::new(program.term),
                        Box::new(data_term(datum.clone())),
                    )),
                    Box::new(redeemer_term),
                )),
                Box::new(context_term),
            ),
            None => Term::Apply(
                Box::new(Term::Apply(
                    Box::new(program.term),
                    Box::new(redeemer_term),
                )),
                Box::new(context_term),
            ),
        };

        // 4. Build execution budget from the transaction's declared ExUnits.
        //    ExUnits.steps → cpu; ExUnits.mem → mem.
        let budget = ExBudget::new(
            eval.ex_units.steps as i64,
            eval.ex_units.mem as i64,
        );

        // 5. Evaluate the applied term.
        let (result, _logs) =
            yggdrasil_plutus::evaluate_term(applied, budget, self.cost_model.clone())
                .map_err(|e| map_machine_error(&eval.script_hash, e))?;

        // 6. PlutusV3 scripts must explicitly return Bool(true).
        //    PlutusV1/V2 accept any non-error result.
        if eval.version == PlutusVersion::V3 {
            match result {
                Value::Constant(Constant::Bool(true)) => Ok(()),
                other => Err(LedgerError::PlutusScriptFailed {
                    hash: eval.script_hash,
                    reason: format!(
                        "PlutusV3 script must return Bool(true), got: {:?}",
                        other
                    ),
                }),
            }
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Wrap a [`PlutusData`] value in a `Term::Constant`.
fn data_term(data: PlutusData) -> Term {
    Term::Constant(Constant::Data(data))
}

fn script_context_data(eval: &PlutusScriptEval) -> PlutusData {
    match eval.version {
        PlutusVersion::V1 | PlutusVersion::V2 => PlutusData::Constr(
            0,
            vec![tx_info_placeholder_data(), script_purpose_data_v1v2(&eval.purpose)],
        ),
        PlutusVersion::V3 => PlutusData::Constr(
            0,
            vec![
                tx_info_placeholder_data(),
                eval.redeemer.clone(),
                script_info_data_v3(&eval.purpose, eval.datum.as_ref()),
            ],
        ),
    }
}

fn tx_info_placeholder_data() -> PlutusData {
    PlutusData::Constr(0, vec![])
}

fn script_purpose_data_v1v2(purpose: &ScriptPurpose) -> PlutusData {
    match purpose {
        ScriptPurpose::Minting { policy_id } => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(policy_id.to_vec())])
        }
        ScriptPurpose::Spending { tx_id, index } => {
            PlutusData::Constr(1, vec![tx_out_ref_data(tx_id, *index)])
        }
        ScriptPurpose::Rewarding { reward_account } => PlutusData::Constr(
            2,
            vec![staking_credential_data(&reward_account.credential)],
        ),
        ScriptPurpose::Certifying {
            cert_index,
            certificate,
        } => certifying_purpose_data(*cert_index, certificate),
        ScriptPurpose::Voting { voter } => {
            PlutusData::Constr(4, vec![voter_data_v3(voter)])
        }
        ScriptPurpose::Proposing {
            proposal_index,
            proposal,
        } => PlutusData::Constr(
            5,
            vec![
                PlutusData::Integer(*proposal_index as i128),
                proposal_procedure_data_v3(proposal)
                    .unwrap_or_else(|| PlutusData::Integer(*proposal_index as i128)),
            ],
        ),
    }
}

fn script_info_data_v3(purpose: &ScriptPurpose, datum: Option<&PlutusData>) -> PlutusData {
    match purpose {
        ScriptPurpose::Minting { policy_id } => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(policy_id.to_vec())])
        }
        ScriptPurpose::Spending { tx_id, index } => PlutusData::Constr(
            1,
            vec![tx_out_ref_data(tx_id, *index), maybe_data(datum.cloned())],
        ),
        ScriptPurpose::Rewarding { reward_account } => PlutusData::Constr(
            2,
            vec![credential_data(&reward_account.credential)],
        ),
        ScriptPurpose::Certifying {
            cert_index,
            certificate,
        } => PlutusData::Constr(3, vec![
            PlutusData::Integer(*cert_index as i128),
            tx_cert_data_v3(certificate).unwrap_or_else(|| PlutusData::Integer(*cert_index as i128)),
        ]),
        ScriptPurpose::Voting { voter } => {
            PlutusData::Constr(4, vec![voter_data_v3(voter)])
        }
        ScriptPurpose::Proposing {
            proposal_index,
            proposal,
        } => PlutusData::Constr(
            5,
            vec![
                PlutusData::Integer(*proposal_index as i128),
                proposal_procedure_data_v3(proposal)
                    .unwrap_or_else(|| PlutusData::Integer(*proposal_index as i128)),
            ],
        ),
    }
}

fn maybe_data(data: Option<PlutusData>) -> PlutusData {
    match data {
        Some(data) => PlutusData::Constr(0, vec![data]),
        None => PlutusData::Constr(1, vec![]),
    }
}

fn certifying_purpose_data(cert_index: u64, certificate: &DCert) -> PlutusData {
    let certificate_data = legacy_dcert_data(certificate)
        .unwrap_or_else(|| PlutusData::Integer(cert_index as i128));
    PlutusData::Constr(3, vec![certificate_data])
}

fn tx_cert_data_v3(certificate: &DCert) -> Option<PlutusData> {
    match certificate {
        DCert::AccountRegistration(credential) => Some(PlutusData::Constr(
            0,
            vec![credential_data(credential), maybe_lovelace(None)],
        )),
        DCert::AccountUnregistration(credential) => Some(PlutusData::Constr(
            1,
            vec![credential_data(credential), maybe_lovelace(None)],
        )),
        DCert::DelegationToStakePool(credential, pool_key_hash) => Some(PlutusData::Constr(
            2,
            vec![credential_data(credential), delegatee_stake_data(pool_key_hash)],
        )),
        DCert::AccountRegistrationDeposit(credential, deposit) => Some(PlutusData::Constr(
            0,
            vec![credential_data(credential), maybe_lovelace(Some(*deposit))],
        )),
        DCert::AccountUnregistrationDeposit(credential, refund) => Some(PlutusData::Constr(
            1,
            vec![credential_data(credential), maybe_lovelace(Some(*refund))],
        )),
        DCert::DelegationToDrep(credential, drep) => Some(PlutusData::Constr(
            2,
            vec![credential_data(credential), delegatee_vote_data(drep)],
        )),
        DCert::DelegationToStakePoolAndDrep(credential, pool_key_hash, drep) => Some(
            PlutusData::Constr(
                2,
                vec![
                    credential_data(credential),
                    delegatee_stake_vote_data(pool_key_hash, drep),
                ],
            ),
        ),
        DCert::AccountRegistrationDelegationToStakePool(credential, pool_key_hash, deposit) => {
            Some(PlutusData::Constr(
                3,
                vec![
                    credential_data(credential),
                    delegatee_stake_data(pool_key_hash),
                    PlutusData::Integer(*deposit as i128),
                ],
            ))
        }
        DCert::AccountRegistrationDelegationToDrep(credential, drep, deposit) => Some(
            PlutusData::Constr(
                3,
                vec![
                    credential_data(credential),
                    delegatee_vote_data(drep),
                    PlutusData::Integer(*deposit as i128),
                ],
            ),
        ),
        DCert::AccountRegistrationDelegationToStakePoolAndDrep(
            credential,
            pool_key_hash,
            drep,
            deposit,
        ) => Some(PlutusData::Constr(
            3,
            vec![
                credential_data(credential),
                delegatee_stake_vote_data(pool_key_hash, drep),
                PlutusData::Integer(*deposit as i128),
            ],
        )),
        DCert::DrepRegistration(credential, deposit, _) => Some(PlutusData::Constr(
            4,
            vec![drep_credential_data(credential), PlutusData::Integer(*deposit as i128)],
        )),
        DCert::DrepUpdate(credential, _) => {
            Some(PlutusData::Constr(5, vec![drep_credential_data(credential)]))
        }
        DCert::DrepUnregistration(credential, refund) => Some(PlutusData::Constr(
            6,
            vec![drep_credential_data(credential), PlutusData::Integer(*refund as i128)],
        )),
        DCert::PoolRegistration(pool_params) => Some(PlutusData::Constr(
            7,
            vec![
                PlutusData::Bytes(pool_params.operator.to_vec()),
                PlutusData::Bytes(pool_params.vrf_keyhash.to_vec()),
            ],
        )),
        DCert::PoolRetirement(pool_key_hash, epoch) => Some(PlutusData::Constr(
            8,
            vec![
                PlutusData::Bytes(pool_key_hash.to_vec()),
                PlutusData::Integer(epoch.0 as i128),
            ],
        )),
        DCert::CommitteeAuthorization(cold, hot) => Some(PlutusData::Constr(
            9,
            vec![
                committee_credential_data(cold),
                committee_credential_data(hot),
            ],
        )),
        DCert::CommitteeResignation(cold, _) => Some(PlutusData::Constr(
            10,
            vec![committee_credential_data(cold)],
        )),
        DCert::GenesisDelegation(_, _, _) => None,
    }
}

fn maybe_lovelace(value: Option<u64>) -> PlutusData {
    match value {
        Some(value) => PlutusData::Constr(0, vec![PlutusData::Integer(value as i128)]),
        None => PlutusData::Constr(1, vec![]),
    }
}

fn delegatee_stake_data(pool_key_hash: &[u8; 28]) -> PlutusData {
    PlutusData::Constr(0, vec![PlutusData::Bytes(pool_key_hash.to_vec())])
}

fn delegatee_vote_data(drep: &yggdrasil_ledger::DRep) -> PlutusData {
    PlutusData::Constr(1, vec![drep_data(drep)])
}

fn delegatee_stake_vote_data(pool_key_hash: &[u8; 28], drep: &yggdrasil_ledger::DRep) -> PlutusData {
    PlutusData::Constr(
        2,
        vec![PlutusData::Bytes(pool_key_hash.to_vec()), drep_data(drep)],
    )
}

fn voter_data_v3(voter: &yggdrasil_ledger::Voter) -> PlutusData {
    match voter {
        yggdrasil_ledger::Voter::CommitteeKeyHash(hash) => {
            PlutusData::Constr(0, vec![committee_credential_data(&StakeCredential::AddrKeyHash(*hash))])
        }
        yggdrasil_ledger::Voter::CommitteeScript(hash) => {
            PlutusData::Constr(0, vec![committee_credential_data(&StakeCredential::ScriptHash(*hash))])
        }
        yggdrasil_ledger::Voter::DRepKeyHash(hash) => {
            PlutusData::Constr(1, vec![drep_credential_data(&StakeCredential::AddrKeyHash(*hash))])
        }
        yggdrasil_ledger::Voter::DRepScript(hash) => {
            PlutusData::Constr(1, vec![drep_credential_data(&StakeCredential::ScriptHash(*hash))])
        }
        yggdrasil_ledger::Voter::StakePool(hash) => {
            PlutusData::Constr(2, vec![PlutusData::Bytes(hash.to_vec())])
        }
    }
}

fn proposal_procedure_data_v3(
    proposal: &yggdrasil_ledger::ProposalProcedure,
) -> Option<PlutusData> {
    let reward_account = yggdrasil_ledger::RewardAccount::from_bytes(&proposal.reward_account)?;
    Some(PlutusData::Constr(
        0,
        vec![
            PlutusData::Integer(proposal.deposit as i128),
            credential_data(&reward_account.credential),
            gov_action_data_v3(&proposal.gov_action),
        ],
    ))
}

fn gov_action_data_v3(gov_action: &yggdrasil_ledger::GovAction) -> PlutusData {
    match gov_action {
        yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id,
            protocol_param_update,
            guardrails_script_hash,
        } => PlutusData::Constr(
            0,
            vec![
                maybe_gov_action_id_data(prev_action_id.as_ref()),
                // Serialize the typed update back to CBOR bytes as a
                // placeholder until full ChangedParameters → PlutusData
                // conversion is implemented.
                PlutusData::Bytes(protocol_param_update.to_cbor_bytes()),
                maybe_script_hash_data(*guardrails_script_hash),
            ],
        ),
        yggdrasil_ledger::GovAction::HardForkInitiation {
            prev_action_id,
            protocol_version,
        } => PlutusData::Constr(
            1,
            vec![
                maybe_gov_action_id_data(prev_action_id.as_ref()),
                protocol_version_data(*protocol_version),
            ],
        ),
        yggdrasil_ledger::GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash,
        } => PlutusData::Constr(
            2,
            vec![
                PlutusData::Map(
                    withdrawals
                        .iter()
                        .map(|(account, lovelace)| {
                            (
                                credential_data(&account.credential),
                                PlutusData::Integer(*lovelace as i128),
                            )
                        })
                        .collect(),
                ),
                maybe_script_hash_data(*guardrails_script_hash),
            ],
        ),
        yggdrasil_ledger::GovAction::NoConfidence { prev_action_id } => {
            PlutusData::Constr(3, vec![maybe_gov_action_id_data(prev_action_id.as_ref())])
        }
        yggdrasil_ledger::GovAction::UpdateCommittee {
            prev_action_id,
            members_to_remove,
            members_to_add,
            quorum,
        } => PlutusData::Constr(
            4,
            vec![
                maybe_gov_action_id_data(prev_action_id.as_ref()),
                PlutusData::List(
                    members_to_remove
                        .iter()
                        .map(committee_credential_data)
                        .collect(),
                ),
                PlutusData::Map(
                    members_to_add
                        .iter()
                        .map(|(credential, epoch)| {
                            (
                                committee_credential_data(credential),
                                PlutusData::Integer(*epoch as i128),
                            )
                        })
                        .collect(),
                ),
                unit_interval_data(quorum),
            ],
        ),
        yggdrasil_ledger::GovAction::NewConstitution {
            prev_action_id,
            constitution,
        } => PlutusData::Constr(
            5,
            vec![
                maybe_gov_action_id_data(prev_action_id.as_ref()),
                constitution_data_v3(constitution),
            ],
        ),
        yggdrasil_ledger::GovAction::InfoAction => PlutusData::Constr(6, vec![]),
    }
}

fn maybe_gov_action_id_data(gov_action_id: Option<&yggdrasil_ledger::GovActionId>) -> PlutusData {
    match gov_action_id {
        Some(gov_action_id) => PlutusData::Constr(0, vec![gov_action_id_data(gov_action_id)]),
        None => PlutusData::Constr(1, vec![]),
    }
}

fn gov_action_id_data(gov_action_id: &yggdrasil_ledger::GovActionId) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::Bytes(gov_action_id.transaction_id.to_vec()),
            PlutusData::Integer(gov_action_id.gov_action_index as i128),
        ],
    )
}

fn protocol_version_data(protocol_version: (u64, u64)) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::Integer(protocol_version.0 as i128),
            PlutusData::Integer(protocol_version.1 as i128),
        ],
    )
}

fn maybe_script_hash_data(script_hash: Option<[u8; 28]>) -> PlutusData {
    match script_hash {
        Some(hash) => PlutusData::Constr(0, vec![PlutusData::Bytes(hash.to_vec())]),
        None => PlutusData::Constr(1, vec![]),
    }
}

fn unit_interval_data(unit_interval: &yggdrasil_ledger::UnitInterval) -> PlutusData {
    PlutusData::List(vec![
        PlutusData::Integer(unit_interval.numerator as i128),
        PlutusData::Integer(unit_interval.denominator as i128),
    ])
}

fn constitution_data_v3(constitution: &yggdrasil_ledger::Constitution) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![maybe_script_hash_data(constitution.guardrails_script_hash)],
    )
}

fn drep_data(drep: &yggdrasil_ledger::DRep) -> PlutusData {
    match drep {
        yggdrasil_ledger::DRep::KeyHash(hash) => {
            PlutusData::Constr(0, vec![drep_credential_data(&StakeCredential::AddrKeyHash(*hash))])
        }
        yggdrasil_ledger::DRep::ScriptHash(hash) => {
            PlutusData::Constr(0, vec![drep_credential_data(&StakeCredential::ScriptHash(*hash))])
        }
        yggdrasil_ledger::DRep::AlwaysAbstain => PlutusData::Constr(1, vec![]),
        yggdrasil_ledger::DRep::AlwaysNoConfidence => PlutusData::Constr(2, vec![]),
    }
}

fn drep_credential_data(credential: &StakeCredential) -> PlutusData {
    PlutusData::Constr(0, vec![credential_data(credential)])
}

fn committee_credential_data(credential: &StakeCredential) -> PlutusData {
    PlutusData::Constr(0, vec![credential_data(credential)])
}

fn legacy_dcert_data(certificate: &DCert) -> Option<PlutusData> {
    match certificate {
        DCert::AccountRegistration(credential) => {
            Some(PlutusData::Constr(0, vec![staking_credential_data(credential)]))
        }
        DCert::AccountUnregistration(credential) => {
            Some(PlutusData::Constr(1, vec![staking_credential_data(credential)]))
        }
        DCert::DelegationToStakePool(credential, pool_key_hash) => Some(PlutusData::Constr(
            2,
            vec![
                staking_credential_data(credential),
                PlutusData::Bytes(pool_key_hash.to_vec()),
            ],
        )),
        DCert::PoolRegistration(pool_params) => Some(PlutusData::Constr(
            3,
            vec![
                PlutusData::Bytes(pool_params.operator.to_vec()),
                PlutusData::Bytes(pool_params.vrf_keyhash.to_vec()),
            ],
        )),
        DCert::PoolRetirement(pool_key_hash, epoch) => Some(PlutusData::Constr(
            4,
            vec![
                PlutusData::Bytes(pool_key_hash.to_vec()),
                PlutusData::Integer(epoch.0 as i128),
            ],
        )),
        DCert::GenesisDelegation(_, _, _) => Some(PlutusData::Constr(5, vec![])),
        DCert::AccountRegistrationDeposit(_, _)
        | DCert::AccountUnregistrationDeposit(_, _)
        | DCert::DelegationToDrep(_, _)
        | DCert::DelegationToStakePoolAndDrep(_, _, _)
        | DCert::AccountRegistrationDelegationToStakePool(_, _, _)
        | DCert::AccountRegistrationDelegationToDrep(_, _, _)
        | DCert::AccountRegistrationDelegationToStakePoolAndDrep(_, _, _, _)
        | DCert::CommitteeAuthorization(_, _)
        | DCert::CommitteeResignation(_, _)
        | DCert::DrepRegistration(_, _, _)
        | DCert::DrepUnregistration(_, _)
        | DCert::DrepUpdate(_, _) => None,
    }
}

fn tx_out_ref_data(tx_id: &[u8; 32], index: u64) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::Bytes(tx_id.to_vec()),
            PlutusData::Integer(index as i128),
        ],
    )
}

fn staking_credential_data(credential: &StakeCredential) -> PlutusData {
    PlutusData::Constr(0, vec![credential_data(credential)])
}

fn credential_data(credential: &StakeCredential) -> PlutusData {
    match credential {
        StakeCredential::AddrKeyHash(hash) => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(hash.to_vec())])
        }
        StakeCredential::ScriptHash(hash) => {
            PlutusData::Constr(1, vec![PlutusData::Bytes(hash.to_vec())])
        }
    }
}

/// Convert a [`MachineError`] into a [`LedgerError::PlutusScriptFailed`].
fn map_machine_error(hash: &[u8; 28], err: MachineError) -> LedgerError {
    LedgerError::PlutusScriptFailed {
        hash: *hash,
        reason: err.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::plutus_validation::{PlutusScriptEval, PlutusVersion, ScriptPurpose};
    use yggdrasil_ledger::{
        RewardAccount, StakeCredential,
        types::Anchor,
        eras::alonzo::ExUnits,
        plutus::PlutusData,
    };

    fn dummy_hash() -> [u8; 28] {
        [0xab; 28]
    }

    fn test_eval(
        version: PlutusVersion,
        purpose: ScriptPurpose,
        datum: Option<PlutusData>,
        redeemer: PlutusData,
    ) -> PlutusScriptEval {
        PlutusScriptEval {
            script_hash: dummy_hash(),
            version,
            script_bytes: vec![],
            purpose,
            datum,
            redeemer,
            ex_units: ExUnits {
                mem: 10_000_000,
                steps: 10_000_000,
            },
        }
    }

    fn mint_eval(script_bytes: Vec<u8>, version: PlutusVersion) -> PlutusScriptEval {
        PlutusScriptEval {
            script_hash: dummy_hash(),
            version,
            script_bytes,
            purpose: ScriptPurpose::Minting {
                policy_id: dummy_hash(),
            },
            datum: None,
            redeemer: PlutusData::Integer(42),
            ex_units: ExUnits {
                mem: 10_000_000,
                steps: 10_000_000,
            },
        }
    }

    #[test]
    fn decode_error_on_empty_bytes() {
        let evaluator = CekPlutusEvaluator::new();
        // Empty script bytes → decode failure.
        let eval = PlutusScriptEval {
            script_bytes: vec![],
            ..mint_eval(vec![], PlutusVersion::V1)
        };
        let result = evaluator.evaluate(&eval);
        assert!(
            result.is_err(),
            "empty script bytes must produce a decode error"
        );
        match result {
            Err(LedgerError::PlutusScriptDecodeError { .. }) => {}
            Err(other) => panic!("expected PlutusScriptDecodeError, got: {:?}", other),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn decode_error_on_garbage_bytes() {
        let evaluator = CekPlutusEvaluator::new();
        let eval = mint_eval(vec![0xff, 0xfe, 0xfd, 0xfc], PlutusVersion::V1);
        let result = evaluator.evaluate(&eval);
        assert!(
            result.is_err(),
            "garbage bytes must produce a decode or evaluation error"
        );
    }

    #[test]
    fn script_context_data_wraps_placeholder_tx_info_and_spending_purpose() {
        let data = script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Spending {
                tx_id: [0x11; 32],
                index: 7,
            },
            None,
            PlutusData::Integer(0),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    PlutusData::Constr(
                        1,
                        vec![PlutusData::Constr(
                            0,
                            vec![
                                PlutusData::Bytes(vec![0x11; 32]),
                                PlutusData::Integer(7),
                            ],
                        )],
                    ),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_encodes_rewarding_purpose_with_staking_credential_shape() {
        let reward_account = RewardAccount {
            network: 1,
            credential: StakeCredential::ScriptHash([0x22; 28]),
        };

        let data = script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Rewarding { reward_account },
            None,
            PlutusData::Integer(0),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    PlutusData::Constr(
                        2,
                        vec![PlutusData::Constr(
                            0,
                            vec![PlutusData::Constr(
                                1,
                                vec![PlutusData::Bytes(vec![0x22; 28])],
                            )],
                        )],
                    ),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_encodes_minting_with_upstream_constructor_index() {
        let data = script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Minting {
                policy_id: [0x33; 28],
            },
            None,
            PlutusData::Integer(0),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x33; 28])]),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_encodes_legacy_certifying_certificate_shape() {
        let data = script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Certifying {
                cert_index: 0,
                certificate: DCert::PoolRetirement([0x44; 28], yggdrasil_ledger::EpochNo(9)),
            },
            None,
            PlutusData::Integer(0),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    PlutusData::Constr(
                        3,
                        vec![PlutusData::Constr(
                            4,
                            vec![
                                PlutusData::Bytes(vec![0x44; 28]),
                                PlutusData::Integer(9),
                            ],
                        )],
                    ),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_falls_back_for_conway_only_certifying_certificate() {
        let data = script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Certifying {
                cert_index: 2,
                certificate: DCert::DrepRegistration(
                    StakeCredential::ScriptHash([0x99; 28]),
                    5,
                    Some(Anchor {
                        url: "https://example.invalid/drep".to_string(),
                        data_hash: [0xaa; 32],
                    }),
                ),
            },
            None,
            PlutusData::Integer(0),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    PlutusData::Constr(3, vec![PlutusData::Integer(2)]),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_uses_v3_three_field_shape_for_spending() {
        let datum = PlutusData::Integer(12);
        let redeemer = PlutusData::Integer(34);
        let data = script_context_data(&test_eval(
            PlutusVersion::V3,
            ScriptPurpose::Spending {
                tx_id: [0x55; 32],
                index: 4,
            },
            Some(datum.clone()),
            redeemer.clone(),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    redeemer,
                    PlutusData::Constr(
                        1,
                        vec![
                            PlutusData::Constr(
                                0,
                                vec![
                                    PlutusData::Bytes(vec![0x55; 32]),
                                    PlutusData::Integer(4),
                                ],
                            ),
                            PlutusData::Constr(0, vec![datum]),
                        ],
                    ),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_uses_v3_certifying_txcert_shape() {
        let data = script_context_data(&test_eval(
            PlutusVersion::V3,
            ScriptPurpose::Certifying {
                cert_index: 1,
                certificate: DCert::DelegationToDrep(
                    StakeCredential::AddrKeyHash([0x66; 28]),
                    yggdrasil_ledger::DRep::AlwaysAbstain,
                ),
            },
            None,
            PlutusData::Integer(77),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    PlutusData::Integer(77),
                    PlutusData::Constr(
                        3,
                        vec![
                            PlutusData::Integer(1),
                            PlutusData::Constr(
                                2,
                                vec![
                                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x66; 28])]),
                                    PlutusData::Constr(1, vec![PlutusData::Constr(1, vec![])]),
                                ],
                            ),
                        ],
                    ),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_uses_v3_voting_script_info_shape() {
        let data = script_context_data(&test_eval(
            PlutusVersion::V3,
            ScriptPurpose::Voting {
                voter: yggdrasil_ledger::Voter::DRepScript([0x77; 28]),
            },
            None,
            PlutusData::Integer(88),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    PlutusData::Integer(88),
                    PlutusData::Constr(
                        4,
                        vec![PlutusData::Constr(
                            1,
                            vec![PlutusData::Constr(
                                0,
                                vec![PlutusData::Constr(
                                    1,
                                    vec![PlutusData::Bytes(vec![0x77; 28])],
                                )],
                            )],
                        )],
                    ),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_uses_v3_proposing_script_info_shape() {
        let proposal = yggdrasil_ledger::ProposalProcedure {
            deposit: 9,
            reward_account: yggdrasil_ledger::RewardAccount {
                network: 1,
                credential: StakeCredential::ScriptHash([0x99; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposing".to_string(),
                data_hash: [0xAA; 32],
            },
        };
        let data = script_context_data(&test_eval(
            PlutusVersion::V3,
            ScriptPurpose::Proposing {
                proposal_index: 2,
                proposal,
            },
            None,
            PlutusData::Integer(101),
        ));

        assert_eq!(
            data,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![]),
                    PlutusData::Integer(101),
                    PlutusData::Constr(
                        5,
                        vec![
                            PlutusData::Integer(2),
                            PlutusData::Constr(
                                0,
                                vec![
                                    PlutusData::Integer(9),
                                    PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x99; 28])]),
                                    PlutusData::Constr(6, vec![]),
                                ],
                            ),
                        ],
                    ),
                ],
            )
        );
    }
}
