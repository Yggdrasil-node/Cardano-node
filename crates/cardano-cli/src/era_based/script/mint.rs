//! EraBased mint sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/script/mint/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Script/Mint.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Script/Mint/*.hs`.

pub mod read;
pub mod r#type;
