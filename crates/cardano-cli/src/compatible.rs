//! Compatible-cluster cardano-cli command surface.
//!
//! Mirrors upstream `Cardano.CLI.Compatible.*`. Compatible is the
//! era-shared command surface that adapts upstream's per-era types
//! (Byron / Shelley / Allegra / Mary / Alonzo / Babbage / Conway)
//! into a single set of subcommands the operator invokes the same
//! way across eras.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! Compatible sub-tree. Upstream `cardano-cli` does not have a
//! `Cardano/CLI/Compatible.hs` top-level file; the Compatible surface
//! lives entirely under `Cardano/CLI/Compatible/{Command, Exception,
//! Governance, Json, Option, Read, Run, StakeAddress, StakePool,
//! Transaction}.hs` (5 top-level + 5 sub-trees, 21 files total).

pub mod command;
pub mod exception;
pub mod governance;
pub mod json;
pub mod option;
pub mod read;
pub mod run;
pub mod stake_address;
pub mod stake_pool;
pub mod transaction;
