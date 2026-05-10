//! EraIndependent cip sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/cip/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Cip.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Cip/*.hs`.

pub mod cip129;
pub mod command;
pub mod common;
pub mod option;
pub mod run;
