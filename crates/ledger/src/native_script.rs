//! Native script evaluation engine.
//!
//! Evaluates Allegra+ native (timelock) scripts against the transaction
//! context: available VKey witnesses and the current slot.
//!
//! Reference:
//! `Cardano.Ledger.Allegra.Scripts` — `evalTimelock`
//! `Cardano.Ledger.Core.NativeScript` — `validateNativeScript`

use crate::eras::allegra::NativeScript;
use std::collections::HashSet;

/// Context required to evaluate a native script.
///
/// The evaluator needs the set of VKey hashes that signed the transaction
/// and the transaction's validity interval to evaluate time-based scripts.
pub struct NativeScriptContext<'a> {
    /// Set of 28-byte VKey hashes (Blake2b-224 of verification keys)
    /// present in the transaction witness set.
    pub vkey_hashes: &'a HashSet<[u8; 28]>,
    /// The current slot (used for `InvalidBefore` / `InvalidHereafter`
    /// scripts). For Shelley-era this is the TTL; for Allegra+ the
    /// transaction's validity interval bounds are checked externally,
    /// so we can use the slot for timelock evaluation.
    pub current_slot: u64,
}

/// Evaluates a native script against the given context.
///
/// Returns `true` when the script is satisfied.
///
/// Reference: upstream `evalTimelock` in `Cardano.Ledger.Allegra.Scripts`.
pub fn evaluate_native_script(script: &NativeScript, ctx: &NativeScriptContext<'_>) -> bool {
    match script {
        NativeScript::ScriptPubkey(keyhash) => ctx.vkey_hashes.contains(keyhash),

        NativeScript::ScriptAll(scripts) => scripts.iter().all(|s| evaluate_native_script(s, ctx)),

        NativeScript::ScriptAny(scripts) => scripts.iter().any(|s| evaluate_native_script(s, ctx)),

        NativeScript::ScriptNOfK(n, scripts) => {
            let required = *n as usize;
            scripts
                .iter()
                .filter(|s| evaluate_native_script(s, ctx))
                .take(required)
                .count()
                >= required
        }

        NativeScript::InvalidBefore(slot) => {
            // The transaction is valid only at or after this slot.
            ctx.current_slot >= *slot
        }

        NativeScript::InvalidHereafter(slot) => {
            // The transaction is invalid at or after this slot.
            ctx.current_slot < *slot
        }
    }
}

/// Computes the Blake2b-224 hash of a native script's CBOR encoding.
///
/// This is the "script hash" used to derive script addresses and to
/// identify native scripts in the witness set. The hash is prefixed
/// with '\x00' (native script language tag) before hashing.
///
/// Reference: upstream `hashScript` for native scripts.
pub fn native_script_hash(script: &NativeScript) -> [u8; 28] {
    use crate::cbor::CborEncode;
    let mut buf = vec![0x00u8]; // language tag for native scripts
    let cbor = script.to_cbor_bytes();
    buf.extend_from_slice(&cbor);
    yggdrasil_crypto::blake2b::hash_bytes_224(&buf).0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(script: &NativeScript, hashes: &[[u8; 28]], slot: u64) -> bool {
        let set: HashSet<[u8; 28]> = hashes.iter().copied().collect();
        let ctx = NativeScriptContext {
            vkey_hashes: &set,
            current_slot: slot,
        };
        evaluate_native_script(script, &ctx)
    }

    #[test]
    fn script_pubkey_satisfied() {
        let key = [1u8; 28];
        let script = NativeScript::ScriptPubkey(key);
        assert!(check(&script, &[key], 0));
    }

    #[test]
    fn script_pubkey_not_satisfied() {
        let key = [1u8; 28];
        let other = [2u8; 28];
        let script = NativeScript::ScriptPubkey(key);
        assert!(!check(&script, &[other], 0));
    }

    #[test]
    fn script_all_satisfied() {
        let k1 = [1u8; 28];
        let k2 = [2u8; 28];
        let script = NativeScript::ScriptAll(vec![
            NativeScript::ScriptPubkey(k1),
            NativeScript::ScriptPubkey(k2),
        ]);
        assert!(check(&script, &[k1, k2], 0));
    }

    #[test]
    fn script_all_partial_fail() {
        let k1 = [1u8; 28];
        let k2 = [2u8; 28];
        let script = NativeScript::ScriptAll(vec![
            NativeScript::ScriptPubkey(k1),
            NativeScript::ScriptPubkey(k2),
        ]);
        assert!(!check(&script, &[k1], 0));
    }

    #[test]
    fn script_any_satisfied() {
        let k1 = [1u8; 28];
        let k2 = [2u8; 28];
        let script = NativeScript::ScriptAny(vec![
            NativeScript::ScriptPubkey(k1),
            NativeScript::ScriptPubkey(k2),
        ]);
        assert!(check(&script, &[k2], 0));
    }

    #[test]
    fn script_any_none_fail() {
        let k1 = [1u8; 28];
        let other = [3u8; 28];
        let script = NativeScript::ScriptAny(vec![NativeScript::ScriptPubkey(k1)]);
        assert!(!check(&script, &[other], 0));
    }

    #[test]
    fn script_n_of_k() {
        let k1 = [1u8; 28];
        let k2 = [2u8; 28];
        let k3 = [3u8; 28];
        let script = NativeScript::ScriptNOfK(
            2,
            vec![
                NativeScript::ScriptPubkey(k1),
                NativeScript::ScriptPubkey(k2),
                NativeScript::ScriptPubkey(k3),
            ],
        );
        assert!(check(&script, &[k1, k3], 0));
        assert!(!check(&script, &[k1], 0));
    }

    #[test]
    fn invalid_before_satisfied() {
        let script = NativeScript::InvalidBefore(100);
        assert!(check(&script, &[], 100));
        assert!(check(&script, &[], 200));
    }

    #[test]
    fn invalid_before_fail() {
        let script = NativeScript::InvalidBefore(100);
        assert!(!check(&script, &[], 99));
    }

    #[test]
    fn invalid_hereafter_satisfied() {
        let script = NativeScript::InvalidHereafter(100);
        assert!(check(&script, &[], 99));
    }

    #[test]
    fn invalid_hereafter_fail() {
        let script = NativeScript::InvalidHereafter(100);
        assert!(!check(&script, &[], 100));
        assert!(!check(&script, &[], 101));
    }

    #[test]
    fn nested_all_with_timelock() {
        let key = [1u8; 28];
        let script = NativeScript::ScriptAll(vec![
            NativeScript::ScriptPubkey(key),
            NativeScript::InvalidBefore(10),
            NativeScript::InvalidHereafter(100),
        ]);
        assert!(check(&script, &[key], 50));
        assert!(!check(&script, &[key], 5));
        assert!(!check(&script, &[key], 100));
    }

    #[test]
    fn empty_all_is_true() {
        let script = NativeScript::ScriptAll(vec![]);
        assert!(check(&script, &[], 0));
    }

    #[test]
    fn empty_any_is_false() {
        let script = NativeScript::ScriptAny(vec![]);
        assert!(!check(&script, &[], 0));
    }

    #[test]
    fn n_of_k_zero_required() {
        let script = NativeScript::ScriptNOfK(0, vec![NativeScript::ScriptPubkey([1u8; 28])]);
        assert!(check(&script, &[], 0));
    }
}
