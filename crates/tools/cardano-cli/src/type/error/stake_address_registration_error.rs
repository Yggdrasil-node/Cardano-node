//! Type stake address registration error.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Type/Error/StakeAddressRegistrationError.hs`.
//! R294 lands the file with the API skeleton. Concrete
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// Type stake address registration error placeholder.
///
/// Mirrors upstream `Cardano.CLI.Type.Error.StakeAddressRegistrationError` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StakeAddressRegistrationErrorPlaceholder {}
