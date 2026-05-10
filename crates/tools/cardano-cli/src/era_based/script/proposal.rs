//! EraBased proposal sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/script/proposal/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Script/Proposal.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Script/Proposal/*.hs`.

pub mod read;
pub mod r#type;
