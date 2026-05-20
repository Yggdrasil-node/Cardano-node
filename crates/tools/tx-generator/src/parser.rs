//! CLI argument parser shell for the `tx-generator` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side top-level help/version
//! compatibility wrapper around [`crate::command`], whose
//! `Command`/`commandParser` mirror lives in `command.rs`.
//! The captured fixtures live at
//! `crates/tools/tx-generator/tests/fixtures/upstream-{help,version}.txt`
//! and are the source of truth for the runtime top-level
//! help/version printing path and the golden tests.

use crate::command::{Command, CommandParseError};

/// Byte-for-byte mirror of upstream `tx-generator --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `tx-generator --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line arguments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Args {
    /// Upstream-shaped subcommand payload.
    pub command: Command,
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
    /// Command-local parse failure.
    #[error("{0}")]
    Invalid(#[from] CommandParseError),
}

/// Parse a slice of command-line arguments.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command_args = Vec::new();
    for arg in args {
        match arg.as_ref() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "-v" | "--version" => return Err(ParseError::VersionRequested),
            other => command_args.push(other.to_string()),
        }
    }
    Ok(Args {
        command: crate::command::parse_command(&command_args)?,
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
    fn parses_typed_json_command() {
        let args = parse_args(["json", "script.json"]).expect("parses");
        assert_eq!(args.command, Command::Json("script.json".into()));
    }

    #[test]
    fn rejects_missing_command() {
        assert_eq!(
            parse_args(std::iter::empty::<&str>()),
            Err(ParseError::Invalid(CommandParseError::MissingCommand))
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
