//! EraBased vote sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/governance/vote/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Governance/Vote.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Governance/Vote/*.hs`.

pub mod command;
pub mod option;
pub mod run;
