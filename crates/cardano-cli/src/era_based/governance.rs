//! EraBased governance sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/governance/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Governance.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Governance/*.hs`.

pub mod actions;
pub mod command;
pub mod committee;
pub mod d_rep;
pub mod genesis_key_delegation_certificate;
pub mod option;
pub mod run;
pub mod vote;
