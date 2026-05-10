//! Compatible-cluster StakeAddress sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `compatible/stake_address/*` sub-modules. Upstream has no `Cardano/CLI/
//! Compatible/StakeAddress.hs` top-level file; the StakeAddress surface lives entirely
//! under `Cardano/CLI/Compatible/StakeAddress/*.hs`.

pub mod command;
pub mod option;
pub mod run;
