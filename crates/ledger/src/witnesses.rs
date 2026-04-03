//! Witness sufficiency checks.
//!
//! Validates that a transaction carries the required VKey witnesses
//! for all spending inputs, certificate actions, and withdrawals.
//!
//! Reference:
//! `Cardano.Ledger.Shelley.Rules.Utxow` — `validateNeededWitnesses`

use std::collections::HashSet;

use crate::error::LedgerError;
use crate::CborDecode;

/// Validates that every required VKey hash is covered by a witness.
///
/// `required_hashes` is the set of 28-byte Blake2b-224 hashes of
/// verification keys that must sign the transaction (derived from
/// spending input addresses, certificate signers, and withdrawal
/// reward accounts).
///
/// `witness_vkey_hashes` is the set of VKey hashes actually present
/// in the transaction's witness set (computed by the caller as
/// Blake2b-224 of each `ShelleyVkeyWitness.vkey`).
///
/// Returns `Ok(())` when every required hash is present, or the first
/// missing hash.
pub fn validate_vkey_witnesses(
    required_hashes: &HashSet<[u8; 28]>,
    witness_vkey_hashes: &HashSet<[u8; 28]>,
) -> Result<(), LedgerError> {
    for required in required_hashes {
        if !witness_vkey_hashes.contains(required) {
            return Err(LedgerError::MissingVKeyWitness { hash: *required });
        }
    }
    Ok(())
}

/// Computes the Blake2b-224 hash of a 32-byte Ed25519 verification key.
///
/// This is the standard credential hash used in Shelley+ addresses and
/// certificate validation.
pub fn vkey_hash(vkey: &[u8; 32]) -> [u8; 28] {
    yggdrasil_crypto::blake2b::hash_bytes_224(vkey).0
}

/// Extracts the set of VKey hashes from a slice of VKey witnesses.
pub fn witness_vkey_hash_set(
    witnesses: &[crate::eras::shelley::ShelleyVkeyWitness],
) -> HashSet<[u8; 28]> {
    witnesses.iter().map(|w| vkey_hash(&w.vkey)).collect()
}

/// Verifies Ed25519 signatures in VKey witnesses against the transaction body hash.
///
/// Each VKey witness carries a 32-byte verification key and a 64-byte Ed25519
/// signature. The signed message is the 32-byte Blake2b-256 hash of the
/// serialized transaction body (i.e. the `TxId` bytes).
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxow` — signature verification.
pub fn verify_vkey_signatures(
    tx_body_hash: &[u8; 32],
    witnesses: &[crate::eras::shelley::ShelleyVkeyWitness],
) -> Result<(), LedgerError> {
    for w in witnesses {
        let vk = yggdrasil_crypto::ed25519::VerificationKey::from_bytes(w.vkey);
        let sig = yggdrasil_crypto::ed25519::Signature::from_bytes(w.signature);
        vk.verify(tx_body_hash, &sig).map_err(|_| {
            LedgerError::InvalidVKeyWitnessSignature { hash: vkey_hash(&w.vkey) }
        })?;
    }
    Ok(())
}

/// Verifies bootstrap witness signatures and attributes against the tx body hash.
pub fn verify_bootstrap_witnesses(
    tx_body_hash: &[u8; 32],
    witnesses: &[crate::eras::shelley::BootstrapWitness],
) -> Result<(), LedgerError> {
    for witness in witnesses {
        let mut dec = crate::cbor::Decoder::new(&witness.attributes);
        let map_len = dec.map().map_err(|_| {
            LedgerError::InvalidBootstrapWitnessAttributes(witness.attributes.clone())
        })?;
        for _ in 0..map_len {
            dec.skip().map_err(|_| {
                LedgerError::InvalidBootstrapWitnessAttributes(witness.attributes.clone())
            })?;
            dec.skip().map_err(|_| {
                LedgerError::InvalidBootstrapWitnessAttributes(witness.attributes.clone())
            })?;
        }
        if dec.position() != witness.attributes.len() {
            return Err(LedgerError::InvalidBootstrapWitnessAttributes(
                witness.attributes.clone(),
            ));
        }

        let vk = yggdrasil_crypto::ed25519::VerificationKey::from_bytes(witness.public_key);
        let sig = yggdrasil_crypto::ed25519::Signature::from_bytes(witness.signature);
        vk.verify(tx_body_hash, &sig).map_err(|_| {
            LedgerError::InvalidBootstrapWitnessSignature {
                hash: vkey_hash(&witness.public_key),
            }
        })?;
    }
    Ok(())
}

/// Reconstructs the Byron address root from a bootstrap witness.
///
/// The address root is `Blake2b_224(SHA3_256(prefix ++ vkey(32) ++ chain_code(32) ++ attributes))`
/// where `prefix = [0x83, 0x00, 0x82, 0x00, 0x58, 0x40]` encodes:
///   - `0x83` → CBOR array of length 3
///   - `0x00` → address type (public key spending, tag 0)
///   - `0x82` → CBOR array of length 2 (spending data)
///   - `0x00` → spending data type (VerKeyASD, tag 0)
///   - `0x58, 0x40` → CBOR byte string of length 64 (XPub = vkey ++ chain_code)
///
/// Reference: `Cardano.Ledger.Keys.Bootstrap` — `bootstrapWitKeyHash`.
pub fn bootstrap_witness_key_hash(
    witness: &crate::eras::shelley::BootstrapWitness,
) -> [u8; 28] {
    const PREFIX: &[u8] = &[0x83, 0x00, 0x82, 0x00, 0x58, 0x40];
    let mut bytes = Vec::with_capacity(PREFIX.len() + 32 + witness.chain_code.len() + witness.attributes.len());
    bytes.extend_from_slice(PREFIX);
    bytes.extend_from_slice(&witness.public_key);
    bytes.extend_from_slice(&witness.chain_code);
    bytes.extend_from_slice(&witness.attributes);
    let sha3 = yggdrasil_crypto::sha3_256(&bytes);
    yggdrasil_crypto::hash_bytes_224(&sha3.0).0
}

/// Extracts the set of address-root key hashes from bootstrap witnesses.
///
/// This is the bootstrap counterpart of `witness_vkey_hash_set` and
/// mirrors upstream `Set.map bootstrapWitKeyHash` in `keyHashWitnessesTxWits`.
pub fn bootstrap_witness_key_hash_set(
    witnesses: &[crate::eras::shelley::BootstrapWitness],
) -> HashSet<[u8; 28]> {
    witnesses.iter().map(bootstrap_witness_key_hash).collect()
}

/// Collects the genesis key hashes required to authorize a PPUP update proposal.
///
/// Upstream `propWits` in `Cardano.Ledger.Shelley.UTxO` — restricts the
/// `genDelegs` key-set to proposers that appear in the update map and
/// inserts those genesis key hashes into the required witness set.
pub fn required_vkey_hashes_from_ppup(
    update: &crate::eras::shelley::ShelleyUpdate,
    gen_delegs: &std::collections::BTreeMap<[u8; 28], crate::state::GenesisDelegationState>,
    out: &mut HashSet<[u8; 28]>,
) {
    for proposer_hash in update.proposed_protocol_parameter_updates.keys() {
        if gen_delegs.contains_key(proposer_hash) {
            out.insert(*proposer_hash);
        }
    }
}

/// Collects the VKey hashes required to authorize a certificate.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — `witsVKeyNeeded`.
pub fn required_vkey_hashes_from_cert(
    cert: &crate::types::DCert,
    out: &mut HashSet<[u8; 28]>,
) {
    use crate::types::DCert;
    match cert {
        // Shelley: unregistration requires the credential key
        DCert::AccountUnregistration(cred)
        | DCert::AccountUnregistrationDeposit(cred, _) => {
            if let crate::types::StakeCredential::AddrKeyHash(h) = cred {
                out.insert(*h);
            }
        }
        // Delegation requires the delegator credential key
        DCert::DelegationToStakePool(cred, _)
        | DCert::DelegationToDrep(cred, _)
        | DCert::DelegationToStakePoolAndDrep(cred, _, _) => {
            if let crate::types::StakeCredential::AddrKeyHash(h) = cred {
                out.insert(*h);
            }
        }
        // Registration + delegation requires the credential key
        DCert::AccountRegistrationDelegationToStakePool(cred, _, _)
        | DCert::AccountRegistrationDelegationToDrep(cred, _, _)
        | DCert::AccountRegistrationDelegationToStakePoolAndDrep(cred, _, _, _) => {
            if let crate::types::StakeCredential::AddrKeyHash(h) = cred {
                out.insert(*h);
            }
        }
        // Pool registration requires the operator key
        DCert::PoolRegistration(params) => {
            out.insert(params.operator);
        }
        // Pool retirement requires the operator key
        DCert::PoolRetirement(operator, _) => {
            out.insert(*operator);
        }
        // Genesis delegation requires the genesis key hash
        DCert::GenesisDelegation(genesis_hash, _, _) => {
            out.insert(*genesis_hash);
        }
        // Committee authorization requires the cold credential key
        DCert::CommitteeAuthorization(cold_cred, _) => {
            if let crate::types::StakeCredential::AddrKeyHash(h) = cold_cred {
                out.insert(*h);
            }
        }
        // Committee resignation requires the cold credential key
        DCert::CommitteeResignation(cold_cred, _) => {
            if let crate::types::StakeCredential::AddrKeyHash(h) = cold_cred {
                out.insert(*h);
            }
        }
        // DRep registration requires the credential key
        DCert::DrepRegistration(cred, _, _)
        | DCert::DrepUnregistration(cred, _)
        | DCert::DrepUpdate(cred, _) => {
            if let crate::types::StakeCredential::AddrKeyHash(h) = cred {
                out.insert(*h);
            }
        }
        // Simple registration does not require a witness in Shelley
        DCert::AccountRegistration(_)
        | DCert::AccountRegistrationDeposit(_, _) => {}
        // MIR certs are signed by genesis delegates (not validated here)
        DCert::MoveInstantaneousReward(_, _) => {}
    }
}

/// Collects VKey hashes required by withdrawal reward accounts.
pub fn required_vkey_hashes_from_withdrawals(
    withdrawals: &std::collections::BTreeMap<crate::types::RewardAccount, u64>,
    out: &mut HashSet<[u8; 28]>,
) {
    for ra in withdrawals.keys() {
        if let crate::types::StakeCredential::AddrKeyHash(h) = &ra.credential {
            out.insert(*h);
        }
    }
}

/// Collects VKey hashes from spending input payment credentials.
///
/// For each input, looks up the corresponding UTxO output, parses the
/// address, and if the payment credential is a key hash, adds it to `out`.
/// Byron bootstrap addresses contribute their address root (28-byte hash)
/// instead of a credential key hash.
///
/// Reference: `Cardano.Ledger.Shelley.UTxO` — `getShelleyWitsVKeyNeededNoGov`
/// handles `AddrBootstrap bootAddr -> Set.insert (asWitness (bootstrapKeyHash bootAddr)) ans`.
pub fn required_vkey_hashes_from_inputs_shelley(
    inputs: &[crate::eras::shelley::ShelleyTxIn],
    utxo: &crate::eras::shelley::ShelleyUtxo,
    out: &mut HashSet<[u8; 28]>,
) {
    for txin in inputs {
        if let Some(txout) = utxo.get(txin) {
            if let Some(addr) = crate::types::Address::from_bytes(&txout.address) {
                if let Some(crate::types::StakeCredential::AddrKeyHash(h)) = addr.payment_credential() {
                    out.insert(*h);
                }
                // Byron addresses: extract the address root
                if let crate::types::Address::Byron(raw) = &addr {
                    if let Some(root) = crate::types::byron_address_root(raw) {
                        out.insert(root);
                    }
                }
            }
        }
    }
}

/// Collects VKey hashes from spending input payment credentials (multi-era).
/// Byron bootstrap addresses contribute their address root.
///
/// Reference: `Cardano.Ledger.Shelley.UTxO` — `getShelleyWitsVKeyNeededNoGov`.
pub fn required_vkey_hashes_from_inputs_multi_era(
    inputs: &[crate::eras::shelley::ShelleyTxIn],
    utxo: &crate::utxo::MultiEraUtxo,
    out: &mut HashSet<[u8; 28]>,
) {
    for txin in inputs {
        if let Some(txout) = utxo.get(txin) {
            if let Some(addr) = crate::types::Address::from_bytes(txout.address()) {
                if let Some(crate::types::StakeCredential::AddrKeyHash(h)) = addr.payment_credential() {
                    out.insert(*h);
                }
                // Byron addresses: extract the address root
                if let crate::types::Address::Byron(raw) = &addr {
                    if let Some(root) = crate::types::byron_address_root(raw) {
                        out.insert(root);
                    }
                }
            }
        }
    }
}

/// Collects required script hashes from spending input payment credentials (Shelley UTxO).
pub fn required_script_hashes_from_inputs_shelley(
    inputs: &[crate::eras::shelley::ShelleyTxIn],
    utxo: &crate::eras::shelley::ShelleyUtxo,
    out: &mut HashSet<[u8; 28]>,
) {
    for txin in inputs {
        if let Some(txout) = utxo.get(txin) {
            if let Some(addr) = crate::types::Address::from_bytes(&txout.address) {
                if let Some(crate::types::StakeCredential::ScriptHash(h)) = addr.payment_credential() {
                    out.insert(*h);
                }
            }
        }
    }
}

/// Collects required script hashes from spending input payment credentials (multi-era).
pub fn required_script_hashes_from_inputs_multi_era(
    inputs: &[crate::eras::shelley::ShelleyTxIn],
    utxo: &crate::utxo::MultiEraUtxo,
    out: &mut HashSet<[u8; 28]>,
) {
    for txin in inputs {
        if let Some(txout) = utxo.get(txin) {
            if let Some(addr) = crate::types::Address::from_bytes(txout.address()) {
                if let Some(crate::types::StakeCredential::ScriptHash(h)) = addr.payment_credential() {
                    out.insert(*h);
                }
            }
        }
    }
}

/// Collects required script hashes from certificate credentials.
pub fn required_script_hashes_from_cert(
    cert: &crate::types::DCert,
    out: &mut HashSet<[u8; 28]>,
) {
    use crate::types::DCert;
    match cert {
        DCert::AccountUnregistration(cred)
        | DCert::AccountUnregistrationDeposit(cred, _)
        | DCert::DelegationToStakePool(cred, _)
        | DCert::DelegationToDrep(cred, _)
        | DCert::DelegationToStakePoolAndDrep(cred, _, _)
        | DCert::AccountRegistrationDelegationToStakePool(cred, _, _)
        | DCert::AccountRegistrationDelegationToDrep(cred, _, _)
        | DCert::AccountRegistrationDelegationToStakePoolAndDrep(cred, _, _, _)
        | DCert::DrepRegistration(cred, _, _)
        | DCert::DrepUnregistration(cred, _)
        | DCert::DrepUpdate(cred, _) => {
            if let crate::types::StakeCredential::ScriptHash(h) = cred {
                out.insert(*h);
            }
        }
        DCert::CommitteeAuthorization(cold_cred, _)
        | DCert::CommitteeResignation(cold_cred, _) => {
            if let crate::types::StakeCredential::ScriptHash(h) = cold_cred {
                out.insert(*h);
            }
        }
        _ => {}
    }
}

/// Collects required script hashes from withdrawal reward accounts.
pub fn required_script_hashes_from_withdrawals(
    withdrawals: &std::collections::BTreeMap<crate::types::RewardAccount, u64>,
    out: &mut HashSet<[u8; 28]>,
) {
    for ra in withdrawals.keys() {
        if let crate::types::StakeCredential::ScriptHash(h) = &ra.credential {
            out.insert(*h);
        }
    }
}

/// Collects required script hashes from mint policy IDs.
pub fn required_script_hashes_from_mint(
    mint: &crate::eras::mary::MintAsset,
    out: &mut HashSet<[u8; 28]>,
) {
    for policy_id in mint.keys() {
        out.insert(*policy_id);
    }
}

/// Collects required VKey hashes from Conway voting procedures.
pub fn required_vkey_hashes_from_voting_procedures(
    voting_procedures: &crate::eras::conway::VotingProcedures,
    out: &mut HashSet<[u8; 28]>,
) {
    for voter in voting_procedures.procedures.keys() {
        match voter {
            crate::eras::conway::Voter::CommitteeKeyHash(hash)
            | crate::eras::conway::Voter::DRepKeyHash(hash)
            | crate::eras::conway::Voter::StakePool(hash) => {
                out.insert(*hash);
            }
            crate::eras::conway::Voter::CommitteeScript(_)
            | crate::eras::conway::Voter::DRepScript(_) => {}
        }
    }
}

/// Collects required script hashes from Conway voting procedures.
pub fn required_script_hashes_from_voting_procedures(
    voting_procedures: &crate::eras::conway::VotingProcedures,
    out: &mut HashSet<[u8; 28]>,
) {
    for voter in voting_procedures.procedures.keys() {
        match voter {
            crate::eras::conway::Voter::CommitteeScript(hash)
            | crate::eras::conway::Voter::DRepScript(hash) => {
                out.insert(*hash);
            }
            crate::eras::conway::Voter::CommitteeKeyHash(_)
            | crate::eras::conway::Voter::DRepKeyHash(_)
            | crate::eras::conway::Voter::StakePool(_) => {}
        }
    }
}

/// Collects required script hashes from Conway proposal procedures.
pub fn required_script_hashes_from_proposal_procedures(
    proposal_procedures: &[crate::eras::conway::ProposalProcedure],
    out: &mut HashSet<[u8; 28]>,
) {
    use crate::eras::conway::GovAction;

    for proposal in proposal_procedures {
        match &proposal.gov_action {
            GovAction::ParameterChange {
                guardrails_script_hash,
                ..
            }
            | GovAction::TreasuryWithdrawals {
                guardrails_script_hash,
                ..
            } => {
                if let Some(hash) = guardrails_script_hash {
                    out.insert(*hash);
                }
            }
            GovAction::NewConstitution { constitution, .. } => {
                if let Some(hash) = constitution.guardrails_script_hash {
                    out.insert(hash);
                }
            }
            GovAction::HardForkInitiation { .. }
            | GovAction::NoConfidence { .. }
            | GovAction::UpdateCommittee { .. }
            | GovAction::InfoAction => {}
        }
    }
}

/// Returns `true` if the certificate list contains at least one MIR certificate.
fn has_mir_cert(certs: Option<&[crate::types::DCert]>) -> bool {
    certs.is_some_and(|cs| {
        cs.iter().any(|c| matches!(c, crate::types::DCert::MoveInstantaneousReward(_, _)))
    })
}

/// Validates that a transaction containing MIR certificates has enough genesis
/// delegate key signatures.
///
/// The upstream rule requires that the number of genesis delegate key hashes
/// present in the transaction's VKey witness set is at least `quorum`.
///
/// `gen_delg_hashes` is a slice of 28-byte genesis **delegate** key hashes
/// (i.e. the `delegate` fields of the active `gen_delegs` map, NOT the
/// genesis owner key hashes).
///
/// Returns `Ok(())` when:
/// - no MIR certificates are present, or
/// - the number of genesis delegate witnesses ≥ `quorum`.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxow` — `validateMIRInsufficientGenesisSigs`.
pub fn validate_mir_genesis_quorum_if_present(
    certs: Option<&[crate::types::DCert]>,
    gen_delg_hashes: &HashSet<[u8; 28]>,
    quorum: u64,
    witness_bytes: Option<&[u8]>,
) -> Result<(), crate::error::LedgerError> {
    if !has_mir_cert(certs) {
        return Ok(());
    }
    if gen_delg_hashes.is_empty() {
        // No genesis delegates configured — skip quorum check.
        return Ok(());
    }
    // Count how many genesis delegate keys actually signed.
    let present = match witness_bytes {
        Some(wb) => {
            let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;
            let wset = witness_vkey_hash_set(&ws.vkey_witnesses);
            wset.intersection(gen_delg_hashes).count()
        }
        None => 0,
    };
    let required = quorum as usize;
    if present < required {
        return Err(crate::error::LedgerError::MIRInsufficientGenesisSigs { required, present });
    }
    Ok(())
}

/// Typed variant of [`validate_mir_genesis_quorum_if_present`] that accepts
/// a pre-decoded witness set (used by submitted-tx paths).
pub fn validate_mir_genesis_quorum_typed(
    certs: Option<&[crate::types::DCert]>,
    gen_delg_hashes: &HashSet<[u8; 28]>,
    quorum: u64,
    ws: &crate::eras::shelley::ShelleyWitnessSet,
) -> Result<(), crate::error::LedgerError> {
    if !has_mir_cert(certs) {
        return Ok(());
    }
    if gen_delg_hashes.is_empty() {
        return Ok(());
    }
    let wset = witness_vkey_hash_set(&ws.vkey_witnesses);
    let present = wset.intersection(gen_delg_hashes).count();
    let required = quorum as usize;
    if present < required {
        return Err(crate::error::LedgerError::MIRInsufficientGenesisSigs { required, present });
    }
    Ok(())
}

/// Builds a `HashSet` of genesis delegate key hashes from the active
/// `gen_delegs` map.  These are the delegate key hashes (NOT genesis owner
/// key hashes) that are expected to sign MIR transactions.
pub fn gen_delg_hash_set(
    gen_delegs: &std::collections::BTreeMap<
        crate::types::GenesisHash,
        crate::state::GenesisDelegationState,
    >,
) -> HashSet<[u8; 28]> {
    gen_delegs.values().map(|d| d.delegate).collect()
}

// ---------------------------------------------------------------------------
// validateScriptsWellFormed (Babbage+ UTXOW rule)
// ---------------------------------------------------------------------------

/// Checks whether a Plutus script's bytes can be decoded into valid UPLC
/// using the provided evaluator's `is_script_well_formed` method.
///
/// Native scripts are always well-formed (returns `true`).
fn is_plutus_script_well_formed(
    script: &crate::plutus::Script,
    evaluator: &dyn crate::plutus_validation::PlutusEvaluator,
) -> bool {
    use crate::plutus_validation::PlutusVersion;
    match script {
        crate::plutus::Script::Native(_) => true,
        crate::plutus::Script::PlutusV1(bytes) => {
            evaluator.is_script_well_formed(PlutusVersion::V1, bytes)
        }
        crate::plutus::Script::PlutusV2(bytes) => {
            evaluator.is_script_well_formed(PlutusVersion::V2, bytes)
        }
        crate::plutus::Script::PlutusV3(bytes) => {
            evaluator.is_script_well_formed(PlutusVersion::V3, bytes)
        }
    }
}

/// Computes the script hash for a `Script` enum value.
pub fn script_hash(script: &crate::plutus::Script) -> [u8; 28] {
    use crate::plutus_validation::PlutusVersion;
    match script {
        crate::plutus::Script::Native(ns) => crate::native_script::native_script_hash(ns),
        crate::plutus::Script::PlutusV1(bytes) => {
            crate::plutus_validation::plutus_script_hash(PlutusVersion::V1, bytes)
        }
        crate::plutus::Script::PlutusV2(bytes) => {
            crate::plutus_validation::plutus_script_hash(PlutusVersion::V2, bytes)
        }
        crate::plutus::Script::PlutusV3(bytes) => {
            crate::plutus_validation::plutus_script_hash(PlutusVersion::V3, bytes)
        }
    }
}

/// Validates that all Plutus script witnesses are well-formed (deserializable).
///
/// Checks PlutusV1 (key 3), PlutusV2 (key 6), and PlutusV3 (key 7) scripts
/// in the witness set. Collects script hashes of any that fail deserialization
/// and returns `MalformedScriptWitnesses` if the set is non-empty.
///
/// Reference: `Cardano.Ledger.Babbage.Rules.Utxow` — `validateScriptsWellFormed`.
pub fn validate_script_witnesses_well_formed(
    witness_set: &crate::eras::shelley::ShelleyWitnessSet,
    evaluator: &dyn crate::plutus_validation::PlutusEvaluator,
) -> Result<(), LedgerError> {
    use crate::plutus_validation::PlutusVersion;

    let mut malformed: Vec<[u8; 28]> = Vec::new();

    for bytes in &witness_set.plutus_v1_scripts {
        if !evaluator.is_script_well_formed(PlutusVersion::V1, bytes) {
            malformed.push(crate::plutus_validation::plutus_script_hash(PlutusVersion::V1, bytes));
        }
    }
    for bytes in &witness_set.plutus_v2_scripts {
        if !evaluator.is_script_well_formed(PlutusVersion::V2, bytes) {
            malformed.push(crate::plutus_validation::plutus_script_hash(PlutusVersion::V2, bytes));
        }
    }
    for bytes in &witness_set.plutus_v3_scripts {
        if !evaluator.is_script_well_formed(PlutusVersion::V3, bytes) {
            malformed.push(crate::plutus_validation::plutus_script_hash(PlutusVersion::V3, bytes));
        }
    }
    if !malformed.is_empty() {
        return Err(LedgerError::MalformedScriptWitnesses(malformed));
    }
    Ok(())
}

/// Validates that all reference scripts in transaction outputs are well-formed.
///
/// Inspects `script_ref` on each output and on the collateral return output.
/// Collects hashes of malformed Plutus scripts.
///
/// Reference: `Cardano.Ledger.Babbage.Rules.Utxow` — `MalformedReferenceScripts`.
pub fn validate_reference_scripts_well_formed(
    outputs: &[crate::eras::babbage::BabbageTxOut],
    collateral_return: Option<&crate::eras::babbage::BabbageTxOut>,
    evaluator: &dyn crate::plutus_validation::PlutusEvaluator,
) -> Result<(), LedgerError> {
    let mut malformed: Vec<[u8; 28]> = Vec::new();

    let iter = outputs.iter().chain(collateral_return.into_iter());
    for txout in iter {
        if let Some(sref) = &txout.script_ref {
            if !is_plutus_script_well_formed(&sref.0, evaluator) {
                malformed.push(script_hash(&sref.0));
            }
        }
    }
    if !malformed.is_empty() {
        return Err(LedgerError::MalformedReferenceScripts(malformed));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_required_present() {
        let h1 = [1u8; 28];
        let h2 = [2u8; 28];
        let required: HashSet<[u8; 28]> = [h1, h2].into_iter().collect();
        let witnesses: HashSet<[u8; 28]> = [h1, h2, [3u8; 28]].into_iter().collect();
        assert!(validate_vkey_witnesses(&required, &witnesses).is_ok());
    }

    #[test]
    fn missing_witness() {
        let h1 = [1u8; 28];
        let h2 = [2u8; 28];
        let required: HashSet<[u8; 28]> = [h1, h2].into_iter().collect();
        let witnesses: HashSet<[u8; 28]> = [h1].into_iter().collect();
        let result = validate_vkey_witnesses(&required, &witnesses);
        assert!(matches!(
            result,
            Err(LedgerError::MissingVKeyWitness { hash }) if hash == h2
        ));
    }

    #[test]
    fn collects_required_script_hashes_from_mint_policy_ids() {
        let mut mint = crate::eras::mary::MintAsset::new();
        mint.insert([1u8; 28], std::collections::BTreeMap::new());
        mint.insert([2u8; 28], std::collections::BTreeMap::new());

        let mut required = HashSet::new();
        required_script_hashes_from_mint(&mint, &mut required);

        assert!(required.contains(&[1u8; 28]));
        assert!(required.contains(&[2u8; 28]));
        assert_eq!(required.len(), 2);
    }

    #[test]
    fn collects_required_script_hashes_from_voting_procedures() {
        let mut inner = std::collections::BTreeMap::new();
        inner.insert(
            crate::eras::conway::GovActionId {
                transaction_id: [0xAA; 32],
                gov_action_index: 0,
            },
            crate::eras::conway::VotingProcedure {
                vote: crate::eras::conway::Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: [(crate::eras::conway::Voter::DRepScript([0xBB; 28]), inner)]
                .into_iter()
                .collect(),
        };

        let mut required = HashSet::new();
        required_script_hashes_from_voting_procedures(&voting_procedures, &mut required);

        assert!(required.contains(&[0xBB; 28]));
        assert_eq!(required.len(), 1);
    }

    #[test]
    fn collects_required_vkey_hashes_from_voting_procedures() {
        let mut inner = std::collections::BTreeMap::new();
        inner.insert(
            crate::eras::conway::GovActionId {
                transaction_id: [0xAA; 32],
                gov_action_index: 0,
            },
            crate::eras::conway::VotingProcedure {
                vote: crate::eras::conway::Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: [
                (crate::eras::conway::Voter::CommitteeKeyHash([0xBB; 28]), inner.clone()),
                (crate::eras::conway::Voter::DRepKeyHash([0xCC; 28]), inner.clone()),
                (crate::eras::conway::Voter::StakePool([0xDD; 28]), inner.clone()),
                (crate::eras::conway::Voter::CommitteeScript([0xEE; 28]), inner.clone()),
                (crate::eras::conway::Voter::DRepScript([0xFF; 28]), inner),
            ]
            .into_iter()
            .collect(),
        };

        let mut required = HashSet::new();
        required_vkey_hashes_from_voting_procedures(&voting_procedures, &mut required);

        assert!(required.contains(&[0xBB; 28]));
        assert!(required.contains(&[0xCC; 28]));
        assert!(required.contains(&[0xDD; 28]));
        assert_eq!(required.len(), 3);
    }

    #[test]
    fn collects_required_script_hashes_from_proposal_procedures() {
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit: 1,
            reward_account: crate::types::RewardAccount {
                network: 1,
                credential: crate::types::StakeCredential::AddrKeyHash([0xCC; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: crate::eras::conway::GovAction::NewConstitution {
                prev_action_id: None,
                constitution: crate::eras::conway::Constitution {
                    anchor: crate::types::Anchor {
                        url: "https://example.invalid/constitution".to_string(),
                        data_hash: [0xDD; 32],
                    },
                    guardrails_script_hash: Some([0xEE; 28]),
                },
            },
            anchor: crate::types::Anchor {
                url: "https://example.invalid/proposal".to_string(),
                data_hash: [0xFF; 32],
            },
        };

        let mut required = HashSet::new();
        required_script_hashes_from_proposal_procedures(&[proposal], &mut required);

        assert!(required.contains(&[0xEE; 28]));
        assert_eq!(required.len(), 1);
    }

    #[test]
    fn empty_required_passes() {
        let required: HashSet<[u8; 28]> = HashSet::new();
        let witnesses: HashSet<[u8; 28]> = HashSet::new();
        assert!(validate_vkey_witnesses(&required, &witnesses).is_ok());
    }

    #[test]
    fn vkey_hash_deterministic() {
        let vkey = [0xab_u8; 32];
        let h1 = vkey_hash(&vkey);
        let h2 = vkey_hash(&vkey);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 28);
    }

    #[test]
    fn witness_hash_set_extracts_hashes() {
        use crate::eras::shelley::ShelleyVkeyWitness;
        let w1 = ShelleyVkeyWitness {
            vkey: [1u8; 32],
            signature: [0u8; 64],
        };
        let w2 = ShelleyVkeyWitness {
            vkey: [2u8; 32],
            signature: [0u8; 64],
        };
        let set = witness_vkey_hash_set(&[w1.clone(), w2.clone()]);
        assert_eq!(set.len(), 2);
        assert!(set.contains(&vkey_hash(&w1.vkey)));
        assert!(set.contains(&vkey_hash(&w2.vkey)));
    }

        // Helper: build a ShelleyWitnessSet with the given VKey hashes in the vkey witness slots.
        // Signatures are zeroed (unused in quorum checks which only look at the hash of the vkey).
        fn witness_set_with_vkeys(vkeys: &[[u8; 32]]) -> crate::eras::shelley::ShelleyWitnessSet {
            crate::eras::shelley::ShelleyWitnessSet {
                vkey_witnesses: vkeys
                    .iter()
                    .map(|vk| crate::eras::shelley::ShelleyVkeyWitness {
                        vkey: *vk,
                        signature: [0u8; 64],
                    })
                    .collect(),
                native_scripts: vec![],
                bootstrap_witnesses: vec![],
                plutus_v1_scripts: vec![],
                plutus_data: vec![],
                redeemers: vec![],
                plutus_v2_scripts: vec![],
                plutus_v3_scripts: vec![],
            }
        }

        // Helper: build a realistic 32-byte raw Ed25519 public key whose Blake2b-224
        // hash equals `expected_hash`.  We use the inverse: store the hash as the
        // first 28 bytes and pad, then compute what hash the vkey_hash() function
        // would produce.  Instead, we take the direct route: create a vkey whose
        // hash we know by computing it ourselves.
        fn vkey_with_known_hash(prefix: u8) -> ([u8; 32], [u8; 28]) {
            let vk = [prefix; 32];
            let hash = vkey_hash(&vk);
            (vk, hash)
        }

        fn mir_cert() -> crate::types::DCert {
            crate::types::DCert::MoveInstantaneousReward(
                crate::types::MirPot::Reserves,
                crate::types::MirTarget::SendToOppositePot(0),
            )
        }

        #[test]
        fn mir_quorum_passes_when_no_mir_certs() {
            // No certs at all → quorum check trivially passes.
            let gen_delg_hashes: HashSet<[u8; 28]> = [[1u8; 28]].into_iter().collect();
            let ws = witness_set_with_vkeys(&[]);
            assert!(validate_mir_genesis_quorum_typed(None, &gen_delg_hashes, 5, &ws).is_ok());
        }

        #[test]
        fn mir_quorum_passes_when_non_mir_certs_only() {
            // A cert that is NOT MIR → quorum check trivially passes.
            let certs = vec![crate::types::DCert::AccountRegistration(
                crate::types::StakeCredential::AddrKeyHash([0u8; 28]),
            )];
            let gen_delg_hashes: HashSet<[u8; 28]> = [[1u8; 28]].into_iter().collect();
            let ws = witness_set_with_vkeys(&[]);
            assert!(validate_mir_genesis_quorum_typed(Some(&certs), &gen_delg_hashes, 5, &ws).is_ok());
        }

        #[test]
        fn mir_quorum_passes_when_gen_delegs_empty() {
            // MIR cert present but no genesis delegations on record → quorum check passes
            // (nothing to intersect against).
            let certs = vec![mir_cert()];
            let gen_delg_hashes: HashSet<[u8; 28]> = HashSet::new();
            let ws = witness_set_with_vkeys(&[]);
            assert!(validate_mir_genesis_quorum_typed(Some(&certs), &gen_delg_hashes, 5, &ws).is_ok());
        }

        #[test]
        fn mir_quorum_fails_when_no_sigs_for_mir_cert() {
            // MIR cert present, quorum=1, but no genesis delegate key in witness set.
            let (_, hash1) = vkey_with_known_hash(0xAA);
            let gen_delg_hashes: HashSet<[u8; 28]> = [hash1].into_iter().collect();
            let ws = witness_set_with_vkeys(&[]);
            let result = validate_mir_genesis_quorum_typed(Some(&[mir_cert()]), &gen_delg_hashes, 1, &ws);
            assert!(matches!(
                result,
                Err(LedgerError::MIRInsufficientGenesisSigs { required: 1, present: 0 })
            ));
        }

        #[test]
        fn mir_quorum_fails_when_insufficient_sigs() {
            // MIR cert present, quorum=3, only 2 delegates sign.
            let (vk1, hash1) = vkey_with_known_hash(0x01);
            let (vk2, hash2) = vkey_with_known_hash(0x02);
            let (_vk3, hash3) = vkey_with_known_hash(0x03);
            let gen_delg_hashes: HashSet<[u8; 28]> = [hash1, hash2, hash3].into_iter().collect();
            // Only 2 of the 3 delegates sign.
            let ws = witness_set_with_vkeys(&[vk1, vk2]);
            let result = validate_mir_genesis_quorum_typed(Some(&[mir_cert()]), &gen_delg_hashes, 3, &ws);
            assert!(matches!(
                result,
                Err(LedgerError::MIRInsufficientGenesisSigs { required: 3, present: 2 })
            ));
        }

        #[test]
        fn mir_quorum_passes_exact_threshold() {
            // MIR cert present, quorum=2, exactly 2 delegates sign → pass.
            let (vk1, hash1) = vkey_with_known_hash(0x01);
            let (vk2, hash2) = vkey_with_known_hash(0x02);
            let gen_delg_hashes: HashSet<[u8; 28]> = [hash1, hash2].into_iter().collect();
            let ws = witness_set_with_vkeys(&[vk1, vk2]);
            assert!(validate_mir_genesis_quorum_typed(Some(&[mir_cert()]), &gen_delg_hashes, 2, &ws).is_ok());
        }

        #[test]
        fn mir_quorum_passes_with_extra_non_delegate_sigs() {
            // MIR cert present, quorum=1.  Witness set contains both a genesis delegate key
            // and an unrelated key.  Should pass because ≥ quorum delegates signed.
            let (vk_delegate, hash_delegate) = vkey_with_known_hash(0xDD);
            let vk_other = [0x99u8; 32]; // not a genesis delegate
            let gen_delg_hashes: HashSet<[u8; 28]> = [hash_delegate].into_iter().collect();
            let ws = witness_set_with_vkeys(&[vk_delegate, vk_other]);
            assert!(validate_mir_genesis_quorum_typed(Some(&[mir_cert()]), &gen_delg_hashes, 1, &ws).is_ok());
        }

        #[test]
        fn gen_delg_hash_set_extracts_delegate_hashes() {
            // gen_delg_hash_set should return the delegate hashes (not genesis owner hashes).
            let mut gen_delegs = std::collections::BTreeMap::new();
            gen_delegs.insert(
                [0xAAu8; 28],
                crate::state::GenesisDelegationState { delegate: [0x11u8; 28], vrf: [0x22u8; 32] },
            );
            gen_delegs.insert(
                [0xBBu8; 28],
                crate::state::GenesisDelegationState { delegate: [0x33u8; 28], vrf: [0x44u8; 32] },
            );
            let set = gen_delg_hash_set(&gen_delegs);
            assert_eq!(set.len(), 2);
            assert!(set.contains(&[0x11u8; 28]));
            assert!(set.contains(&[0x33u8; 28]));
            // The genesis owner keys are NOT in the set.
            assert!(!set.contains(&[0xAAu8; 28]));
            assert!(!set.contains(&[0xBBu8; 28]));
        }

        // ── Bootstrap witness key hash (upstream bootstrapWitKeyHash) ────

        /// Builds a minimal Byron address with the given 28-byte address root.
        ///
        /// Byron CBOR: `[tag 24 CBOR([root, attributes={}, type=0]), CRC32]`
        fn make_byron_address(address_root: &[u8; 28]) -> Vec<u8> {
            // Inner payload: CBOR array(3) [bstr(28), map(0), uint(0)]
            let mut inner = crate::cbor::Encoder::new();
            inner.array(3);
            inner.bytes(address_root);
            inner.map(0);
            inner.unsigned(0);
            let inner_bytes = inner.into_bytes();

            // Outer: array(2) [tag 24 bstr(inner), CRC32]
            let mut outer = crate::cbor::Encoder::new();
            outer.array(2);
            outer.tag(24);
            outer.bytes(&inner_bytes);
            let crc = test_crc32_ieee(&inner_bytes);
            outer.unsigned(u64::from(crc));
            outer.into_bytes()
        }

        fn test_crc32_ieee(bytes: &[u8]) -> u32 {
            let mut crc = 0xffff_ffffu32;
            for &byte in bytes {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    let mask = (crc & 1).wrapping_neg() & 0xedb8_8320;
                    crc = (crc >> 1) ^ mask;
                }
            }
            !crc
        }

        /// Computes the Byron address root from (vkey, chain_code, attributes)
        /// using the same formula as upstream `bootstrapWitKeyHash`.
        fn compute_address_root(vkey: &[u8; 32], chain_code: &[u8], attributes: &[u8]) -> [u8; 28] {
            const PREFIX: &[u8] = &[0x83, 0x00, 0x82, 0x00, 0x58, 0x40];
            let mut bytes = Vec::with_capacity(PREFIX.len() + 32 + chain_code.len() + attributes.len());
            bytes.extend_from_slice(PREFIX);
            bytes.extend_from_slice(vkey);
            bytes.extend_from_slice(chain_code);
            bytes.extend_from_slice(attributes);
            let sha3 = yggdrasil_crypto::sha3_256(&bytes);
            yggdrasil_crypto::hash_bytes_224(&sha3.0).0
        }

        #[test]
        fn bootstrap_witness_key_hash_matches_address_root() {
            // Build a bootstrap witness with known vkey + chain_code + attributes.
            let vkey = [0x42u8; 32];
            let chain_code = [0xAAu8; 32];
            let attributes_enc = {
                let mut enc = crate::cbor::Encoder::new();
                enc.map(0);
                enc.into_bytes()
            };

            let witness = crate::eras::shelley::BootstrapWitness {
                public_key: vkey,
                signature: [0u8; 64],
                chain_code,
                attributes: attributes_enc.clone(),
            };

            let computed_hash = bootstrap_witness_key_hash(&witness);
            let expected = compute_address_root(&vkey, &chain_code, &attributes_enc);
            assert_eq!(computed_hash, expected);
        }

        #[test]
        fn bootstrap_witness_key_hash_set_collects_all() {
            let bw1 = crate::eras::shelley::BootstrapWitness {
                public_key: [0x01u8; 32],
                signature: [0u8; 64],
                chain_code: [0u8; 32],
                attributes: vec![0xA0], // CBOR map(0)
            };
            let bw2 = crate::eras::shelley::BootstrapWitness {
                public_key: [0x02u8; 32],
                signature: [0u8; 64],
                chain_code: [0u8; 32],
                attributes: vec![0xA0],
            };
            let set = bootstrap_witness_key_hash_set(&[bw1.clone(), bw2.clone()]);
            assert_eq!(set.len(), 2);
            assert!(set.contains(&bootstrap_witness_key_hash(&bw1)));
            assert!(set.contains(&bootstrap_witness_key_hash(&bw2)));
        }

        #[test]
        fn byron_address_root_extraction_matches_witness_key_hash() {
            // This is the critical parity test: the address root extracted
            // from a Byron address must equal the key hash reconstructed
            // from the corresponding bootstrap witness.
            let vkey = [0x55u8; 32];
            let chain_code = [0xBBu8; 32];
            let attributes = {
                let mut enc = crate::cbor::Encoder::new();
                enc.map(0);
                enc.into_bytes()
            };

            // Compute the expected address root
            let expected_root = compute_address_root(&vkey, &chain_code, &attributes);

            // Build a Byron address with that root
            let byron_addr_bytes = make_byron_address(&expected_root);
            let extracted_root = crate::types::byron_address_root(&byron_addr_bytes);
            assert_eq!(extracted_root, Some(expected_root));

            // Build the matching bootstrap witness
            let witness = crate::eras::shelley::BootstrapWitness {
                public_key: vkey,
                signature: [0u8; 64],
                chain_code,
                attributes: attributes.clone(),
            };
            let witness_root = bootstrap_witness_key_hash(&witness);
            assert_eq!(witness_root, expected_root);

            // Key parity assertion: address root == witness key hash
            assert_eq!(extracted_root.expect("extraction succeeded"), witness_root);
        }

        #[test]
        fn byron_input_generates_witness_obligation() {
            // Create a Byron address and wire it through the witness
            // requirement flow to verify it generates a needed hash.
            let vkey = [0x77u8; 32];
            let chain_code = [0xCCu8; 32];
            let attributes = vec![0xA0]; // CBOR map(0)
            let root = compute_address_root(&vkey, &chain_code, &attributes);
            let byron_addr_bytes = make_byron_address(&root);

            // Create a Shelley UTxO with a Byron address
            let txin = crate::eras::shelley::ShelleyTxIn {
                transaction_id: [0xAA; 32],
                index: 0,
            };
            let txout = crate::eras::shelley::ShelleyTxOut {
                address: byron_addr_bytes,
                amount: 1_000_000,
            };
            let mut utxo = crate::eras::shelley::ShelleyUtxo::new();
            utxo.insert(txin.clone(), txout);

            let mut required = HashSet::new();
            required_vkey_hashes_from_inputs_shelley(&[txin], &utxo, &mut required);

            // The required set must include the Byron address root
            assert!(required.contains(&root),
                "Byron input must generate witness obligation (address root)");
            assert_eq!(required.len(), 1);
        }

        // ------------------------------------------------------------------
        // PPUP proposer witness requirement tests
        // Reference: `Cardano.Ledger.Shelley.UTxO` — `propWits`
        // ------------------------------------------------------------------

        #[test]
        fn ppup_proposer_in_gen_delegs_is_required() {
            use crate::eras::shelley::ShelleyUpdate;
            use crate::protocol_params::ProtocolParameterUpdate;
            use crate::state::GenesisDelegationState;
            use std::collections::BTreeMap;

            let proposer: [u8; 28] = [0x01; 28];
            let non_proposer: [u8; 28] = [0x02; 28];

            let mut gen_delegs = BTreeMap::new();
            gen_delegs.insert(proposer, GenesisDelegationState {
                delegate: [0xAA; 28],
                vrf: [0xBB; 32],
            });
            gen_delegs.insert(non_proposer, GenesisDelegationState {
                delegate: [0xCC; 28],
                vrf: [0xDD; 32],
            });

            let mut updates = BTreeMap::new();
            updates.insert(proposer, ProtocolParameterUpdate::default());

            let update = ShelleyUpdate {
                proposed_protocol_parameter_updates: updates,
                epoch: 100,
            };

            let mut required = HashSet::new();
            required_vkey_hashes_from_ppup(&update, &gen_delegs, &mut required);

            // proposer that IS in gen_delegs must be required
            assert!(required.contains(&proposer));
            // non_proposer not in the update — must NOT appear
            assert!(!required.contains(&non_proposer));
            assert_eq!(required.len(), 1);
        }

        #[test]
        fn ppup_proposer_not_in_gen_delegs_excluded() {
            use crate::eras::shelley::ShelleyUpdate;
            use crate::protocol_params::ProtocolParameterUpdate;
            use crate::state::GenesisDelegationState;
            use std::collections::BTreeMap;

            let outsider: [u8; 28] = [0xFF; 28];

            // gen_delegs does NOT contain the outsider
            let gen_delegs = BTreeMap::new();

            let mut updates = BTreeMap::new();
            updates.insert(outsider, ProtocolParameterUpdate::default());

            let update = ShelleyUpdate {
                proposed_protocol_parameter_updates: updates,
                epoch: 100,
            };

            let mut required = HashSet::new();
            required_vkey_hashes_from_ppup(&update, &gen_delegs, &mut required);

            // proposer NOT in gen_delegs → excluded
            assert!(required.is_empty());
        }

        #[test]
        fn ppup_empty_update_produces_no_required() {
            use crate::eras::shelley::ShelleyUpdate;
            use crate::state::GenesisDelegationState;
            use std::collections::BTreeMap;

            let gen_key: [u8; 28] = [0x01; 28];
            let mut gen_delegs = BTreeMap::new();
            gen_delegs.insert(gen_key, GenesisDelegationState {
                delegate: [0xAA; 28],
                vrf: [0xBB; 32],
            });

            let update = ShelleyUpdate {
                proposed_protocol_parameter_updates: BTreeMap::new(),
                epoch: 100,
            };

            let mut required = HashSet::new();
            required_vkey_hashes_from_ppup(&update, &gen_delegs, &mut required);
            assert!(required.is_empty());
        }
    }
