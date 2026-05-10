//! Legacy cardano-cli surface.
//!
//! Mirrors upstream `Cardano.CLI.Legacy.*`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! Legacy sub-tree. Upstream has no top-level
//! `Cardano/CLI/Legacy.hs`; the surface lives entirely under
//! `Cardano/CLI/Legacy/*.hs`.

pub mod command;
pub mod genesis;
pub mod option;
pub mod run;
