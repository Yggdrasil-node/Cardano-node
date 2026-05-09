//! Pure-Rust port of upstream `Codec.Binary.Bech32` — BIP-0173 / Bech32m
//! address-format encoding and decoding.
//!
//! Foundation: `bech32` v0.11+ from `rust-bitcoin/rust-bech32` (workspace
//! dependency added at R330). Concrete encode/decode implementations
//! land at R333 (Phase A.1 of the R326–R459 sister-tools port arc).
//!
//! ## Naming parity
//!
//! **Strict mirror:** Codec/Binary/Bech32.hs. The Rust crate root
//! (`lib.rs`) is the canonical 1:1 mirror of upstream
//! `bech32/bech32/src/Codec/Binary/Bech32.hs` — the public library
//! API surface. The internal helper module lives in `internal.rs`
//! (mirroring upstream `Codec/Binary/Bech32/Internal.hs`); the
//! binary entry point lives in `main.rs` (mirroring upstream
//! `bech32/bech32/app/Main.hs`).
//!
//! Upstream's `bech32-th/src/Codec/Binary/Bech32/TH.hs` (Template
//! Haskell helpers) has no Rust analog — Rust uses `macro_rules!`
//! and proc-macros directly. No corresponding `crates/bech32/src/th.rs`
//! is needed; the strict-mirror policy supports this absence per the
//! `Setup.hs` / `Orphans.hs` precedents.

pub mod internal;

// -----------------------------------------------------------------------
// Core type placeholders (R331 skeleton)
//
// These types mirror upstream's exported API surface. R331 declares
// them as empty placeholder enums/structs so the file compiles and
// downstream consumers can see the type names; concrete fields +
// methods land at R333 alongside the encode/decode implementations.
// -----------------------------------------------------------------------

/// `DataPart` — the data payload of a Bech32 string (post-`1` separator).
///
/// Upstream: `Codec.Binary.Bech32::DataPart`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataPartPlaceholder {
    _placeholder: (),
}

/// `HumanReadablePart` — the prefix of a Bech32 string (pre-`1` separator).
///
/// Upstream: `Codec.Binary.Bech32::HumanReadablePart`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanReadablePartPlaceholder {
    _placeholder: (),
}

/// `EncodingError` — failure modes for `encode` (HRP-too-long,
/// HRP-too-short, etc.).
///
/// Upstream: `Codec.Binary.Bech32::EncodingError`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EncodingErrorPlaceholder {}

/// `DecodingError` — failure modes for `decode` (invalid character,
/// invalid checksum, etc.).
///
/// Upstream: `Codec.Binary.Bech32::DecodingError`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodingErrorPlaceholder {}

/// `HumanReadablePartError` — failure modes for HRP construction.
///
/// Upstream: `Codec.Binary.Bech32::HumanReadablePartError`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HumanReadablePartErrorPlaceholder {}

/// `CharPosition` — 0-based position of a character within a Bech32
/// string. Used by error variants to point at the offending column.
///
/// Upstream: `Codec.Binary.Bech32::CharPosition`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CharPositionPlaceholder(pub usize);

/// `Word5` — the 5-bit word type used by Bech32's data section.
///
/// Upstream: `Codec.Binary.Bech32::Word5`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Word5Placeholder(pub u8);

/// Placeholder run-loop entry called by the binary `main`.
///
/// R333 replaces this stub with the concrete CLI dispatcher matching
/// the upstream binary's encode/decode/HRP/data subcommand surface.
pub fn run() -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-bech32: not yet implemented (R331 skeleton); \
         see docs/operational-runs/ for the bech32 port progress."
    ))
}
