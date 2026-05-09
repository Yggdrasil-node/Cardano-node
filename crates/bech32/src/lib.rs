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
pub mod parser;

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

/// Placeholder run-loop entry called by legacy callers (R327 era).
///
/// R333 replaces the inner stub with the concrete CLI dispatcher
/// matching the upstream binary's encode/decode subcommand surface.
/// New callers should use [`run_with`] which takes parsed [`parser::Args`].
pub fn run() -> eyre::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = parser::parse_args(&argv).map_err(|e| eyre::eyre!("{e}"))?;
    run_with(args)
}

/// Concrete run-loop entry called by `main` after argument parsing.
///
/// R332 stub: returns "encode/decode not yet implemented (R333)" sentinel
/// because the actual encode/decode dispatch lands at R333. The argument
/// parser surface IS functional at R332 — `bech32 --help` and
/// `bech32 --version` both produce byte-equivalent output via
/// `parser::HELP_TEXT` / `parser::VERSION_TEXT`.
pub fn run_with(_args: parser::Args) -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-bech32: encode/decode not yet implemented (R332 skeleton; R333 lands it); \
         see docs/operational-runs/ for the bech32 port progress."
    ))
}
