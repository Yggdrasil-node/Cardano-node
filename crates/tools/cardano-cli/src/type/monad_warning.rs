//! Type monad warning.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Type/MonadWarning.hs`.
//! R294 lands the file with the API skeleton. Concrete
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// Type monad warning placeholder.
///
/// Mirrors upstream `Cardano.CLI.Type.MonadWarning` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MonadWarningPlaceholder {}
