//! EraBased vote sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/script/vote/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Script/Vote.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Script/Vote/*.hs`.

pub mod read;
pub mod r#type;
