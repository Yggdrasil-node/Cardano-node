//! Pure-Rust port of upstream `cardano-cli`.
//!
//! Mirrors the upstream cabal package
//! `cardano-cli/cardano-cli/cardano-cli.cabal` library component.
//! Submodules track upstream's `Cardano.CLI.*` module hierarchy 1:1
//! at the file-name level (snake_case-of-PascalCase).
//!
//! The crate exists as a separate workspace member (rather than
//! growing inside `node/src/`) because the cardano-cli surface is
//! large (~150 upstream files), has its own dependency graph
//! (cardano-api types, transaction-construction, key derivation,
//! text-envelope codec), and shipping it independently keeps `node/`
//! an integration layer per the workspace topology rule in
//! [`CLAUDE.md`](../../CLAUDE.md).
//!
//! ## Phase F — R289 bootstrap
//!
//! R289 lands the crate skeleton. The Byron / Compatible / Shelley /
//! Alonzo / Babbage / Conway clusters land in R290–R295 (~150 files
//! total). The `yggdrasil-node cardano-cli` subcommand currently
//! delegates to `node/src/commands/cardano_cli.rs`; the delegation
//! moves through this crate as the per-cluster rounds land.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. `lib.rs` is a Rust convention — upstream
//! does not have a `Lib.hs`. The crate's surface entry points live
//! in [`command`], [`run`], [`parser`], and [`render`] (each mirroring
//! a single upstream `.hs` file 1:1). This module file just declares
//! the sub-module tree.

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod byron;
pub mod command;
pub mod compatible;
pub mod environment;
pub mod era_based;
pub mod era_independent;
pub mod helper;
pub mod option;
pub mod orphan;
pub mod parser;
pub mod render;
pub mod run;

pub use command::Command;
pub use run::run_command;
