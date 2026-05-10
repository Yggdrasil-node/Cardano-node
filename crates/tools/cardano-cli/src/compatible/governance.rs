//! Compatible-cluster Governance sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `compatible/governance/*` sub-modules. Upstream has no `Cardano/CLI/
//! Compatible/Governance.hs` top-level file; the Governance surface lives entirely
//! under `Cardano/CLI/Compatible/Governance/*.hs`.

pub mod command;
pub mod option;
pub mod run;
pub mod types;
