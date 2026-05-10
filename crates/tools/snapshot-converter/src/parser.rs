//! CLI argument parser for the `snapshot-converter` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/app/snapshot-converter.hs.
//!
//! Direct port of upstream's `parseConfig :: Parser Config`. The
//! grammar dispatches between two mutually-exclusive modes:
//!
//! ## Daemon mode
//!
//! All three flags are required; converts new LSM snapshots to Mem
//! format as they appear in the watched directory.
//!
//! - `--monitor-lsm-snapshots-in PATH` — directory to watch.
//! - `--lsm-database PATH` — backing LSM database file.
//! - `--output-mem-snapshots-in PATH` — output directory.
//!
//! ## Oneshot mode
//!
//! Convert one snapshot. Requires exactly one input form and one
//! output form:
//!
//! Input forms (mutually exclusive):
//! - `--input-mem PATH` (Mem-format input).
//! - `--input-lsm-snapshot PATH` + `--input-lsm-database PATH`
//!   (LSM-format input).
//!
//! Output forms (mutually exclusive):
//! - `--output-mem PATH` (Mem-format output).
//! - `--output-lsm-snapshot PATH` + `--output-lsm-database PATH`
//!   (LSM-format output).
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `snapshot-converter` binary; fixtures captured at R335 live at
//! `crates/snapshot-converter/tests/fixtures/upstream-{help,version}.txt`.

use std::path::PathBuf;

use crate::types::{
    Config, LsmDatabaseFilePath, SnapshotSpec, SnapshotsDirectory, SnapshotsDirectoryWithFormat,
    StandaloneFormat,
};

/// Byte-for-byte mirror of upstream `snapshot-converter --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `snapshot-converter --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line arguments — the [`Config`] form returned by
/// upstream's `parseConfig`.
pub type Args = Config;

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen.
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` was seen.
    #[error("(--version requested)")]
    VersionRequested,
    /// Mode could not be determined: neither daemon nor oneshot
    /// flags supplied.
    #[error(
        "missing mode: supply daemon flags (--monitor-lsm-snapshots-in + --lsm-database + --output-mem-snapshots-in) OR oneshot flags (--input-{{mem,lsm-snapshot}} + --output-{{mem,lsm-snapshot}})"
    )]
    MissingMode,
    /// Daemon-mode flags AND oneshot-mode flags were both supplied.
    #[error(
        "conflicting modes: daemon flags (--monitor-lsm-snapshots-in / --lsm-database / --output-mem-snapshots-in) and oneshot flags (--input-* / --output-*) are mutually exclusive"
    )]
    ConflictingModes,
    /// Daemon mode was selected but one of its three required flags
    /// was missing.
    #[error("missing required daemon flag: --{0}")]
    MissingDaemonFlag(&'static str),
    /// Oneshot mode was selected but no input form was specified.
    #[error(
        "missing oneshot input: supply --input-mem PATH OR --input-lsm-snapshot PATH + --input-lsm-database PATH"
    )]
    MissingOneshotInput,
    /// Oneshot mode was selected but no output form was specified.
    #[error(
        "missing oneshot output: supply --output-mem PATH OR --output-lsm-snapshot PATH + --output-lsm-database PATH"
    )]
    MissingOneshotOutput,
    /// Both Mem and LSM input forms were supplied.
    #[error(
        "conflicting oneshot input: --input-mem and --input-lsm-snapshot are mutually exclusive"
    )]
    ConflictingOneshotInput,
    /// Both Mem and LSM output forms were supplied.
    #[error(
        "conflicting oneshot output: --output-mem and --output-lsm-snapshot are mutually exclusive"
    )]
    ConflictingOneshotOutput,
    /// LSM input form had `--input-lsm-snapshot` without
    /// `--input-lsm-database` (or vice-versa).
    #[error("--input-lsm-snapshot and --input-lsm-database must be supplied together")]
    LsmInputMissingDatabase,
    /// LSM output form had `--output-lsm-snapshot` without
    /// `--output-lsm-database` (or vice-versa).
    #[error("--output-lsm-snapshot and --output-lsm-database must be supplied together")]
    LsmOutputMissingDatabase,
    /// An unknown flag was passed.
    #[error("Invalid option `{0}'")]
    UnknownFlag(String),
    /// A flag requiring a value was passed without one.
    #[error("flag `{0}' requires a value")]
    MissingValue(String),
}

#[derive(Clone, Debug, Default)]
struct RawArgs {
    monitor_lsm_snapshots_in: Option<PathBuf>,
    lsm_database: Option<PathBuf>,
    output_mem_snapshots_in: Option<PathBuf>,
    input_mem: Option<PathBuf>,
    input_lsm_snapshot: Option<PathBuf>,
    input_lsm_database: Option<PathBuf>,
    output_mem: Option<PathBuf>,
    output_lsm_snapshot: Option<PathBuf>,
    output_lsm_database: Option<PathBuf>,
}

/// Parse a slice of command-line arguments into a [`Config`]. Mirror
/// of upstream `parseConfig`.
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
            "--monitor-lsm-snapshots-in" => {
                let v = take_value(&mut iter, &arg)?;
                raw.monitor_lsm_snapshots_in = Some(PathBuf::from(v));
            }
            "--lsm-database" => {
                let v = take_value(&mut iter, &arg)?;
                raw.lsm_database = Some(PathBuf::from(v));
            }
            "--output-mem-snapshots-in" => {
                let v = take_value(&mut iter, &arg)?;
                raw.output_mem_snapshots_in = Some(PathBuf::from(v));
            }
            "--input-mem" => {
                let v = take_value(&mut iter, &arg)?;
                raw.input_mem = Some(PathBuf::from(v));
            }
            "--input-lsm-snapshot" => {
                let v = take_value(&mut iter, &arg)?;
                raw.input_lsm_snapshot = Some(PathBuf::from(v));
            }
            "--input-lsm-database" => {
                let v = take_value(&mut iter, &arg)?;
                raw.input_lsm_database = Some(PathBuf::from(v));
            }
            "--output-mem" => {
                let v = take_value(&mut iter, &arg)?;
                raw.output_mem = Some(PathBuf::from(v));
            }
            "--output-lsm-snapshot" => {
                let v = take_value(&mut iter, &arg)?;
                raw.output_lsm_snapshot = Some(PathBuf::from(v));
            }
            "--output-lsm-database" => {
                let v = take_value(&mut iter, &arg)?;
                raw.output_lsm_database = Some(PathBuf::from(v));
            }
            other => return Err(ParseError::UnknownFlag(other.to_string())),
        }
    }

    promote_to_config(raw)
}

fn promote_to_config(raw: RawArgs) -> Result<Config, ParseError> {
    let any_daemon = raw.monitor_lsm_snapshots_in.is_some()
        || raw.lsm_database.is_some()
        || raw.output_mem_snapshots_in.is_some();
    let any_oneshot = raw.input_mem.is_some()
        || raw.input_lsm_snapshot.is_some()
        || raw.input_lsm_database.is_some()
        || raw.output_mem.is_some()
        || raw.output_lsm_snapshot.is_some()
        || raw.output_lsm_database.is_some();

    if any_daemon && any_oneshot {
        return Err(ParseError::ConflictingModes);
    }
    if !any_daemon && !any_oneshot {
        return Err(ParseError::MissingMode);
    }

    if any_daemon {
        let monitor = raw
            .monitor_lsm_snapshots_in
            .ok_or(ParseError::MissingDaemonFlag("monitor-lsm-snapshots-in"))?;
        let database = raw
            .lsm_database
            .ok_or(ParseError::MissingDaemonFlag("lsm-database"))?;
        let output = raw
            .output_mem_snapshots_in
            .ok_or(ParseError::MissingDaemonFlag("output-mem-snapshots-in"))?;
        return Ok(Config::Daemon {
            watch: SnapshotsDirectoryWithFormat::LsmSnapshot {
                directory: SnapshotsDirectory::new(monitor),
                database: LsmDatabaseFilePath::new(database),
            },
            output: SnapshotsDirectory::new(output),
        });
    }

    // Oneshot mode.
    let input = match (
        raw.input_mem,
        raw.input_lsm_snapshot,
        raw.input_lsm_database,
    ) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
            return Err(ParseError::ConflictingOneshotInput);
        }
        (Some(path), None, None) => SnapshotSpec::standalone(path, StandaloneFormat::Mem),
        (None, Some(path), Some(db)) => SnapshotSpec::lsm(path, LsmDatabaseFilePath::new(db)),
        (None, Some(_), None) | (None, None, Some(_)) => {
            return Err(ParseError::LsmInputMissingDatabase);
        }
        (None, None, None) => return Err(ParseError::MissingOneshotInput),
    };

    let output = match (
        raw.output_mem,
        raw.output_lsm_snapshot,
        raw.output_lsm_database,
    ) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
            return Err(ParseError::ConflictingOneshotOutput);
        }
        (Some(path), None, None) => SnapshotSpec::standalone(path, StandaloneFormat::Mem),
        (None, Some(path), Some(db)) => SnapshotSpec::lsm(path, LsmDatabaseFilePath::new(db)),
        (None, Some(_), None) | (None, None, Some(_)) => {
            return Err(ParseError::LsmOutputMissingDatabase);
        }
        (None, None, None) => return Err(ParseError::MissingOneshotOutput),
    };

    Ok(Config::Oneshot { input, output })
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
    fn empty_argv_errors_on_missing_mode() {
        let argv: Vec<String> = Vec::new();
        assert_eq!(parse_args(&argv), Err(ParseError::MissingMode));
    }

    #[test]
    fn unknown_flag_rejected() {
        assert!(matches!(
            parse_args(["--frobnicate"]),
            Err(ParseError::UnknownFlag(_))
        ));
    }

    #[test]
    fn missing_value_rejected() {
        assert!(matches!(
            parse_args(["--input-mem"]),
            Err(ParseError::MissingValue(_))
        ));
    }

    #[test]
    fn parses_daemon_mode_canonical() {
        let args = parse_args([
            "--monitor-lsm-snapshots-in",
            "/in",
            "--lsm-database",
            "/lsm.db",
            "--output-mem-snapshots-in",
            "/out",
        ])
        .expect("parses");
        match args {
            Config::Daemon {
                watch:
                    SnapshotsDirectoryWithFormat::LsmSnapshot {
                        directory,
                        database,
                    },
                output,
            } => {
                assert_eq!(directory.as_path().to_str(), Some("/in"));
                assert_eq!(database.as_path().to_str(), Some("/lsm.db"));
                assert_eq!(output.as_path().to_str(), Some("/out"));
            }
            Config::Oneshot { .. } => panic!("expected Daemon"),
        }
    }

    #[test]
    fn daemon_mode_missing_lsm_database_rejected() {
        let args = parse_args([
            "--monitor-lsm-snapshots-in",
            "/in",
            "--output-mem-snapshots-in",
            "/out",
        ]);
        assert!(matches!(
            args,
            Err(ParseError::MissingDaemonFlag("lsm-database"))
        ));
    }

    #[test]
    fn daemon_mode_missing_output_rejected() {
        let args = parse_args([
            "--monitor-lsm-snapshots-in",
            "/in",
            "--lsm-database",
            "/lsm.db",
        ]);
        assert!(matches!(
            args,
            Err(ParseError::MissingDaemonFlag("output-mem-snapshots-in"))
        ));
    }

    #[test]
    fn parses_oneshot_mem_to_lsm() {
        let args = parse_args([
            "--input-mem",
            "/in/100",
            "--output-lsm-snapshot",
            "/out/100",
            "--output-lsm-database",
            "/out/lsm.db",
        ])
        .expect("parses");
        match args {
            Config::Oneshot { input, output } => {
                match input {
                    SnapshotSpec::Standalone { path, format } => {
                        assert_eq!(path.to_str(), Some("/in/100"));
                        assert_eq!(format, StandaloneFormat::Mem);
                    }
                    SnapshotSpec::Lsm { .. } => panic!("input expected Standalone"),
                }
                match output {
                    SnapshotSpec::Lsm { path, database } => {
                        assert_eq!(path.to_str(), Some("/out/100"));
                        assert_eq!(database.as_path().to_str(), Some("/out/lsm.db"));
                    }
                    SnapshotSpec::Standalone { .. } => panic!("output expected Lsm"),
                }
            }
            Config::Daemon { .. } => panic!("expected Oneshot"),
        }
    }

    #[test]
    fn parses_oneshot_lsm_to_mem() {
        let args = parse_args([
            "--input-lsm-snapshot",
            "/in/100",
            "--input-lsm-database",
            "/in/lsm.db",
            "--output-mem",
            "/out/100",
        ])
        .expect("parses");
        match args {
            Config::Oneshot { input, output } => {
                assert!(matches!(input, SnapshotSpec::Lsm { .. }));
                match output {
                    SnapshotSpec::Standalone { path, format } => {
                        assert_eq!(path.to_str(), Some("/out/100"));
                        assert_eq!(format, StandaloneFormat::Mem);
                    }
                    SnapshotSpec::Lsm { .. } => panic!("output expected Standalone"),
                }
            }
            Config::Daemon { .. } => panic!("expected Oneshot"),
        }
    }

    #[test]
    fn parses_oneshot_mem_to_mem() {
        let args =
            parse_args(["--input-mem", "/in/100", "--output-mem", "/out/100"]).expect("parses");
        assert!(matches!(args, Config::Oneshot { .. }));
    }

    #[test]
    fn parses_oneshot_lsm_to_lsm() {
        let args = parse_args([
            "--input-lsm-snapshot",
            "/in/100",
            "--input-lsm-database",
            "/in/lsm.db",
            "--output-lsm-snapshot",
            "/out/100",
            "--output-lsm-database",
            "/out/lsm.db",
        ])
        .expect("parses");
        assert!(matches!(args, Config::Oneshot { .. }));
    }

    #[test]
    fn conflicting_modes_rejected() {
        let args = parse_args([
            "--monitor-lsm-snapshots-in",
            "/in",
            "--input-mem",
            "/in/100",
        ]);
        assert_eq!(args, Err(ParseError::ConflictingModes));
    }

    #[test]
    fn conflicting_oneshot_input_rejected() {
        let args = parse_args([
            "--input-mem",
            "/in",
            "--input-lsm-snapshot",
            "/in",
            "--input-lsm-database",
            "/in/db",
            "--output-mem",
            "/out",
        ]);
        assert_eq!(args, Err(ParseError::ConflictingOneshotInput));
    }

    #[test]
    fn conflicting_oneshot_output_rejected() {
        let args = parse_args([
            "--input-mem",
            "/in",
            "--output-mem",
            "/out",
            "--output-lsm-snapshot",
            "/out",
            "--output-lsm-database",
            "/out/db",
        ]);
        assert_eq!(args, Err(ParseError::ConflictingOneshotOutput));
    }

    #[test]
    fn lsm_input_missing_database_rejected() {
        let args = parse_args(["--input-lsm-snapshot", "/in", "--output-mem", "/out"]);
        assert_eq!(args, Err(ParseError::LsmInputMissingDatabase));
    }

    #[test]
    fn lsm_output_missing_database_rejected() {
        let args = parse_args(["--input-mem", "/in", "--output-lsm-snapshot", "/out"]);
        assert_eq!(args, Err(ParseError::LsmOutputMissingDatabase));
    }

    #[test]
    fn missing_oneshot_output_rejected() {
        let args = parse_args(["--input-mem", "/in"]);
        assert_eq!(args, Err(ParseError::MissingOneshotOutput));
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
