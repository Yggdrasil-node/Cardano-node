//! EraBased internal sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/transaction/internal/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Transaction/Internal.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Transaction/Internal/*.hs`.

pub mod hash_check;
