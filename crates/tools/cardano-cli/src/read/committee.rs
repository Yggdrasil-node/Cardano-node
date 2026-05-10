//! Read committee sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `read/committee/*` sub-modules. Upstream has no `Cardano/CLI/Read/Committee.hs`
//! top-level file; the surface lives under
//! `Cardano/CLI/Read/Committee/*.hs`.

pub mod cold_key;
pub mod hot_key;
