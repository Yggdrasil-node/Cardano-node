//! Compatible-cluster TxOut construction helpers (value + datum + reference-script).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Compatible/Transaction/TxOut.hs`.
//! R291 lands the file with the API skeleton; concrete Compatible-
//! cluster command implementations port from upstream over
//! subsequent rounds + after the integration test in R295 confirms
//! the wire shape against the upstream `cardano-cli` binary.

/// Compatible-cluster TxOut construction helpers (value + datum + reference-script) placeholder.
///
/// Mirrors upstream `Cardano.CLI.Compatible.Transaction.TxOut` types; the
/// Rust port lands as concrete subcommand implementations come
/// online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxOutPlaceholder {}
