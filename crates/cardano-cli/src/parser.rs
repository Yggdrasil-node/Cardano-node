//! cardano-cli argument parser.
//!
//! Mirrors upstream `Cardano.CLI.Parser` — the optparse-applicative
//! parser that produces a `ClientCommand` from `argv`. Yggdrasil uses
//! `clap` (`derive` style) instead of optparse-applicative; the
//! upstream parser layout is the conceptual mirror.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Parser.hs`.
//! R289 ships only the top-level `parse_command` shell. The full
//! parser tree (per-cluster sub-parsers for Byron / Compatible /
//! per-era / Legacy) lands in R290–R295 alongside the runners.

use clap::Parser;

use crate::command::Command;

/// Parse `argv` into a [`Command`].
///
/// Mirrors upstream `parseClientCommand` from `Cardano.CLI.Parser`.
///
/// # R289 bootstrap state
///
/// Returns a stub error directing callers to use the node binary's
/// existing `yggdrasil-node cardano-cli` subcommand parser (in
/// `node/src/cli.rs::CardanoCliCommand`). The migration into this
/// crate happens in R290+.
pub fn parse_command<I, T>(_args: I) -> Result<Command, ParseError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    // R289 stub: defer to the node binary's existing clap parser
    // until per-cluster sub-parsers land.
    Err(ParseError::NotYetMigrated)
}

/// Parse error returned by [`parse_command`].
///
/// Mirrors upstream `ClientCommandErrors` from `Cardano.CLI.Run`.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The full optparse-applicative parser has not yet been ported
    /// into this crate; callers should use `node/src/cli.rs` for now.
    #[error(
        "yggdrasil-cardano-cli parser is the R289 skeleton; use the \
         node binary's `cardano-cli` subcommand for now (migration \
         scheduled for R290+)"
    )]
    NotYetMigrated,
    /// A clap parser failure surfaced through.
    #[error("clap parser error: {0}")]
    Clap(#[from] clap::Error),
}

/// Re-export of `clap::Parser` so consumers can wire their own
/// derive-based parsers against the same Command type. Useful for the
/// node binary's transitional integration in R289.
pub trait ClapBackend: Parser {}
