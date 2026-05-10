//! CLI argument parser for the `db-synthesizer` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/app/DBSynthesizer/Parsers.hs.
//!
//! Direct port of upstream's `parserCommandLine ::
//! Parser (NodeFilePaths, NodeCredentials, DBSynthesizerOptions)`
//! and the per-section `parseNodeFilePaths` /
//! `parseNodeCredentials` / `parseDBSynthesizerOptions` parsers.
//!
//! Mandatory flags:
//!
//! - `--config FILE` — path to the node's `config.json`
//!   (`parseNodeConfigFilePath`).
//! - `--db PATH` — path to the Chain DB (`parseChainDBFilePath`).
//!
//! Forge-limit flags (mutually exclusive; one required):
//!
//! - `-s` / `--slots NUMBER` (`ForgeLimitSlot`).
//! - `-b` / `--blocks NUMBER` (`ForgeLimitBlock`).
//! - `-e` / `--epochs NUMBER` (`ForgeLimitEpoch`).
//!
//! Open-mode flags (mutually exclusive; default `OpenCreate`):
//!
//! - `-f` — Force overwrite an existing Chain DB (`OpenCreateForce`).
//! - `-a` — Append to an existing Chain DB (`OpenAppend`).
//!
//! Optional credential flags:
//!
//! - `--shelley-operational-certificate FILE`
//! - `--shelley-vrf-key FILE`
//! - `--shelley-kes-key FILE`
//! - `--bulk-credentials-file FILE`
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `db-synthesizer` binary; fixtures captured at R335 live at
//! `crates/db-synthesizer/tests/fixtures/upstream-{help,version}.txt`.

use std::path::PathBuf;

use yggdrasil_ledger::SlotNo;

use crate::types::{
    DBSynthesizerOpenMode, DBSynthesizerOptions, ForgeLimit, NodeCredentials, NodeFilePaths,
};

/// Byte-for-byte mirror of upstream `db-synthesizer --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `db-synthesizer --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line arguments — the upstream
/// `(NodeFilePaths, NodeCredentials, DBSynthesizerOptions)` triple.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Args {
    /// Node file paths (config + chain-DB).
    pub paths: NodeFilePaths,
    /// Operator credentials (cert / VRF / KES / bulk).
    pub credentials: NodeCredentials,
    /// Synthesizer-run options (forge limit + open mode).
    pub options: DBSynthesizerOptions,
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
    /// `--config FILE` was not supplied.
    #[error("missing required flag: --config")]
    MissingConfig,
    /// `--db PATH` was not supplied.
    #[error("missing required flag: --db")]
    MissingDb,
    /// No forge-limit flag was supplied (need exactly one of `-s`/`-b`/`-e`).
    #[error("missing forge limit: supply --slots, --blocks, or --epochs")]
    MissingForgeLimit,
    /// Multiple forge-limit flags were supplied.
    #[error("conflicting forge limits: --slots / --blocks / --epochs are mutually exclusive")]
    ConflictingForgeLimits,
    /// Both `-f` and `-a` were supplied.
    #[error("conflicting open modes: -f (force) and -a (append) are mutually exclusive")]
    ConflictingOpenModes,
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

#[derive(Clone, Debug, Default)]
struct RawArgs {
    config: Option<PathBuf>,
    db: Option<PathBuf>,
    op_cert: Option<PathBuf>,
    vrf_key: Option<PathBuf>,
    kes_key: Option<PathBuf>,
    bulk_creds: Option<PathBuf>,
    slots: Option<u64>,
    blocks: Option<u64>,
    epochs: Option<u64>,
    force: bool,
    append: bool,
}

/// Parse a slice of command-line arguments into [`Args`]. Mirror of
/// upstream `parserCommandLine`.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut raw = RawArgs::default();
    let mut iter = args.into_iter().peekable();

    while let Some(arg) = iter.next() {
        let arg = arg.as_ref().to_string();
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "--version" => return Err(ParseError::VersionRequested),
            "--config" => {
                let v = take_value(&mut iter, &arg)?;
                raw.config = Some(PathBuf::from(v));
            }
            "--db" => {
                let v = take_value(&mut iter, &arg)?;
                raw.db = Some(PathBuf::from(v));
            }
            "--shelley-operational-certificate" => {
                let v = take_value(&mut iter, &arg)?;
                raw.op_cert = Some(PathBuf::from(v));
            }
            "--shelley-vrf-key" => {
                let v = take_value(&mut iter, &arg)?;
                raw.vrf_key = Some(PathBuf::from(v));
            }
            "--shelley-kes-key" => {
                let v = take_value(&mut iter, &arg)?;
                raw.kes_key = Some(PathBuf::from(v));
            }
            "--bulk-credentials-file" => {
                let v = take_value(&mut iter, &arg)?;
                raw.bulk_creds = Some(PathBuf::from(v));
            }
            "-s" | "--slots" => {
                let v = take_value(&mut iter, &arg)?;
                raw.slots = Some(parse_u64(&arg, &v)?);
            }
            "-b" | "--blocks" => {
                let v = take_value(&mut iter, &arg)?;
                raw.blocks = Some(parse_u64(&arg, &v)?);
            }
            "-e" | "--epochs" => {
                let v = take_value(&mut iter, &arg)?;
                raw.epochs = Some(parse_u64(&arg, &v)?);
            }
            "-f" => raw.force = true,
            "-a" => raw.append = true,
            other => return Err(ParseError::UnknownFlag(other.to_string())),
        }
    }

    promote(raw)
}

fn promote(raw: RawArgs) -> Result<Args, ParseError> {
    let config = raw.config.ok_or(ParseError::MissingConfig)?;
    let db = raw.db.ok_or(ParseError::MissingDb)?;

    if raw.force && raw.append {
        return Err(ParseError::ConflictingOpenModes);
    }
    let open_mode = if raw.force {
        DBSynthesizerOpenMode::OpenCreateForce
    } else if raw.append {
        DBSynthesizerOpenMode::OpenAppend
    } else {
        DBSynthesizerOpenMode::OpenCreate
    };

    let limit_count =
        raw.slots.is_some() as u8 + raw.blocks.is_some() as u8 + raw.epochs.is_some() as u8;
    if limit_count == 0 {
        return Err(ParseError::MissingForgeLimit);
    }
    if limit_count > 1 {
        return Err(ParseError::ConflictingForgeLimits);
    }
    let limit = if let Some(s) = raw.slots {
        ForgeLimit::Slot(SlotNo(s))
    } else if let Some(b) = raw.blocks {
        ForgeLimit::Block(b)
    } else if let Some(e) = raw.epochs {
        ForgeLimit::Epoch(e)
    } else {
        unreachable!("limit_count > 0 was just verified")
    };

    Ok(Args {
        paths: NodeFilePaths {
            config,
            chain_db: db,
        },
        credentials: NodeCredentials {
            cert_file: raw.op_cert,
            vrf_file: raw.vrf_key,
            kes_file: raw.kes_key,
            bulk_file: raw.bulk_creds,
        },
        options: DBSynthesizerOptions { limit, open_mode },
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

    fn minimal() -> Vec<&'static str> {
        vec![
            "--config",
            "/etc/c.json",
            "--db",
            "/var/lib/db",
            "--slots",
            "100",
        ]
    }

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
    fn parses_minimal_canonical_invocation() {
        let args = parse_args(minimal()).expect("parses");
        assert_eq!(args.paths.config.to_str(), Some("/etc/c.json"));
        assert_eq!(args.paths.chain_db.to_str(), Some("/var/lib/db"));
        assert_eq!(args.options.limit, ForgeLimit::Slot(SlotNo(100)));
        assert_eq!(args.options.open_mode, DBSynthesizerOpenMode::OpenCreate);
        assert!(args.credentials.cert_file.is_none());
    }

    #[test]
    fn parses_blocks_forge_limit() {
        let args =
            parse_args(["--config", "/c.json", "--db", "/db", "--blocks", "50"]).expect("parses");
        assert_eq!(args.options.limit, ForgeLimit::Block(50));
    }

    #[test]
    fn parses_epochs_forge_limit() {
        let args = parse_args(["--config", "/c.json", "--db", "/db", "-e", "5"]).expect("parses");
        assert_eq!(args.options.limit, ForgeLimit::Epoch(5));
    }

    #[test]
    fn parses_force_open_mode() {
        let args =
            parse_args(["--config", "/c.json", "--db", "/db", "-s", "1", "-f"]).expect("parses");
        assert_eq!(
            args.options.open_mode,
            DBSynthesizerOpenMode::OpenCreateForce
        );
    }

    #[test]
    fn parses_append_open_mode() {
        let args =
            parse_args(["--config", "/c.json", "--db", "/db", "-s", "1", "-a"]).expect("parses");
        assert_eq!(args.options.open_mode, DBSynthesizerOpenMode::OpenAppend);
    }

    #[test]
    fn parses_all_credential_flags() {
        let args = parse_args([
            "--config",
            "/c.json",
            "--db",
            "/db",
            "-s",
            "1",
            "--shelley-operational-certificate",
            "/keys/op.cert",
            "--shelley-vrf-key",
            "/keys/vrf.key",
            "--shelley-kes-key",
            "/keys/kes.key",
            "--bulk-credentials-file",
            "/keys/bulk.json",
        ])
        .expect("parses");
        assert_eq!(
            args.credentials
                .cert_file
                .as_ref()
                .map(|p| p.to_str().unwrap_or("")),
            Some("/keys/op.cert")
        );
        assert_eq!(
            args.credentials
                .vrf_file
                .as_ref()
                .map(|p| p.to_str().unwrap_or("")),
            Some("/keys/vrf.key")
        );
        assert_eq!(
            args.credentials
                .kes_file
                .as_ref()
                .map(|p| p.to_str().unwrap_or("")),
            Some("/keys/kes.key")
        );
        assert_eq!(
            args.credentials
                .bulk_file
                .as_ref()
                .map(|p| p.to_str().unwrap_or("")),
            Some("/keys/bulk.json")
        );
    }

    #[test]
    fn missing_config_rejected() {
        let args = parse_args(["--db", "/db", "-s", "1"]);
        assert_eq!(args, Err(ParseError::MissingConfig));
    }

    #[test]
    fn missing_db_rejected() {
        let args = parse_args(["--config", "/c.json", "-s", "1"]);
        assert_eq!(args, Err(ParseError::MissingDb));
    }

    #[test]
    fn missing_forge_limit_rejected() {
        let args = parse_args(["--config", "/c.json", "--db", "/db"]);
        assert_eq!(args, Err(ParseError::MissingForgeLimit));
    }

    #[test]
    fn conflicting_forge_limits_rejected() {
        let args = parse_args(["--config", "/c.json", "--db", "/db", "-s", "1", "-b", "2"]);
        assert_eq!(args, Err(ParseError::ConflictingForgeLimits));
    }

    #[test]
    fn conflicting_open_modes_rejected() {
        let args = parse_args(["--config", "/c.json", "--db", "/db", "-s", "1", "-f", "-a"]);
        assert_eq!(args, Err(ParseError::ConflictingOpenModes));
    }

    #[test]
    fn unknown_flag_rejected() {
        let args = parse_args(["--frobnicate"]);
        assert!(matches!(args, Err(ParseError::UnknownFlag(_))));
    }

    #[test]
    fn missing_value_rejected() {
        let args = parse_args(["--config"]);
        assert!(matches!(args, Err(ParseError::MissingValue(_))));
    }

    #[test]
    fn invalid_slot_number_rejected() {
        let args = parse_args(["--config", "/c.json", "--db", "/db", "-s", "abc"]);
        assert!(matches!(args, Err(ParseError::InvalidValue(_, _))));
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
