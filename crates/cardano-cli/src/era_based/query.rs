//! EraBased query sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/query/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Query.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Query/*.hs`.

pub mod command;
pub mod option;
pub mod run;
