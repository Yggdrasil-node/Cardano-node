//! CLI argument parser for the `kes-agent` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/cli/AgentMain.hs.
//!
//! Direct port of upstream's `pProgramOptions`,
//! `pProgramModeOptions`, `pNormalModeOptions`, `readLogLevel`, and
//! `readLogTarget` parser surface, plus the environment-derived
//! `nmoFromEnv` / `smoFromEnv` option overlays. The daemon/socket
//! runtime remains deferred in [`crate::run`], but argv and env
//! options are now shaped like upstream before that boundary.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Byte-for-byte mirror of upstream `kes-agent --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `kes-agent --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Logging target. Mirrors upstream `data LogTarget`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum LogTarget {
    /// `LogDevNull`.
    LogDevNull,
    /// `LogStdout`.
    LogStdout,
    /// `LogSyslog`.
    LogSyslog,
}

/// Logging priority. Mirrors the upstream `Priority` values accepted
/// by `readLogLevel` in `AgentMain.hs`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum LogLevel {
    /// `Debug`.
    Debug,
    /// `Info`.
    Info,
    /// `Notice`.
    Notice,
    /// `Warning`.
    Warning,
    /// `Error`.
    Error,
    /// `Critical`.
    Critical,
    /// `Emergency`.
    Emergency,
}

/// `run` subcommand options. Mirrors upstream `NormalModeOptions`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalModeOptions {
    /// `nmoServicePath`.
    pub service_path: Option<String>,
    /// `nmoControlPath`.
    pub control_path: Option<String>,
    /// `nmoBootstrapPaths`.
    pub bootstrap_paths: BTreeSet<String>,
    /// `nmoLogLevel`.
    pub log_level: Option<LogLevel>,
    /// `nmoColdVerKeyFile`.
    pub cold_ver_key_file: Option<PathBuf>,
    /// `nmoGenesisFile`.
    pub genesis_file: Option<PathBuf>,
    /// `nmoLogTarget`.
    pub log_target: Option<LogTarget>,
}

impl Default for NormalModeOptions {
    fn default() -> Self {
        Self::defaults()
    }
}

impl NormalModeOptions {
    /// Default values matching upstream `defNormalModeOptions`.
    pub fn defaults() -> Self {
        Self {
            service_path: Some("/tmp/kes-agent-service.socket".to_string()),
            control_path: Some("/tmp/kes-agent-control.socket".to_string()),
            bootstrap_paths: BTreeSet::new(),
            log_level: Some(LogLevel::Notice),
            cold_ver_key_file: None,
            genesis_file: None,
            log_target: None,
        }
    }

    /// Derive normal-mode options from process environment variables.
    /// Mirrors upstream `nmoFromEnv`.
    pub fn from_env() -> Result<Self, ParseError> {
        nmo_from_env()
    }

    /// Test-friendly variant of [`Self::from_env`] using a supplied
    /// variable lookup function.
    pub fn from_env_lookup<F>(lookup: F) -> Result<Self, ParseError>
    where
        F: Fn(&str) -> Option<String>,
    {
        nmo_from_env_lookup(lookup)
    }
}

/// Service command options. Mirrors upstream `ServiceModeOptions`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceModeOptions {
    /// `smoServicePath`.
    pub service_path: Option<String>,
    /// `smoControlPath`.
    pub control_path: Option<String>,
    /// `smoBootstrapPaths`.
    pub bootstrap_paths: BTreeSet<String>,
    /// `smoUser`.
    pub user: Option<String>,
    /// `smoGroup`.
    pub group: Option<String>,
    /// `smoColdVerKeyFile`.
    pub cold_ver_key_file: Option<PathBuf>,
    /// `smoGenesisFile`.
    pub genesis_file: Option<PathBuf>,
}

impl Default for ServiceModeOptions {
    fn default() -> Self {
        Self::defaults()
    }
}

impl ServiceModeOptions {
    /// Default values matching upstream `defServiceModeOptions`.
    pub fn defaults() -> Self {
        Self {
            service_path: Some("/tmp/kes-agent-service.socket".to_string()),
            control_path: Some("/tmp/kes-agent-control.socket".to_string()),
            bootstrap_paths: BTreeSet::new(),
            user: Some("kes-agent".to_string()),
            group: Some("kes-agent".to_string()),
            cold_ver_key_file: None,
            genesis_file: None,
        }
    }

    /// Empty service-mode options matching upstream
    /// `nullServiceModeOptions`.
    pub fn null_options() -> Self {
        Self {
            service_path: None,
            control_path: None,
            bootstrap_paths: BTreeSet::new(),
            user: None,
            group: None,
            cold_ver_key_file: None,
            genesis_file: None,
        }
    }

    /// Derive service-mode options from process environment variables.
    /// Mirrors upstream `smoFromEnv`.
    pub fn from_env() -> Result<Self, ParseError> {
        smo_from_env()
    }

    /// Test-friendly variant of [`Self::from_env`] using a supplied
    /// variable lookup function.
    pub fn from_env_lookup<F>(lookup: F) -> Result<Self, ParseError>
    where
        F: Fn(&str) -> Option<String>,
    {
        smo_from_env_lookup(lookup)
    }
}

/// Top-level command mode. Mirrors upstream `ProgramModeOptions`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramModeOptions {
    /// `RunAsService ServiceModeOptions`.
    RunAsService(ServiceModeOptions),
    /// `RunNormally NormalModeOptions`.
    RunNormally(NormalModeOptions),
}

/// Parsed command-line arguments. Mirrors upstream `ProgramOptions`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramOptions {
    /// `poMode`.
    pub mode: ProgramModeOptions,
    /// `poExtraConfigPath`.
    pub extra_config_path: Option<PathBuf>,
}

/// Backwards-compatible alias for callers using the parser module's
/// previous `Args` name.
pub type Args = ProgramOptions;

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen.
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` was seen.
    #[error("(--version requested)")]
    VersionRequested,
    /// No command was supplied.
    #[error("missing command: expected one of start, stop, restart, status, run")]
    MissingCommand,
    /// An unknown command was supplied.
    #[error("unknown command: {0}")]
    UnknownCommand(String),
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

/// Parse a slice of command-line arguments. Mirror of upstream
/// `pProgramOptions`.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let argv: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    for arg in &argv {
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "--version" => return Err(ParseError::VersionRequested),
            _ => {}
        }
    }

    let command_idx = argv
        .iter()
        .position(|a| matches!(a.as_str(), "start" | "stop" | "restart" | "status" | "run"))
        .ok_or_else(|| {
            if let Some(positional) = argv.iter().find(|a| !a.starts_with('-')) {
                ParseError::UnknownCommand(positional.clone())
            } else {
                ParseError::MissingCommand
            }
        })?;

    let command = &argv[command_idx];
    let before = &argv[..command_idx];
    let after = &argv[command_idx + 1..];
    let extra_config_path = parse_config_path(before, after)?;
    let mode = match command.as_str() {
        "start" | "stop" | "restart" | "status" => {
            reject_non_config_flags(before)?;
            reject_non_config_flags(after)?;
            ProgramModeOptions::RunAsService(ServiceModeOptions::null_options())
        }
        "run" => {
            reject_non_config_flags(before)?;
            ProgramModeOptions::RunNormally(parse_normal_mode_options(after)?)
        }
        other => return Err(ParseError::UnknownCommand(other.to_string())),
    };
    Ok(ProgramOptions {
        mode,
        extra_config_path,
    })
}

fn parse_config_path(before: &[String], after: &[String]) -> Result<Option<PathBuf>, ParseError> {
    let mut out = None;
    for window in [before, after] {
        let mut iter = window.iter().peekable();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-F" | "--config" | "--config-file" => {
                    out = Some(PathBuf::from(take_value(&mut iter, arg)?));
                }
                _ => {}
            }
        }
    }
    Ok(out)
}

fn reject_non_config_flags(window: &[String]) -> Result<(), ParseError> {
    let mut iter = window.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-F" | "--config" | "--config-file" => {
                let _ = take_value(&mut iter, arg)?;
            }
            other if other.starts_with('-') => return Err(ParseError::UnknownFlag(other.into())),
            other => return Err(ParseError::UnknownCommand(other.into())),
        }
    }
    Ok(())
}

fn parse_normal_mode_options(window: &[String]) -> Result<NormalModeOptions, ParseError> {
    let mut out = NormalModeOptions {
        service_path: None,
        control_path: None,
        bootstrap_paths: BTreeSet::new(),
        log_level: None,
        cold_ver_key_file: None,
        genesis_file: None,
        log_target: None,
    };
    let mut iter = window.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-F" | "--config" | "--config-file" => {
                let _ = take_value(&mut iter, arg)?;
            }
            "-s" | "--service-address" => {
                out.service_path = Some(take_value(&mut iter, arg)?);
            }
            "-c" | "--control-address" => {
                out.control_path = Some(take_value(&mut iter, arg)?);
            }
            "-b" | "--bootstrap-address" => {
                out.bootstrap_paths.insert(take_value(&mut iter, arg)?);
            }
            "-l" | "--log-level" => {
                let value = take_value(&mut iter, arg)?;
                out.log_level = Some(
                    read_log_level(&value)
                        .map_err(|e| ParseError::InvalidValue(arg.to_string(), e))?,
                );
            }
            "--cold-verification-key" => {
                out.cold_ver_key_file = Some(PathBuf::from(take_value(&mut iter, arg)?));
            }
            "--genesis-file" => {
                out.genesis_file = Some(PathBuf::from(take_value(&mut iter, arg)?));
            }
            "--log-target" => {
                let value = take_value(&mut iter, arg)?;
                out.log_target = Some(
                    read_log_target(&value)
                        .map_err(|e| ParseError::InvalidValue(arg.to_string(), e))?,
                );
            }
            other => return Err(ParseError::UnknownFlag(other.to_string())),
        }
    }
    Ok(out)
}

fn take_value<'a, I>(iter: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, ParseError>
where
    I: Iterator<Item = &'a String>,
{
    iter.next()
        .cloned()
        .ok_or_else(|| ParseError::MissingValue(flag.to_string()))
}

/// Mirror of upstream `readLogLevel`.
pub fn read_log_level(value: &str) -> Result<LogLevel, String> {
    match value {
        "debug" => Ok(LogLevel::Debug),
        "info" => Ok(LogLevel::Info),
        "warn" => Ok(LogLevel::Warning),
        "notice" => Ok(LogLevel::Notice),
        "error" => Ok(LogLevel::Error),
        "critical" => Ok(LogLevel::Critical),
        "emergency" => Ok(LogLevel::Emergency),
        x => Err(format!("Invalid log level {x:?}")),
    }
}

/// Mirror of upstream `readLogTarget`.
pub fn read_log_target(value: &str) -> Result<LogTarget, String> {
    match value {
        "null" => Ok(LogTarget::LogDevNull),
        "stdout" => Ok(LogTarget::LogStdout),
        "syslog" => Ok(LogTarget::LogSyslog),
        x => Err(format!("Invalid log target {x:?}")),
    }
}

/// Split on a separator, matching upstream `splitBy`.
///
/// Empty input returns no segments. Leading empty segments are
/// preserved, and a trailing separator does not add a final empty
/// segment.
pub fn split_by(sep: char, value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut rest = value;
    loop {
        match rest.find(sep) {
            Some(idx) => {
                out.push(rest[..idx].to_string());
                rest = &rest[idx + sep.len_utf8()..];
                if rest.is_empty() {
                    break;
                }
            }
            None => {
                out.push(rest.to_string());
                break;
            }
        }
    }
    out
}

/// Derive normal-mode options from process environment variables.
/// Mirrors upstream `nmoFromEnv`.
pub fn nmo_from_env() -> Result<NormalModeOptions, ParseError> {
    nmo_from_env_lookup(|key| std::env::var(key).ok())
}

/// Test-friendly variant of [`nmo_from_env`] using a supplied variable
/// lookup function.
pub fn nmo_from_env_lookup<F>(lookup: F) -> Result<NormalModeOptions, ParseError>
where
    F: Fn(&str) -> Option<String>,
{
    let bootstrap_paths = lookup("KES_AGENT_BOOTSTRAP_PATHS")
        .map(|raw| split_by(':', &raw).into_iter().collect())
        .unwrap_or_default();
    let log_level = lookup("KES_AGENT_LOG_LEVEL")
        .map(|value| {
            read_log_level(&value)
                .map_err(|e| ParseError::InvalidValue("KES_AGENT_LOG_LEVEL".to_string(), e))
        })
        .transpose()?;
    let log_target = lookup("KES_AGENT_LOG_TARGET")
        .map(|value| {
            read_log_target(&value)
                .map_err(|e| ParseError::InvalidValue("KES_AGENT_LOG_TARGET".to_string(), e))
        })
        .transpose()?;
    Ok(NormalModeOptions {
        service_path: lookup("KES_AGENT_SERVICE_PATH"),
        control_path: lookup("KES_AGENT_CONTROL_PATH"),
        bootstrap_paths,
        log_level,
        cold_ver_key_file: lookup("KES_AGENT_COLD_VK").map(PathBuf::from),
        genesis_file: lookup("KES_AGENT_GENESIS_FILE").map(PathBuf::from),
        log_target,
    })
}

/// Derive service-mode options from process environment variables.
/// Mirrors upstream `smoFromEnv`.
pub fn smo_from_env() -> Result<ServiceModeOptions, ParseError> {
    smo_from_env_lookup(|key| std::env::var(key).ok())
}

/// Test-friendly variant of [`smo_from_env`] using a supplied variable
/// lookup function.
pub fn smo_from_env_lookup<F>(lookup: F) -> Result<ServiceModeOptions, ParseError>
where
    F: Fn(&str) -> Option<String>,
{
    let bootstrap_paths = lookup("KES_AGENT_BOOTSTRAP_PATHS")
        .map(|raw| split_by(':', &raw).into_iter().collect())
        .unwrap_or_default();
    Ok(ServiceModeOptions {
        service_path: lookup("KES_AGENT_SERVICE_PATH"),
        control_path: lookup("KES_AGENT_CONTROL_PATH"),
        bootstrap_paths,
        user: lookup("KES_AGENT_USER"),
        group: lookup("KES_AGENT_GROUP"),
        cold_ver_key_file: lookup("KES_AGENT_COLD_VK").map(PathBuf::from),
        genesis_file: lookup("KES_AGENT_GENESIS_FILE").map(PathBuf::from),
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
    fn rejects_version_short_like_upstream() {
        assert!(matches!(
            parse_args(["-v"]),
            Err(ParseError::MissingCommand | ParseError::UnknownFlag(_))
        ));
    }

    #[test]
    fn help_fixture_non_empty() {
        assert!(!HELP_TEXT.is_empty());
    }

    #[test]
    fn version_fixture_non_empty() {
        assert!(!VERSION_TEXT.is_empty());
    }

    #[test]
    fn rejects_missing_command() {
        let argv: Vec<String> = Vec::new();
        assert_eq!(parse_args(argv), Err(ParseError::MissingCommand));
    }

    #[test]
    fn rejects_unknown_command() {
        assert!(matches!(
            parse_args(["frobnicate"]),
            Err(ParseError::UnknownCommand(_))
        ));
    }

    #[test]
    fn service_commands_map_to_run_as_service_null_options() {
        for command in ["start", "stop", "restart", "status"] {
            let args = parse_args([command]).expect("parses");
            assert_eq!(args.extra_config_path, None);
            assert_eq!(
                args.mode,
                ProgramModeOptions::RunAsService(ServiceModeOptions::null_options())
            );
        }
    }

    #[test]
    fn config_file_aliases_parse() {
        for flag in ["-F", "--config", "--config-file"] {
            let args = parse_args(["run", flag, "agent.toml"]).expect("parses");
            assert_eq!(
                args.extra_config_path.as_deref().and_then(|p| p.to_str()),
                Some("agent.toml")
            );
        }
    }

    #[test]
    fn parses_run_mode_options() {
        let args = parse_args([
            "run",
            "--service-address",
            "/tmp/service.sock",
            "--control-address",
            "",
            "--bootstrap-address",
            "/tmp/peer-a.sock",
            "--bootstrap-address",
            "/tmp/peer-b.sock",
            "--log-level",
            "warn",
            "--cold-verification-key",
            "cold.vkey",
            "--genesis-file",
            "genesis.json",
            "--log-target",
            "stdout",
        ])
        .expect("parses");
        match args.mode {
            ProgramModeOptions::RunNormally(o) => {
                assert_eq!(o.service_path.as_deref(), Some("/tmp/service.sock"));
                assert_eq!(o.control_path.as_deref(), Some(""));
                assert_eq!(o.bootstrap_paths.len(), 2);
                assert_eq!(o.log_level, Some(LogLevel::Warning));
                assert_eq!(
                    o.cold_ver_key_file.as_deref().and_then(|p| p.to_str()),
                    Some("cold.vkey")
                );
                assert_eq!(
                    o.genesis_file.as_deref().and_then(|p| p.to_str()),
                    Some("genesis.json")
                );
                assert_eq!(o.log_target, Some(LogTarget::LogStdout));
            }
            _ => panic!("wrong mode"),
        }
    }

    #[test]
    fn normal_defaults_match_upstream() {
        let d = NormalModeOptions::defaults();
        assert_eq!(
            d.service_path.as_deref(),
            Some("/tmp/kes-agent-service.socket")
        );
        assert_eq!(
            d.control_path.as_deref(),
            Some("/tmp/kes-agent-control.socket")
        );
        assert!(d.bootstrap_paths.is_empty());
        assert_eq!(d.log_level, Some(LogLevel::Notice));
        assert!(d.cold_ver_key_file.is_none());
        assert!(d.genesis_file.is_none());
        assert!(d.log_target.is_none());
    }

    #[test]
    fn service_defaults_and_null_options_match_upstream() {
        let d = ServiceModeOptions::defaults();
        assert_eq!(d.user.as_deref(), Some("kes-agent"));
        assert_eq!(d.group.as_deref(), Some("kes-agent"));
        let n = ServiceModeOptions::null_options();
        assert!(n.service_path.is_none());
        assert!(n.control_path.is_none());
        assert!(n.user.is_none());
        assert!(n.group.is_none());
        assert!(n.bootstrap_paths.is_empty());
    }

    #[test]
    fn read_log_level_matches_upstream_spellings() {
        assert_eq!(read_log_level("debug"), Ok(LogLevel::Debug));
        assert_eq!(read_log_level("info"), Ok(LogLevel::Info));
        assert_eq!(read_log_level("warn"), Ok(LogLevel::Warning));
        assert_eq!(read_log_level("notice"), Ok(LogLevel::Notice));
        assert_eq!(read_log_level("error"), Ok(LogLevel::Error));
        assert_eq!(read_log_level("critical"), Ok(LogLevel::Critical));
        assert_eq!(read_log_level("emergency"), Ok(LogLevel::Emergency));
        assert!(read_log_level("warning").is_err());
    }

    #[test]
    fn read_log_target_matches_upstream_spellings() {
        assert_eq!(read_log_target("null"), Ok(LogTarget::LogDevNull));
        assert_eq!(read_log_target("stdout"), Ok(LogTarget::LogStdout));
        assert_eq!(read_log_target("syslog"), Ok(LogTarget::LogSyslog));
        assert!(read_log_target("stderr").is_err());
    }

    #[test]
    fn split_by_matches_upstream_edges() {
        assert_eq!(split_by(':', ""), Vec::<String>::new());
        assert_eq!(split_by(':', "a:b"), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            split_by(':', "a:b:"),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(split_by(':', ":a"), vec!["".to_string(), "a".to_string()]);
        assert_eq!(
            split_by(':', "a::b"),
            vec!["a".to_string(), "".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn nmo_from_env_lookup_reads_upstream_variables() {
        let opts = nmo_from_env_lookup(|key| match key {
            "KES_AGENT_SERVICE_PATH" => Some("/tmp/service.sock".to_string()),
            "KES_AGENT_CONTROL_PATH" => Some("/tmp/control.sock".to_string()),
            "KES_AGENT_BOOTSTRAP_PATHS" => Some("/tmp/a.sock:/tmp/b.sock".to_string()),
            "KES_AGENT_COLD_VK" => Some("cold.vkey".to_string()),
            "KES_AGENT_GENESIS_FILE" => Some("genesis.json".to_string()),
            "KES_AGENT_LOG_LEVEL" => Some("debug".to_string()),
            "KES_AGENT_LOG_TARGET" => Some("syslog".to_string()),
            _ => None,
        })
        .expect("env parses");
        assert_eq!(opts.service_path.as_deref(), Some("/tmp/service.sock"));
        assert_eq!(opts.control_path.as_deref(), Some("/tmp/control.sock"));
        assert_eq!(opts.bootstrap_paths.len(), 2);
        assert!(opts.bootstrap_paths.contains("/tmp/a.sock"));
        assert!(opts.bootstrap_paths.contains("/tmp/b.sock"));
        assert_eq!(
            opts.cold_ver_key_file.as_deref().and_then(|p| p.to_str()),
            Some("cold.vkey")
        );
        assert_eq!(
            opts.genesis_file.as_deref().and_then(|p| p.to_str()),
            Some("genesis.json")
        );
        assert_eq!(opts.log_level, Some(LogLevel::Debug));
        assert_eq!(opts.log_target, Some(LogTarget::LogSyslog));
    }

    #[test]
    fn nmo_from_env_lookup_rejects_invalid_log_values() {
        assert!(matches!(
            nmo_from_env_lookup(|key| match key {
                "KES_AGENT_LOG_LEVEL" => Some("warning".to_string()),
                _ => None,
            }),
            Err(ParseError::InvalidValue(flag, _)) if flag == "KES_AGENT_LOG_LEVEL"
        ));
        assert!(matches!(
            nmo_from_env_lookup(|key| match key {
                "KES_AGENT_LOG_TARGET" => Some("stderr".to_string()),
                _ => None,
            }),
            Err(ParseError::InvalidValue(flag, _)) if flag == "KES_AGENT_LOG_TARGET"
        ));
    }

    #[test]
    fn smo_from_env_lookup_reads_upstream_variables() {
        let opts = smo_from_env_lookup(|key| match key {
            "KES_AGENT_SERVICE_PATH" => Some("/tmp/service.sock".to_string()),
            "KES_AGENT_CONTROL_PATH" => Some("/tmp/control.sock".to_string()),
            "KES_AGENT_BOOTSTRAP_PATHS" => Some("/tmp/a.sock:/tmp/b.sock".to_string()),
            "KES_AGENT_COLD_VK" => Some("cold.vkey".to_string()),
            "KES_AGENT_GROUP" => Some("kes-group".to_string()),
            "KES_AGENT_USER" => Some("kes-user".to_string()),
            "KES_AGENT_GENESIS_FILE" => Some("genesis.json".to_string()),
            _ => None,
        })
        .expect("env parses");
        assert_eq!(opts.service_path.as_deref(), Some("/tmp/service.sock"));
        assert_eq!(opts.control_path.as_deref(), Some("/tmp/control.sock"));
        assert_eq!(opts.bootstrap_paths.len(), 2);
        assert_eq!(opts.user.as_deref(), Some("kes-user"));
        assert_eq!(opts.group.as_deref(), Some("kes-group"));
        assert_eq!(
            opts.cold_ver_key_file.as_deref().and_then(|p| p.to_str()),
            Some("cold.vkey")
        );
        assert_eq!(
            opts.genesis_file.as_deref().and_then(|p| p.to_str()),
            Some("genesis.json")
        );
    }

    #[test]
    fn rejects_unknown_run_flag() {
        assert!(matches!(
            parse_args(["run", "--not-a-real-flag"]),
            Err(ParseError::UnknownFlag(_))
        ));
    }

    #[test]
    fn rejects_invalid_log_level() {
        assert!(matches!(
            parse_args(["run", "--log-level", "warning"]),
            Err(ParseError::InvalidValue(_, _))
        ));
    }
}
