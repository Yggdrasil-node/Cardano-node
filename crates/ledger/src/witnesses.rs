//! Witness sufficiency checks.
//!
//! Validates that a transaction carries the required VKey witnesses
//! for all spending inputs, certificate actions, and withdrawals.
//!
//! Reference:
//! `Cardano.Ledger.Shelley.Rules.Utxow` — `validateNeededWitnesses`

use std::collections::HashSet;

use crate::error::LedgerError;

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
            }
        }
    }
}

/// Collects VKey hashes from spending input payment credentials (multi-era).
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
}
