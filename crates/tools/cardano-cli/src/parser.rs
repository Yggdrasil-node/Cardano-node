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

/// Top-level clap `Parser` shell. The `command: Command` field
/// expands via the `Subcommand` derive on `Command` into the
/// three subcommand arms `version` / `show-upstream-config` /
/// `query-tip`. Mirrors upstream's `ClientCommand` optparse-
/// applicative aggregate parser.
#[derive(Parser)]
#[command(name = "yggdrasil-cardano-cli", version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

/// Parse `argv` into a [`Command`].
///
/// Mirrors upstream `parseClientCommand` from `Cardano.CLI.Parser`.
/// R503 (May 2026): wired to clap's `try_parse_from` via the new
/// `Args { command: Command }` aggregate — was an
/// `Err(NotYetMigrated)` stub before. Callers (the standalone
/// `[[bin]]` target when it lands, plus the existing
/// `parser::tests`) can now actually produce `Command::Version` /
/// `ShowUpstreamConfig { upstream_config_root }` /
/// `QueryTip { socket_path, network_magic }` from argv.
pub fn parse_command<I, T>(args: I) -> Result<Command, ParseError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let parsed = Args::try_parse_from(args)?;
    Ok(parsed.command)
}

/// Parse error returned by [`parse_command`].
///
/// Mirrors upstream `ClientCommandErrors` from `Cardano.CLI.Run`.
/// R503 retired the prior `NotYetMigrated` variant after `parse_command`
/// became operational via `Args::try_parse_from`; today the only
/// failure mode is the clap parser itself (which also raises
/// `--help` / `--version` short-circuit "errors" with
/// `kind() == DisplayHelp / DisplayVersion`).
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// A clap parser failure surfaced through. Also wraps clap's
    /// `--help` / `--version` short-circuit "errors" — the binary
    /// `main` discriminates via `err.kind()` and prints those on
    /// stdout (exit 0) vs other variants on stderr (exit 2).
    #[error("{0}")]
    Clap(#[from] clap::Error),
}

/// Re-export of `clap::Parser` so consumers can wire their own
/// derive-based parsers against the same Command type. Useful for the
/// node binary's transitional integration in R289.
pub trait ClapBackend: Parser {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `version` subcommand parses cleanly to `Command::Version`.
    #[test]
    fn parses_version_subcommand() {
        let cmd = parse_command(["yggdrasil-cardano-cli", "version"]).expect("parse");
        assert_eq!(cmd, Command::Version);
    }

    /// `show-upstream-config --network mainnet` parses to the
    /// expected variant with `upstream_config_root: None`.
    #[test]
    fn parses_show_upstream_config_default() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "show-upstream-config",
            "--network",
            "mainnet",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::ShowUpstreamConfig {
                network: "mainnet".to_string(),
                upstream_config_root: None,
            }
        );
    }

    /// `show-upstream-config --network preview --upstream-config-root /opt/...`
    /// parses the operator-supplied override.
    #[test]
    fn parses_show_upstream_config_with_root() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "show-upstream-config",
            "--network",
            "preview",
            "--upstream-config-root",
            "/opt/cardano",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::ShowUpstreamConfig {
                network: "preview".to_string(),
                upstream_config_root: Some(PathBuf::from("/opt/cardano")),
            }
        );
    }

    /// `query-tip --socket-path /tmp/node.socket` parses with the
    /// canonical socket-path argument.
    #[test]
    fn parses_query_tip_with_socket_path() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "query-tip",
            "--socket-path",
            "/tmp/node.socket",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::QueryTip {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: None,
            }
        );
    }

    /// `query-tip --socket-path … --network-magic N` parses the
    /// magic override.
    #[test]
    fn parses_query_tip_with_network_magic() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "query-tip",
            "--socket-path",
            "/tmp/node.socket",
            "--network-magic",
            "764824073",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::QueryTip {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: Some(764_824_073),
            }
        );
    }

    /// `address-key-gen --verification-key-file … --signing-key-file …`
    /// parses to the expected variant.
    #[test]
    fn parses_address_key_gen() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "address-key-gen",
            "--verification-key-file",
            "/tmp/p.vkey",
            "--signing-key-file",
            "/tmp/p.skey",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::AddressKeyGen {
                verification_key_file: PathBuf::from("/tmp/p.vkey"),
                signing_key_file: PathBuf::from("/tmp/p.skey"),
            }
        );
    }

    /// Unknown subcommand surfaces through `ParseError::Clap`.
    #[test]
    fn rejects_unknown_subcommand() {
        let result = parse_command(["yggdrasil-cardano-cli", "bogus-subcommand"]);
        assert!(
            matches!(result, Err(ParseError::Clap(_))),
            "expected ParseError::Clap; got {result:?}"
        );
    }
}
