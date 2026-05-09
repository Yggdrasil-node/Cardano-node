//! EraIndependent key sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/key/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Key.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Key/*.hs`.

pub mod command;
pub mod option;
pub mod run;
