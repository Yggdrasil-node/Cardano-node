//! EraBased internal sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/genesis/internal/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Genesis/Internal.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Genesis/Internal/*.hs`.

pub mod byron;
pub mod common;
