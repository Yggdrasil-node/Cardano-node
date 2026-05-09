//! Byron-era byron legacy operator-key conversion (byron-era format <-> shelley-era format).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Byron/Legacy.hs`.
//! R290 lands the file with the API skeleton; concrete Byron-era
//! command implementations port from upstream over subsequent
//! rounds + after the integration test in R295 confirms the wire
//! shape against the upstream `cardano-cli` binary.

/// Byron-era byron legacy operator-key conversion (byron-era format <-> shelley-era format) placeholder.
///
/// Mirrors upstream `Cardano.CLI.Byron.Legacy` types; the Rust
/// port lands as concrete subcommand implementations come online.
/// Currently empty enum so the module compiles + can be extended
/// in subsequent rounds without breaking the public path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LegacyPlaceholder {}
