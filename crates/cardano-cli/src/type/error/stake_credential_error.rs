//! Type stake credential error.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Type/Error/StakeCredentialError.hs`.
//! R294 lands the file with the API skeleton. Concrete
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// Type stake credential error placeholder.
///
/// Mirrors upstream `Cardano.CLI.Type.Error.StakeCredentialError` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StakeCredentialErrorPlaceholder {}
