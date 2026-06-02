//! EraIndependent hash parser notes.
//!
//! Mirrors upstream `Cardano.CLI.EraIndependent.Hash.Option`, where
//! `pHashCmds` defines the `hash` subparser and `pGenesisHash` parses
//! `hash genesis-file --genesis FILE`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Hash/Option.hs`.
//! Yggdrasil uses `clap` derive rather than optparse-applicative; the
//! executable parser is therefore the `Subcommand` derive on
//! [`crate::era_independent::hash::command::HashCmds`] plus the
//! top-level `Command::Hash` arm.

pub use crate::era_independent::hash::command::HashCmds;
