//! Compatible-cluster Transaction sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `compatible/transaction/*` sub-modules. Upstream has no `Cardano/CLI/
//! Compatible/Transaction.hs` top-level file; the Transaction surface lives entirely
//! under `Cardano/CLI/Compatible/Transaction/*.hs`.

pub mod command;
pub mod option;
pub mod run;
pub mod script_witness;
pub mod tx_out;
