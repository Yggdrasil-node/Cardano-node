//! Byron-era cardano-cli command surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the 10
//! Byron sub-modules. Upstream `cardano-cli` does not have a
//! `Cardano/CLI/Byron.hs` top-level file; the Byron-era surface lives
//! entirely under `Cardano/CLI/Byron/*.hs` (10 modules: Command,
//! Delegation, Genesis, Key, Legacy, Parser, Run, Tx, UpdateProposal,
//! Vote). This Rust parent file aggregates them into the
//! `cardano_cli::byron` namespace.

pub mod command;
pub mod delegation;
pub mod genesis;
pub mod key;
pub mod legacy;
pub mod parser;
pub mod run;
pub mod tx;
pub mod update_proposal;
pub mod vote;
