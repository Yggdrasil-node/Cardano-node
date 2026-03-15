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
