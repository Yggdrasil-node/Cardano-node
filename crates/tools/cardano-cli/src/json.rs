//! Json cardano-cli surface.
//!
//! Mirrors upstream `Cardano.CLI.Json.*`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! Json sub-tree. Upstream has no top-level
//! `Cardano/CLI/Json.hs`; the surface lives entirely under
//! `Cardano/CLI/Json/*.hs`.

pub mod encode;
