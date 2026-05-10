//! EraBased transaction sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/transaction/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Transaction.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Transaction/*.hs`.

pub mod command;
pub mod internal;
pub mod option;
pub mod run;
