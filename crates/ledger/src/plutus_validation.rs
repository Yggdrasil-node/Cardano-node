//! Plutus Phase-2 script validation bridge.
//!
//! This module defines the [`PlutusEvaluator`] trait that higher layers
//! (e.g. the node crate) implement using the actual CEK machine, and
//! provides script resolution and orchestration helpers that map redeemers
//! to their corresponding scripts and invoke the evaluator.
//!
//! # Architecture
//!
//! The ledger crate cannot depend on `yggdrasil-plutus` (which depends on
//! the ledger crate for `PlutusData`) so the evaluation is behind a trait.
//! During block application, `validate_plutus_scripts()` resolves which
//! scripts need evaluation, collects their datums and redeemers, then
//! delegates to the injected evaluator.
//!
//! Reference: `Cardano.Ledger.Alonzo.PlutusScriptApi`.

use std::collections::HashMap;

use crate::cbor::CborDecode;
use crate::eras::conway::{ProposalProcedure, Voter};
use crate::error::LedgerError;
use crate::eras::alonzo::{ExUnits, Redeemer};
use crate::eras::babbage::DatumOption;
use crate::plutus::PlutusData;
use crate::types::{Address, DCert, RewardAccount, StakeCredential};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};

// ---------------------------------------------------------------------------
// Plutus language version
// ---------------------------------------------------------------------------

/// Plutus script language version.
///
/// Each version corresponds to a CDDL language tag and a distinct set of
/// available builtins and ScriptContext shapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PlutusVersion {
    /// Plutus V1 (Alonzo, language tag 1).
    V1,
    /// Plutus V2 (Babbage, language tag 2).
    V2,
    /// Plutus V3 (Conway, language tag 3).
    V3,
}

impl PlutusVersion {
    /// Language tag byte used when computing the script hash.
    pub fn language_tag(self) -> u8 {
        match self {
            Self::V1 => 0x01,
            Self::V2 => 0x02,
            Self::V3 => 0x03,
        }
    }
}

// ---------------------------------------------------------------------------
// Script purpose
// ---------------------------------------------------------------------------

/// The purpose for which a Plutus script is being evaluated.
///
/// Each purpose corresponds to a redeemer tag (CDDL `redeemer_tag`) and
/// determines how the script receives its arguments.
///
/// Reference: `Cardano.Ledger.Alonzo.Tx` — `ScriptPurpose`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScriptPurpose {
    /// Spending a UTxO input (redeemer tag 0).
    Spending { tx_id: [u8; 32], index: u64 },
    /// Minting under a policy (redeemer tag 1).
    Minting { policy_id: [u8; 28] },
    /// Certifying a delegation action (redeemer tag 2).
    Certifying { cert_index: u64, certificate: DCert },
    /// Withdrawing from a reward account (redeemer tag 3).
    Rewarding { reward_account: RewardAccount },
    /// Casting governance votes as a Conway voter (redeemer tag 4).
    Voting { voter: Voter },
    /// Submitting a governance proposal (redeemer tag 5).
    Proposing { proposal_index: u64, proposal: ProposalProcedure },
}

// ---------------------------------------------------------------------------
// Evaluation target
// ---------------------------------------------------------------------------

/// All information needed to evaluate a single Plutus script.
#[derive(Clone, Debug)]
pub struct PlutusScriptEval {
    /// The script hash identifying this script.
    pub script_hash: [u8; 28],
    /// Script language version.
    pub version: PlutusVersion,
    /// Raw script bytes (Flat-encoded, possibly CBOR-wrapped).
    pub script_bytes: Vec<u8>,
    /// Purpose that triggered this evaluation.
    pub purpose: ScriptPurpose,
    /// Datum (required for spending validators, `None` for minting/cert/reward).
    pub datum: Option<PlutusData>,
    /// Redeemer data.
    pub redeemer: PlutusData,
    /// Execution budget allocated by the transaction for this script.
    pub ex_units: ExUnits,
}

// ---------------------------------------------------------------------------
// PlutusEvaluator trait
// ---------------------------------------------------------------------------

/// Trait for Plutus script evaluation, implemented by higher layers.
///
/// The ledger crate defines what needs evaluating; the implementor (typically
/// in the `node` crate) calls the actual CEK machine.
pub trait PlutusEvaluator {
    /// Evaluate a single Plutus script.
    ///
    /// The implementor should:
    /// 1. Decode `eval.script_bytes` (Flat decode / CBOR unwrap).
    /// 2. Apply `eval.datum` (if spending), `eval.redeemer`, and a
    ///    `ScriptContext` as arguments to the decoded program.
    /// 3. Evaluate within `eval.ex_units` budget.
    /// 4. Return `Ok(())` on success, or a `LedgerError` on failure.
    fn evaluate(&self, eval: &PlutusScriptEval) -> Result<(), LedgerError>;
}

// ---------------------------------------------------------------------------
// Script hashing
// ---------------------------------------------------------------------------

/// Compute the Blake2b-224 hash of a Plutus script.
///
/// The hash is `Blake2b-224(language_tag || script_bytes)` where
/// `language_tag` is the single-byte tag for the Plutus version.
pub fn plutus_script_hash(version: PlutusVersion, script_bytes: &[u8]) -> [u8; 28] {
    let mut buf = Vec::with_capacity(1 + script_bytes.len());
    buf.push(version.language_tag());
    buf.extend_from_slice(script_bytes);
    yggdrasil_crypto::blake2b::hash_bytes_224(&buf).0
}

// ---------------------------------------------------------------------------
// Script collection from witness set
// ---------------------------------------------------------------------------

/// Collects all Plutus scripts from a witness set into a hash → (version, bytes) map.
pub fn collect_plutus_scripts(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
) -> HashMap<[u8; 28], (PlutusVersion, Vec<u8>)> {
    let mut scripts = HashMap::new();
    for s in &ws.plutus_v1_scripts {
        let hash = plutus_script_hash(PlutusVersion::V1, s);
        scripts.insert(hash, (PlutusVersion::V1, s.clone()));
    }
    for s in &ws.plutus_v2_scripts {
        let hash = plutus_script_hash(PlutusVersion::V2, s);
        scripts.insert(hash, (PlutusVersion::V2, s.clone()));
    }
    for s in &ws.plutus_v3_scripts {
        let hash = plutus_script_hash(PlutusVersion::V3, s);
        scripts.insert(hash, (PlutusVersion::V3, s.clone()));
    }
    scripts
}

/// Builds a datum lookup map from the witness set's `plutus_data` list.
///
/// Keys are Blake2b-256 hashes of the CBOR-encoded datum; values are the
/// typed `PlutusData`.
pub fn collect_datum_map(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
) -> HashMap<[u8; 32], PlutusData> {
    use crate::cbor::CborEncode;
    let mut map = HashMap::new();
    for datum in &ws.plutus_data {
        let cbor = datum.to_cbor_bytes();
        let hash = yggdrasil_crypto::blake2b::hash_bytes_256(&cbor).0;
        map.insert(hash, datum.clone());
    }
    map
}

// ---------------------------------------------------------------------------
// Redeemer → purpose resolution
// ---------------------------------------------------------------------------

/// Resolves a redeemer tag + index to a concrete `ScriptPurpose`.
///
/// For spending (tag 0), the index refers to the sorted input list.
/// For minting (tag 1), the index refers to the sorted list of minted
/// policy IDs. For certifying (tag 2), it indexes into the certificate
/// list. For rewarding (tag 3), it indexes into the sorted withdrawals.
/// For voting (tag 4), it indexes into the sorted voter list; for proposing
/// (tag 5), it indexes into the proposal procedure list.
pub fn resolve_script_purpose(
    redeemer: &Redeemer,
    sorted_inputs: &[crate::eras::shelley::ShelleyTxIn],
    sorted_policy_ids: &[[u8; 28]],
    certificates: &[crate::types::DCert],
    sorted_reward_accounts: &[Vec<u8>],
    sorted_voters: &[Voter],
    proposal_procedures: &[ProposalProcedure],
) -> Result<ScriptPurpose, LedgerError> {
    match redeemer.tag {
        0 => {
            // Spending: index into sorted inputs
            let input = sorted_inputs.get(redeemer.index as usize).ok_or_else(|| {
                LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!("spend index {} out of range ({})", redeemer.index, sorted_inputs.len()),
                }
            })?;
            Ok(ScriptPurpose::Spending {
                tx_id: input.transaction_id,
                index: input.index as u64,
            })
        }
        1 => {
            // Minting: index into sorted policy IDs
            let policy = sorted_policy_ids.get(redeemer.index as usize).ok_or_else(|| {
                LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!("mint index {} out of range ({})", redeemer.index, sorted_policy_ids.len()),
                }
            })?;
            Ok(ScriptPurpose::Minting { policy_id: *policy })
        }
        2 => {
            // Certifying: index into certificates
            let certificate = certificates.get(redeemer.index as usize).ok_or_else(|| {
                LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!("cert index {} out of range ({})", redeemer.index, certificates.len()),
                }
            })?;
            Ok(ScriptPurpose::Certifying {
                cert_index: redeemer.index,
                certificate: certificate.clone(),
            })
        }
        3 => {
            // Rewarding: index into sorted reward accounts
            let acct = sorted_reward_accounts.get(redeemer.index as usize).ok_or_else(|| {
                LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!("reward index {} out of range ({})", redeemer.index, sorted_reward_accounts.len()),
                }
            })?;
            let reward_account = RewardAccount::from_bytes(acct).ok_or_else(|| {
                LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!("reward account at index {} is not a valid reward address", redeemer.index),
                }
            })?;
            Ok(ScriptPurpose::Rewarding { reward_account })
        }
        4 => {
            let voter = sorted_voters.get(redeemer.index as usize).ok_or_else(|| {
                LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!("voting index {} out of range ({})", redeemer.index, sorted_voters.len()),
                }
            })?;
            Ok(ScriptPurpose::Voting {
                voter: voter.clone(),
            })
        }
        5 => {
            let proposal = proposal_procedures
                .get(redeemer.index as usize)
                .ok_or_else(|| LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!(
                        "proposal index {} out of range ({})",
                        redeemer.index,
                        proposal_procedures.len()
                    ),
                })?;
            Ok(ScriptPurpose::Proposing {
                proposal_index: redeemer.index,
                proposal: proposal.clone(),
            })
        }
        _ => Err(LedgerError::MissingRedeemer {
            hash: [0; 28],
            purpose: format!("unknown redeemer tag {}", redeemer.tag),
        }),
    }
}

// ---------------------------------------------------------------------------
// Orchestrated Plutus validation
// ---------------------------------------------------------------------------

/// Validates all Plutus scripts referenced by a transaction.
///
/// This is the main entry point called from per-era `apply_block()` functions.
/// When `evaluator` is `None`, Plutus scripts are silently skipped (allowing
/// sync without a CEK machine configured). When required scripts are not
/// found in the witness set, an error is returned regardless of the
/// evaluator.
///
/// `required_scripts` is the set of script hashes that need either native
/// or Plutus satisfaction. Scripts already satisfied by native evaluation
/// should be removed before calling this function.
pub fn validate_plutus_scripts(
    evaluator: Option<&dyn PlutusEvaluator>,
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &std::collections::HashSet<[u8; 28]>,
    spending_utxo: &MultiEraUtxo,
    sorted_inputs: &[crate::eras::shelley::ShelleyTxIn],
    sorted_policy_ids: &[[u8; 28]],
    certificates: &[crate::types::DCert],
    sorted_reward_accounts: &[Vec<u8>],
    sorted_voters: &[Voter],
    proposal_procedures: &[ProposalProcedure],
) -> Result<(), LedgerError> {
    // If no required scripts, nothing to do.
    if required_script_hashes.is_empty() {
        return Ok(());
    }

    let wb = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()), // soft-skip like witness validation
    };

    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;

    // Collect available Plutus scripts and datum map.
    let plutus_scripts = collect_plutus_scripts(&ws);
    let datum_map = collect_datum_map(&ws);

    // Determine which required script hashes need Plutus evaluation
    // (those that are in the Plutus scripts collection).
    let plutus_required: Vec<[u8; 28]> = required_script_hashes
        .iter()
        .filter(|h| plutus_scripts.contains_key(h.as_slice()))
        .copied()
        .collect();

    if plutus_required.is_empty() {
        return Ok(());
    }

    // If no evaluator is configured, skip Plutus validation.
    let evaluator = match evaluator {
        Some(e) => e,
        None => return Ok(()),
    };

    // For each redeemer, resolve its purpose, find its script, find datum,
    // and build an evaluation target.
    for redeemer in &ws.redeemers {
        let purpose = resolve_script_purpose(
            redeemer,
            sorted_inputs,
            sorted_policy_ids,
            certificates,
            sorted_reward_accounts,
            sorted_voters,
            proposal_procedures,
        )?;

        // Determine which script hash this redeemer targets.
        let target_hash = match &purpose {
            ScriptPurpose::Spending { tx_id, index } => {
                let txin = crate::eras::shelley::ShelleyTxIn {
                    transaction_id: *tx_id,
                    index: *index as u16,
                };
                spending_utxo
                    .get(&txin)
                    .and_then(spending_script_hash_from_txout)
            }
            ScriptPurpose::Minting { policy_id } => Some(*policy_id),
            ScriptPurpose::Certifying { certificate, .. } => {
                certifying_script_hash_from_cert(certificate)
            }
            ScriptPurpose::Rewarding { reward_account } => {
                credential_script_hash(&reward_account.credential)
            }
            ScriptPurpose::Voting { voter } => voting_voter_script_hash(voter),
            ScriptPurpose::Proposing { proposal, .. } => proposal_script_hash_from_proposal(proposal),
        };

        // If we can identify the target script, evaluate it.
        if let Some(hash) = target_hash {
            if let Some((version, script_bytes)) = plutus_scripts.get(&hash) {
                let datum = match &purpose {
                    ScriptPurpose::Spending { tx_id, index } => {
                        let txin = crate::eras::shelley::ShelleyTxIn {
                            transaction_id: *tx_id,
                            index: *index as u16,
                        };
                        let txout = spending_utxo
                            .get(&txin)
                            .ok_or(LedgerError::InputNotInUtxo)?;
                        Some(resolve_spending_datum(txout, &datum_map, *tx_id, *index)?)
                    }
                    _ => None,
                };

                let eval_target = PlutusScriptEval {
                    script_hash: hash,
                    version: *version,
                    script_bytes: script_bytes.clone(),
                    purpose,
                    datum,
                    redeemer: redeemer.data.clone(),
                    ex_units: redeemer.ex_units,
                };

                evaluator.evaluate(&eval_target)?;
            }
        }
    }

    Ok(())
}

fn spending_script_hash_from_txout(txout: &MultiEraTxOut) -> Option<[u8; 28]> {
    let address = Address::from_bytes(txout.address())?;
    match address.payment_credential() {
        Some(StakeCredential::ScriptHash(hash)) => Some(*hash),
        _ => None,
    }
}

fn certifying_script_hash_from_cert(cert: &DCert) -> Option<[u8; 28]> {
    use crate::types::DRep;

    match cert {
        DCert::AccountRegistration(cred)
        | DCert::AccountUnregistration(cred)
        | DCert::AccountRegistrationDeposit(cred, _)
        | DCert::AccountUnregistrationDeposit(cred, _)
        | DCert::DelegationToStakePool(cred, _)
        | DCert::AccountRegistrationDelegationToStakePool(cred, _, _)
        | DCert::CommitteeAuthorization(cred, _)
        | DCert::CommitteeResignation(cred, _)
        | DCert::DrepRegistration(cred, _, _)
        | DCert::DrepUnregistration(cred, _)
        | DCert::DrepUpdate(cred, _) => credential_script_hash(cred),
        DCert::DelegationToDrep(cred, drep)
        | DCert::DelegationToStakePoolAndDrep(cred, _, drep)
        | DCert::AccountRegistrationDelegationToDrep(cred, drep, _)
        | DCert::AccountRegistrationDelegationToStakePoolAndDrep(cred, _, drep, _) => {
            credential_script_hash(cred).or_else(|| match drep {
                DRep::ScriptHash(hash) => Some(*hash),
                _ => None,
            })
        }
        DCert::PoolRegistration(_) | DCert::PoolRetirement(_, _) | DCert::GenesisDelegation(_, _, _) => None,
    }
}

fn credential_script_hash(credential: &StakeCredential) -> Option<[u8; 28]> {
    match credential {
        StakeCredential::ScriptHash(hash) => Some(*hash),
        StakeCredential::AddrKeyHash(_) => None,
    }
}

fn voting_voter_script_hash(voter: &Voter) -> Option<[u8; 28]> {
    match voter {
        Voter::CommitteeScript(hash) | Voter::DRepScript(hash) => Some(*hash),
        Voter::CommitteeKeyHash(_) | Voter::DRepKeyHash(_) | Voter::StakePool(_) => None,
    }
}

fn proposal_script_hash_from_proposal(proposal: &ProposalProcedure) -> Option<[u8; 28]> {
    use crate::eras::conway::GovAction;

    match &proposal.gov_action {
        GovAction::ParameterChange {
            guardrails_script_hash,
            ..
        }
        | GovAction::TreasuryWithdrawals {
            guardrails_script_hash,
            ..
        } => *guardrails_script_hash,
        GovAction::NewConstitution { constitution, .. } => constitution.guardrails_script_hash,
        GovAction::HardForkInitiation { .. }
        | GovAction::NoConfidence { .. }
        | GovAction::UpdateCommittee { .. }
        | GovAction::InfoAction => None,
    }
}

fn resolve_spending_datum(
    txout: &MultiEraTxOut,
    datum_map: &HashMap<[u8; 32], PlutusData>,
    tx_id: [u8; 32],
    index: u64,
) -> Result<PlutusData, LedgerError> {
    match txout {
        MultiEraTxOut::Alonzo(output) => {
            let hash = output
                .datum_hash
                .ok_or(LedgerError::MissingDatum { tx_id, index })?;
            datum_map
                .get(&hash)
                .cloned()
                .ok_or(LedgerError::MissingDatum { tx_id, index })
        }
        MultiEraTxOut::Babbage(output) => match &output.datum_option {
            Some(DatumOption::Hash(hash)) => datum_map
                .get(hash)
                .cloned()
                .ok_or(LedgerError::MissingDatum { tx_id, index }),
            Some(DatumOption::Inline(datum)) => Ok(datum.clone()),
            None => Err(LedgerError::MissingDatum { tx_id, index }),
        },
        MultiEraTxOut::Shelley(_) | MultiEraTxOut::Mary(_) => {
            Err(LedgerError::MissingDatum { tx_id, index })
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cbor::CborEncode;
    use crate::eras::conway::{GovAction, ProposalProcedure, Voter};
    use crate::eras::alonzo::AlonzoTxOut;
    use crate::eras::babbage::{BabbageTxOut, DatumOption};
    use crate::eras::mary::Value;
    use crate::eras::shelley::{ShelleyTxIn, ShelleyWitnessSet};
    use crate::types::{Address, DRep, EnterpriseAddress, RewardAccount, StakeCredential};
    use crate::utxo::{MultiEraTxOut, MultiEraUtxo};

    #[test]
    fn plutus_v1_script_hash_uses_tag_01() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
        // Verify it's Blake2b-224 of [0x01, 0x01, 0x02, 0x03]
        let expected = yggdrasil_crypto::blake2b::hash_bytes_224(
            &[0x01, 0x01, 0x02, 0x03],
        ).0;
        assert_eq!(hash, expected);
    }

    #[test]
    fn plutus_v2_script_hash_uses_tag_02() {
        let script_bytes = vec![0xAA, 0xBB];
        let hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);
        let expected = yggdrasil_crypto::blake2b::hash_bytes_224(
            &[0x02, 0xAA, 0xBB],
        ).0;
        assert_eq!(hash, expected);
    }

    #[test]
    fn plutus_v3_script_hash_uses_tag_03() {
        let script_bytes = vec![0xFF];
        let hash = plutus_script_hash(PlutusVersion::V3, &script_bytes);
        let expected = yggdrasil_crypto::blake2b::hash_bytes_224(
            &[0x03, 0xFF],
        ).0;
        assert_eq!(hash, expected);
    }

    #[test]
    fn collect_plutus_scripts_returns_all_versions() {
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![vec![0x01]],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![vec![0x02]],
            plutus_v3_scripts: vec![vec![0x03]],
        };
        let scripts = collect_plutus_scripts(&ws);
        assert_eq!(scripts.len(), 3);
        let h1 = plutus_script_hash(PlutusVersion::V1, &[0x01]);
        let h2 = plutus_script_hash(PlutusVersion::V2, &[0x02]);
        let h3 = plutus_script_hash(PlutusVersion::V3, &[0x03]);
        assert_eq!(scripts[&h1].0, PlutusVersion::V1);
        assert_eq!(scripts[&h2].0, PlutusVersion::V2);
        assert_eq!(scripts[&h3].0, PlutusVersion::V3);
    }

    #[test]
    fn collect_datum_map_hashes_cbor() {
        let datum = PlutusData::Integer(42.into());
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![datum.clone()],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let map = collect_datum_map(&ws);
        assert_eq!(map.len(), 1);
        let cbor = datum.to_cbor_bytes();
        let hash = yggdrasil_crypto::blake2b::hash_bytes_256(&cbor).0;
        assert_eq!(map[&hash], datum);
    }

    #[test]
    fn resolve_spending_purpose() {
        let inputs = vec![
            crate::eras::shelley::ShelleyTxIn { transaction_id: [0xAA; 32], index: 0 },
            crate::eras::shelley::ShelleyTxIn { transaction_id: [0xBB; 32], index: 1 },
        ];
        let redeemer = Redeemer {
            tag: 0,
            index: 1,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 100, steps: 200 },
        };
        let purpose = resolve_script_purpose(&redeemer, &inputs, &[], &[], &[], &[], &[]).unwrap();
        assert!(matches!(
            purpose,
            ScriptPurpose::Spending { tx_id, index } if tx_id == [0xBB; 32] && index == 1
        ));
    }

    #[test]
    fn resolve_minting_purpose() {
        let policies = vec![[0xCC; 28]];
        let redeemer = Redeemer {
            tag: 1,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 100, steps: 200 },
        };
        let purpose = resolve_script_purpose(&redeemer, &[], &policies, &[], &[], &[], &[]).unwrap();
        assert!(matches!(purpose, ScriptPurpose::Minting { policy_id } if policy_id == [0xCC; 28]));
    }

    #[test]
    fn resolve_certifying_purpose_carries_certificate() {
        let certificate = DCert::AccountRegistration(StakeCredential::ScriptHash([0xDD; 28]));
        let redeemer = Redeemer {
            tag: 2,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 100, steps: 200 },
        };

        let purpose = resolve_script_purpose(&redeemer, &[], &[], &[certificate.clone()], &[], &[], &[]).unwrap();

        assert!(matches!(
            purpose,
            ScriptPurpose::Certifying { cert_index, certificate: carried }
                if cert_index == 0 && carried == certificate
        ));
    }

    #[test]
    fn resolve_voting_purpose_carries_voter() {
        let voter = Voter::DRepScript([0xAB; 28]);
        let redeemer = Redeemer {
            tag: 4,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 100, steps: 200 },
        };

        let purpose = resolve_script_purpose(&redeemer, &[], &[], &[], &[], &[voter.clone()], &[]).unwrap();

        assert!(matches!(purpose, ScriptPurpose::Voting { voter: carried } if carried == voter));
    }

    #[test]
    fn resolve_proposing_purpose_carries_procedure() {
        let proposal = ProposalProcedure {
            deposit: 5,
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0xCC; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: crate::types::Anchor {
                url: "https://example.invalid/proposal".to_string(),
                data_hash: [0xDD; 32],
            },
        };
        let redeemer = Redeemer {
            tag: 5,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 100, steps: 200 },
        };

        let purpose = resolve_script_purpose(&redeemer, &[], &[], &[], &[], &[], &[proposal.clone()]).unwrap();

        assert!(matches!(
            purpose,
            ScriptPurpose::Proposing {
                proposal_index,
                proposal: carried,
            } if proposal_index == 0 && carried == proposal
        ));
    }

    #[test]
    fn resolve_spending_out_of_range_fails() {
        let redeemer = Redeemer {
            tag: 0,
            index: 5,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 100, steps: 200 },
        };
        let err = resolve_script_purpose(&redeemer, &[], &[], &[], &[], &[], &[]).unwrap_err();
        assert!(matches!(err, LedgerError::MissingRedeemer { .. }));
    }

    /// Mock evaluator that always succeeds.
    struct AlwaysSucceeds;

    impl PlutusEvaluator for AlwaysSucceeds {
        fn evaluate(&self, _eval: &PlutusScriptEval) -> Result<(), LedgerError> {
            Ok(())
        }
    }

    /// Mock evaluator that always fails.
    struct AlwaysFails;

    impl PlutusEvaluator for AlwaysFails {
        fn evaluate(&self, eval: &PlutusScriptEval) -> Result<(), LedgerError> {
            Err(LedgerError::PlutusScriptFailed {
                hash: eval.script_hash,
                reason: "always fails".to_string(),
            })
        }
    }

    struct ExpectDatum(pub PlutusData);

    impl PlutusEvaluator for ExpectDatum {
        fn evaluate(&self, eval: &PlutusScriptEval) -> Result<(), LedgerError> {
            assert_eq!(eval.datum, Some(self.0.clone()));
            Ok(())
        }
    }

    #[test]
    fn validate_plutus_scripts_skips_without_evaluator() {
        use std::collections::HashSet;
        // Even with required scripts, None evaluator means soft-skip.
        let mut required = HashSet::new();
        required.insert([0xAA; 28]);
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![vec![0x01]],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let wb = ws.to_cbor_bytes();
        let utxo = MultiEraUtxo::new();
        let result = validate_plutus_scripts(
            None, Some(&wb), &required, &utxo, &[], &[], &[], &[], &[], &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn validate_minting_script_with_mock_evaluator() {
        use std::collections::HashSet;
        let script_bytes = vec![0x01, 0x02, 0x03];
        let policy_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
        let mut required = HashSet::new();
        required.insert(policy_hash);
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![script_bytes],
            plutus_data: vec![],
            redeemers: vec![Redeemer {
                tag: 1, // minting
                index: 0,
                data: PlutusData::Integer(42.into()),
                ex_units: ExUnits { mem: 1000, steps: 2000 },
            }],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let wb = ws.to_cbor_bytes();
        let utxo = MultiEraUtxo::new();
        let result = validate_plutus_scripts(
            Some(&AlwaysSucceeds),
            Some(&wb),
            &required,
            &utxo,
            &[],
            &[policy_hash],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn validate_minting_script_fails_with_rejecting_evaluator() {
        use std::collections::HashSet;
        let script_bytes = vec![0x01, 0x02, 0x03];
        let policy_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
        let mut required = HashSet::new();
        required.insert(policy_hash);
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![script_bytes],
            plutus_data: vec![],
            redeemers: vec![Redeemer {
                tag: 1,
                index: 0,
                data: PlutusData::Integer(42.into()),
                ex_units: ExUnits { mem: 1000, steps: 2000 },
            }],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let wb = ws.to_cbor_bytes();
        let utxo = MultiEraUtxo::new();
        let result = validate_plutus_scripts(
            Some(&AlwaysFails),
            Some(&wb),
            &required,
            &utxo,
            &[],
            &[policy_hash],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(matches!(
            result.unwrap_err(),
            LedgerError::PlutusScriptFailed { hash, .. } if hash == policy_hash
        ));
    }

    #[test]
    fn validate_plutus_scripts_empty_required_set_is_noop() {
        let required = std::collections::HashSet::new();
        let utxo = MultiEraUtxo::new();
        let result = validate_plutus_scripts(
            Some(&AlwaysSucceeds), None, &required, &utxo, &[], &[], &[], &[], &[], &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn validate_spending_script_resolves_alonzo_datum_hash() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let script_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
        let datum = PlutusData::Integer(99.into());
        let datum_hash = yggdrasil_crypto::blake2b::hash_bytes_256(&datum.to_cbor_bytes()).0;
        let txin = ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        };

        let mut required = std::collections::HashSet::new();
        required.insert(script_hash);

        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![script_bytes],
            plutus_data: vec![datum.clone()],
            redeemers: vec![Redeemer {
                tag: 0,
                index: 0,
                data: PlutusData::Integer(42.into()),
                ex_units: ExUnits { mem: 1000, steps: 2000 },
            }],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let wb = ws.to_cbor_bytes();

        let address = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::ScriptHash(script_hash),
        })
        .to_bytes();
        let mut utxo = MultiEraUtxo::new();
        utxo.insert(
            txin.clone(),
            MultiEraTxOut::Alonzo(AlonzoTxOut {
                address,
                amount: Value::Coin(1),
                datum_hash: Some(datum_hash),
            }),
        );

        let result = validate_plutus_scripts(
            Some(&ExpectDatum(datum)),
            Some(&wb),
            &required,
            &utxo,
            &[txin],
            &[],
            &[],
            &[],
            &[],
            &[],
        );

        assert!(result.is_ok());
    }

    #[test]
    fn validate_spending_script_uses_inline_babbage_datum() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let script_hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);
        let datum = PlutusData::Integer(7.into());
        let txin = ShelleyTxIn {
            transaction_id: [0xCD; 32],
            index: 1,
        };

        let mut required = std::collections::HashSet::new();
        required.insert(script_hash);

        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![Redeemer {
                tag: 0,
                index: 0,
                data: PlutusData::Integer(1.into()),
                ex_units: ExUnits { mem: 1000, steps: 2000 },
            }],
            plutus_v2_scripts: vec![script_bytes],
            plutus_v3_scripts: vec![],
        };
        let wb = ws.to_cbor_bytes();

        let address = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::ScriptHash(script_hash),
        })
        .to_bytes();
        let mut utxo = MultiEraUtxo::new();
        utxo.insert(
            txin.clone(),
            MultiEraTxOut::Babbage(BabbageTxOut {
                address,
                amount: Value::Coin(1),
                datum_option: Some(DatumOption::Inline(datum.clone())),
                script_ref: None,
            }),
        );

        let result = validate_plutus_scripts(
            Some(&ExpectDatum(datum)),
            Some(&wb),
            &required,
            &utxo,
            &[txin],
            &[],
            &[],
            &[],
            &[],
            &[],
        );

        assert!(result.is_ok());
    }

    #[test]
    fn validate_spending_script_fails_when_datum_hash_missing_from_witnesses() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let script_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
        let txin = ShelleyTxIn {
            transaction_id: [0xEF; 32],
            index: 2,
        };

        let mut required = std::collections::HashSet::new();
        required.insert(script_hash);

        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![script_bytes],
            plutus_data: vec![],
            redeemers: vec![Redeemer {
                tag: 0,
                index: 0,
                data: PlutusData::Integer(0.into()),
                ex_units: ExUnits { mem: 1000, steps: 2000 },
            }],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let wb = ws.to_cbor_bytes();

        let address = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::ScriptHash(script_hash),
        })
        .to_bytes();
        let mut utxo = MultiEraUtxo::new();
        utxo.insert(
            txin.clone(),
            MultiEraTxOut::Alonzo(AlonzoTxOut {
                address,
                amount: Value::Coin(1),
                datum_hash: Some([0x44; 32]),
            }),
        );

        let result = validate_plutus_scripts(
            Some(&AlwaysSucceeds),
            Some(&wb),
            &required,
            &utxo,
            &[txin],
            &[],
            &[],
            &[],
            &[],
            &[],
        );

        assert!(matches!(
            result,
            Err(LedgerError::MissingDatum { tx_id, index }) if tx_id == [0xEF; 32] && index == 2
        ));
    }

    #[test]
    fn validate_certifying_script_resolves_drep_script_hash() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let script_hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![Redeemer {
                tag: 2,
                index: 0,
                data: PlutusData::Integer(5.into()),
                ex_units: ExUnits { mem: 1000, steps: 2000 },
            }],
            plutus_v2_scripts: vec![script_bytes],
            plutus_v3_scripts: vec![],
        };
        let certs = vec![DCert::DelegationToDrep(
            StakeCredential::AddrKeyHash([0x11; 28]),
            DRep::ScriptHash(script_hash),
        )];
        let mut required = std::collections::HashSet::new();
        required.insert(script_hash);
        let utxo = MultiEraUtxo::new();

        let result = validate_plutus_scripts(
            Some(&AlwaysSucceeds),
            Some(&ws.to_cbor_bytes()),
            &required,
            &utxo,
            &[],
            &[],
            &certs,
            &[],
            &[],
            &[],
        );

        assert!(result.is_ok());
    }

    #[test]
    fn validate_rewarding_script_requires_script_reward_account() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let script_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
        let reward_account = RewardAccount {
            network: 1,
            credential: StakeCredential::ScriptHash(script_hash),
        };
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![script_bytes],
            plutus_data: vec![],
            redeemers: vec![Redeemer {
                tag: 3,
                index: 0,
                data: PlutusData::Integer(8.into()),
                ex_units: ExUnits { mem: 1000, steps: 2000 },
            }],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let mut required = std::collections::HashSet::new();
        required.insert(script_hash);
        let utxo = MultiEraUtxo::new();

        let result = validate_plutus_scripts(
            Some(&AlwaysSucceeds),
            Some(&ws.to_cbor_bytes()),
            &required,
            &utxo,
            &[],
            &[],
            &[],
            &[reward_account.to_bytes().to_vec()],
            &[],
            &[],
        );

        assert!(result.is_ok());
    }

    #[test]
    fn validate_voting_script_resolves_script_voter_hash() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let script_hash = plutus_script_hash(PlutusVersion::V3, &script_bytes);
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![Redeemer {
                tag: 4,
                index: 0,
                data: PlutusData::Integer(9.into()),
                ex_units: ExUnits { mem: 1000, steps: 2000 },
            }],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![script_bytes],
        };
        let mut required = std::collections::HashSet::new();
        required.insert(script_hash);
        let utxo = MultiEraUtxo::new();
        let voters = vec![Voter::DRepScript(script_hash)];

        let result = validate_plutus_scripts(
            Some(&AlwaysSucceeds),
            Some(&ws.to_cbor_bytes()),
            &required,
            &utxo,
            &[],
            &[],
            &[],
            &[],
            &voters,
            &[],
        );

        assert!(result.is_ok());
    }
}
