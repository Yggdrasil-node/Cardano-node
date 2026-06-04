//! CLI argument parser for the `kes-agent-control` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/cli/ControlMain.hs.
//!
//! Direct port of upstream's `pProgramOptions :: Parser ProgramOptions`
//! and the per-subcommand `pCommonOptions` / `pGenKeyOptions` /
//! `pQueryKeyOptions` / `pDropStagedKeyOptions` / `pDropKeyOptions` /
//! `pInstallKeyOptions` parsers.
//!
//! Subcommand grammar:
//!
//! | Subcommand            | Maps to                                   |
//! |-----------------------|-------------------------------------------|
//! | `gen-staged-key`      | [`ProgramOptions::RunGenKey`]             |
//! | `export-staged-vkey`  | [`ProgramOptions::RunQueryKey`]           |
//! | `drop-staged-key`     | [`ProgramOptions::RunDropStagedKey`]      |
//! | `install-key`         | [`ProgramOptions::RunInstallKey`]         |
//! | `drop-key`            | [`ProgramOptions::RunDropKey`]            |
//! | `info`                | [`ProgramOptions::RunGetInfo`]            |
//!
//! Common options (any of these may appear before OR after the
//! subcommand — upstream's optparse-applicative threading lets the
//! outer `pCommonOptions` and the per-subcommand options compose via
//! the `WithCommonOptions` typeclass; the Rust port walks the argv
//! once and accumulates common-option overrides into a single
//! [`CommonOptions`] before applying it to the chosen subcommand via
//! [`ProgramOptions::with_common_options`]):
//!
//! - `-c` / `--control-address ADDR` — `$KES_AGENT_CONTROL_PATH` override.
//! - `-v` / `--verbose N` — verbosity level (note: clashes with the
//!   upstream `--version` flag's short form; upstream resolves this by
//!   making `-v` always mean verbose, never version, and using only
//!   `--version` for version dispatch — Yggdrasil mirrors this
//!   precedence).
//! - `--retry-interval MS` (alias `--retry-delay`).
//! - `--retry-exponential` (boolean switch).
//! - `--retry-attempts N`.
//!
//! Per-subcommand options:
//!
//! - `gen-staged-key` / `export-staged-vkey`: `--kes-verification-key-file FILEPATH`
//!   (where to write the verification key).
//! - `install-key`: `--opcert-file FILEPATH` (operational certificate
//!   path).
//! - `drop-staged-key` / `drop-key` / `info`: common-options-only.
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `kes-agent-control` binary; fixtures captured at R335 live at
//! `crates/tools/kes-agent-control/tests/fixtures/upstream-{help,version}.txt`.

use std::path::PathBuf;

use crate::types::{
    CommonOptions, DropKeyOptions, DropStagedKeyOptions, GenKeyOptions, InstallKeyOptions,
    ProgramOptions, QueryKeyOptions,
};

/// Byte-for-byte mirror of upstream `kes-agent-control --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `kes-agent-control --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line dispatch — the [`ProgramOptions`] form returned
/// by upstream's `pProgramOptions`.
pub type Args = ProgramOptions;

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen.
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` was seen (upstream's `--version` is parsed by the
    /// `helper`/`infoOption` combinator and short-circuits at parse
    /// time, just like `--help`).
    #[error("(--version requested)")]
    VersionRequested,
    /// No subcommand was supplied.
    #[error(
        "missing subcommand: expected one of gen-staged-key, export-staged-vkey, drop-staged-key, install-key, drop-key, info"
    )]
    MissingSubcommand,
    /// An unknown subcommand was supplied.
    #[error("unknown subcommand: {0}")]
    UnknownSubcommand(String),
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

/// Parse a slice of command-line arguments into a [`ProgramOptions`].
/// Mirror of upstream `pProgramOptions`.
///
/// Two-pass walk:
/// 1. Locate the subcommand keyword and split argv into "before
///    subcommand" + "subcommand" + "after subcommand" segments.
/// 2. Parse common options from both before- and after-segments,
///    parse subcommand-specific options from the after-segment.
/// 3. Apply common-options overrides to the chosen subcommand via
///    [`ProgramOptions::with_common_options`].
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let argv: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();

    // First, peek for --help / --version anywhere; they short-circuit.
    for arg in &argv {
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "--version" => return Err(ParseError::VersionRequested),
            _ => {}
        }
    }

    // Locate the subcommand keyword.
    let subcommand_idx = argv
        .iter()
        .position(|a| {
            matches!(
                a.as_str(),
                "gen-staged-key"
                    | "export-staged-vkey"
                    | "drop-staged-key"
                    | "install-key"
                    | "drop-key"
                    | "info"
            )
        })
        .ok_or_else(|| {
            // No known subcommand. If there's a non-flag positional, treat
            // it as an unknown subcommand; otherwise complain about the
            // missing dispatch.
            if let Some(positional) = argv.iter().find(|a| !a.starts_with('-')) {
                ParseError::UnknownSubcommand(positional.clone())
            } else {
                ParseError::MissingSubcommand
            }
        })?;

    let subcommand = &argv[subcommand_idx];
    let before = &argv[..subcommand_idx];
    let after = &argv[subcommand_idx + 1..];

    // Parse common options from both before- and after-segments. Because
    // upstream lets you write `--verbose 1 gen-staged-key
    // --kes-verification-key-file foo.vkey` OR `gen-staged-key
    // --kes-verification-key-file foo.vkey --verbose 1`,
    // we accumulate from both windows. Per-subcommand flags are filtered
    // out of the after-segment by `parse_subcommand_options`.
    let mut common = parse_common_options(before)?;
    let common_from_after = parse_common_options_in_subcommand_window(after)?;
    common = common_from_after.merge(common);

    let chosen = match subcommand.as_str() {
        "gen-staged-key" => {
            let ver_key_file = extract_ver_key_file(after)?;
            ProgramOptions::RunGenKey(GenKeyOptions {
                common: CommonOptions::default(),
                kes_verification_key_file: ver_key_file,
            })
        }
        "export-staged-vkey" => {
            let ver_key_file = extract_ver_key_file(after)?;
            ProgramOptions::RunQueryKey(QueryKeyOptions {
                common: CommonOptions::default(),
                kes_verification_key_file: ver_key_file,
            })
        }
        "drop-staged-key" => ProgramOptions::RunDropStagedKey(DropStagedKeyOptions {
            common: CommonOptions::default(),
        }),
        "install-key" => {
            let op_cert = extract_op_cert(after)?;
            ProgramOptions::RunInstallKey(InstallKeyOptions {
                common: CommonOptions::default(),
                op_cert_file: op_cert,
            })
        }
        "drop-key" => ProgramOptions::RunDropKey(DropKeyOptions {
            common: CommonOptions::default(),
        }),
        "info" => ProgramOptions::RunGetInfo(CommonOptions::default()),
        other => return Err(ParseError::UnknownSubcommand(other.to_string())),
    };

    Ok(chosen.with_common_options(common))
}

/// Parse common options from a window of argv that contains ONLY
/// common options (no subcommand keyword and no per-subcommand flags).
fn parse_common_options(window: &[String]) -> Result<CommonOptions, ParseError> {
    let mut out = CommonOptions::default();
    let mut iter = window.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-c" | "--control-address" => {
                let v = take_value(&mut iter, arg)?;
                out.control_path = Some(v);
            }
            "-v" | "--verbose" => {
                let v = take_value(&mut iter, arg)?;
                out.verbosity = Some(parse_i32(arg, &v)?);
            }
            "--retry-interval" | "--retry-delay" => {
                let v = take_value(&mut iter, arg)?;
                out.retry_delay = Some(parse_i64(arg, &v)?);
            }
            "--retry-exponential" => {
                out.retry_exponential = Some(true);
            }
            "--retry-attempts" => {
                let v = take_value(&mut iter, arg)?;
                out.retry_attempts = Some(parse_i64(arg, &v)?);
            }
            other => return Err(ParseError::UnknownFlag(other.to_string())),
        }
    }
    Ok(out)
}

/// Parse common options from the after-subcommand window. Filters out
/// per-subcommand flags (`--kes-verification-key-file`, `--opcert-file`) so the common-options
/// parser only sees its own grammar.
fn parse_common_options_in_subcommand_window(
    window: &[String],
) -> Result<CommonOptions, ParseError> {
    let filtered = filter_common_options(window)?;
    parse_common_options(&filtered)
}

/// Filter the after-subcommand window to keep only common-option flags
/// and their values. Per-subcommand flags (`--kes-verification-key-file`,
/// `--opcert-file`) are dropped along with their values.
fn filter_common_options(window: &[String]) -> Result<Vec<String>, ParseError> {
    let mut out: Vec<String> = Vec::new();
    let mut iter = window.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--kes-verification-key-file" | "--opcert-file" => {
                // Drop the flag and its value.
                let _ = iter.next();
            }
            "--retry-exponential" => {
                out.push(arg.clone());
            }
            // Common-option flags that take a value:
            "-c" | "--control-address" | "-v" | "--verbose" | "--retry-interval"
            | "--retry-delay" | "--retry-attempts" => {
                out.push(arg.clone());
                if let Some(value) = iter.next() {
                    out.push(value.clone());
                } else {
                    return Err(ParseError::MissingValue(arg.clone()));
                }
            }
            other => return Err(ParseError::UnknownFlag(other.to_string())),
        }
    }
    Ok(out)
}

/// Extract `--kes-verification-key-file FILEPATH` from a subcommand window (returns
/// `None` if absent).
fn extract_ver_key_file(window: &[String]) -> Result<Option<PathBuf>, ParseError> {
    let mut iter = window.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--kes-verification-key-file" {
            let v = take_value(&mut iter, arg)?;
            return Ok(Some(PathBuf::from(v)));
        }
    }
    Ok(None)
}

/// Extract `--opcert-file FILEPATH` from a subcommand window (returns
/// `None` if absent).
fn extract_op_cert(window: &[String]) -> Result<Option<PathBuf>, ParseError> {
    let mut iter = window.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--opcert-file" {
            let v = take_value(&mut iter, arg)?;
            return Ok(Some(PathBuf::from(v)));
        }
    }
    Ok(None)
}

fn take_value<'a, I>(iter: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, ParseError>
where
    I: Iterator<Item = &'a String>,
{
    iter.next()
        .cloned()
        .ok_or_else(|| ParseError::MissingValue(flag.to_string()))
}

fn parse_i32(flag: &str, value: &str) -> Result<i32, ParseError> {
    value.parse().map_err(|e: std::num::ParseIntError| {
        ParseError::InvalidValue(flag.to_string(), e.to_string())
    })
}

fn parse_i64(flag: &str, value: &str) -> Result<i64, ParseError> {
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
    fn detects_version() {
        assert_eq!(parse_args(["--version"]), Err(ParseError::VersionRequested));
    }

    #[test]
    fn empty_argv_errors_on_missing_subcommand() {
        let argv: Vec<String> = Vec::new();
        assert_eq!(parse_args(&argv), Err(ParseError::MissingSubcommand));
    }

    #[test]
    fn unknown_subcommand_errors() {
        assert!(matches!(
            parse_args(["frobnicate"]),
            Err(ParseError::UnknownSubcommand(_))
        ));
    }

    #[test]
    fn parses_gen_staged_key_minimal() {
        let args = parse_args(["gen-staged-key"]).expect("parses");
        assert!(matches!(args, ProgramOptions::RunGenKey(_)));
    }

    #[test]
    fn parses_gen_staged_key_with_ver_key_file() {
        let args = parse_args(["gen-staged-key", "--kes-verification-key-file", "out.vkey"])
            .expect("parses");
        match args {
            ProgramOptions::RunGenKey(o) => {
                assert_eq!(
                    o.kes_verification_key_file
                        .as_ref()
                        .map(|p| p.to_str().unwrap_or("")),
                    Some("out.vkey")
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parses_export_staged_vkey() {
        let args = parse_args([
            "export-staged-vkey",
            "--kes-verification-key-file",
            "current.vkey",
        ])
        .expect("parses");
        match args {
            ProgramOptions::RunQueryKey(o) => {
                assert_eq!(
                    o.kes_verification_key_file
                        .as_ref()
                        .map(|p| p.to_str().unwrap_or("")),
                    Some("current.vkey")
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parses_drop_staged_key() {
        let args = parse_args(["drop-staged-key"]).expect("parses");
        assert!(matches!(args, ProgramOptions::RunDropStagedKey(_)));
    }

    #[test]
    fn parses_drop_key() {
        let args = parse_args(["drop-key"]).expect("parses");
        assert!(matches!(args, ProgramOptions::RunDropKey(_)));
    }

    #[test]
    fn parses_install_key_with_opcert_file() {
        let args = parse_args(["install-key", "--opcert-file", "node.cert"]).expect("parses");
        match args {
            ProgramOptions::RunInstallKey(o) => {
                assert_eq!(
                    o.op_cert_file.as_ref().map(|p| p.to_str().unwrap_or("")),
                    Some("node.cert")
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parses_info() {
        let args = parse_args(["info"]).expect("parses");
        assert!(matches!(args, ProgramOptions::RunGetInfo(_)));
    }

    #[test]
    fn common_options_before_subcommand_apply() {
        let args = parse_args([
            "--control-address",
            "/var/run/kes.sock",
            "gen-staged-key",
            "--kes-verification-key-file",
            "out.vkey",
        ])
        .expect("parses");
        match args {
            ProgramOptions::RunGenKey(o) => {
                assert_eq!(o.common.control_path.as_deref(), Some("/var/run/kes.sock"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn common_options_after_subcommand_apply() {
        let args = parse_args([
            "gen-staged-key",
            "--kes-verification-key-file",
            "out.vkey",
            "--verbose",
            "2",
        ])
        .expect("parses");
        match args {
            ProgramOptions::RunGenKey(o) => {
                assert_eq!(o.common.verbosity, Some(2));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn common_options_short_form_control_address() {
        let args = parse_args(["-c", "/tmp/k.sock", "info"]).expect("parses");
        match args {
            ProgramOptions::RunGetInfo(c) => {
                assert_eq!(c.control_path.as_deref(), Some("/tmp/k.sock"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn common_options_retry_exponential_switch() {
        let args = parse_args(["info", "--retry-exponential"]).expect("parses");
        match args {
            ProgramOptions::RunGetInfo(c) => {
                assert_eq!(c.retry_exponential, Some(true));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn common_options_retry_interval_value() {
        let args = parse_args(["info", "--retry-interval", "500"]).expect("parses");
        match args {
            ProgramOptions::RunGetInfo(c) => {
                assert_eq!(c.retry_delay, Some(500));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn common_options_retry_delay_alias_works() {
        let args = parse_args(["info", "--retry-delay", "750"]).expect("parses");
        match args {
            ProgramOptions::RunGetInfo(c) => {
                assert_eq!(c.retry_delay, Some(750));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn common_options_retry_attempts_value() {
        let args = parse_args(["info", "--retry-attempts", "10"]).expect("parses");
        match args {
            ProgramOptions::RunGetInfo(c) => {
                assert_eq!(c.retry_attempts, Some(10));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn rejects_missing_value() {
        assert!(matches!(
            parse_args(["info", "--control-address"]),
            Err(ParseError::MissingValue(_))
        ));
    }

    #[test]
    fn rejects_invalid_verbosity_number() {
        assert!(matches!(
            parse_args(["--verbose", "abc", "info"]),
            Err(ParseError::InvalidValue(_, _))
        ));
    }

    #[test]
    fn rejects_unknown_flag_before_subcommand() {
        assert!(matches!(
            parse_args(["--definitely-not-real", "info"]),
            Err(ParseError::UnknownFlag(_))
        ));
    }

    #[test]
    fn full_canonical_install_key_invocation() {
        let args = parse_args([
            "--control-address",
            "/var/run/kes.sock",
            "--verbose",
            "1",
            "install-key",
            "--opcert-file",
            "node.cert",
            "--retry-attempts",
            "5",
        ])
        .expect("parses");
        match args {
            ProgramOptions::RunInstallKey(o) => {
                assert_eq!(o.common.control_path.as_deref(), Some("/var/run/kes.sock"));
                assert_eq!(o.common.verbosity, Some(1));
                assert_eq!(o.common.retry_attempts, Some(5));
                assert_eq!(
                    o.op_cert_file.as_ref().map(|p| p.to_str().unwrap_or("")),
                    Some("node.cert")
                );
            }
            _ => panic!("wrong variant"),
        }
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
