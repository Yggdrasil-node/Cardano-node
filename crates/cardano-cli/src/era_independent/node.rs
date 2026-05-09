//! EraIndependent node sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/node/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Node.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Node/*.hs`.

pub mod command;
pub mod option;
pub mod run;
