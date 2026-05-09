//! Type stake pool cmd error.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Type/Error/StakePoolCmdError.hs`.
//! R294 lands the file with the API skeleton. Concrete
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// Type stake pool cmd error placeholder.
///
/// Mirrors upstream `Cardano.CLI.Type.Error.StakePoolCmdError` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StakePoolCmdErrorPlaceholder {}
