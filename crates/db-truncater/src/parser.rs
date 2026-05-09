//! CLI argument parser shell for the `db-truncater` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/app/DBTruncater/Parsers.hs.
//!
//! Direct ports of upstream's `commandLineParser`:
//!
//! - `--db PATH` → [`Args::db`] (mandatory).
//! - `--truncate-after-slot SLOT_NUMBER` → [`Args::truncate_after_slot`]
//!   (mutually exclusive with `--truncate-after-block`; one is mandatory).
//! - `--truncate-after-block BLOCK_NUMBER` → [`Args::truncate_after_block`].
//! - `--verbose` → [`Args::verbose`].
//! - `--help` / `--version` short-circuit via [`ParseError`] variants.
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `db-truncater` binary; fixtures captured at R335 live at
//! `crates/db-truncater/tests/fixtures/upstream-{help,version}.txt`.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - Upstream's `parseCardanoArgs` (consensus-arg threading via
//!   `CardanoBlockArgs`) is era-aware; Yggdrasil's storage layer is
//!   era-agnostic at the on-disk level so this surface is collapsed.
//!   Tracked in `crates/db-truncater/AGENTS.md`.

use crate::types::{DBTruncaterConfig, TruncateAfter};
use yggdrasil_ledger::{BlockNo, SlotNo};

/// Byte-for-byte mirror of upstream `db-truncater --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `db-truncater --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line arguments.
///
/// Mandatory fields are required to construct a [`DBTruncaterConfig`]
/// via [`into_config`]. Optional fields have sensible defaults
/// (`verbose` defaults to `false`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Args {
    /// `--db PATH`. Mandatory; `into_config` errors if absent.
    pub db: Option<String>,
    /// `--truncate-after-slot SLOT_NUMBER`. One of `--truncate-after-slot`
    /// or `--truncate-after-block` is mandatory; `into_config` errors
    /// if both or neither are supplied.
    pub truncate_after_slot: Option<u64>,
    /// `--truncate-after-block BLOCK_NUMBER`.
    pub truncate_after_block: Option<u64>,
    /// `--verbose` flag.
    pub verbose: bool,
}

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen.
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` / `-v` was seen.
    #[error("(--version requested)")]
    VersionRequested,
    /// An unknown flag was passed.
    #[error("Invalid option `{0}'")]
    UnknownFlag(String),
    /// A flag requiring a value was passed without one.
    #[error("flag `{0}' requires a value")]
    MissingValue(String),
    /// A flag's value failed to parse.
    #[error("flag `{0}' has invalid value: {1}")]
    InvalidValue(String, String),
}

/// Errors when promoting parsed [`Args`] into a [`DBTruncaterConfig`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConfigError {
    /// `--db PATH` was not supplied.
    #[error("missing required flag: --db")]
    MissingDb,
    /// Neither `--truncate-after-slot` nor `--truncate-after-block` was supplied.
    #[error("missing truncate target: --truncate-after-slot or --truncate-after-block")]
    MissingTruncateTarget,
    /// Both `--truncate-after-slot` and `--truncate-after-block` were supplied.
    #[error(
        "conflicting truncate targets: --truncate-after-slot and --truncate-after-block are mutually exclusive"
    )]
    ConflictingTruncateTargets,
}

/// Parse a slice of command-line arguments. Mirrors upstream's
/// `commandLineParser` flag grammar.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Args::default();
    let mut iter = args.into_iter().peekable();

    while let Some(arg) = iter.next() {
        let arg = arg.as_ref().to_string();
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "-v" | "--version" => return Err(ParseError::VersionRequested),
            "--verbose" => {
                out.verbose = true;
            }
            "--db" => {
                let v = take_value(&mut iter, &arg)?;
                out.db = Some(v);
            }
            "--truncate-after-slot" => {
                let v = take_value(&mut iter, &arg)?;
                out.truncate_after_slot = Some(parse_u64(&arg, &v)?);
            }
            "--truncate-after-block" => {
                let v = take_value(&mut iter, &arg)?;
                out.truncate_after_block = Some(parse_u64(&arg, &v)?);
            }
            other if other.starts_with('-') => {
                return Err(ParseError::UnknownFlag(other.to_string()));
            }
            other => {
                return Err(ParseError::UnknownFlag(other.to_string()));
            }
        }
    }

    Ok(out)
}

/// Promote parsed [`Args`] to a fully-validated [`DBTruncaterConfig`].
///
/// Errors:
/// - [`ConfigError::MissingDb`] if `--db` was not supplied.
/// - [`ConfigError::MissingTruncateTarget`] if neither truncate flag.
/// - [`ConfigError::ConflictingTruncateTargets`] if both truncate flags.
pub fn into_config(args: &Args) -> Result<DBTruncaterConfig, ConfigError> {
    let db_dir = args
        .db
        .as_ref()
        .map(std::path::PathBuf::from)
        .ok_or(ConfigError::MissingDb)?;

    let truncate_after = match (args.truncate_after_slot, args.truncate_after_block) {
        (Some(_), Some(_)) => return Err(ConfigError::ConflictingTruncateTargets),
        (Some(slot), None) => TruncateAfter::TruncateAfterSlot(SlotNo(slot)),
        (None, Some(block)) => TruncateAfter::TruncateAfterBlock(BlockNo(block)),
        (None, None) => return Err(ConfigError::MissingTruncateTarget),
    };

    Ok(DBTruncaterConfig {
        db_dir,
        truncate_after,
        verbose: args.verbose,
    })
}

fn take_value<I, S>(iter: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, ParseError>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    iter.next()
        .map(|v| v.as_ref().to_string())
        .ok_or_else(|| ParseError::MissingValue(flag.to_string()))
}

fn parse_u64(flag: &str, value: &str) -> Result<u64, ParseError> {
    value.parse().map_err(|e: std::num::ParseIntError| {
        ParseError::InvalidValue(flag.to_string(), e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_help_long() {
        assert_eq!(parse_args(["--help"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_help_short() {
        assert_eq!(parse_args(["-h"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_version_long() {
        assert_eq!(parse_args(["--version"]), Err(ParseError::VersionRequested));
    }

    #[test]
    fn parses_db_flag() {
        let args = parse_args(["--db", "/var/lib/cardano-node/db"]).expect("parses");
        assert_eq!(args.db.as_deref(), Some("/var/lib/cardano-node/db"));
    }

    #[test]
    fn parses_truncate_after_slot() {
        let args = parse_args(["--truncate-after-slot", "100000"]).expect("parses");
        assert_eq!(args.truncate_after_slot, Some(100_000));
    }

    #[test]
    fn parses_truncate_after_block() {
        let args = parse_args(["--truncate-after-block", "5000"]).expect("parses");
        assert_eq!(args.truncate_after_block, Some(5000));
    }

    #[test]
    fn parses_verbose() {
        let args = parse_args(["--verbose"]).expect("parses");
        assert!(args.verbose);
    }

    #[test]
    fn parses_full_canonical_invocation_slot_form() {
        let args = parse_args([
            "--db",
            "/var/lib/cardano-node/db",
            "--truncate-after-slot",
            "1000000",
            "--verbose",
        ])
        .expect("parses");
        assert_eq!(args.db.as_deref(), Some("/var/lib/cardano-node/db"));
        assert_eq!(args.truncate_after_slot, Some(1_000_000));
        assert!(args.verbose);
    }

    #[test]
    fn parses_full_canonical_invocation_block_form() {
        let args =
            parse_args(["--db", "/tmp/db", "--truncate-after-block", "5000"]).expect("parses");
        assert_eq!(args.db.as_deref(), Some("/tmp/db"));
        assert_eq!(args.truncate_after_block, Some(5000));
        assert!(!args.verbose);
    }

    #[test]
    fn rejects_unknown_flag() {
        assert!(matches!(
            parse_args(["--definitely-not-real"]),
            Err(ParseError::UnknownFlag(_)),
        ));
    }

    #[test]
    fn rejects_missing_value() {
        assert!(matches!(
            parse_args(["--db"]),
            Err(ParseError::MissingValue(_)),
        ));
    }

    #[test]
    fn rejects_invalid_slot_number() {
        assert!(matches!(
            parse_args(["--truncate-after-slot", "not-a-number"]),
            Err(ParseError::InvalidValue(_, _)),
        ));
    }

    #[test]
    fn into_config_full_slot_invocation() {
        let args = parse_args([
            "--db",
            "/var/lib/cardano-node/db",
            "--truncate-after-slot",
            "1000000",
            "--verbose",
        ])
        .expect("parses");
        let config = into_config(&args).expect("validates");
        assert_eq!(config.db_dir.to_str(), Some("/var/lib/cardano-node/db"));
        assert!(config.verbose);
        assert!(matches!(
            config.truncate_after,
            TruncateAfter::TruncateAfterSlot(SlotNo(1_000_000))
        ));
    }

    #[test]
    fn into_config_full_block_invocation() {
        let args =
            parse_args(["--db", "/tmp/db", "--truncate-after-block", "5000"]).expect("parses");
        let config = into_config(&args).expect("validates");
        assert!(matches!(
            config.truncate_after,
            TruncateAfter::TruncateAfterBlock(BlockNo(5000))
        ));
    }

    #[test]
    fn into_config_rejects_missing_db() {
        let args = parse_args(["--truncate-after-slot", "100"]).expect("parses");
        assert_eq!(into_config(&args), Err(ConfigError::MissingDb));
    }

    #[test]
    fn into_config_rejects_missing_truncate_target() {
        let args = parse_args(["--db", "/tmp/db"]).expect("parses");
        assert_eq!(into_config(&args), Err(ConfigError::MissingTruncateTarget));
    }

    #[test]
    fn into_config_rejects_conflicting_truncate_targets() {
        let args = parse_args([
            "--db",
            "/tmp/db",
            "--truncate-after-slot",
            "100",
            "--truncate-after-block",
            "5",
        ])
        .expect("parses");
        assert_eq!(
            into_config(&args),
            Err(ConfigError::ConflictingTruncateTargets)
        );
    }

    #[test]
    fn help_fixture_non_empty() {
        assert!(!HELP_TEXT.is_empty());
    }

    #[test]
    fn version_fixture_non_empty() {
        assert!(!VERSION_TEXT.is_empty());
    }
}
