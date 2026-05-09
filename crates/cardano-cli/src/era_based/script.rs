//! EraBased script sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/script/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Script.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Script/*.hs`.

pub mod certificate;
pub mod mint;
pub mod proposal;
pub mod read;
pub mod spend;
pub mod r#type;
pub mod vote;
pub mod withdrawal;
