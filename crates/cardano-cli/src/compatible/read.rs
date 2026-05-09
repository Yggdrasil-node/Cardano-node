//! Compatible-cluster envelope-file reader helpers (text-envelope CBOR + JSON).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Compatible/Read.hs`.
//! R291 lands the file with the API skeleton; concrete Compatible-
//! cluster command implementations port from upstream over
//! subsequent rounds + after the integration test in R295 confirms
//! the wire shape against the upstream `cardano-cli` binary.

/// Compatible-cluster envelope-file reader helpers (text-envelope CBOR + JSON) placeholder.
///
/// Mirrors upstream `Cardano.CLI.Compatible.Read` types; the
/// Rust port lands as concrete subcommand implementations come
/// online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadPlaceholder {}
