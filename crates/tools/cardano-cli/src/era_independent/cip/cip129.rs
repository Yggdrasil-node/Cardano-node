//! EraIndependent cip129 sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/cip/cip129/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Cip/Cip129.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Cip/Cip129/*.hs`.

pub mod command;
pub mod internal;
pub mod option;
pub mod run;
