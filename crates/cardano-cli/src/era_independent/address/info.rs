//! EraIndependent info sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/address/info/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Address/Info.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Address/Info/*.hs`.

pub mod run;
