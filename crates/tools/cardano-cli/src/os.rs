//! OS cardano-cli surface.
//!
//! Mirrors upstream `Cardano.CLI.OS.*`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! OS sub-tree. Upstream has no top-level
//! `Cardano/CLI/OS.hs`; the surface lives entirely under
//! `Cardano/CLI/OS/*.hs`.

pub mod posix;
