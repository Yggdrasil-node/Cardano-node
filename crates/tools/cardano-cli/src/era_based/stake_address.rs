//! EraBased stake address sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/stake_address/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/StakeAddress.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/StakeAddress/*.hs`.

pub mod command;
pub mod option;
pub mod run;
