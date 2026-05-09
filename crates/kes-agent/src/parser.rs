//! CLI argument parser shell for the `kes-agent` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parser shell with byte-
//! equivalent `--help` / `--version` output captured from the
//! upstream `kes-agent` binary at R335. The captured fixtures live at
//! `crates/kes-agent/tests/fixtures/upstream-{help,version}.txt`
//! and are the source of truth for both the runtime help-printing
//! path and the golden tests.

/// Byte-for-byte mirror of upstream `kes-agent --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `kes-agent --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line arguments. R335-pattern skeleton: holds the
/// raw `Vec<String>` since the upstream binary's full subcommand
/// grammar is large; concrete typed parsing lands in later rounds.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Args {
    /// Raw passthrough of all positional + non-flag arguments. Later
    /// rounds replace this with typed sub-command/flag fields.
    pub passthrough: Vec<String>,
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
}

/// Parse a slice of command-line arguments. R335-pattern skeleton:
/// recognises only `-h`/`--help` and `-v`/`--version`; everything
/// else is collected into `passthrough` for later-round typed
/// dispatch.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut passthrough = Vec::new();
    for arg in args {
        match arg.as_ref() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "-v" | "--version" => return Err(ParseError::VersionRequested),
            other => passthrough.push(other.to_string()),
        }
    }
    Ok(Args { passthrough })
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
    fn collects_passthrough() {
        let args = parse_args(["foo", "bar"]).expect("parses");
        assert_eq!(args.passthrough, vec!["foo".to_string(), "bar".to_string()]);
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
