//! Pure-Rust port of upstream `cardano-cli`.
//!
//! Mirrors the upstream cabal package
//! `cardano-cli/cardano-cli/cardano-cli.cabal` library component.
//! Submodules track upstream's `Cardano.CLI.*` module hierarchy 1:1
//! at the file-name level (snake_case-of-PascalCase).
//!
//! The crate exists as a separate workspace member (rather than
//! growing inside `crates/node/cardano-node/src/`) because the cardano-cli surface is
//! large (~150 upstream files), has its own dependency graph
//! (cardano-api types, transaction-construction, key derivation,
//! text-envelope codec), and shipping it independently keeps
//! `crates/node/` an integration layer per the workspace topology rule in
//! [`CLAUDE.md`](../../CLAUDE.md).
//!
//! ## Phase F — operator surface
//!
//! The crate skeleton landed in R289 and the upstream-shaped Byron /
//! Compatible / Shelley / Alonzo / Babbage / Conway file tree landed in
//! R290-R295. The operator-essential C-arc is now implemented here:
//! standalone `yggdrasil-cardano-cli` commands dispatch through this
//! crate, and the `yggdrasil-node cardano-cli` compatibility wrapper
//! should remain a thin parser adapter over these helpers.
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
pub mod io;
pub mod json;
pub mod legacy;
pub mod lsq;
#[cfg(feature = "lsq-tokio")]
pub mod lsq_tokio;
pub mod option;
pub mod orphan;
pub mod os;
pub mod parser;
pub mod read;
pub mod render;
pub mod run;
pub mod top_handler;
pub mod r#type;

pub use command::Command;
pub use run::run_command;
