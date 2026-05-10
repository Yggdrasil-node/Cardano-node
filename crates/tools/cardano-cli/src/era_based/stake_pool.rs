//! EraBased stake pool sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/stake_pool/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/StakePool.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/StakePool/*.hs`.

pub mod command;
pub mod internal;
pub mod option;
pub mod run;
