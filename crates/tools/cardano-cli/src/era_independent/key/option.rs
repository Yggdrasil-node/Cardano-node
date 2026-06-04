//! EraIndependent option.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Key/Option.hs`.
//! R293 landed the file with the API skeleton. R520 maps upstream
//! `pKeyCmds` to clap's derive parser on [`KeyCmds`].

pub use crate::era_independent::key::command::KeyCmds;
