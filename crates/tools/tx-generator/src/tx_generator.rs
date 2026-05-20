//! Transaction-generator support namespace.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Parent shell for upstream
//! `Cardano.TxGenerator.*` support modules. Concrete leaf files mirror
//! their upstream Haskell counterparts.

pub mod fund;
pub mod fund_queue;
pub mod internal;
pub mod plutus_context;
pub mod utils;
pub mod utxo;
