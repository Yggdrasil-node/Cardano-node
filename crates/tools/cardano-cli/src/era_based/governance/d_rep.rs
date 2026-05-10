//! EraBased d rep sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/governance/d_rep/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Governance/DRep.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Governance/DRep/*.hs`.

pub mod command;
pub mod option;
pub mod run;
