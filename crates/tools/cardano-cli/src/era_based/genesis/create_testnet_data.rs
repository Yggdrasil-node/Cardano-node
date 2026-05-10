//! EraBased create testnet data sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/genesis/create_testnet_data/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Genesis/CreateTestnetData.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Genesis/CreateTestnetData/*.hs`.

pub mod run;
