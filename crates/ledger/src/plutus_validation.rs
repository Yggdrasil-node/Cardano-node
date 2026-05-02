//! Plutus Phase-2 script validation bridge.
//!
//! This module defines the [`PlutusEvaluator`](crate::plutus_validation::PlutusEvaluator) trait that higher layers
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
use crate::eras::alonzo::{ExUnits, Redeemer};
use crate::eras::babbage::DatumOption;
use crate::eras::conway::{ProposalProcedure, Voter, VotingProcedures};
use crate::eras::mary::MintAsset;
use crate::eras::shelley::ShelleyTxIn;
use crate::error::LedgerError;
use crate::plutus::PlutusData;
use crate::protocol_params::ProtocolParameters;
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

    /// CDDL cost-model map key (0 = V1, 1 = V2, 2 = V3).
    pub fn cost_model_key(self) -> u8 {
        match self {
            Self::V1 => 0,
            Self::V2 => 1,
            Self::V3 => 2,
        }
    }

    /// First protocol major version that supports this Plutus language.
    ///
    /// Mirrors upstream `guardPlutus` / `decodePlutusRunnable` availability:
    /// V1 at PV5, V2 at PV7, and V3 at PV9.
    pub fn first_supported_protocol_major(self) -> u64 {
        match self {
            Self::V1 => 5,
            Self::V2 => 7,
            Self::V3 => 9,
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
    Proposing {
        proposal_index: u64,
        proposal: ProposalProcedure,
    },
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
    /// Raw on-chain `PlutusBinary` script bytes.
    ///
    /// These are the bytes inside the ledger witness/reference-script CBOR
    /// item. Upstream `PlutusBinary` stores a `SerialisedScript`, which is
    /// itself a CBOR bytestring containing the Flat-encoded UPLC program.
    pub script_bytes: Vec<u8>,
    /// Purpose that triggered this evaluation.
    pub purpose: ScriptPurpose,
    /// Datum (required for spending validators, `None` for minting/cert/reward).
    pub datum: Option<PlutusData>,
    /// Redeemer data.
    pub redeemer: PlutusData,
    /// Execution budget allocated by the transaction for this script.
    pub ex_units: ExUnits,
    /// Active protocol-parameter cost model for this script language.
    ///
    /// The ledger owns the epoch-specific protocol parameters, while the node
    /// owns the concrete CEK implementation. Carrying the ordered CDDL array
    /// here lets the evaluator charge exactly the cost model active for the
    /// transaction rather than a startup fallback.
    pub cost_model: Option<Vec<i64>>,
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
    /// Protocol version `(major, minor)` for version-dependent ScriptContext
    /// encoding.  When `major == 9` (Conway bootstrap phase), PlutusV3
    /// `RegDepositTxCert` / `UnRegDepositTxCert` deposit fields are omitted
    /// to match the upstream PV9 bug preserved by `hardforkConwayBootstrapPhase`.
    pub protocol_version: Option<(u64, u64)>,
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
    /// 1. Decode `eval.script_bytes` as upstream `PlutusBinary`
    ///    (`SerialisedScript` CBOR bytestring, then Flat).
    /// 2. Apply `eval.datum` (if spending), `eval.redeemer`, and a
    ///    `ScriptContext` as arguments to the decoded program.
    /// 3. Evaluate within `eval.ex_units` budget.
    /// 4. Return `Ok(())` on success, or a `LedgerError` on failure.
    fn evaluate(&self, eval: &PlutusScriptEval, tx_ctx: &TxContext) -> Result<(), LedgerError>;

    /// Check whether Plutus script bytes are well-formed for the active
    /// protocol version (upstream `decodePlutusRunnable` from the UTXOW rule).
    ///
    /// `script_bytes` are the raw on-chain `PlutusBinary` bytes after ledger
    /// CBOR decoding has removed only the witness/reference-script bytestring.
    /// Evaluators must still decode the `SerialisedScript` CBOR bytestring
    /// contained in `PlutusBinary` before Flat-decoding the UPLC program. The
    /// optional protocol version lets evaluators reject languages before their
    /// upstream activation point. The default implementation returns `true` so
    /// callers without a CEK machine still pass the check.
    fn is_script_well_formed(
        &self,
        _version: PlutusVersion,
        _protocol_version: Option<(u64, u64)>,
        _script_bytes: &[u8],
    ) -> bool {
        true
    }
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
    utxo: Option<&MultiEraUtxo>,
    reference_inputs: Option<&[ShelleyTxIn]>,
    spending_inputs: Option<&[ShelleyTxIn]>,
    required_script_hashes: Option<&HashSet<[u8; 28]>>,
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

    let raw_redeemers = witness_bytes
        .map(|wb| extract_witness_set_field_raw(wb, 5))
        .transpose()?
        .flatten();
    let raw_datums = witness_bytes
        .map(|wb| extract_witness_set_field_raw(wb, 4))
        .transpose()?
        .flatten();

    let redeemers_bytes = raw_redeemers.unwrap_or_else(|| {
        encode_redeemers_for_script_data_hash(&ws.redeemers, conway_redeemer_format)
    });
    let datums_bytes = if ws.plutus_data.is_empty() {
        // Upstream `SafeToHash (ScriptIntegrity era)` omits `TxDats`
        // whenever the decoded datum map is empty, even if witness field 4
        // was present as an empty array. Only non-empty datums preserve their
        // memoized original bytes.
        Vec::new()
    } else {
        raw_datums.unwrap_or_else(|| encode_datums_for_script_data_hash(&ws.plutus_data))
    };
    let language_views = encode_language_views_for_script_data_hash(
        &ws,
        protocol_params,
        utxo,
        reference_inputs,
        spending_inputs,
        required_script_hashes,
    );

    let mut preimage =
        Vec::with_capacity(redeemers_bytes.len() + datums_bytes.len() + language_views.len());
    preimage.extend_from_slice(&redeemers_bytes);
    preimage.extend_from_slice(&datums_bytes);
    preimage.extend_from_slice(&language_views);

    Ok(yggdrasil_crypto::hash_bytes_256(&preimage).0)
}

/// Validate a declared `script_data_hash` against locally computed value.
///
/// Implements the upstream `mkScriptIntegrity` / `checkScriptIntegrityHash`
/// check: a script integrity hash is required when ANY of (redeemers,
/// datums, language views) is non-empty.  Only when all three are empty
/// does upstream return `SNothing` and expect no declared hash.
///
/// At protocol version >= 11 (Conway post-bootstrap) the mismatch error is
/// reported as `ScriptIntegrityHashMismatch` instead of
/// `PPViewHashesDontMatch`.  Pre-PV11 or when version is unknown, the
/// legacy error is returned.
///
/// Reference: `Cardano.Ledger.Alonzo.Tx` — `mkScriptIntegrity`,
/// `checkScriptIntegrityHash`;
/// `Cardano.Ledger.Conway.Rules.Utxo` — `ScriptIntegrityHashMismatch`.
pub fn validate_script_data_hash(
    declared: Option<[u8; 32]>,
    witness_bytes: Option<&[u8]>,
    protocol_params: Option<&ProtocolParameters>,
    conway_redeemer_format: bool,
    utxo: Option<&MultiEraUtxo>,
    reference_inputs: Option<&[ShelleyTxIn]>,
    spending_inputs: Option<&[ShelleyTxIn]>,
    required_script_hashes: Option<&HashSet<[u8; 28]>>,
    protocol_version: Option<(u64, u64)>,
) -> Result<(), LedgerError> {
    // Upstream `mkScriptIntegrity` returns `SNothing` only when ALL THREE
    // of (redeemers, langViews, datums) are null.  If any one is non-empty,
    // the integrity hash is computed and must be present and match.
    //
    // Reference: `Cardano.Ledger.Alonzo.Tx.mkScriptIntegrity`:
    //   | null (txRedeemers ^. unRedeemersL)
    //   , null langViews
    //   , null (txDats ^. unTxDatsL) = SNothing
    //   | otherwise = SJust $ ScriptIntegrity txRedeemers txDats langViews
    let needs_hash = script_integrity_needed(
        witness_bytes,
        utxo,
        reference_inputs,
        spending_inputs,
        required_script_hashes,
    );

    match (declared, needs_hash) {
        (None, false) => Ok(()),
        (Some(declared_hash), false) => Err(LedgerError::UnexpectedScriptIntegrityHash {
            declared: declared_hash,
        }),
        (None, true) => Err(LedgerError::MissingRequiredScriptIntegrityHash),
        (Some(declared_hash), true) => {
            let computed = compute_script_data_hash(
                witness_bytes,
                protocol_params,
                conway_redeemer_format,
                utxo,
                reference_inputs,
                spending_inputs,
                required_script_hashes,
            )?;
            if computed != declared_hash {
                // PV >= 11: ScriptIntegrityHashMismatch
                // PV < 11 or unknown: PPViewHashesDontMatch
                let pv_ge_11 = matches!(protocol_version, Some((major, _)) if major >= 11);
                if pv_ge_11 {
                    return Err(LedgerError::ScriptIntegrityHashMismatch {
                        declared: declared_hash,
                        computed,
                    });
                }
                return Err(LedgerError::PPViewHashesDontMatch {
                    declared: declared_hash,
                    computed,
                });
            }
            Ok(())
        }
    }
}

/// Determine whether a script integrity hash is needed for this transaction.
///
/// Upstream `mkScriptIntegrity` returns `SNothing` only when ALL THREE
/// of (redeemers, txDats, langViews) are null.  We replicate that logic:
/// if any one of the three components is non-empty, the hash is required.
///
/// Reference: `Cardano.Ledger.Alonzo.Tx.mkScriptIntegrity`
fn script_integrity_needed(
    witness_bytes: Option<&[u8]>,
    utxo: Option<&MultiEraUtxo>,
    reference_inputs: Option<&[ShelleyTxIn]>,
    spending_inputs: Option<&[ShelleyTxIn]>,
    required_script_hashes: Option<&HashSet<[u8; 28]>>,
) -> bool {
    let Some(wb) = witness_bytes else {
        return false;
    };
    let ws = match crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb) {
        Ok(ws) => ws,
        // Parse failure ⇒ conservative: treat as no components present.
        Err(_) => return false,
    };

    // 1. Redeemers non-empty?
    if !ws.redeemers.is_empty() {
        return true;
    }

    // 2. Datums non-empty?
    if !ws.plutus_data.is_empty() {
        return true;
    }

    // 3. Language views non-empty?  This is derived from Plutus scripts
    //    that are both provided AND needed (i.e. in `required_script_hashes`).
    let scripts = collect_all_plutus_scripts(
        &ws,
        utxo.unwrap_or(&MultiEraUtxo::new()),
        reference_inputs,
        spending_inputs,
    );
    let has_lang_views = scripts.iter().any(|(hash, _)| {
        required_script_hashes
            .map(|required| required.contains(hash))
            .unwrap_or(true)
    });
    if has_lang_views {
        return true;
    }

    false
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
            enc.array(2).unsigned(r.tag as u64).unsigned(r.index);
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

fn extract_witness_set_field_raw(
    witness_bytes: &[u8],
    wanted_key: u64,
) -> Result<Option<Vec<u8>>, LedgerError> {
    fn visit_value(dec: &mut crate::cbor::Decoder<'_>) -> Result<Vec<u8>, LedgerError> {
        let start = dec.position();
        dec.skip()?;
        Ok(dec.slice(start, dec.position())?.to_vec())
    }

    let mut dec = crate::cbor::Decoder::new(witness_bytes);
    match dec.map_begin()? {
        Some(count) => {
            for _ in 0..count {
                let key = dec.unsigned()?;
                let value = visit_value(&mut dec)?;
                if key == wanted_key {
                    return Ok(Some(value));
                }
            }
        }
        None => {
            while !dec.is_break() {
                let key = dec.unsigned()?;
                let value = visit_value(&mut dec)?;
                if key == wanted_key {
                    return Ok(Some(value));
                }
            }
            dec.consume_break()?;
        }
    }
    Ok(None)
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
    utxo: Option<&MultiEraUtxo>,
    reference_inputs: Option<&[ShelleyTxIn]>,
    spending_inputs: Option<&[ShelleyTxIn]>,
    required_script_hashes: Option<&HashSet<[u8; 28]>>,
) -> Vec<u8> {
    let mut langs: Vec<u8> = collect_all_plutus_scripts(
        ws,
        utxo.unwrap_or(&MultiEraUtxo::new()),
        reference_inputs,
        spending_inputs,
    )
    .into_iter()
    .filter(|(hash, _)| {
        required_script_hashes
            .map(|required| required.contains(hash))
            .unwrap_or(true)
    })
    .map(|(_, (version, _))| version.cost_model_key())
    .collect();
    langs.sort_unstable();
    langs.dedup();

    let cost_models = protocol_params.and_then(|p| p.cost_models.as_ref());

    // Build (tag_bytes, value_bytes) pairs per language, matching upstream
    // `getLanguageView` in `Cardano.Ledger.Alonzo.PParams`.
    let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for lang in langs {
        // --- Key encoding ---
        // PlutusV1: double-serialized language tag = byte string wrapping
        //   `serialize' (serialize' lang)` = 0x41 0x00
        // PlutusV2+: single-serialized = CBOR unsigned integer (1 or 2).
        let tag_bytes = if lang == 0 {
            vec![0x41, 0x00]
        } else {
            let mut te = crate::cbor::Encoder::new();
            te.unsigned(lang as u64);
            te.into_bytes()
        };

        // --- Value encoding ---
        let cm_bytes = if let Some(cm) = cost_models.and_then(|m| m.get(&lang)) {
            // V1 uses the historical indefinite-length array quirk.
            encode_cost_model_values(cm, lang == 0)
        } else {
            // Missing cost model -> CBOR null
            vec![0xf6]
        };
        let value_bytes = if lang == 0 {
            // PlutusV1: cost model array is wrapped in a CBOR byte string
            // (double-encoded).
            // Reference: `getLanguageView` — `serialize' version costModelEncoding` for V1.
            let mut ve = crate::cbor::Encoder::new();
            ve.bytes(&cm_bytes);
            ve.into_bytes()
        } else {
            // PlutusV2+: cost model is the raw CBOR array, NOT wrapped.
            // Reference: `getLanguageView` — `costModelEncoding` for V2+.
            cm_bytes
        };

        pairs.push((tag_bytes, value_bytes));
    }

    // Sort by upstream `shortLex`: shorter tags first, then lexicographic.
    // Reference: `encodeLangViews` in `Cardano.Ledger.Alonzo.PParams`.
    pairs.sort_by(|a, b| {
        let la = a.0.len();
        let lb = b.0.len();
        if la != lb { la.cmp(&lb) } else { a.0.cmp(&b.0) }
    });

    let mut enc = crate::cbor::Encoder::new();
    enc.map(pairs.len() as u64);
    for (tag_bytes, value_bytes) in pairs {
        enc.raw(&tag_bytes);
        enc.raw(&value_bytes);
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
/// Keys are Blake2b-256 hashes of the canonical CBOR-encoded datum; values
/// are the typed `PlutusData`. Use
/// [`collect_datum_map_from_witness_bytes`] when original witness bytes are
/// available, because upstream `TxDats` hashes the memoized datum bytes.
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

/// Builds a datum lookup map using the original witness-set datum bytes.
///
/// Upstream `TxDats` is memoized: `hashData` is computed from the original
/// datum CBOR bytes, not from a canonical re-encoding after decode. This is
/// observable for non-canonical but accepted encodings such as wider integer
/// forms or indefinite containers.
fn collect_datum_map_from_witness_bytes(
    witness_bytes: Option<&[u8]>,
    ws: &crate::eras::shelley::ShelleyWitnessSet,
) -> Result<HashMap<[u8; 32], PlutusData>, LedgerError> {
    let Some(raw_datums) = witness_bytes
        .map(|wb| extract_witness_set_field_raw(wb, 4))
        .transpose()?
        .flatten()
    else {
        return Ok(collect_datum_map(ws));
    };
    collect_raw_datum_map(&raw_datums)
}

fn collect_raw_datum_map(raw_datums: &[u8]) -> Result<HashMap<[u8; 32], PlutusData>, LedgerError> {
    let mut dec = crate::cbor::Decoder::new(raw_datums);
    if dec.peek_major()? == 6 {
        let tag = dec.tag()?;
        if tag != 258 {
            return Err(LedgerError::CborTypeMismatch {
                expected: 4,
                actual: 6,
            });
        }
    }

    let mut map = HashMap::new();
    match dec.array_begin()? {
        Some(count) => {
            for _ in 0..count {
                collect_one_raw_datum(&mut dec, &mut map)?;
            }
        }
        None => {
            while !dec.is_break() {
                collect_one_raw_datum(&mut dec, &mut map)?;
            }
            dec.consume_break()?;
        }
    }
    if !dec.is_empty() {
        return Err(LedgerError::CborTrailingBytes(dec.remaining()));
    }
    Ok(map)
}

fn collect_one_raw_datum(
    dec: &mut crate::cbor::Decoder<'_>,
    map: &mut HashMap<[u8; 32], PlutusData>,
) -> Result<(), LedgerError> {
    let start = dec.position();
    let datum = PlutusData::decode_cbor(dec)?;
    let raw = dec.slice(start, dec.position())?;
    let hash = yggdrasil_crypto::blake2b::hash_bytes_256(raw).0;
    map.insert(hash, datum);
    Ok(())
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
                    purpose: format!(
                        "spend index {} out of range ({})",
                        redeemer.index,
                        sorted_inputs.len()
                    ),
                }
            })?;
            Ok(ScriptPurpose::Spending {
                tx_id: input.transaction_id,
                index: input.index as u64,
            })
        }
        1 => {
            // Minting: index into sorted policy IDs
            let policy = sorted_policy_ids
                .get(redeemer.index as usize)
                .ok_or_else(|| LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!(
                        "mint index {} out of range ({})",
                        redeemer.index,
                        sorted_policy_ids.len()
                    ),
                })?;
            Ok(ScriptPurpose::Minting { policy_id: *policy })
        }
        2 => {
            // Certifying: index into certificates
            let certificate = certificates.get(redeemer.index as usize).ok_or_else(|| {
                LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!(
                        "cert index {} out of range ({})",
                        redeemer.index,
                        certificates.len()
                    ),
                }
            })?;
            Ok(ScriptPurpose::Certifying {
                cert_index: redeemer.index,
                certificate: certificate.clone(),
            })
        }
        3 => {
            // Rewarding: index into sorted reward accounts
            let acct = sorted_reward_accounts
                .get(redeemer.index as usize)
                .ok_or_else(|| LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!(
                        "reward index {} out of range ({})",
                        redeemer.index,
                        sorted_reward_accounts.len()
                    ),
                })?;
            let reward_account =
                RewardAccount::from_bytes(acct).ok_or_else(|| LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!(
                        "reward account at index {} is not a valid reward address",
                        redeemer.index
                    ),
                })?;
            Ok(ScriptPurpose::Rewarding { reward_account })
        }
        4 => {
            let voter = sorted_voters.get(redeemer.index as usize).ok_or_else(|| {
                LedgerError::MissingRedeemer {
                    hash: [0; 28],
                    purpose: format!(
                        "voting index {} out of range ({})",
                        redeemer.index,
                        sorted_voters.len()
                    ),
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

/// Collects all Plutus scripts from a witness set and from UTxO entries
/// pointed to by both spending and reference inputs.
///
/// Upstream `getBabbageScriptsProvided` uses
/// `ins = referenceInputsTxBodyL ∪ inputsTxBodyL` — i.e. reference scripts
/// from *all* transaction inputs (spending + reference) are "provided".
///
/// Reference: `Cardano.Ledger.Babbage.UTxO.getBabbageScriptsProvided`.
pub(crate) fn collect_all_plutus_scripts(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
    utxo: &crate::utxo::MultiEraUtxo,
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
    spending_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
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
    // Collect Plutus reference scripts from spending + reference input UTxOs.
    // Upstream iterates `referenceInputsTxBodyL ∪ inputsTxBodyL`.
    let empty: &[crate::eras::shelley::ShelleyTxIn] = &[];
    let all_inputs = reference_inputs
        .unwrap_or(empty)
        .iter()
        .chain(spending_inputs.unwrap_or(empty).iter());
    for txin in all_inputs {
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

    // tx_hashes: all datum hashes from witness set, keyed by the original
    // datum CBOR bytes like upstream memoized `TxDats`.
    let tx_hashes: HashSet<[u8; 32]> = collect_datum_map_from_witness_bytes(Some(wb), &ws)?
        .into_keys()
        .collect();

    // Collect Plutus scripts (witness + reference + spending) to identify Plutus-locked inputs.
    let ref_txins: Vec<_> = reference_input_utxos
        .iter()
        .map(|(txin, _)| txin.clone())
        .collect();
    let plutus_scripts = collect_all_plutus_scripts(
        &ws,
        spending_utxo,
        if ref_txins.is_empty() {
            None
        } else {
            Some(&ref_txins)
        },
        Some(spending_inputs),
    );

    // input_hashes: datum hashes from Plutus-locked spending-input UTxOs.
    let mut input_hashes = HashSet::new();
    for txin in spending_inputs {
        if let Some(txout) = spending_utxo.get(txin) {
            // Only count if the input is Plutus-locked (not VKey/native).
            if let Some(script_hash) = spending_script_hash_from_txout(txout) {
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

    // Phase-1 check: input_hashes ⊆ tx_hashes — every Plutus-locked
    // spending input's datum hash must be present in the witness datum map.
    // Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.missingRequiredDatums`.
    for dh in &input_hashes {
        if !tx_hashes.contains(dh) {
            return Err(LedgerError::MissingRequiredDatums { hash: *dh });
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

/// Validates that Plutus-script-locked spending inputs have a datum.
///
/// In Alonzo and Babbage+, a spending input locked by a Plutus script
/// MUST have datum information attached (datum hash in Alonzo, datum hash
/// or inline datum in Babbage+).
///
/// This check is performed AFTER native script validation, so we can skip
/// inputs whose scripts were satisfied by native scripts.
///
/// CIP-0069 / Conway: PlutusV3 spending scripts do NOT require a datum on
/// the UTxO.  When `v3_script_hashes` is provided and a locking script hash
/// appears in that set, the datum check is skipped.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.missingRequiredDatums` and
/// `Cardano.Ledger.Alonzo.UTxO.getInputDataHashesTxBody` — Conway branch
/// filters out `lang >= PlutusV3`.
pub fn validate_unspendable_utxo_no_datum_hash(
    spending_utxo: &MultiEraUtxo,
    spending_inputs: &[crate::eras::shelley::ShelleyTxIn],
    native_satisfied: &HashSet<[u8; 28]>,
    v3_script_hashes: Option<&HashSet<[u8; 28]>>,
) -> Result<(), LedgerError> {
    for txin in spending_inputs {
        if let Some(txout) = spending_utxo.get(txin) {
            // Check if this output is script-locked
            if let Some(script_hash) = spending_script_hash_from_txout(txout) {
                // Skip if this script was satisfied by a native script
                if native_satisfied.contains(&script_hash) {
                    continue;
                }

                // CIP-0069: PlutusV3 spending scripts do not require a datum.
                if let Some(v3) = v3_script_hashes {
                    if v3.contains(&script_hash) {
                        continue;
                    }
                }

                // For Plutus-locked inputs, verify datum information is present.
                let has_datum = match &txout {
                    // Shelley/Mary: not Plutus-capable (no scripts)
                    MultiEraTxOut::Shelley(_) | MultiEraTxOut::Mary(_) => {
                        unreachable!(
                            "spending_script_hash_from_txout returned Some but era doesn't support scripts"
                        )
                    }
                    // Alonzo: requires datum_hash
                    MultiEraTxOut::Alonzo(out) => out.datum_hash.is_some(),
                    // Babbage/Conway: requires datum_option (Hash or Inline)
                    MultiEraTxOut::Babbage(out) => out.datum_option.is_some(),
                };

                if !has_datum {
                    return Err(LedgerError::UnspendableUTxONoDatumHash {
                        tx_id: txin.transaction_id,
                        index: txin.index as u64,
                    });
                }
            }
        }
    }

    Ok(())
}

/// Collects the set of PlutusV3 script hashes from witness-set scripts and
/// reference scripts.  Used by CIP-0069 to exempt V3 spending inputs from
/// the datum requirement.
pub fn collect_v3_script_hashes(
    ws: Option<&crate::eras::shelley::ShelleyWitnessSet>,
    utxo: Option<&MultiEraUtxo>,
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
) -> HashSet<[u8; 28]> {
    let mut v3 = HashSet::new();
    if let Some(ws) = ws {
        for s in &ws.plutus_v3_scripts {
            v3.insert(plutus_script_hash(PlutusVersion::V3, s));
        }
    }
    if let (Some(utxo), Some(ref_inputs)) = (utxo, reference_inputs) {
        for txin in ref_inputs {
            if let Some(txout) = utxo.get(txin) {
                if let Some(sr) = txout.script_ref() {
                    if matches!(sr.0, crate::plutus::Script::PlutusV3(_)) {
                        v3.insert(crate::witnesses::script_hash(&sr.0));
                    }
                }
            }
        }
    }
    v3
}

/// Validates that newly created outputs sent to Alonzo-era Plutus script
/// addresses include a datum hash.
///
/// In Alonzo, every output locked to a Plutus script address **must** carry
/// a `datum_hash`.  Without it the output is permanently unspendable.
/// Babbage+ relaxes this by allowing inline datums, so this check applies
/// only to Alonzo-era outputs.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` —
///   `validateOutputMissingDatumHashForScriptOutputs`.
pub fn validate_outputs_missing_datum_hash_alonzo(
    outputs: &[crate::eras::alonzo::AlonzoTxOut],
) -> Result<(), LedgerError> {
    for output in outputs {
        if let Some(addr) = Address::from_bytes(&output.address) {
            if let Some(cred) = addr.payment_credential() {
                if cred.is_script_hash() && output.datum_hash.is_none() {
                    return Err(LedgerError::MissingDatumHashOnScriptOutput {
                        address: output.address.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Phase-1 ExtraRedeemers check: every redeemer must target a purpose backed
/// by a Plutus script.
///
/// This is a standalone extraction of the UTXOW `hasExactSetOfRedeemers`
/// predicate.  Called from both block-apply and submitted-tx paths as a
/// Phase-1 UTXOW check, unconditionally before `is_valid` dispatching.
/// The check also runs redundantly inside [`validate_plutus_scripts`] for
/// defence-in-depth.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers`
pub fn validate_no_extra_redeemers(
    witness_bytes: Option<&[u8]>,
    spending_utxo: &MultiEraUtxo,
    sorted_inputs: &[crate::eras::shelley::ShelleyTxIn],
    sorted_policy_ids: &[[u8; 28]],
    certificates: &[DCert],
    sorted_reward_accounts: &[Vec<u8>],
    sorted_voters: &[Voter],
    proposal_procedures: &[ProposalProcedure],
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
) -> Result<(), LedgerError> {
    let wb = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };

    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;
    if ws.redeemers.is_empty() {
        return Ok(());
    }

    let plutus_scripts =
        collect_all_plutus_scripts(&ws, spending_utxo, reference_inputs, Some(sorted_inputs));

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

    Ok(())
}

/// Validates that every required Plutus-backed purpose has a matching
/// redeemer pointer in the witness set (Phase-1 UTXOW check).
///
/// This is the `MissingRedeemers` half of upstream `hasExactSetOfRedeemers`.
/// Paired with [`validate_no_extra_redeemers`] (the `ExtraRedeemers` half),
/// these two functions together replicate the full predicate.
///
/// Must be called unconditionally before the `is_valid` dispatch so that
/// `is_valid=false` transactions are also caught.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers`
pub fn validate_no_missing_redeemers(
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &std::collections::HashSet<[u8; 28]>,
    spending_utxo: &MultiEraUtxo,
    sorted_inputs: &[crate::eras::shelley::ShelleyTxIn],
    sorted_policy_ids: &[[u8; 28]],
    certificates: &[DCert],
    sorted_reward_accounts: &[Vec<u8>],
    sorted_voters: &[Voter],
    proposal_procedures: &[ProposalProcedure],
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
) -> Result<(), LedgerError> {
    let wb = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };

    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;

    let plutus_scripts =
        collect_all_plutus_scripts(&ws, spending_utxo, reference_inputs, Some(sorted_inputs));

    let actual_redeemer_ptrs: std::collections::HashSet<(u8, u64)> =
        ws.redeemers.iter().map(|r| (r.tag, r.index)).collect();

    for expected in collect_required_plutus_redeemers(
        required_script_hashes,
        &plutus_scripts,
        spending_utxo,
        sorted_inputs,
        sorted_policy_ids,
        certificates,
        sorted_reward_accounts,
        sorted_voters,
        proposal_procedures,
    ) {
        if !actual_redeemer_ptrs.contains(&(expected.tag, expected.index)) {
            return Err(LedgerError::MissingRedeemer {
                hash: expected.hash,
                purpose: expected.purpose,
            });
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
///
/// `cost_models` is the CDDL cost-model map from protocol parameters.
/// If a required Plutus script uses a language version whose cost model is
/// absent, a [`LedgerError::NoCostModel`] (Phase-1) error is returned
/// before any CEK evaluation takes place.
///
/// Reference: `Cardano.Ledger.Alonzo.Plutus.Evaluate.collectPlutusScriptsWithContext`.
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
    cost_models: Option<&crate::protocol_params::CostModels>,
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
        Some(sorted_inputs),
    );
    let datum_map = collect_datum_map_from_witness_bytes(Some(wb), &ws)?;
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

    // ── MissingRedeemer check (UTXOW Phase-1) ────────────────────────
    // Every required Plutus-backed purpose must have a corresponding
    // redeemer pointer. Reject missing redeemers before CEK evaluation.
    // Reference: Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers
    let actual_redeemer_ptrs: std::collections::HashSet<(u8, u64)> = ws
        .redeemers
        .iter()
        .map(|redeemer| (redeemer.tag, redeemer.index))
        .collect();
    for expected in collect_required_plutus_redeemers(
        required_script_hashes,
        &plutus_scripts,
        spending_utxo,
        sorted_inputs,
        sorted_policy_ids,
        certificates,
        sorted_reward_accounts,
        sorted_voters,
        proposal_procedures,
    ) {
        if !actual_redeemer_ptrs.contains(&(expected.tag, expected.index)) {
            return Err(LedgerError::MissingRedeemer {
                hash: expected.hash,
                purpose: expected.purpose,
            });
        }
    }

    // Build an augmented TxContext with the fields that require access to the
    // witness set and spending UTxO (inputs, certificates, witness_datums).
    // These are not available at the call-sites in state.rs so we populate
    // them here, where all the raw data is in scope.
    let resolved_inputs: Vec<(ShelleyTxIn, MultiEraTxOut)> = sorted_inputs
        .iter()
        .filter_map(|txin| {
            spending_utxo
                .get(txin)
                .map(|txout| (txin.clone(), txout.clone()))
        })
        .collect();
    let resolved_reference_inputs: Vec<(ShelleyTxIn, MultiEraTxOut)> = tx_ctx
        .reference_inputs
        .iter()
        .filter_map(|txin| {
            spending_utxo
                .get(txin)
                .map(|txout| (txin.clone(), txout.clone()))
        })
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
        protocol_version: tx_ctx.protocol_version,
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

    // ── NoCostModel check (Phase-1, upstream CollectErrors) ──────────
    // Every required Plutus script's language version must have a
    // corresponding cost-model entry in the protocol parameters.  If any
    // version is missing the tx is rejected *before* CEK evaluation.
    //
    // When `cost_models` is `None` (protocol parameters not configured or
    // cost_models field absent), the check is skipped (soft-skip for sync
    // without full protocol parameters).
    //
    // Reference: Cardano.Ledger.Alonzo.Plutus.Evaluate
    //            — collectPlutusScriptsWithContext / NoCostModel
    if let Some(cm) = cost_models {
        let mut required_versions: std::collections::HashSet<PlutusVersion> =
            std::collections::HashSet::new();
        for h in &plutus_required {
            if let Some((v, _)) = plutus_scripts.get(h.as_slice()) {
                required_versions.insert(*v);
            }
        }
        for version in &required_versions {
            let key = version.cost_model_key();
            if !cm.contains_key(&key) {
                return Err(LedgerError::NoCostModel { language: key });
            }
        }
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
            ScriptPurpose::Proposing { proposal, .. } => {
                proposal_script_hash_from_proposal(proposal)
            }
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
                    cost_model: cost_models
                        .and_then(|models| models.get(&version.cost_model_key()))
                        .cloned(),
                };

                evaluator
                    .evaluate(&eval_target, &augmented_tx_ctx)
                    .map_err(|err| annotate_plutus_evaluation_error(err, &eval_target, tx_ctx))?;
            }
        }
    }

    Ok(())
}

fn annotate_plutus_evaluation_error(
    err: LedgerError,
    eval: &PlutusScriptEval,
    tx_ctx: &TxContext,
) -> LedgerError {
    match err {
        LedgerError::PlutusScriptFailed { hash, reason } => LedgerError::PlutusScriptFailed {
            hash,
            reason: format!(
                "purpose {:?}, version {:?}, tx {}, ex_units(mem={}, steps={}): {}",
                eval.purpose,
                eval.version,
                hex_lower(&tx_ctx.tx_hash),
                eval.ex_units.mem,
                eval.ex_units.steps,
                reason
            ),
        },
        other => other,
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

struct RequiredPlutusRedeemer {
    tag: u8,
    index: u64,
    hash: [u8; 28],
    purpose: String,
}

fn collect_required_plutus_redeemers(
    required_script_hashes: &std::collections::HashSet<[u8; 28]>,
    plutus_scripts: &HashMap<[u8; 28], (PlutusVersion, Vec<u8>)>,
    spending_utxo: &MultiEraUtxo,
    sorted_inputs: &[crate::eras::shelley::ShelleyTxIn],
    sorted_policy_ids: &[[u8; 28]],
    certificates: &[DCert],
    sorted_reward_accounts: &[Vec<u8>],
    sorted_voters: &[Voter],
    proposal_procedures: &[ProposalProcedure],
) -> Vec<RequiredPlutusRedeemer> {
    let mut required = Vec::new();

    for (index, txin) in sorted_inputs.iter().enumerate() {
        if let Some(hash) = spending_utxo
            .get(txin)
            .and_then(spending_script_hash_from_txout)
            .filter(|hash| required_script_hashes.contains(hash))
            .filter(|hash| plutus_scripts.contains_key(hash))
        {
            required.push(RequiredPlutusRedeemer {
                tag: 0,
                index: index as u64,
                hash,
                purpose: format!("spending index {}", index),
            });
        }
    }

    for (index, policy_id) in sorted_policy_ids.iter().enumerate() {
        if required_script_hashes.contains(policy_id) && plutus_scripts.contains_key(policy_id) {
            required.push(RequiredPlutusRedeemer {
                tag: 1,
                index: index as u64,
                hash: *policy_id,
                purpose: format!("minting index {}", index),
            });
        }
    }

    for (index, certificate) in certificates.iter().enumerate() {
        if let Some(hash) = certifying_script_hash_from_cert(certificate)
            .filter(|hash| required_script_hashes.contains(hash))
            .filter(|hash| plutus_scripts.contains_key(hash))
        {
            required.push(RequiredPlutusRedeemer {
                tag: 2,
                index: index as u64,
                hash,
                purpose: format!("certificate index {}", index),
            });
        }
    }

    for (index, reward_account) in sorted_reward_accounts.iter().enumerate() {
        if let Some(reward_account) = RewardAccount::from_bytes(reward_account) {
            if let Some(hash) = credential_script_hash(&reward_account.credential)
                .filter(|hash| required_script_hashes.contains(hash))
                .filter(|hash| plutus_scripts.contains_key(hash))
            {
                required.push(RequiredPlutusRedeemer {
                    tag: 3,
                    index: index as u64,
                    hash,
                    purpose: format!("reward index {}", index),
                });
            }
        }
    }

    for (index, voter) in sorted_voters.iter().enumerate() {
        if let Some(hash) = voting_voter_script_hash(voter)
            .filter(|hash| required_script_hashes.contains(hash))
            .filter(|hash| plutus_scripts.contains_key(hash))
        {
            required.push(RequiredPlutusRedeemer {
                tag: 4,
                index: index as u64,
                hash,
                purpose: format!("voting index {}", index),
            });
        }
    }

    for (index, proposal) in proposal_procedures.iter().enumerate() {
        if let Some(hash) = proposal_script_hash_from_proposal(proposal)
            .filter(|hash| required_script_hashes.contains(hash))
            .filter(|hash| plutus_scripts.contains_key(hash))
        {
            required.push(RequiredPlutusRedeemer {
                tag: 5,
                index: index as u64,
                hash,
                purpose: format!("proposal index {}", index),
            });
        }
    }

    required
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
            credential_script_hash(cred).or(match drep {
                DRep::ScriptHash(hash) => Some(*hash),
                _ => None,
            })
        }
        DCert::PoolRegistration(_)
        | DCert::PoolRetirement(_, _)
        | DCert::GenesisDelegation(_, _, _)
        | DCert::MoveInstantaneousReward(_, _) => None,
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
    use crate::eras::alonzo::AlonzoTxOut;
    use crate::eras::babbage::{BabbageTxOut, DatumOption};
    use crate::eras::conway::{GovAction, ProposalProcedure, Voter};
    use crate::eras::mary::Value;
    use crate::eras::shelley::{ShelleyTxIn, ShelleyWitnessSet};
    use crate::types::{Address, DRep, EnterpriseAddress, RewardAccount, StakeCredential};
    use crate::utxo::{MultiEraTxOut, MultiEraUtxo};

    #[test]
    fn plutus_v1_script_hash_uses_tag_01() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
        // Verify it's Blake2b-224 of [0x01, 0x01, 0x02, 0x03]
        let expected = yggdrasil_crypto::blake2b::hash_bytes_224(&[0x01, 0x01, 0x02, 0x03]).0;
        assert_eq!(hash, expected);
    }

    #[test]
    fn plutus_v2_script_hash_uses_tag_02() {
        let script_bytes = vec![0xAA, 0xBB];
        let hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);
        let expected = yggdrasil_crypto::blake2b::hash_bytes_224(&[0x02, 0xAA, 0xBB]).0;
        assert_eq!(hash, expected);
    }

    #[test]
    fn plutus_v3_script_hash_uses_tag_03() {
        let script_bytes = vec![0xFF];
        let hash = plutus_script_hash(PlutusVersion::V3, &script_bytes);
        let expected = yggdrasil_crypto::blake2b::hash_bytes_224(&[0x03, 0xFF]).0;
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
    fn script_data_hash_uses_raw_witness_redeemers_and_datums_bytes() {
        // Witness set:
        // { 5: [_], 4: [_] } where both arrays use indefinite-length
        // encodings. Upstream hashes the memoized original bytes for
        // `Redeemers` and `TxDats`, not a canonical reconstruction.
        let witness = [0xa2, 0x05, 0x9f, 0xff, 0x04, 0x9f, 0x01, 0xff];

        let computed =
            compute_script_data_hash(Some(&witness), None, false, None, None, None, None)
                .expect("script data hash");

        let expected =
            yggdrasil_crypto::blake2b::hash_bytes_256(&[0x9f, 0xff, 0x9f, 0x01, 0xff, 0xa0]).0;
        let canonical_reencoded =
            yggdrasil_crypto::blake2b::hash_bytes_256(&[0x80, 0x81, 0x01, 0xa0]).0;

        assert_eq!(computed, expected);
        assert_ne!(computed, canonical_reencoded);
    }

    #[test]
    fn script_data_hash_omits_present_empty_datums_field() {
        // Witness set with one redeemer and field 4 present as an empty datum
        // array. Upstream checks decoded `TxDats` emptiness, so the `80` bytes
        // for the empty field are not part of the script integrity preimage.
        let raw_redeemers = [0x81, 0x84, 0x00, 0x00, 0x00, 0x82, 0x01, 0x02];
        let witness = [
            0xa2, 0x05, 0x81, 0x84, 0x00, 0x00, 0x00, 0x82, 0x01, 0x02, 0x04, 0x80,
        ];

        let computed =
            compute_script_data_hash(Some(&witness), None, false, None, None, None, None)
                .expect("script data hash");

        let mut expected_preimage = Vec::new();
        expected_preimage.extend_from_slice(&raw_redeemers);
        expected_preimage.push(0xa0);
        let expected = yggdrasil_crypto::blake2b::hash_bytes_256(&expected_preimage).0;

        let mut wrong_preimage = Vec::new();
        wrong_preimage.extend_from_slice(&raw_redeemers);
        wrong_preimage.push(0x80);
        wrong_preimage.push(0xa0);
        let wrong = yggdrasil_crypto::blake2b::hash_bytes_256(&wrong_preimage).0;

        assert_eq!(computed, expected);
        assert_ne!(computed, wrong);
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
    fn collect_datum_map_hashes_raw_witness_datum_bytes() {
        // Witness set { 4: [0] }, but the datum integer 0 is encoded in
        // non-canonical uint8 form `0x18 0x00`. Upstream `TxDats` hashes
        // those memoized bytes, not the canonical re-encoding `0x00`.
        let witness = [0xa1, 0x04, 0x81, 0x18, 0x00];
        let ws = ShelleyWitnessSet::from_cbor_bytes(&witness).expect("witness set");
        let datum = PlutusData::Integer(0.into());
        assert_eq!(ws.plutus_data, vec![datum.clone()]);

        let map = collect_datum_map_from_witness_bytes(Some(&witness), &ws).expect("datum map");
        let raw_hash = yggdrasil_crypto::blake2b::hash_bytes_256(&[0x18, 0x00]).0;
        let canonical_hash = yggdrasil_crypto::blake2b::hash_bytes_256(&datum.to_cbor_bytes()).0;

        assert_eq!(map[&raw_hash], datum);
        assert!(!map.contains_key(&canonical_hash));
    }

    #[test]
    fn resolve_spending_purpose() {
        let inputs = vec![
            crate::eras::shelley::ShelleyTxIn {
                transaction_id: [0xAA; 32],
                index: 0,
            },
            crate::eras::shelley::ShelleyTxIn {
                transaction_id: [0xBB; 32],
                index: 1,
            },
        ];
        let redeemer = Redeemer {
            tag: 0,
            index: 1,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits {
                mem: 100,
                steps: 200,
            },
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
            ex_units: ExUnits {
                mem: 100,
                steps: 200,
            },
        };
        let purpose =
            resolve_script_purpose(&redeemer, &[], &policies, &[], &[], &[], &[]).unwrap();
        assert!(matches!(purpose, ScriptPurpose::Minting { policy_id } if policy_id == [0xCC; 28]));
    }

    #[test]
    fn resolve_certifying_purpose_carries_certificate() {
        let certificate = DCert::AccountRegistration(StakeCredential::ScriptHash([0xDD; 28]));
        let redeemer = Redeemer {
            tag: 2,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits {
                mem: 100,
                steps: 200,
            },
        };

        let purpose = resolve_script_purpose(
            &redeemer,
            &[],
            &[],
            std::slice::from_ref(&certificate),
            &[],
            &[],
            &[],
        )
        .unwrap();

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
            ex_units: ExUnits {
                mem: 100,
                steps: 200,
            },
        };

        let purpose = resolve_script_purpose(
            &redeemer,
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&voter),
            &[],
        )
        .unwrap();

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
            ex_units: ExUnits {
                mem: 100,
                steps: 200,
            },
        };

        let purpose = resolve_script_purpose(
            &redeemer,
            &[],
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&proposal),
        )
        .unwrap();

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
            ex_units: ExUnits {
                mem: 100,
                steps: 200,
            },
        };
        let err = resolve_script_purpose(&redeemer, &[], &[], &[], &[], &[], &[]).unwrap_err();
        assert!(matches!(err, LedgerError::MissingRedeemer { .. }));
    }

    /// Mock evaluator that always succeeds.
    struct AlwaysSucceeds;

    impl PlutusEvaluator for AlwaysSucceeds {
        fn evaluate(
            &self,
            _eval: &PlutusScriptEval,
            _tx_ctx: &TxContext,
        ) -> Result<(), LedgerError> {
            Ok(())
        }
    }

    /// Mock evaluator that always fails.
    struct AlwaysFails;

    impl PlutusEvaluator for AlwaysFails {
        fn evaluate(
            &self,
            eval: &PlutusScriptEval,
            _tx_ctx: &TxContext,
        ) -> Result<(), LedgerError> {
            Err(LedgerError::PlutusScriptFailed {
                hash: eval.script_hash,
                reason: "always fails".to_string(),
            })
        }
    }

    struct ExpectDatum(pub PlutusData);

    impl PlutusEvaluator for ExpectDatum {
        fn evaluate(
            &self,
            eval: &PlutusScriptEval,
            _tx_ctx: &TxContext,
        ) -> Result<(), LedgerError> {
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
            None,
            Some(&wb),
            &required,
            &utxo,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &TxContext::default(),
            None,
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
                ex_units: ExUnits {
                    mem: 1000,
                    steps: 2000,
                },
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
            None,
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
                ex_units: ExUnits {
                    mem: 1000,
                    steps: 2000,
                },
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
            None,
        );
        assert!(matches!(
            result.unwrap_err(),
            LedgerError::PlutusScriptFailed { hash, .. } if hash == policy_hash
        ));
    }

    #[test]
    fn validate_minting_script_missing_redeemer_fails_before_evaluation() {
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
            redeemers: vec![],
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
            None,
        );

        assert!(matches!(
            result,
            Err(LedgerError::MissingRedeemer { hash, purpose })
                if hash == policy_hash && purpose == "minting index 0"
        ));
    }

    #[test]
    fn validate_plutus_scripts_empty_required_set_is_noop() {
        let required = std::collections::HashSet::new();
        let utxo = MultiEraUtxo::new();
        let result = validate_plutus_scripts(
            Some(&AlwaysSucceeds),
            None,
            &required,
            &utxo,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &TxContext::default(),
            None,
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
                ex_units: ExUnits {
                    mem: 1000,
                    steps: 2000,
                },
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
            None,
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
                ex_units: ExUnits {
                    mem: 1000,
                    steps: 2000,
                },
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
            None,
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
                ex_units: ExUnits {
                    mem: 1000,
                    steps: 2000,
                },
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
            None,
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
                ex_units: ExUnits {
                    mem: 1000,
                    steps: 2000,
                },
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
            None,
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
                ex_units: ExUnits {
                    mem: 1000,
                    steps: 2000,
                },
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
            None,
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
                ex_units: ExUnits {
                    mem: 1000,
                    steps: 2000,
                },
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
            None,
        );

        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Language view encoding parity tests
    // -----------------------------------------------------------------------

    /// Helper: produce language-views encoding for a witness set containing
    /// one dummy Plutus script of the given version, with protocol params
    /// carrying the given cost model values for that language.
    fn encode_views_for_single_lang(version: PlutusVersion, cm_values: &[i64]) -> Vec<u8> {
        let script = vec![0xDE, 0xAD]; // dummy script bytes
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: if version == PlutusVersion::V1 {
                vec![script.clone()]
            } else {
                vec![]
            },
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: if version == PlutusVersion::V2 {
                vec![script.clone()]
            } else {
                vec![]
            },
            plutus_v3_scripts: if version == PlutusVersion::V3 {
                vec![script]
            } else {
                vec![]
            },
        };
        let mut pp = ProtocolParameters::default();
        let mut cm_map = std::collections::BTreeMap::new();
        cm_map.insert(version.cost_model_key(), cm_values.to_vec());
        pp.cost_models = Some(cm_map);
        encode_language_views_for_script_data_hash(&ws, Some(&pp), None, None, None, None)
    }

    #[test]
    fn v1_language_view_key_is_byte_string() {
        let cm = vec![1i64, 2, 3];
        let bytes = encode_views_for_single_lang(PlutusVersion::V1, &cm);
        // Map(1) { bytes(1, [0x00]) => bytes(...) }
        // 0xa1 = map(1)
        assert_eq!(bytes[0], 0xa1);
        // Key: 0x41 0x00 = byte string of length 1 containing [0x00]
        assert_eq!(bytes[1], 0x41);
        assert_eq!(bytes[2], 0x00);
        // Value starts with a byte string header (major type 2)
        assert!(
            (bytes[3] & 0xe0) == 0x40,
            "V1 value should be a CBOR byte string"
        );
    }

    #[test]
    fn v2_language_view_key_is_unsigned_int() {
        let cm = vec![10i64, 20, 30];
        let bytes = encode_views_for_single_lang(PlutusVersion::V2, &cm);
        // Map(1) { unsigned(1) => array(...) }
        assert_eq!(bytes[0], 0xa1);
        // Key: 0x01 = CBOR unsigned integer 1
        assert_eq!(bytes[1], 0x01);
        // Value starts with an array header (major type 4), NOT byte string
        assert!(
            (bytes[2] & 0xe0) == 0x80,
            "V2 value should be a CBOR array, not byte string"
        );
    }

    #[test]
    fn v3_language_view_key_is_unsigned_int() {
        let cm = vec![100i64];
        let bytes = encode_views_for_single_lang(PlutusVersion::V3, &cm);
        // Map(1) { unsigned(2) => array(...) }
        assert_eq!(bytes[0], 0xa1);
        // Key: 0x02 = CBOR unsigned integer 2
        assert_eq!(bytes[1], 0x02);
        // Value starts with an array header (major type 4)
        assert!((bytes[2] & 0xe0) == 0x80, "V3 value should be a CBOR array");
    }

    #[test]
    fn v1_cost_model_uses_indefinite_array() {
        let cm = vec![5i64, 10];
        let bytes = encode_views_for_single_lang(PlutusVersion::V1, &cm);
        // After map header (0xa1) + key (0x41, 0x00) + byte-string header,
        // the byte-string payload should start with 0x9f (indefinite array)
        // and end with 0xff (break).
        // Map(1) = 0xa1, key = 0x41 0x00, value = bytes(N, payload)
        // Skip to byte-string payload:
        let value_start = 3; // after map header + key
        // Decode byte string header to find payload start
        let major = bytes[value_start] >> 5;
        assert_eq!(major, 2, "value should be byte string");
        // Additional info tells length
        let info = bytes[value_start] & 0x1f;
        let (payload_start, _payload_len) = match info {
            0..=23 => (value_start + 1, info as usize),
            24 => (value_start + 2, bytes[value_start + 1] as usize),
            _ => panic!("unexpected byte string length encoding"),
        };
        // First byte of payload should be indefinite array start
        assert_eq!(
            bytes[payload_start], 0x9f,
            "V1 cost model should use indefinite array"
        );
        // Last byte should be break
        assert_eq!(
            *bytes.last().unwrap(),
            0xff,
            "V1 cost model should end with break"
        );
    }

    #[test]
    fn v2_cost_model_uses_definite_array() {
        let cm = vec![7i64, 8, 9];
        let bytes = encode_views_for_single_lang(PlutusVersion::V2, &cm);
        // After map header (0xa1) + key (0x01), value starts directly
        let value_start = 2;
        let major = bytes[value_start] >> 5;
        assert_eq!(major, 4, "V2 cost model should be a definite array");
    }

    #[test]
    fn mixed_v1_v2_ordering_follows_shortlex() {
        // When both V1 and V2 are present, V2 key (0x01, 1 byte) should
        // come BEFORE V1 key (0x41 0x00, 2 bytes) per upstream shortLex.
        let script = vec![0xDE, 0xAD];
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![script.clone()],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![script],
            plutus_v3_scripts: vec![],
        };
        let mut pp = ProtocolParameters::default();
        let mut cm_map = std::collections::BTreeMap::new();
        cm_map.insert(0u8, vec![1i64, 2]);
        cm_map.insert(1u8, vec![3i64, 4]);
        pp.cost_models = Some(cm_map);

        let bytes =
            encode_language_views_for_script_data_hash(&ws, Some(&pp), None, None, None, None);
        // Map(2) = 0xa2
        assert_eq!(bytes[0], 0xa2);
        // First key: V2 = 0x01 (unsigned int 1, 1 byte) — shorter
        assert_eq!(bytes[1], 0x01);
        // Scan past V2 value (definite array) to find second key
        // Second key should be V1 = 0x41 0x00 (byte string, 2 bytes)
        let mut pos = 2;
        // Skip V2 value: definite array of 2 elements
        // 0x82 = array(2), then two integers
        assert_eq!(bytes[pos] >> 5, 4, "V2 value should be array");
        let arr_len = (bytes[pos] & 0x1f) as usize;
        pos += 1;
        for _ in 0..arr_len {
            // Skip each integer (could be 1 byte for small values)
            match bytes[pos] {
                0..=23 => pos += 1,
                24 => pos += 2,
                _ => panic!("test values should be small"),
            }
        }
        // Now we should be at V1 key
        assert_eq!(bytes[pos], 0x41, "second key should be V1 byte string");
        assert_eq!(bytes[pos + 1], 0x00, "second key payload should be 0x00");
    }

    #[test]
    fn collect_scripts_includes_spending_input_reference_scripts() {
        // Upstream `getBabbageScriptsProvided` uses
        // `referenceInputsTxBodyL ∪ inputsTxBodyL` — scripts from
        // spending-input UTxOs should be collected, not just reference inputs.
        use crate::eras::babbage::BabbageTxOut;
        use crate::eras::mary::Value;
        use crate::eras::shelley::ShelleyTxIn;
        use crate::plutus::{Script, ScriptRef};
        use crate::utxo::MultiEraTxOut;
        use crate::utxo::MultiEraUtxo;

        let script_bytes = vec![0xAA, 0xBB, 0xCC];
        let script_hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);

        let spending_input = ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        };

        let mut utxo = MultiEraUtxo::new();
        utxo.insert(
            spending_input.clone(),
            MultiEraTxOut::Babbage(BabbageTxOut {
                address: vec![0x61; 29],
                amount: Value::Coin(2_000_000),
                datum_option: None,
                script_ref: Some(ScriptRef(Script::PlutusV2(script_bytes.clone()))),
            }),
        );

        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };

        // Without spending_inputs — script should NOT be found
        let scripts_without = collect_all_plutus_scripts(&ws, &utxo, None, None);
        assert!(
            !scripts_without.contains_key(&script_hash),
            "should not find spending-input ref script when spending_inputs=None",
        );

        // With spending_inputs — script should be found
        let scripts_with = collect_all_plutus_scripts(&ws, &utxo, None, Some(&[spending_input]));
        assert!(
            scripts_with.contains_key(&script_hash),
            "should find spending-input reference script when spending_inputs provided",
        );
        let (version, bytes) = scripts_with.get(&script_hash).unwrap();
        assert_eq!(*version, PlutusVersion::V2);
        assert_eq!(bytes, &script_bytes);
    }

    /// Phase-1 check: Plutus-locked spending inputs whose datum hash is
    /// not in the witness datum map must be rejected with
    /// `MissingRequiredDatums` before script evaluation (not Phase-2).
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.missingRequiredDatums`.
    #[test]
    fn validate_supplemental_datums_rejects_missing_required_datum() {
        let script_bytes = vec![0x01, 0x02, 0x03];
        let script_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);

        let txin = ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        };
        let address = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::ScriptHash(script_hash),
        })
        .to_bytes();

        let mut utxo = MultiEraUtxo::new();
        let datum_hash = [0x99; 32];
        utxo.insert(
            txin.clone(),
            MultiEraTxOut::Alonzo(AlonzoTxOut {
                address,
                amount: Value::Coin(5_000_000),
                datum_hash: Some(datum_hash),
            }),
        );

        // Witness set with the PlutusV1 script but NO datum entries.
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![script_bytes],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let wb = ws.to_cbor_bytes();

        let result = validate_supplemental_datums(Some(&wb), &utxo, &[txin], &[], &[]);
        assert!(
            matches!(result, Err(LedgerError::MissingRequiredDatums { hash }) if hash == datum_hash),
            "must reject with MissingRequiredDatums when datum hash not in witness set, got: {:?}",
            result,
        );
    }
}
