//! Subcommand implementations for the `yggdrasil-node` binary.
//!
//! Mirrors upstream `Cardano.CLI.*` organization. Each submodule
//! groups the helpers and dispatchers for one CLI subcommand surface.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-cli/tree/master/cardano-cli/src/Cardano/CLI>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over
//! the per-CLI-subcommand modules under `commands/` (run,
//! query, status, submit-tx, tx-mempool, etc.). Mirrors the
//! upstream cardano-cli organization where each subcommand
//! lives in its own `Cardano.CLI.<Domain>.<Subcommand>` module;
//! Yggdrasil collapses the subset relevant to the node binary
//! into `commands/*.rs`.

pub mod cardano_cli;
pub mod configuration;
#[cfg(unix)]
pub mod query;
pub mod run;
pub mod status;
#[cfg(unix)]
pub mod submit_tx;
#[cfg(unix)]
pub mod tx_mempool;
pub mod validate_config;
