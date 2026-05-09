//! Legacy genesis sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `legacy/genesis/*` sub-modules. Upstream has no `Cardano/CLI/Legacy/Genesis.hs`
//! top-level file; the surface lives under
//! `Cardano/CLI/Legacy/Genesis/*.hs`.

pub mod command;
pub mod run;
