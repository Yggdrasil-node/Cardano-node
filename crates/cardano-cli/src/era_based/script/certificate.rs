//! EraBased certificate sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/script/certificate/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Script/Certificate.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Script/Certificate/*.hs`.

pub mod read;
pub mod r#type;
