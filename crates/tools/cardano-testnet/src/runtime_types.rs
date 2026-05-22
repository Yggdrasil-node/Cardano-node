//! cardano-testnet runtime and key-file types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side port of upstream
//! `cardano-testnet/src/Testnet/Types.hs` — the basename `types.rs`
//! is already taken by the `Testnet/Start/Types.hs` mirror, and the
//! `lib.rs` layout table maps `Testnet/Types.hs` to this
//! `runtime_types.rs`.
//!
//! This slice ports the portable key-file types — `KeyPair` and the
//! key-kind markers. The process-handle-backed runtime types
//! (`TestnetRuntime`, `TestnetNode`, `TestnetKesAgent`) land with the
//! testnet-harness rounds. Upstream's `VKey` / `SKey` are `File`-tag
//! phantoms with no Rust counterpart — yggdrasil's `KeyPair` stores
//! `PathBuf` directly rather than a typed `File`.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

/// Key-kind marker — a VRF key. Mirror of upstream `data VrfKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct VrfKey;

/// Key-kind marker — a stake-pool cold key. Mirror of upstream
/// `data StakePoolKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StakePoolKey;

/// Key-kind marker — a stake key. Mirror of upstream `data StakeKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StakeKey;

/// Key-kind marker — a payment key. Mirror of upstream
/// `data PaymentKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PaymentKey;

/// Key-kind marker — a KES (key-evolving-signature) key. Mirror of
/// upstream `data KesKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct KesKey;

/// Key-kind marker — a DRep (delegated-representative) key. Mirror of
/// upstream `data DRepKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DRepKey;

/// A verification + signing key-file pair, phantom-typed by key kind.
///
/// Mirror of upstream `data KeyPair k` (`Testnet/Types.hs`) — the
/// `k` parameter is one of the key-kind markers above, giving
/// compile-time safety against mixing, say, a `KeyPair<PaymentKey>`
/// with a `KeyPair<StakeKey>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyPair<K> {
    /// Path to the verification (public) key file.
    pub verification_key: PathBuf,
    /// Path to the signing (private) key file.
    pub signing_key: PathBuf,
    _kind: PhantomData<K>,
}

impl<K> KeyPair<K> {
    /// Construct a key pair from its verification- and signing-key
    /// file paths.
    pub fn new(
        verification_key: impl Into<PathBuf>,
        signing_key: impl Into<PathBuf>,
    ) -> KeyPair<K> {
        KeyPair {
            verification_key: verification_key.into(),
            signing_key: signing_key.into(),
            _kind: PhantomData,
        }
    }

    /// The verification-key file path. Mirror of upstream
    /// `verificationKeyFp`.
    pub fn verification_key_fp(&self) -> &Path {
        &self.verification_key
    }

    /// The signing-key file path. Mirror of upstream `signingKeyFp`.
    pub fn signing_key_fp(&self) -> &Path {
        &self.signing_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_pair_accessors_return_the_paths() {
        let kp: KeyPair<PaymentKey> = KeyPair::new("/keys/pay.vkey", "/keys/pay.skey");
        assert_eq!(kp.verification_key_fp().to_str(), Some("/keys/pay.vkey"));
        assert_eq!(kp.signing_key_fp().to_str(), Some("/keys/pay.skey"));
    }

    #[test]
    fn key_pair_equality_is_by_path() {
        let a: KeyPair<StakeKey> = KeyPair::new("/k/v", "/k/s");
        let b: KeyPair<StakeKey> = KeyPair::new("/k/v", "/k/s");
        let c: KeyPair<StakeKey> = KeyPair::new("/k/v", "/k/other");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn key_pair_is_phantom_typed_per_kind() {
        // Each key kind yields a distinct `KeyPair` type — exercised
        // here by constructing one of every kind.
        let _vrf: KeyPair<VrfKey> = KeyPair::new("v", "s");
        let _spo: KeyPair<StakePoolKey> = KeyPair::new("v", "s");
        let _stake: KeyPair<StakeKey> = KeyPair::new("v", "s");
        let _pay: KeyPair<PaymentKey> = KeyPair::new("v", "s");
        let _kes: KeyPair<KesKey> = KeyPair::new("v", "s");
        let _drep: KeyPair<DRepKey> = KeyPair::new("v", "s");
    }
}
