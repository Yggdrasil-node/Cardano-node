//! IO cardano-cli surface.
//!
//! Mirrors upstream `Cardano.CLI.IO.*`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! IO sub-tree. Upstream has no top-level
//! `Cardano/CLI/IO.hs`; the surface lives entirely under
//! `Cardano/CLI/IO/*.hs`.

pub mod lazy;
