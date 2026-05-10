//! EraBased common.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraBased/Script/Read/Common.hs`.
//! R292 lands the file with the API skeleton. Concrete EraBased
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// EraBased common placeholder.
///
/// Mirrors upstream `Cardano.CLI.EraBased.Script.Read.Common` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommonPlaceholder {}
