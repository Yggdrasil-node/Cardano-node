//! EraIndependent internal sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/hash/internal/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Hash/Internal.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Hash/Internal/*.hs`.

pub mod common;
