//! Type bootstrap witness error.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Type/Error/BootstrapWitnessError.hs`.
//! R294 lands the file with the API skeleton. Concrete
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// Type bootstrap witness error placeholder.
///
/// Mirrors upstream `Cardano.CLI.Type.Error.BootstrapWitnessError` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootstrapWitnessErrorPlaceholder {}
