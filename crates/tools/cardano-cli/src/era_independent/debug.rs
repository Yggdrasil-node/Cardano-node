//! EraIndependent debug sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_independent/debug/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraIndependent/Debug.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraIndependent/Debug/*.hs`.

pub mod check_node_configuration;
pub mod command;
pub mod log_epoch_state;
pub mod option;
pub mod run;
pub mod transaction_view;
