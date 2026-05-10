//! Internal helpers for Bech32 encoding — character set, polynomial
//! checksum, encoding-version selection (BIP-0173 vs Bech32m).
//!
//! ## Naming parity
//!
//! **Strict mirror:** Codec/Binary/Bech32/Internal.hs. The Rust file
//! is the canonical 1:1 mirror of upstream
//! `bech32/bech32/src/Codec/Binary/Bech32/Internal.hs`. R333 lands
//! the public constants needed by the binary's CLI dispatch
//! (CHARSET, separator char, checksum length); the polynomial
//! checksum machinery is internal to the `bech32` crate (workspace
//! dep landed at R330) and doesn't need a Yggdrasil-side mirror.
//!
//! Per upstream's module documentation, this file carries:
//! - the 32-character Bech32 alphabet (`charset`);
//! - the polynomial constants for the BIP-0173 (`bech32`) and
//!   Bech32m (`bech32m`) checksum variants;
//! - charset position lookup (`charsetMap`);
//! - the `polymod` core checksum function;
//! - the encoding-spec discriminator (`Bech32` vs `Bech32m`).
//!
//! In Yggdrasil, the polynomial / checksum / encoding-spec machinery
//! is provided by the `bech32` crate's `Bech32` and `Bech32m`
//! `Checksum` impls. We only need the public CHARSET + separator
//! char + checksum length here for the encoding-detection heuristic
//! in `lib.rs::resemble_bech32`.

/// Encoding spec version — BIP-0173 (`Bech32`) or BIP-0350
/// (`Bech32m`). Determines the polynomial constant for the checksum.
///
/// In Yggdrasil this is delegated to the `bech32` crate's
/// type-parameter checksum classes (`bech32::Bech32` and
/// `bech32::Bech32m`); this enum exists for documentation parity
/// with upstream's exported `EncodingSpec` type.
///
/// Upstream: `Codec.Binary.Bech32.Internal::EncodingSpec`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingSpec {
    /// BIP-0173 Bech32 (used by Cardano addresses + Bitcoin segwit v0).
    Bech32,
    /// BIP-0350 Bech32m (used by Bitcoin taproot).
    Bech32m,
}

/// 32-character Bech32 alphabet — `qpzry9x8gf2tvdw0s3jn54khce6mua7l`.
///
/// Upstream: `Codec.Binary.Bech32.Internal::charset`.
pub const CHARSET: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin CHARSET against drift. Bech32's 32-character alphabet is
    /// fixed by the BIP-0173 spec; the constant here mirrors upstream
    /// `Codec.Binary.Bech32.Internal::charset` and a typo would
    /// silently break encoding round-trips against upstream binaries.
    #[test]
    fn charset_matches_bip0173() {
        assert_eq!(CHARSET, "qpzry9x8gf2tvdw0s3jn54khce6mua7l");
        assert_eq!(CHARSET.len(), 32);
    }

    #[test]
    fn encoding_spec_variants_are_distinct() {
        assert_ne!(EncodingSpec::Bech32, EncodingSpec::Bech32m);
    }
}
