//! Type cardano-cli surface.
//!
//! Mirrors upstream `Cardano.CLI.Type.*`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! Type sub-tree. Upstream has no top-level
//! `Cardano/CLI/Type.hs`; the surface lives entirely under
//! `Cardano/CLI/Type/*.hs`.

pub mod common;
pub mod error;
pub mod governance;
pub mod key;
pub mod monad_warning;
pub mod output;
