//! EraIndependent address sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/address/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Address.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Address/*.hs`.

pub mod command;
pub mod info;
pub mod option;
pub mod run;
