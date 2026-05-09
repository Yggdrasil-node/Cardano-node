//! Byron-era byron heavyweight-delegation certificate construction + verification.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Byron/Delegation.hs`.
//! R290 lands the file with the API skeleton; concrete Byron-era
//! command implementations port from upstream over subsequent
//! rounds + after the integration test in R295 confirms the wire
//! shape against the upstream `cardano-cli` binary.

/// Byron-era byron heavyweight-delegation certificate construction + verification placeholder.
///
/// Mirrors upstream `Cardano.CLI.Byron.Delegation` types; the Rust
/// port lands as concrete subcommand implementations come online.
/// Currently empty enum so the module compiles + can be extended
/// in subsequent rounds without breaking the public path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DelegationPlaceholder {}
