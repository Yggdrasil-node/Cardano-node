//! EraIndependent option.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Address/Option.hs`.
//! R293 landed the file with the API skeleton. R519 maps upstream
//! `pAddressCmds` to clap's derive parser on [`AddressCmds`].

pub use crate::era_independent::address::command::AddressCmds;
