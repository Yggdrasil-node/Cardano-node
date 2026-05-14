//! yggdrasil-error — workspace-wide error envelope.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis crate that
//! unifies the per-crate error enums from `yggdrasil-{crypto, ledger,
//! consensus, storage, plutus}` into a single envelope
//! (`YggdrasilError`) used at the binary boundary. Upstream
//! `cardano-node` raises domain errors as STS predicate failures
//! inside per-rule Haskell modules and catches them at the runtime
//! layer; Yggdrasil unifies them in one typed envelope so RPC error
//! reporting and trace-correlation are deterministic.
//!
//! This crate intentionally has **no** `From` impl for `eyre::Report`
//! or `anyhow::Error`. The binary `main` keeps using `eyre` for its
//! top-level boundary; typed APIs convert into `YggdrasilError` only
//! at the public boundary where the envelope adds value.

#![cfg_attr(test, allow(clippy::unwrap_used))]

use thiserror::Error;

use yggdrasil_consensus::ConsensusError;
use yggdrasil_crypto::CryptoError;
use yggdrasil_ledger::LedgerError;
use yggdrasil_plutus::MachineError;
use yggdrasil_storage::StorageError;

/// Convenience alias for `Result<T, YggdrasilError>` used at the
/// binary boundary.
pub type Result<T, E = YggdrasilError> = core::result::Result<T, E>;

/// Cross-crate error envelope. Each variant wraps the domain-specific
/// error enum defined in its source crate. The envelope itself is
/// `Send + Sync` so it traverses tokio task boundaries cleanly.
#[derive(Debug, Error)]
pub enum YggdrasilError {
    /// Cryptographic primitives (Ed25519 / KES / VRF / BLS / SHA / Blake2b).
    #[error(transparent)]
    Crypto(#[from] CryptoError),

    /// Ledger validation (CBOR / UTxO / fee / witness / era / pool).
    #[error(transparent)]
    Ledger(#[from] LedgerError),

    /// Consensus / Praos / TPraos (VRF leader check / KES / opcert / rollback).
    #[error(transparent)]
    Consensus(#[from] ConsensusError),

    /// Storage backends (immutable / volatile / ledger-db / chain-db).
    #[error(transparent)]
    Storage(#[from] StorageError),

    /// Plutus / UPLC evaluation (CEK machine / builtins / cost-model).
    #[error(transparent)]
    Plutus(#[from] MachineError),

    /// I/O failure at a boundary that doesn't fit the domain enums above
    /// (e.g. config file load, socket open, snapshot write).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Helper trait so call-sites can write
/// `do_thing().into_yggdrasil()?` instead of always wrapping at the
/// `?` boundary. Mirrors the `ExceptT`-into-envelope convention used
/// at upstream `Cardano.Node.Run`'s outer boundary.
pub trait IntoYggdrasil<T> {
    /// Convert a domain `Result<T, E>` (where `E: Into<YggdrasilError>`)
    /// into `Result<T, YggdrasilError>` at the public boundary.
    fn into_yggdrasil(self) -> Result<T>;
}

impl<T, E> IntoYggdrasil<T> for core::result::Result<T, E>
where
    E: Into<YggdrasilError>,
{
    fn into_yggdrasil(self) -> Result<T> {
        self.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_crypto_works() {
        let e: YggdrasilError = CryptoError::InvalidVrfProof.into();
        assert!(matches!(
            e,
            YggdrasilError::Crypto(CryptoError::InvalidVrfProof)
        ));
    }

    #[test]
    fn from_io_works() {
        let e: YggdrasilError = std::io::Error::other("disk full").into();
        assert!(matches!(e, YggdrasilError::Io(_)));
    }

    #[test]
    fn into_yggdrasil_trait() {
        let result: core::result::Result<u8, CryptoError> = Err(CryptoError::InvalidPoint);
        let upgraded: Result<u8> = result.into_yggdrasil();
        assert!(matches!(
            upgraded,
            Err(YggdrasilError::Crypto(CryptoError::InvalidPoint))
        ));
    }

    #[test]
    fn display_passthrough() {
        let e: YggdrasilError = CryptoError::InvalidVrfProof.into();
        assert_eq!(e.to_string(), "invalid vrf proof");
    }

    #[test]
    fn envelope_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<YggdrasilError>();
    }
}
