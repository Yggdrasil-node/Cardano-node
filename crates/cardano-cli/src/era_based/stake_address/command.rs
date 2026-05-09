//! EraBased command.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraBased/StakeAddress/Command.hs`.
//! R292 lands the file with the API skeleton. Concrete EraBased
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// EraBased command placeholder.
///
/// Mirrors upstream `Cardano.CLI.EraBased.StakeAddress.Command` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandPlaceholder {}
