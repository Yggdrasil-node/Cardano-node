//! EraIndependent option.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Cip/Cip129/Option.hs`.
//! R293 lands the file with the API skeleton. Concrete
//! EraIndependent command implementations port from upstream over
//! subsequent rounds + R295 integration tests.

/// EraIndependent option placeholder.
///
/// Mirrors upstream `Cardano.CLI.EraIndependent.Cip.Cip129.Option` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptionPlaceholder {}
