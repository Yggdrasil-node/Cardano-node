//! EraIndependent transaction view sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/debug/transaction_view/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Debug/TransactionView.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Debug/TransactionView/*.hs`.

pub mod command;
pub mod run;
