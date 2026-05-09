//! EraBased internal sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/stake_pool/internal/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/StakePool/Internal.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/StakePool/Internal/*.hs`.

pub mod metadata;
