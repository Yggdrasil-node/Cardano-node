//! Type cardano address signing key conversion error.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Type/Error/CardanoAddressSigningKeyConversionError.hs`.
//! R294 lands the file with the API skeleton. Concrete
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// Type cardano address signing key conversion error placeholder.
///
/// Mirrors upstream `Cardano.CLI.Type.Error.CardanoAddressSigningKeyConversionError` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CardanoAddressSigningKeyConversionErrorPlaceholder {}
