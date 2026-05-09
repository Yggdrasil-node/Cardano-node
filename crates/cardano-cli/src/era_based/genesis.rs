//! EraBased genesis sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/genesis/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Genesis.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Genesis/*.hs`.

pub mod command;
pub mod create_testnet_data;
pub mod internal;
pub mod option;
pub mod run;
