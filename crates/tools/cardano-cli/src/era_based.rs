//! EraBased cardano-cli command surface.
//!
//! Mirrors upstream `Cardano.CLI.EraBased.*`. EraBased is the per-
//! era-aware command surface that adapts upstream's
//! `cardano.api.IsShelleyBasedEra` constraint into a single set of
//! subcommands operators invoke against any post-Byron era (Shelley,
//! Allegra, Mary, Alonzo, Babbage, Conway).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! EraBased sub-tree. Upstream `cardano-cli` does not have a top-level
//! `Cardano/CLI/EraBased.hs`; the EraBased surface lives entirely under
//! `Cardano/CLI/EraBased/*.hs` (57 files across 25 sub-directories).

pub mod command;
pub mod common;
pub mod genesis;
pub mod governance;
pub mod option;
pub mod query;
pub mod run;
pub mod script;
pub mod stake_address;
pub mod stake_pool;
pub mod text_view;
pub mod transaction;
