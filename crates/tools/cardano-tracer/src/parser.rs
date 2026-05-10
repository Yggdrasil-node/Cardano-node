//! CLI argument parser for the `cardano-tracer` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/CLI.hs.
//!
//! Direct port of upstream's
//! `parseTracerParams :: Parser TracerParams` — a thin 3-flag CLI
//! shell. The bulk of the operator surface lives in the YAML config
//! file (parsed at startup via [`crate::configuration::parse_tracer_config_json`]).
//!
//! Flags:
//!
//! - `-c` / `--config FILEPATH` — mandatory; path to the tracer's
//!   YAML/JSON config file.
//! - `--state-dir FILEPATH` — optional; if specified, RTView saves
//!   its state in this directory. (RTView itself is carved out per
//!   the plan; the flag is parsed verbatim so operator scripts
//!   continue to work, with the value handed to the future RTView
//!   port.)
//! - `--min-log-severity SEVERITY` — optional; drop messages less
//!   severe than this. Accepts the upstream `Cardano.Logging.SeverityS`
//!   constructors `Debug | Info | Notice | Warning | Error | Critical |
//!   Alert | Emergency`.
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `cardano-tracer` binary; fixtures captured at R335 live at
//! `crates/cardano-tracer/tests/fixtures/upstream-{help,version}.txt`.

use std::path::PathBuf;

/// Byte-for-byte mirror of upstream `cardano-tracer --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `cardano-tracer --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Severity level for the `--min-log-severity` filter.
///
/// Mirrors upstream `Cardano.Logging.SeverityS` (which is a
/// re-export from `iohk-monitoring-framework`):
/// `Debug | Info | Notice | Warning | Error | Critical | Alert |
/// Emergency`. The 8-level scheme is the syslog `LOG_*` set with
/// the addition of `Notice` between `Info` and `Warning`.
///
/// Distinct from the [`crate::configuration::Verbosity`] enum
/// (`Minimum | ErrorsOnly | Maximum`) which controls the tracer's
/// own verbosity rather than the per-message severity floor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Default)]
pub enum SeverityS {
    /// Most verbose; all diagnostic output.
    Debug,
    /// Default operating-message severity.
    #[default]
    Info,
    /// Notable-but-not-warning operational events.
    Notice,
    /// Recoverable degradation.
    Warning,
    /// Operator-visible failure.
    Error,
    /// Service-degrading failure.
    Critical,
    /// Action-required failure.
    Alert,
    /// Catastrophic failure.
    Emergency,
}

impl SeverityS {
    /// Parse a [`SeverityS`] from the upstream-canonical Haskell
    /// constructor name (case-sensitive, matching `option auto`'s
    /// `Read` instance for the type).
    pub fn from_str_strict(s: &str) -> Result<Self, ParseError> {
        match s {
            "Debug" => Ok(SeverityS::Debug),
            "Info" => Ok(SeverityS::Info),
            "Notice" => Ok(SeverityS::Notice),
            "Warning" => Ok(SeverityS::Warning),
            "Error" => Ok(SeverityS::Error),
            "Critical" => Ok(SeverityS::Critical),
            "Alert" => Ok(SeverityS::Alert),
            "Emergency" => Ok(SeverityS::Emergency),
            other => Err(ParseError::InvalidSeverity(other.to_string())),
        }
    }
}

/// Parsed CLI parameters — direct mirror of upstream's `TracerParams`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct Args {
    /// Path to the tracer's config file. Mirrors upstream
    /// `tracerConfig :: FilePath`.
    pub tracer_config: PathBuf,
    /// Optional state-directory for RTView. Mirrors upstream
    /// `stateDir :: Maybe FilePath`.
    pub state_dir: Option<PathBuf>,
    /// Optional severity-floor filter. Mirrors upstream
    /// `logSeverity :: Maybe SeverityS`.
    pub log_severity: Option<SeverityS>,
}

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen.
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` was seen.
    #[error("(--version requested)")]
    VersionRequested,
    /// `--config FILEPATH` was not supplied.
    #[error("missing required flag: --config")]
    MissingConfig,
    /// `--min-log-severity SEVERITY` value was not a recognized
    /// SeverityS constructor.
    #[error(
        "invalid --min-log-severity `{0}': expected Debug | Info | Notice | Warning | Error | Critical | Alert | Emergency"
    )]
    InvalidSeverity(String),
    /// An unknown flag was passed.
    #[error("Invalid option `{0}'")]
    UnknownFlag(String),
    /// A flag requiring a value was passed without one.
    #[error("flag `{0}' requires a value")]
    MissingValue(String),
}

/// Parse a slice of command-line arguments into [`Args`]. Mirror of
/// upstream `parseTracerParams`.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut tracer_config: Option<PathBuf> = None;
    let mut state_dir: Option<PathBuf> = None;
    let mut log_severity: Option<SeverityS> = None;
    let mut iter = args.into_iter().peekable();

    while let Some(arg) = iter.next() {
        let arg = arg.as_ref().to_string();
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "--version" => return Err(ParseError::VersionRequested),
            "-c" | "--config" => {
                let v = take_value(&mut iter, &arg)?;
                tracer_config = Some(PathBuf::from(v));
            }
            "--state-dir" => {
                let v = take_value(&mut iter, &arg)?;
                state_dir = Some(PathBuf::from(v));
            }
            "--min-log-severity" => {
                let v = take_value(&mut iter, &arg)?;
                log_severity = Some(SeverityS::from_str_strict(&v)?);
            }
            other => return Err(ParseError::UnknownFlag(other.to_string())),
        }
    }

    Ok(Args {
        tracer_config: tracer_config.ok_or(ParseError::MissingConfig)?,
        state_dir,
        log_severity,
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
    fn parses_minimal_config_only() {
        let args = parse_args(["--config", "/etc/tracer.yaml"]).expect("parses");
        assert_eq!(args.tracer_config.to_str(), Some("/etc/tracer.yaml"));
        assert!(args.state_dir.is_none());
        assert!(args.log_severity.is_none());
    }

    #[test]
    fn parses_config_short_form() {
        let args = parse_args(["-c", "/etc/tracer.yaml"]).expect("parses");
        assert_eq!(args.tracer_config.to_str(), Some("/etc/tracer.yaml"));
    }

    #[test]
    fn parses_state_dir() {
        let args = parse_args([
            "--config",
            "/etc/tracer.yaml",
            "--state-dir",
            "/var/lib/rtview",
        ])
        .expect("parses");
        assert_eq!(
            args.state_dir.as_ref().map(|p| p.to_str().unwrap_or("")),
            Some("/var/lib/rtview")
        );
    }

    #[test]
    fn parses_all_severity_levels() {
        for (s, expected) in [
            ("Debug", SeverityS::Debug),
            ("Info", SeverityS::Info),
            ("Notice", SeverityS::Notice),
            ("Warning", SeverityS::Warning),
            ("Error", SeverityS::Error),
            ("Critical", SeverityS::Critical),
            ("Alert", SeverityS::Alert),
            ("Emergency", SeverityS::Emergency),
        ] {
            let args =
                parse_args(["--config", "/c.yaml", "--min-log-severity", s]).expect("parses");
            assert_eq!(args.log_severity, Some(expected), "severity={s}");
        }
    }

    #[test]
    fn rejects_unknown_severity() {
        let args = parse_args(["--config", "/c.yaml", "--min-log-severity", "verbose"]);
        assert!(matches!(args, Err(ParseError::InvalidSeverity(_))));
    }

    #[test]
    fn severity_parse_is_case_sensitive() {
        let args = parse_args(["--config", "/c.yaml", "--min-log-severity", "info"]);
        assert!(matches!(args, Err(ParseError::InvalidSeverity(_))));
    }

    #[test]
    fn full_canonical_invocation() {
        let args = parse_args([
            "-c",
            "/etc/tracer.yaml",
            "--state-dir",
            "/var/lib/rtview",
            "--min-log-severity",
            "Warning",
        ])
        .expect("parses");
        assert_eq!(args.tracer_config.to_str(), Some("/etc/tracer.yaml"));
        assert_eq!(
            args.state_dir.as_ref().map(|p| p.to_str().unwrap_or("")),
            Some("/var/lib/rtview")
        );
        assert_eq!(args.log_severity, Some(SeverityS::Warning));
    }

    #[test]
    fn missing_config_rejected() {
        let args = parse_args(["--state-dir", "/var/lib/rtview"]);
        assert_eq!(args, Err(ParseError::MissingConfig));
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
    fn severity_default_is_info() {
        assert_eq!(SeverityS::default(), SeverityS::Info);
    }

    #[test]
    fn severity_ordering_is_natural() {
        assert!(SeverityS::Debug < SeverityS::Info);
        assert!(SeverityS::Info < SeverityS::Warning);
        assert!(SeverityS::Warning < SeverityS::Error);
        assert!(SeverityS::Error < SeverityS::Emergency);
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
