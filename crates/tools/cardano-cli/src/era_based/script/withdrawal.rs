//! EraBased withdrawal sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/script/withdrawal/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Script/Withdrawal.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Script/Withdrawal/*.hs`.

pub mod read;
pub mod r#type;
