//! CLI module umbrella (Yggdrasil-side parent shell for cli/types.rs + cli/parsers.rs).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell that
//! re-exports the per-concern leaf modules to keep call sites
//! unchanged from upstream's `Cardano.TxSubmit.CLI.<Leaf>`
//! qualified names. R335 file-mirror skeleton.

pub mod parsers;
pub mod types;
