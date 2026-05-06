//! Phase-1 transaction validation helpers.
//!
//! Pre-CEK rules that gate transactions on protocol parameters: tx size,
//! fee minima, min-UTxO, ExUnits, witnesses, native scripts, script
//! witness coverage, auxiliary data hash + metadata size, and network ID.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.Shelley.Rules.Utxow`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs)
//! /
//! [`Cardano.Ledger.Alonzo.Rules.Utxo`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxo.hs)
//! /
//! [`Cardano.Ledger.Alonzo.Rules.Utxow`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs).
//!
//! Each helper validates a specific phase-1 predicate; the per-era
//! `apply_*_block` methods on `LedgerState` (in `state.rs`) sequence them.
//!
//! Extracted from `state.rs` in R269 fifth slice as part of the strict 1:1
//! filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269e-state-phase1-validation-extraction.md`.

use super::PoolRelayAccessPoint;
use crate::eras::mary::MultiAsset;
use crate::types::{Relay, RewardAccount};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::{CborDecode, LedgerError};
use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Validates a pre-Alonzo transaction against protocol parameters.
///
/// Checks: transaction size limit, linear fee minimum, and min-UTxO per output.
pub(super) fn validate_pre_alonzo_tx(
    params: &crate::protocol_params::ProtocolParameters,
    tx_body_size: usize,
    declared_fee: u64,
    outputs: &[MultiEraTxOut],
) -> Result<(), LedgerError> {
    crate::fees::validate_tx_size(params, tx_body_size)?;
    crate::fees::validate_fee(params, tx_body_size, None, declared_fee)?;
    crate::min_utxo::validate_all_outputs_min_utxo(params, outputs)?;
    // Mary+ can carry multi-asset output values; enforce max_val_size
    // when the protocol parameter is set (no-op for Shelley/Allegra
    // where max_val_size is None).
    // Reference: `Cardano.Ledger.Mary.Rules.Utxo` — `validateOutputTooBigUTxO`.
    crate::min_utxo::validate_output_not_too_big(params, outputs)?;
    // Mary+ values are expected to be normalized before validation. Pre-Conway
    // raw decoders prune zero quantities via upstream `decodeWithPrunning`;
    // keep this as a defensive invariant for constructed values.
    crate::min_utxo::validate_no_zero_valued_multi_asset(outputs)?;
    crate::min_utxo::validate_output_boot_addr_attrs(outputs)?;
    Ok(())
}

/// Validates an Alonzo+ transaction against protocol parameters.
///
/// Checks: transaction size limit, fee minimum (including script costs
/// when `total_ex_units` is provided), min-UTxO per output, per-tx
/// execution-unit limits, mandatory collateral when redeemers are present,
/// and collateral sufficiency when collateral inputs are declared.
///
/// `has_redeemers` indicates whether the transaction's witness set
/// contains at least one redeemer (phase-2 scripts).  When `true`,
/// collateral inputs are mandatory per the upstream `feesOK` rule.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `feesOK`.
pub(super) fn validate_alonzo_plus_tx(
    params: &crate::protocol_params::ProtocolParameters,
    utxo: &MultiEraUtxo,
    tx_body_size: usize,
    declared_fee: u64,
    outputs: &[MultiEraTxOut],
    output_raw_sizes: Option<&[usize]>,
    collateral_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
    total_ex_units: Option<&crate::eras::alonzo::ExUnits>,
    collateral_return: Option<&MultiEraTxOut>,
    collateral_return_raw_size: Option<usize>,
    total_collateral: Option<u64>,
    has_redeemers: bool,
    ref_scripts_size: usize,
    enforce_collateral_input_limit: bool,
) -> Result<(), LedgerError> {
    crate::fees::validate_tx_size(params, tx_body_size)?;
    // Conway adds the tiered reference-script fee to the minimum.
    // For pre-Conway eras, ref_scripts_size is 0 so this is equivalent
    // to the standard `validate_fee`.
    crate::fees::validate_conway_fee(
        params,
        tx_body_size,
        total_ex_units,
        ref_scripts_size,
        declared_fee,
    )?;
    if let Some(eu) = total_ex_units {
        crate::fees::validate_tx_ex_units(params, eu)?;
    }
    // Upstream uses `allSizedOutputsTxBodyF` which includes collateral_return.
    // Reference: Cardano.Ledger.Babbage.TxBody — allSizedOutputsTxBodyF.
    let mut all_outputs_buf: Vec<MultiEraTxOut>;
    let all_outputs: &[MultiEraTxOut] = if let Some(cr) = collateral_return {
        all_outputs_buf = Vec::with_capacity(outputs.len() + 1);
        all_outputs_buf.extend_from_slice(outputs);
        all_outputs_buf.push(cr.clone());
        &all_outputs_buf
    } else {
        outputs
    };
    let mut all_output_sizes_buf: Vec<usize>;
    let all_output_raw_sizes = match (output_raw_sizes, collateral_return) {
        (Some(sizes), Some(_)) => {
            all_output_sizes_buf = Vec::with_capacity(sizes.len() + 1);
            all_output_sizes_buf.extend_from_slice(sizes);
            if let Some(size) = collateral_return_raw_size {
                all_output_sizes_buf.push(size);
                Some(all_output_sizes_buf.as_slice())
            } else {
                None
            }
        }
        (Some(sizes), None) => Some(sizes),
        _ => None,
    };
    if let Some(sizes) = all_output_raw_sizes {
        crate::min_utxo::validate_all_outputs_min_utxo_with_sizes(params, all_outputs, sizes)?;
    } else {
        crate::min_utxo::validate_all_outputs_min_utxo(params, all_outputs)?;
    }
    crate::min_utxo::validate_output_not_too_big(params, all_outputs)?;
    // Pre-Conway raw decoders prune zero quantities before this point; this is
    // a defensive invariant until Conway/Dijkstra strict decode is era-gated.
    crate::min_utxo::validate_no_zero_valued_multi_asset(all_outputs)?;
    crate::min_utxo::validate_output_boot_addr_attrs(all_outputs)?;

    // Babbage/Conway apply this as a standalone UTXO check, independent of
    // redeemer presence.
    // Reference: Cardano.Ledger.Babbage.Rules.Utxo — validateTooManyCollateralInputs.
    if enforce_collateral_input_limit {
        if let Some(collateral) = collateral_inputs {
            if let Some(max) = params.max_collateral_inputs {
                let count = collateral.len();
                if count > max as usize {
                    return Err(LedgerError::TooManyCollateralInputs { count, max });
                }
            }
        }
    }

    // When the transaction carries phase-2 scripts (redeemers ≠ ∅),
    // collateral is mandatory.
    // Reference: Cardano.Ledger.Alonzo.Rules.Utxo — feesOK Part 2.
    if has_redeemers {
        let has_collateral = collateral_inputs.is_some_and(|c| !c.is_empty());
        if !has_collateral {
            return Err(LedgerError::MissingCollateralForScripts);
        }
    }

    // Upstream `feesOK` only validates collateral when redeemers are present.
    // Reference: Cardano.Ledger.Alonzo.Rules.Utxo `feesOK` part 2.
    if has_redeemers {
        if let Some(collateral) = collateral_inputs {
            if !collateral.is_empty() {
                crate::collateral::validate_collateral(
                    params,
                    utxo,
                    collateral,
                    declared_fee,
                    collateral_return,
                    total_collateral,
                )?;
            }
        }
    }
    Ok(())
}

/// Validates that the total execution units across all transactions in a block
/// do not exceed `max_block_ex_units` from protocol parameters.
///
/// Implements the upstream Alonzo BBODY rule:
/// `totalExUnits(txs) <= maxBlockExUnits(pp)`.
///
/// Each transaction's redeemer ExUnits are summed from their witness sets.
/// When protocol parameters or `max_block_ex_units` are absent the check is
/// skipped (soft-skip semantics for pre-Alonzo eras or missing params).
pub(super) fn validate_block_ex_units(
    params: Option<&crate::protocol_params::ProtocolParameters>,
    witness_sets: &[Option<&[u8]>],
) -> Result<(), LedgerError> {
    let params = match params {
        Some(p) => p,
        None => return Ok(()),
    };
    let max = match &params.max_block_ex_units {
        Some(m) => m,
        None => return Ok(()),
    };
    let mut block_mem: u64 = 0;
    let mut block_steps: u64 = 0;
    for wb in witness_sets {
        if let Some(eu) = sum_redeemer_ex_units_from_bytes(*wb) {
            block_mem = block_mem.saturating_add(eu.mem);
            block_steps = block_steps.saturating_add(eu.steps);
        }
    }
    if block_mem > max.mem || block_steps > max.steps {
        return Err(LedgerError::BlockExUnitsExceeded {
            block_mem,
            block_steps,
            max_mem: max.mem,
            max_steps: max.steps,
        });
    }
    Ok(())
}

/// Sums execution units across all redeemers in a witness set.
pub(super) fn sum_redeemer_ex_units(
    witness_set: &crate::eras::shelley::ShelleyWitnessSet,
) -> Option<crate::eras::alonzo::ExUnits> {
    if witness_set.redeemers.is_empty() {
        return None;
    }
    let mut total = crate::eras::alonzo::ExUnits { mem: 0, steps: 0 };
    for redeemer in &witness_set.redeemers {
        total.mem = total.mem.saturating_add(redeemer.ex_units.mem);
        total.steps = total.steps.saturating_add(redeemer.ex_units.steps);
    }
    Some(total)
}

/// Validates each individual redeemer's ExUnits against `maxTxExUnits`.
///
/// Upstream: `validateExUnitsTooBigUTxO` checks `all pointWiseExUnits (<=)`.
pub(super) fn validate_per_redeemer_ex_units_from_witness_set(
    witness_set: &crate::eras::shelley::ShelleyWitnessSet,
    params: &crate::protocol_params::ProtocolParameters,
) -> Result<(), LedgerError> {
    if witness_set.redeemers.is_empty() {
        return Ok(());
    }
    crate::fees::validate_per_redeemer_ex_units(params, &witness_set.redeemers)
}

/// Validates each individual redeemer's ExUnits from raw witness bytes.
pub(super) fn validate_per_redeemer_ex_units_from_bytes(
    witness_bytes: Option<&[u8]>,
    params: &crate::protocol_params::ProtocolParameters,
) -> Result<(), LedgerError> {
    let wb = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };
    let ws = match crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb) {
        Ok(ws) => ws,
        Err(_) => return Ok(()), // malformed witness handled elsewhere
    };
    validate_per_redeemer_ex_units_from_witness_set(&ws, params)
}

/// Extracts total redeemer execution units from raw witness bytes.
///
/// Returns `None` when witness bytes are absent, malformed, or carry no
/// redeemers — matching the soft-skip semantics used elsewhere.
pub(super) fn sum_redeemer_ex_units_from_bytes(
    witness_bytes: Option<&[u8]>,
) -> Option<crate::eras::alonzo::ExUnits> {
    let wb = witness_bytes?;
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb).ok()?;
    sum_redeemer_ex_units(&ws)
}

/// Decodes a witness set from raw bytes and validates that all required
/// VKey hashes are covered.
///
/// `required` is the set of 28-byte Blake2b-224 hashes that must be
/// witnessed (spending inputs, certificates, withdrawals, required_signers).
///
/// `tx_body_hash` is the 32-byte Blake2b-256 hash of the serialized
/// transaction body — the message that each VKey witness must sign.
pub(super) fn validate_witnesses_if_present(
    witness_bytes: Option<&[u8]>,
    required: &HashSet<[u8; 28]>,
    tx_body_hash: &[u8; 32],
) -> Result<(), LedgerError> {
    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    // Merge VKey hashes + bootstrap witness address-root hashes into the
    // provided set.  Reference: `keyHashWitnessesTxWits` in
    // `Cardano.Ledger.Core` combines `witVKeyHash` and `bootstrapWitKeyHash`.
    let mut provided = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);
    for bw_hash in crate::witnesses::bootstrap_witness_key_hash_set(&ws.bootstrap_witnesses) {
        provided.insert(bw_hash);
    }
    crate::witnesses::validate_vkey_witnesses(required, &provided)?;
    crate::witnesses::verify_vkey_signatures(tx_body_hash, &ws.vkey_witnesses)?;
    crate::witnesses::verify_bootstrap_witnesses(tx_body_hash, &ws.bootstrap_witnesses)
}

/// Validates VKey witnesses given a typed witness set (no re-parse).
///
/// Used by submitted-tx paths where the witness set is already decoded.
pub(super) fn validate_witnesses_typed(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
    required: &HashSet<[u8; 28]>,
    tx_body_hash: &[u8; 32],
) -> Result<(), LedgerError> {
    // Merge VKey hashes + bootstrap witness address-root hashes.
    let mut provided = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);
    for bw_hash in crate::witnesses::bootstrap_witness_key_hash_set(&ws.bootstrap_witnesses) {
        provided.insert(bw_hash);
    }
    crate::witnesses::validate_vkey_witnesses(required, &provided)?;
    crate::witnesses::verify_vkey_signatures(tx_body_hash, &ws.vkey_witnesses)?;
    crate::witnesses::verify_bootstrap_witnesses(tx_body_hash, &ws.bootstrap_witnesses)
}

/// Validates native scripts referenced by script-hash credentials.
///
/// For each required script hash, looks up the native script in the
/// witness set, computes its hash, and evaluates it. Skips validation
/// when witness bytes are absent (backward compatibility).
pub(super) fn validate_native_scripts_if_present(
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &HashSet<[u8; 28]>,
    current_slot: u64,
) -> Result<HashSet<[u8; 28]>, LedgerError> {
    if required_script_hashes.is_empty() {
        return Ok(HashSet::new());
    }
    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(HashSet::new()),
    };
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    let vkey_hashes = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);
    let mut native_satisfied = HashSet::new();

    // Build a lookup from script hash → native script
    let mut script_map: std::collections::HashMap<[u8; 28], &crate::eras::allegra::NativeScript> =
        std::collections::HashMap::new();
    for ns in &ws.native_scripts {
        let h = crate::native_script::native_script_hash(ns);
        script_map.insert(h, ns);
    }

    let ctx = crate::native_script::NativeScriptContext {
        vkey_hashes: &vkey_hashes,
        current_slot,
    };

    for required_hash in required_script_hashes {
        if let Some(script) = script_map.get(required_hash) {
            if !crate::native_script::evaluate_native_script(script, &ctx) {
                return Err(LedgerError::NativeScriptFailed {
                    hash: *required_hash,
                });
            }
            native_satisfied.insert(*required_hash);
        }
        // When a required script is not in the native_scripts witness
        // list, it may be a Plutus script and is checked separately.
    }

    Ok(native_satisfied)
}

/// Ensures every required script hash is present in either native or Plutus
/// script witnesses (including reference scripts).
pub(super) fn validate_required_script_witnesses(
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &HashSet<[u8; 28]>,
    native_satisfied: &HashSet<[u8; 28]>,
    spending_utxo: &MultiEraUtxo,
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
    spending_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
) -> Result<(), LedgerError> {
    if required_script_hashes.is_empty() {
        return Ok(());
    }

    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => {
            let missing = required_script_hashes
                .iter()
                .find(|hash| !native_satisfied.contains(*hash))
                .copied();
            return match missing {
                Some(hash) => Err(LedgerError::MissingScriptWitness { hash }),
                None => Ok(()),
            };
        }
    };

    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    let plutus_scripts = crate::plutus_validation::collect_all_plutus_scripts(
        &ws,
        spending_utxo,
        reference_inputs,
        spending_inputs,
    );

    for required_hash in required_script_hashes {
        if native_satisfied.contains(required_hash) {
            continue;
        }
        if !plutus_scripts.contains_key(required_hash) {
            return Err(LedgerError::MissingScriptWitness {
                hash: *required_hash,
            });
        }
    }

    Ok(())
}

/// Collect the set of script hashes provided in the witness set (native
/// scripts + Plutus V1/V2/V3 scripts).
pub(super) fn provided_script_hashes_from_witnesses(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
) -> HashSet<[u8; 28]> {
    let mut provided = HashSet::new();
    for ns in &ws.native_scripts {
        provided.insert(crate::native_script::native_script_hash(ns));
    }
    for s in &ws.plutus_v1_scripts {
        provided.insert(crate::plutus_validation::plutus_script_hash(
            crate::plutus_validation::PlutusVersion::V1,
            s,
        ));
    }
    for s in &ws.plutus_v2_scripts {
        provided.insert(crate::plutus_validation::plutus_script_hash(
            crate::plutus_validation::PlutusVersion::V2,
            s,
        ));
    }
    for s in &ws.plutus_v3_scripts {
        provided.insert(crate::plutus_validation::plutus_script_hash(
            crate::plutus_validation::PlutusVersion::V3,
            s,
        ));
    }
    provided
}

/// Collects script hashes from reference input UTxOs (Babbage+).
///
/// For each reference input that resolves to a Babbage `BabbageTxOut` with a
/// `script_ref`, computes the script hash. Returns the set of script hashes
/// available via references.
///
/// Reference: upstream `getReferenceScripts` — `referenceScriptHashes`.
pub(super) fn collect_reference_script_hashes(
    utxo: &crate::utxo::MultiEraUtxo,
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
) -> HashSet<[u8; 28]> {
    let mut hashes = HashSet::new();
    if let Some(ref_inputs) = reference_inputs {
        for txin in ref_inputs {
            if let Some(txout) = utxo.get(txin) {
                if let Some(sr) = txout.script_ref() {
                    hashes.insert(crate::witnesses::script_hash(&sr.0));
                }
            }
        }
    }
    hashes
}

/// Validates that no scripts in the witness set are extraneous — every
/// provided script must be required by an input, certificate, withdrawal,
/// mint, vote, or proposal in the transaction.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.extraneousScriptWitnessesUTXOW`.
pub(super) fn validate_no_extraneous_script_witnesses(
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &HashSet<[u8; 28]>,
    reference_script_hashes: Option<&HashSet<[u8; 28]>>,
) -> Result<(), LedgerError> {
    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    let provided = provided_script_hashes_from_witnesses(&ws);
    // Upstream Babbage: `neededNonRefs = sNeeded \ sRefs`; extraneous = `sReceived \ neededNonRefs`.
    // For Shelley/Alonzo (no reference inputs), reference_script_hashes is None which
    // degenerates to `sReceived \ sNeeded`.
    let needed_non_refs: HashSet<[u8; 28]> = match reference_script_hashes {
        Some(refs) => required_script_hashes.difference(refs).copied().collect(),
        None => required_script_hashes.clone(),
    };
    for hash in &provided {
        if !needed_non_refs.contains(hash) {
            return Err(LedgerError::ExtraneousScriptWitness { hash: *hash });
        }
    }
    Ok(())
}

/// Typed variant for submitted-path where we already have a decoded
/// `ShelleyWitnessSet`.
pub(super) fn validate_no_extraneous_script_witnesses_typed(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
    required_script_hashes: &HashSet<[u8; 28]>,
    reference_script_hashes: Option<&HashSet<[u8; 28]>>,
) -> Result<(), LedgerError> {
    let provided = provided_script_hashes_from_witnesses(ws);
    let needed_non_refs: HashSet<[u8; 28]> = match reference_script_hashes {
        Some(refs) => required_script_hashes.difference(refs).copied().collect(),
        None => required_script_hashes.clone(),
    };
    for hash in &provided {
        if !needed_non_refs.contains(hash) {
            return Err(LedgerError::ExtraneousScriptWitness { hash: *hash });
        }
    }
    Ok(())
}

/// Validates that a transaction's auxiliary data hash matches its auxiliary
/// data content.
///
/// If the transaction body declares an `auxiliary_data_hash`, the
/// corresponding raw CBOR auxiliary data must be present and its
/// Blake2b-256 hash must match the declared value. If no hash is declared
/// the data must be absent.
///
/// When `protocol_version` is `Some((major, minor))` and the version is
/// greater than (2, 0), the metadata content is additionally validated:
/// all byte strings and text strings within transaction metadatum entries
/// must be ≤ 64 bytes (upstream `validMetadatum`).
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxow` — `validateMetadata`.
pub(super) fn validate_auxiliary_data(
    declared_hash: Option<&[u8; 32]>,
    auxiliary_data: Option<&[u8]>,
    protocol_version: Option<(u64, u64)>,
) -> Result<(), LedgerError> {
    match (declared_hash, auxiliary_data) {
        (Some(declared), Some(data)) => {
            let computed = yggdrasil_crypto::hash_bytes_256(data).0;
            if *declared != computed {
                return Err(LedgerError::AuxiliaryDataHashMismatch {
                    declared: *declared,
                    computed,
                });
            }
            // Upstream `SoftForks.validMetadata`: active when pv > ProtVer 2 0.
            if let Some((major, minor)) = protocol_version {
                if major > 2 || (major == 2 && minor > 0) {
                    validate_auxiliary_data_metadata_sizes(data)?;
                }
            }
            Ok(())
        }
        (Some(_), None) => Err(LedgerError::AuxiliaryDataMissing),
        // Upstream `validateMissingTxBodyMetadataHash`: if auxiliary data is
        // present in the transaction, the body MUST declare its hash.
        (None, Some(_)) => Err(LedgerError::MissingTxBodyMetadataHash),
        // Neither hash nor data — nothing to validate.
        (None, None) => Ok(()),
    }
}

/// Validates that all transaction metadatum values within auxiliary data
/// conform to CDDL size constraints: byte strings ≤ 64 and text strings
/// ≤ 64 bytes.
///
/// Auxiliary data CBOR layouts:
/// - Shelley: `metadata` (a map of uint → transaction_metadatum)
/// - Allegra/Mary: `[metadata, [scripts]]`
/// - Alonzo+: `#6.259({? 0 => metadata, ? 1 => [native_scripts], ...})`
///
/// Reference: `Cardano.Ledger.Metadata` — `validMetadatum`.
pub(super) fn validate_auxiliary_data_metadata_sizes(raw: &[u8]) -> Result<(), LedgerError> {
    use crate::cbor::Decoder;
    let mut dec = Decoder::new(raw);
    if dec.is_empty() {
        return Ok(());
    }
    let major = dec.peek_major().unwrap_or(0xff);
    match major {
        // Major type 5 (map): Shelley-style metadata — the whole thing is
        // `{ * uint => transaction_metadatum }`.
        5 => validate_metadata_map(&mut dec),
        // Major type 4 (array): Allegra/Mary `[metadata, [scripts]]`.
        4 => {
            let len = dec.array().map_err(|_| LedgerError::InvalidMetadata)?;
            if len == 0 {
                return Ok(());
            }
            // First element is the metadata map.
            validate_metadata_map(&mut dec)
            // Remaining elements (scripts) are not checked for metadata sizes.
        }
        // Major type 6 (tag): Alonzo+ `#6.259({...})`.
        6 => {
            let _tag = dec.tag().map_err(|_| LedgerError::InvalidMetadata)?;
            // Expect a map inside the tag.
            let count = dec.map().map_err(|_| LedgerError::InvalidMetadata)?;
            for _ in 0..count {
                let key = dec.unsigned().map_err(|_| LedgerError::InvalidMetadata)?;
                if key == 0 {
                    // Key 0 is the metadata map.
                    return validate_metadata_map(&mut dec);
                }
                // Skip non-metadata entries (scripts, etc.).
                dec.skip().map_err(|_| LedgerError::InvalidMetadata)?;
            }
            Ok(())
        }
        _ => {
            // Unknown auxiliary data format — skip validation rather than
            // reject valid blocks with future CBOR layouts.
            Ok(())
        }
    }
}

/// Validates entries in a `metadata = { * uint => transaction_metadatum }` map.
pub(super) fn validate_metadata_map(dec: &mut crate::cbor::Decoder<'_>) -> Result<(), LedgerError> {
    let count = dec.map().map_err(|_| LedgerError::InvalidMetadata)?;
    for _ in 0..count {
        // Key is uint — skip it.
        dec.skip().map_err(|_| LedgerError::InvalidMetadata)?;
        // Value is a transaction_metadatum — recursively validate.
        if !validate_metadatum(dec)? {
            return Err(LedgerError::InvalidMetadata);
        }
    }
    Ok(())
}

/// Recursively validates a single `transaction_metadatum` CBOR item.
///
/// Returns `Ok(true)` when the metadatum and all sub-items are valid,
/// `Ok(false)` when a bytes/text item exceeds 64 bytes.
///
/// ```text
/// transaction_metadatum =
///     int
///   / bytes .size (0..64)
///   / text .size (0..64)
///   / [ * transaction_metadatum ]
///   / { * transaction_metadatum => transaction_metadatum }
/// ```
pub(super) fn validate_metadatum(dec: &mut crate::cbor::Decoder<'_>) -> Result<bool, LedgerError> {
    let major = dec.peek_major().map_err(|_| LedgerError::InvalidMetadata)?;
    match major {
        // Major type 0 (unsigned) or 1 (negative): integer — always valid.
        0 | 1 => {
            dec.skip().map_err(|_| LedgerError::InvalidMetadata)?;
            Ok(true)
        }
        // Major type 2 (bytes): must be ≤ 64 bytes.
        2 => {
            let bs = dec.bytes().map_err(|_| LedgerError::InvalidMetadata)?;
            Ok(bs.len() <= 64)
        }
        // Major type 3 (text): UTF-8 bytes must be ≤ 64.
        3 => {
            let s = dec.text().map_err(|_| LedgerError::InvalidMetadata)?;
            Ok(s.len() <= 64)
        }
        // Major type 4 (array): recurse into elements.
        4 => {
            let count = dec.array().map_err(|_| LedgerError::InvalidMetadata)?;
            for _ in 0..count {
                if !validate_metadatum(dec)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        // Major type 5 (map): recurse into keys and values.
        5 => {
            let count = dec.map().map_err(|_| LedgerError::InvalidMetadata)?;
            for _ in 0..count {
                if !validate_metadatum(dec)? {
                    return Ok(false);
                }
                if !validate_metadatum(dec)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        // Tags or other types — skip (not standard metadatum but tolerate).
        _ => {
            dec.skip().map_err(|_| LedgerError::InvalidMetadata)?;
            Ok(true)
        }
    }
}

/// Extracts the network ID from raw Shelley-family address bytes.
///
/// Returns `None` for Byron addresses (header type 8) and reserved types
/// (9–13), and `Some(net)` for Shelley types 0–7 (base/pointer/enterprise)
/// and 14–15 (reward key/script) where `net = header & 0x0f`.
pub(super) fn shelley_address_network_id(addr_bytes: &[u8]) -> Option<u8> {
    let header = *addr_bytes.first()?;
    let addr_type = (header >> 4) & 0x0f;
    // Shelley address types: 0–7 (base/pointer/enterprise), 14–15 (reward).
    // Byron type 8 and reserved 9–13 do not carry a Shelley network ID.
    match addr_type {
        0..=7 | 14 | 15 => Some(header & 0x0f),
        _ => None,
    }
}

/// Validates that all transaction output addresses have the expected network
/// ID.
///
/// Byron addresses are exempt since they do not carry a network ID in the
/// Shelley sense.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetwork`.
pub(super) fn validate_output_network_ids(
    expected: u8,
    outputs: &[MultiEraTxOut],
) -> Result<(), LedgerError> {
    for output in outputs {
        let addr_bytes = output.address();
        if let Some(net) = shelley_address_network_id(addr_bytes) {
            if net != expected {
                return Err(LedgerError::WrongNetwork {
                    expected,
                    found: net,
                });
            }
        }
    }
    Ok(())
}

/// Validates that all withdrawal reward accounts have the expected network
/// ID.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetworkWithdrawal`.
pub(super) fn validate_withdrawal_network_ids<'a, I>(
    expected: u8,
    withdrawals: I,
) -> Result<(), LedgerError>
where
    I: IntoIterator<Item = (&'a RewardAccount, &'a u64)>,
{
    for (acct, _) in withdrawals {
        if acct.network != expected {
            return Err(LedgerError::WrongNetworkWithdrawal {
                expected,
                found: acct.network,
            });
        }
    }
    Ok(())
}

/// Validates that the `network_id` field declared in the transaction body
/// (Alonzo+) matches the expected network.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `WrongNetworkInTxBody`.
pub(super) fn validate_tx_body_network_id(
    expected: u8,
    declared: Option<u8>,
) -> Result<(), LedgerError> {
    if let Some(net) = declared {
        if net != expected {
            return Err(LedgerError::WrongNetworkInTxBody {
                expected,
                found: net,
            });
        }
    }
    Ok(())
}

pub(super) fn accumulate_multi_asset(total: &mut MultiAsset, assets: &MultiAsset) {
    for (policy, entries) in assets {
        let policy_total = total.entry(*policy).or_default();
        for (asset_name, quantity) in entries {
            let entry = policy_total.entry(asset_name.clone()).or_default();
            *entry = entry.saturating_add(*quantity);
        }
    }
}

pub(super) fn relay_access_points_from_relays(relays: &[Relay]) -> Vec<PoolRelayAccessPoint> {
    let mut access_points = Vec::new();

    for relay in relays {
        match relay {
            Relay::SingleHostAddr(Some(port), ipv4, ipv6) => {
                if let Some(ipv4) = ipv4 {
                    access_points.push(PoolRelayAccessPoint {
                        address: Ipv4Addr::from(*ipv4).to_string(),
                        port: *port,
                    });
                }
                if let Some(ipv6) = ipv6 {
                    access_points.push(PoolRelayAccessPoint {
                        address: Ipv6Addr::from(*ipv6).to_string(),
                        port: *port,
                    });
                }
            }
            Relay::SingleHostName(Some(port), domain) => {
                access_points.push(PoolRelayAccessPoint {
                    address: domain.clone(),
                    port: *port,
                });
            }
            Relay::SingleHostAddr(None, _, _)
            | Relay::SingleHostName(None, _)
            | Relay::MultiHostName(_) => {}
        }
    }

    access_points
}
