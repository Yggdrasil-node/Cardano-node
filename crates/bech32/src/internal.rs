//! Internal helpers for Bech32 encoding — character set, polynomial
//! checksum, encoding-version selection (BIP-0173 vs Bech32m).
//!
//! ## Naming parity
//!
//! **Strict mirror:** Codec/Binary/Bech32/Internal.hs. The Rust file
//! is the canonical 1:1 mirror of upstream
//! `bech32/bech32/src/Codec/Binary/Bech32/Internal.hs`. R331 declares
//! the type placeholders + module surface; concrete polynomial /
//! charset / checksum implementations land at R333.
//!
//! Per upstream's module documentation, this file carries:
//! - the 32-character Bech32 alphabet (`charset`);
//! - the polynomial constants for the BIP-0173 (`bech32`) and
//!   Bech32m (`bech32m`) checksum variants;
//! - charset position lookup (`charsetMap`);
//! - the `polymod` core checksum function;
//! - the encoding-spec discriminator (`Bech32` vs `Bech32m`).
//!
//! These helpers are PUBLIC in upstream's Internal module so
//! third-party libraries can reuse them; R333 mirrors that public
//! surface.

/// Encoding spec version — BIP-0173 (`Bech32`) or Bech32m
/// (`Bech32m`). Determines the polynomial constant for the checksum.
///
/// Upstream: `Codec.Binary.Bech32.Internal::EncodingSpec`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingSpecPlaceholder {}

/// 32-character Bech32 alphabet — `qpzry9x8gf2tvdw0s3jn54khce6mua7l`.
///
/// Upstream: `Codec.Binary.Bech32.Internal::charset`.
pub const CHARSET_PLACEHOLDER: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
