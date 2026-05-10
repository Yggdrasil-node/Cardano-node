//! EraIndependent check node configuration sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/debug/check_node_configuration/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Debug/CheckNodeConfiguration.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Debug/CheckNodeConfiguration/*.hs`.

pub mod command;
pub mod run;
