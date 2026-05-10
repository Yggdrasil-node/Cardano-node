//! EraIndependent ping sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/ping/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Ping.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Ping/*.hs`.

pub mod command;
pub mod option;
pub mod run;
