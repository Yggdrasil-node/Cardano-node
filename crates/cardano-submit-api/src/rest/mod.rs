//! REST module umbrella for rest/{types,parsers,web}.rs.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell that
//! re-exports the per-concern leaf modules to keep call sites
//! unchanged from upstream's `Cardano.TxSubmit.Rest.<Leaf>`
//! qualified names. R335 file-mirror skeleton.

pub mod parsers;
pub mod types;
pub mod web;
