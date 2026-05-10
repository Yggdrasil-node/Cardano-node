//! Compatible-cluster StakePool sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `compatible/stake_pool/*` sub-modules. Upstream has no `Cardano/CLI/
//! Compatible/StakePool.hs` top-level file; the StakePool surface lives entirely
//! under `Cardano/CLI/Compatible/StakePool/*.hs`.

pub mod command;
pub mod option;
pub mod run;
