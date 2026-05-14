//! Wave 9 PR 28 follow-on — property-based CBOR roundtrip tests.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side property-test harness;
//! upstream `cardano-ledger` has its own Haskell `QuickCheck`
//! roundtrip tests for every era's `EncCBOR` / `DecCBOR` instance.
//! This Rust port covers the foundational newtypes
//! (`SlotNo` / `BlockNo` / `EpochNo`) and the `Era` enum — the
//! primitives that every era's `TxBody` / `Value` / `MultiAsset`
//! ultimately compose. Future PRs add per-era proptest scaffolds
//! along the same lines.
//!
//! Run:
//!   cargo test -p yggdrasil-ledger --test proptest_cbor

#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use yggdrasil_ledger::cbor::{CborDecode, CborEncode};
use yggdrasil_ledger::eras::Era;
use yggdrasil_ledger::types::{BlockNo, EpochNo, SlotNo};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    /// `SlotNo(x).to_cbor_bytes()` → `SlotNo::from_cbor_bytes` ↔ `SlotNo(x)`.
    /// The encoder writes a single CBOR unsigned-integer value per
    /// the Shelley+ era encoding; the decoder reverses it.
    #[test]
    fn slot_no_cbor_roundtrip(slot in any::<u64>()) {
        let original = SlotNo(slot);
        let bytes = original.to_cbor_bytes();
        let decoded = SlotNo::from_cbor_bytes(&bytes)
            .expect("CBOR-encoded SlotNo must decode back");
        prop_assert_eq!(original, decoded);
    }

    /// `BlockNo` shares the same wire shape as `SlotNo` (CBOR uint),
    /// but the roundtrip is independent — a bug in either encoder
    /// would surface here.
    #[test]
    fn block_no_cbor_roundtrip(block in any::<u64>()) {
        let original = BlockNo(block);
        let bytes = original.to_cbor_bytes();
        let decoded = BlockNo::from_cbor_bytes(&bytes)
            .expect("CBOR-encoded BlockNo must decode back");
        prop_assert_eq!(original, decoded);
    }

    /// `EpochNo` shares the same wire shape; tested for the same
    /// reason as `BlockNo`.
    #[test]
    fn epoch_no_cbor_roundtrip(epoch in any::<u64>()) {
        let original = EpochNo(epoch);
        let bytes = original.to_cbor_bytes();
        let decoded = EpochNo::from_cbor_bytes(&bytes)
            .expect("CBOR-encoded EpochNo must decode back");
        prop_assert_eq!(original, decoded);
    }
}

#[cfg(test)]
mod era_cases {
    //! Era is a finite enum so an exhaustive case-table beats
    //! proptest for it. Every variant should roundtrip.

    use super::*;

    fn all_eras() -> [Era; 7] {
        [Era::Byron, Era::Shelley, Era::Allegra, Era::Mary, Era::Alonzo, Era::Babbage, Era::Conway]
    }

    #[test]
    fn every_era_variant_cbor_roundtrips() {
        for era in all_eras() {
            let bytes = era.to_cbor_bytes();
            let decoded = Era::from_cbor_bytes(&bytes)
                .unwrap_or_else(|err| panic!("Era::{era:?} failed to decode: {err}"));
            assert_eq!(
                era, decoded,
                "Era::{era:?} did not roundtrip cleanly through CBOR",
            );
        }
    }

    #[test]
    fn era_cbor_is_deterministic() {
        // Encoding the same Era twice must produce the same bytes.
        // Any encoding non-determinism would break the byte-for-byte
        // parity contract with upstream cardano-ledger.
        for era in all_eras() {
            let a = era.to_cbor_bytes();
            let b = era.to_cbor_bytes();
            assert_eq!(
                a, b,
                "Era::{era:?} CBOR encoding is non-deterministic (got {a:02x?} vs {b:02x?})",
            );
        }
    }
}

#[cfg(test)]
mod fixed_cases {
    //! Fixed-byte vector roundtrips for the known-stable encodings.
    //! Insurance against proptest's case-reduction shrinking past a
    //! real regression at common values.

    use super::*;

    #[test]
    fn slot_zero_cbor_shape() {
        // CBOR unsigned 0 = 0x00.
        let bytes = SlotNo(0).to_cbor_bytes();
        assert_eq!(bytes, vec![0x00]);
        assert_eq!(SlotNo::from_cbor_bytes(&bytes).unwrap(), SlotNo(0));
    }

    #[test]
    fn slot_max_cbor_roundtrips() {
        let bytes = SlotNo(u64::MAX).to_cbor_bytes();
        // CBOR major 0 (unsigned), additional info 27 (u64 follows),
        // then 8 bytes of 0xff.
        assert_eq!(bytes.first(), Some(&0x1b));
        assert_eq!(bytes.len(), 9);
        assert_eq!(SlotNo::from_cbor_bytes(&bytes).unwrap(), SlotNo(u64::MAX));
    }
}
