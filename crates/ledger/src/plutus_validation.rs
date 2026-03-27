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

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::cbor::{CborDecode, CborEncode};
use crate::eras::conway::{ProposalProcedure, Voter, VotingProcedures};
use crate::error::LedgerError;
use crate::eras::alonzo::{ExUnits, Redeemer};
use crate::eras::babbage::DatumOption;
use crate::eras::mary::MintAsset;
use crate::eras::shelley::ShelleyTxIn;
use crate::plutus::PlutusData;
use crate::types::{Address, DCert, RewardAccount, StakeCredential};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::protocol_params::ProtocolParameters;

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
// Transaction context for TxInfo construction
// ---------------------------------------------------------------------------

/// Normalised transaction body data needed by the CEK evaluator to build
/// the Plutus `TxInfo` / `ScriptContext`.
///
/// This is era-independent: the per-era `apply_*_block` functions populate
/// it from the concrete tx body and pass it through to the evaluator.
#[derive(Clone, Debug, Default)]
pub struct TxContext {
    /// Blake2b-256 hash of the serialised transaction body.
    pub tx_hash: [u8; 32],
    /// Transaction fee in lovelace.
    pub fee: u64,
    /// Transaction outputs.
    pub outputs: Vec<MultiEraTxOut>,
    /// Slot of the lower bound of the validity interval (`None` = -∞).
    pub validity_start: Option<u64>,
    /// Slot of the upper bound / TTL (`None` = +∞).
    pub ttl: Option<u64>,
    /// Required signer key hashes.
    pub required_signers: Vec<[u8; 28]>,
    /// Mint / burn map (policy → asset_name → quantity).
    pub mint: MintAsset,
    /// Withdrawals (reward_account → lovelace).
    pub withdrawals: BTreeMap<RewardAccount, u64>,
    /// Reference inputs (Babbage+ only).
    pub reference_inputs: Vec<ShelleyTxIn>,
    /// Current treasury value (Conway only).
    pub current_treasury_value: Option<u64>,
    /// Treasury donation (Conway only).
    pub treasury_donation: Option<u64>,
    /// Resolved spending inputs: (txin, resolved txout) sorted by txin.
    /// Populated in `validate_plutus_scripts` from the live UTxO set.
    pub inputs: Vec<(ShelleyTxIn, MultiEraTxOut)>,
    /// Certificates in transaction order (verbatim from the tx body).
    /// Used to build the `dcert` field of `TxInfo`.
    pub certificates: Vec<DCert>,
    /// Witness-set datum map: Blake2b-256(cbor(datum)) → PlutusData.
    /// Populated in `validate_plutus_scripts` from the ShelleyWitnessSet.
    pub witness_datums: HashMap<[u8; 32], PlutusData>,
    /// Resolved reference inputs: (txin, resolved txout) sorted by txin.
    /// Populated in `validate_plutus_scripts` from the live UTxO set.
    pub resolved_reference_inputs: Vec<(ShelleyTxIn, MultiEraTxOut)>,
    /// All redeemers resolved to their concrete Plutus script purposes.
    /// Populated in `validate_plutus_scripts` after ledger-side pointer resolution.
    pub redeemers: Vec<(ScriptPurpose, PlutusData)>,
    /// Conway voting procedures carried by the transaction body.
    pub voting_procedures: Option<VotingProcedures>,
    /// Conway proposal procedures carried by the transaction body.
    pub proposal_procedures: Vec<ProposalProcedure>,
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
    fn evaluate(&self, eval: &PlutusScriptEval, tx_ctx: &TxContext) -> Result<(), LedgerError>;
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

/// Compute the Alonzo-family script integrity hash (`script_data_hash`) from
/// witness-set content and protocol parameters.
///
/// This is the local parity helper for `PPViewHashesDontMatch` checks.
/// The preimage is built as:
/// - redeemers encoding (legacy array for Alonzo, map for Conway-style)
/// - optional datums encoding
/// - language views encoding derived from used Plutus script versions
pub fn compute_script_data_hash(
    witness_bytes: Option<&[u8]>,
    protocol_params: Option<&ProtocolParameters>,
    conway_redeemer_format: bool,
) -> Result<[u8; 32], LedgerError> {
    let ws = match witness_bytes {
        Some(wb) => crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?,
        None => crate::eras::shelley::ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
    };

    let redeemers_bytes = encode_redeemers_for_script_data_hash(&ws.redeemers, conway_redeemer_format);
    let datums_bytes = encode_datums_for_script_data_hash(&ws.plutus_data);
    let language_views = encode_language_views_for_script_data_hash(&ws, protocol_params);

    let mut preimage = Vec::with_capacity(
        redeemers_bytes.len() + datums_bytes.len() + language_views.len(),
    );
    preimage.extend_from_slice(&redeemers_bytes);
    preimage.extend_from_slice(&datums_bytes);
    preimage.extend_from_slice(&language_views);

    Ok(yggdrasil_crypto::hash_bytes_256(&preimage).0)
}

/// Validate a declared `script_data_hash` against locally computed value.
///
/// Returns `Ok(())` when no hash is declared.
pub fn validate_script_data_hash(
    declared: Option<[u8; 32]>,
    witness_bytes: Option<&[u8]>,
    protocol_params: Option<&ProtocolParameters>,
    conway_redeemer_format: bool,
) -> Result<(), LedgerError> {
    let Some(declared_hash) = declared else {
        return Ok(());
    };
    let computed = compute_script_data_hash(
        witness_bytes,
        protocol_params,
        conway_redeemer_format,
    )?;
    if computed != declared_hash {
        return Err(LedgerError::PPViewHashesDontMatch {
            declared: declared_hash,
            computed,
        });
    }
    Ok(())
}

fn encode_redeemers_for_script_data_hash(
    redeemers: &[Redeemer],
    conway_redeemer_format: bool,
) -> Vec<u8> {
    let mut enc = crate::cbor::Encoder::new();
    if conway_redeemer_format {
        // Conway map format: { [tag, index] => [data, ex_units] }
        let mut sorted = redeemers.to_vec();
        sorted.sort_by_key(|r| (r.tag, r.index));
        enc.map(sorted.len() as u64);
        for r in sorted {
            enc.array(2)
                .unsigned(r.tag as u64)
                .unsigned(r.index);
            enc.array(2);
            r.data.encode_cbor(&mut enc);
            r.ex_units.encode_cbor(&mut enc);
        }
    } else {
        // Alonzo legacy array format: [* redeemer]
        enc.array(redeemers.len() as u64);
        for r in redeemers {
            r.encode_cbor(&mut enc);
        }
    }
    enc.into_bytes()
}

fn encode_datums_for_script_data_hash(datums: &[PlutusData]) -> Vec<u8> {
    if datums.is_empty() {
        return Vec::new();
    }
    let mut enc = crate::cbor::Encoder::new();
    enc.array(datums.len() as u64);
    for d in datums {
        d.encode_cbor(&mut enc);
    }
    enc.into_bytes()
}

fn encode_cost_model_values(values: &[i64], indefinite: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if indefinite {
        // 0x9f = start indefinite-length array
        out.push(0x9f);
        for v in values {
            let mut enc = crate::cbor::Encoder::new();
            enc.signed(*v);
            out.extend_from_slice(&enc.into_bytes());
        }
        // 0xff = break
        out.push(0xff);
    } else {
        let mut enc = crate::cbor::Encoder::new();
        enc.array(values.len() as u64);
        for v in values {
            enc.signed(*v);
        }
        out = enc.into_bytes();
    }
    out
}

fn encode_language_views_for_script_data_hash(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
    protocol_params: Option<&ProtocolParameters>,
) -> Vec<u8> {
    let mut langs: Vec<u8> = Vec::new();
    if !ws.plutus_v1_scripts.is_empty() {
        langs.push(0);
    }
    if !ws.plutus_v2_scripts.is_empty() {
        langs.push(1);
    }
    if !ws.plutus_v3_scripts.is_empty() {
        langs.push(2);
    }
    langs.sort_unstable();
    langs.dedup();

    let cost_models = protocol_params.and_then(|p| p.cost_models.as_ref());

    let mut enc = crate::cbor::Encoder::new();
    // Local canonical map by integer language tag.
    enc.map(langs.len() as u64);
    for lang in langs {
        enc.unsigned(lang as u64);
        let cm_bytes = if let Some(cm) = cost_models.and_then(|m| m.get(&lang)) {
            // V1 uses the historical indefinite-array quirk.
            encode_cost_model_values(cm, lang == 0)
        } else {
            // Missing cost model -> null
            vec![0xf6]
        };
        enc.bytes(&cm_bytes);
    }
    enc.into_bytes()
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

/// Collects all Plutus scripts from a witness set and from reference input UTxOs.
pub(crate) fn collect_all_plutus_scripts(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
    utxo: &crate::utxo::MultiEraUtxo,
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
) -> HashMap<[u8; 28], (PlutusVersion, Vec<u8>)> {
    let mut scripts = HashMap::new();
    // Collect from witness set
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
    // Collect from reference inputs' UTxO entries
    if let Some(ref_inputs) = reference_inputs {
        for txin in ref_inputs {
            if let Some(txout) = utxo.get(txin) {
                if let Some(sref) = txout.script_ref() {
                    match &sref.0 {
                        crate::plutus::Script::PlutusV1(bytes) => {
                            let hash = plutus_script_hash(PlutusVersion::V1, bytes);
                            scripts.insert(hash, (PlutusVersion::V1, bytes.clone()));
                        }
                        crate::plutus::Script::PlutusV2(bytes) => {
                            let hash = plutus_script_hash(PlutusVersion::V2, bytes);
                            scripts.insert(hash, (PlutusVersion::V2, bytes.clone()));
                        }
                        crate::plutus::Script::PlutusV3(bytes) => {
                            let hash = plutus_script_hash(PlutusVersion::V3, bytes);
                            scripts.insert(hash, (PlutusVersion::V3, bytes.clone()));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    scripts
}

// ---------------------------------------------------------------------------
// Supplemental datum validation
// ---------------------------------------------------------------------------

/// Validates that every witness datum not required by a Plutus spending input
/// is "allowed" — its hash matches a datum hash on a transaction output or
/// (Babbage+) a reference-input UTxO.
///
/// # Sets
///
/// * **`input_hashes`** — datum hashes from spending-input UTxOs locked by a
///   Plutus script.
/// * **`tx_hashes`** — Blake2b-256 hashes of all datums in the witness set
///   (`plutus_data`).
/// * **`allowed_supplemental`** — datum hashes on transaction outputs (Alonzo)
///   plus reference-input UTxOs (Babbage+).
///
/// Supplemental = `tx_hashes \ input_hashes`.  Every supplemental hash must
/// be in `allowed_supplemental`.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.missingRequiredDatums`
/// (`NotAllowedSupplementalDatums`).
pub fn validate_supplemental_datums(
    witness_bytes: Option<&[u8]>,
    spending_utxo: &MultiEraUtxo,
    spending_inputs: &[crate::eras::shelley::ShelleyTxIn],
    tx_outputs: &[MultiEraTxOut],
    reference_input_utxos: &[(crate::eras::shelley::ShelleyTxIn, MultiEraTxOut)],
) -> Result<(), LedgerError> {
    let wb = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };

    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;

    // tx_hashes: all datum hashes from witness set
    let tx_hashes: HashSet<[u8; 32]> = collect_datum_map(&ws).into_keys().collect();
    if tx_hashes.is_empty() {
        return Ok(());
    }

    // Collect Plutus scripts (witness + reference) to identify Plutus-locked inputs.
    let ref_txins: Vec<_> = reference_input_utxos.iter().map(|(txin, _)| txin.clone()).collect();
    let plutus_scripts = collect_all_plutus_scripts(
        &ws,
        spending_utxo,
        if ref_txins.is_empty() { None } else { Some(&ref_txins) },
    );

    // input_hashes: datum hashes from Plutus-locked spending-input UTxOs.
    let mut input_hashes = HashSet::new();
    for txin in spending_inputs {
        if let Some(txout) = spending_utxo.get(txin) {
            // Only count if the input is Plutus-locked (not VKey/native).
            if let Some(script_hash) = spending_script_hash_from_txout(&txout) {
                if plutus_scripts.contains_key(&script_hash) {
                    if let Some(dh) = txout.datum_hash() {
                        input_hashes.insert(dh);
                    }
                }
            }
        }
    }

    // allowed_supplemental: datum hashes from tx outputs + reference-input UTxOs (Babbage+).
    let mut allowed = HashSet::new();
    for txout in tx_outputs {
        if let Some(dh) = txout.datum_hash() {
            allowed.insert(dh);
        }
    }
    for (_, ref_txout) in reference_input_utxos {
        if let Some(dh) = ref_txout.datum_hash() {
            allowed.insert(dh);
        }
    }

    // supplemental = tx_hashes \ input_hashes — must all be allowed.
    for dh in &tx_hashes {
        if !input_hashes.contains(dh) && !allowed.contains(dh) {
            return Err(LedgerError::NotAllowedSupplementalDatums { hash: *dh });
        }
    }
    Ok(())
}

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
    certificates: &[DCert],
    sorted_reward_accounts: &[Vec<u8>],
    sorted_voters: &[Voter],
    proposal_procedures: &[ProposalProcedure],
    tx_ctx: &TxContext,
) -> Result<(), LedgerError> {
    let wb = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()), // soft-skip like witness validation
    };

    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;

    // Collect available Plutus scripts and datum map.
    let plutus_scripts = collect_all_plutus_scripts(
        &ws,
        spending_utxo,
        if tx_ctx.reference_inputs.is_empty() {
            None
        } else {
            Some(&tx_ctx.reference_inputs)
        },
    );
    let datum_map = collect_datum_map(&ws);
    let resolved_redeemers: Vec<(ScriptPurpose, PlutusData)> = ws
        .redeemers
        .iter()
        .map(|redeemer| {
            Ok((
                resolve_script_purpose(
                    redeemer,
                    sorted_inputs,
                    sorted_policy_ids,
                    certificates,
                    sorted_reward_accounts,
                    sorted_voters,
                    proposal_procedures,
                )?,
                redeemer.data.clone(),
            ))
        })
        .collect::<Result<_, LedgerError>>()?;

    // ── ExtraRedeemers check (UTXOW Phase-1) ──────────────────────────
    // Every redeemer must target a purpose backed by a Plutus script.
    // Reference: Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers
    for (redeemer, (purpose, _)) in ws.redeemers.iter().zip(&resolved_redeemers) {
        let target_hash = match purpose {
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
            ScriptPurpose::Proposing { proposal, .. } => {
                proposal_script_hash_from_proposal(proposal)
            }
        };
        match target_hash {
            Some(hash) if plutus_scripts.contains_key(&hash) => {}
            _ => {
                return Err(LedgerError::ExtraRedeemer {
                    tag: redeemer.tag,
                    index: redeemer.index,
                });
            }
        }
    }

    // Build an augmented TxContext with the fields that require access to the
    // witness set and spending UTxO (inputs, certificates, witness_datums).
    // These are not available at the call-sites in state.rs so we populate
    // them here, where all the raw data is in scope.
    let resolved_inputs: Vec<(ShelleyTxIn, MultiEraTxOut)> = sorted_inputs
        .iter()
        .filter_map(|txin| spending_utxo.get(txin).map(|txout| (txin.clone(), txout.clone())))
        .collect();
    let resolved_reference_inputs: Vec<(ShelleyTxIn, MultiEraTxOut)> = tx_ctx
        .reference_inputs
        .iter()
        .filter_map(|txin| spending_utxo.get(txin).map(|txout| (txin.clone(), txout.clone())))
        .collect();
    let augmented_tx_ctx = TxContext {
        inputs: resolved_inputs,
        certificates: certificates.to_vec(),
        witness_datums: datum_map.clone(),
        resolved_reference_inputs,
        // Clone all other fields from the caller-supplied context.
        tx_hash: tx_ctx.tx_hash,
        fee: tx_ctx.fee,
        outputs: tx_ctx.outputs.clone(),
        validity_start: tx_ctx.validity_start,
        ttl: tx_ctx.ttl,
        required_signers: tx_ctx.required_signers.clone(),
        mint: tx_ctx.mint.clone(),
        withdrawals: tx_ctx.withdrawals.clone(),
        reference_inputs: tx_ctx.reference_inputs.clone(),
        current_treasury_value: tx_ctx.current_treasury_value,
        treasury_donation: tx_ctx.treasury_donation,
        redeemers: resolved_redeemers.clone(),
        voting_procedures: tx_ctx.voting_procedures.clone(),
        proposal_procedures: tx_ctx.proposal_procedures.clone(),
    };

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
    for (redeemer, (purpose, redeemer_data)) in ws.redeemers.iter().zip(&resolved_redeemers) {
        let purpose = purpose.clone();

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
                    redeemer: redeemer_data.clone(),
                    ex_units: redeemer.ex_units,
                };

                evaluator.evaluate(&eval_target, &augmented_tx_ctx)?;
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
        DCert::PoolRegistration(_) | DCert::PoolRetirement(_, _)
        | DCert::GenesisDelegation(_, _, _) | DCert::MoveInstantaneousReward(_, _) => None,
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
        fn evaluate(&self, _eval: &PlutusScriptEval, _tx_ctx: &TxContext) -> Result<(), LedgerError> {
            Ok(())
        }
    }

    /// Mock evaluator that always fails.
    struct AlwaysFails;

    impl PlutusEvaluator for AlwaysFails {
        fn evaluate(&self, eval: &PlutusScriptEval, _tx_ctx: &TxContext) -> Result<(), LedgerError> {
            Err(LedgerError::PlutusScriptFailed {
                hash: eval.script_hash,
                reason: "always fails".to_string(),
            })
        }
    }

    struct ExpectDatum(pub PlutusData);

    impl PlutusEvaluator for ExpectDatum {
        fn evaluate(&self, eval: &PlutusScriptEval, _tx_ctx: &TxContext) -> Result<(), LedgerError> {
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
            &TxContext::default(),
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
            &TxContext::default(),
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
            &TxContext::default(),
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
            &TxContext::default(),
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
            &TxContext::default(),
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
            &TxContext::default(),
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
            &TxContext::default(),
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
            &TxContext::default(),
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
            &TxContext::default(),
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
            &TxContext::default(),
        );

        assert!(result.is_ok());
    }
}
