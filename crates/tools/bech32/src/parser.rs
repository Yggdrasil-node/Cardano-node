//! CLI argument parser for the `bech32` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parser shell wrapping the
//! upstream optparse-applicative parser embedded inline in
//! `bech32/app/Main.hs::main`. clap can't match optparse-applicative's
//! exact help-text format byte-for-byte (different conventions for
//! flag rendering, ANSI escape codes, multi-line option descriptions),
//! so this module bypasses clap's auto-generated `--help` / `--version`
//! and emits hand-crafted byte-equivalent output captured from the
//! upstream binary at R332. The captured fixture lives at
//! `crates/tools/bech32/tests/fixtures/upstream-help.txt` and is the
//! source of truth for both the runtime help-printing path and the
//! golden test in `tests/cli_help_golden.rs`.
//!
//! Argument-parsing logic itself uses simple manual scanning of
//! `std::env::args()` — the upstream CLI surface is just one
//! optional positional (`PREFIX`) plus the standard `-h`/`--help`
//! and `-v`/`--version` flags, so a clap derive layer would be
//! pure overhead.

/// Byte-for-byte mirror of upstream `bech32 --help` (captured from
/// `.reference-haskell-cardano-node/install/bin/bech32 --help` at R332).
///
/// The fixture includes ANSI escape sequences (`\x1b[0;4m` underline,
/// `\x1b[0;1m` bold) emitted by upstream's optparse-applicative help
/// renderer. They render as styled text on TTY-capable terminals and
/// pass through unchanged when stdout is redirected.
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `bech32 --version` (captured at R332).
///
/// Upstream emits `1.1.10\n` — just the upstream version string with
/// a trailing newline. Yggdrasil's port targets bech32 v1.1.10 surface
/// parity so the byte-equivalent output is the right semantic claim:
/// "this binary implements the bech32 1.1.10 protocol".
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line arguments for the `bech32` binary.
///
/// Mirrors upstream's inline parser shape from `bech32/app/Main.hs::main`:
/// an optional positional `PREFIX` argument plus the implicit
/// `--help` / `--version` flags handled before reaching the parser
/// stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Args {
    /// Optional human-readable prefix (e.g. `addr`, `stake`,
    /// `addr_test`). When `Some(prefix)`, the binary reads stdin,
    /// decodes from base16/bech32/base58, and re-encodes to bech32
    /// using `prefix`. When `None`, the binary reads stdin (expected
    /// to be bech32) and emits base16.
    pub prefix: Option<String>,
}

/// Errors from CLI argument parsing.
///
/// Mirrors upstream's `optparse-applicative` error path: unknown
/// flags / extra positional arguments cause `bech32` to print a
/// short usage line to stderr and exit with code 1. The `HelpPrinted`
/// / `VersionPrinted` variants are not errors per se but flow-control
/// signals — `main` translates them into exit code 0.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen; caller should print [`HELP_TEXT`]
    /// and exit 0.
    #[error("(--help requested)")]
    HelpRequested,

    /// `--version` / `-v` was seen; caller should print
    /// [`VERSION_TEXT`] and exit 0.
    #[error("(--version requested)")]
    VersionRequested,

    /// An unknown flag was passed.
    #[error("Invalid option `{0}'")]
    UnknownFlag(String),

    /// More than one positional argument was passed (upstream's
    /// parser only accepts a single optional `PREFIX`).
    #[error("too many positional arguments (expected at most one PREFIX)")]
    TooManyPositionals,
}

/// Parse a slice of command-line arguments (typically
/// `std::env::args().skip(1).collect()`) into [`Args`].
///
/// Returns:
/// - `Ok(Args { prefix: None })` if no args.
/// - `Ok(Args { prefix: Some(p) })` if exactly one positional.
/// - `Err(ParseError::HelpRequested)` on `-h` / `--help`.
/// - `Err(ParseError::VersionRequested)` on `-v` / `--version`.
/// - `Err(ParseError::UnknownFlag)` on any other flag.
/// - `Err(ParseError::TooManyPositionals)` on >1 positional.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut prefix: Option<String> = None;
    for arg in args {
        let arg_ref = arg.as_ref();
        match arg_ref {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "-v" | "--version" => return Err(ParseError::VersionRequested),
            other if other.starts_with('-') => {
                return Err(ParseError::UnknownFlag(other.to_string()));
            }
            other => {
                if prefix.is_some() {
                    return Err(ParseError::TooManyPositionals);
                }
                prefix = Some(other.to_string());
            }
        }
    }
    Ok(Args { prefix })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_no_args_as_no_prefix() {
        assert_eq!(
            parse_args::<_, &str>(Vec::<&str>::new()),
            Ok(Args { prefix: None })
        );
    }

    #[test]
    fn parses_single_positional_as_prefix() {
        assert_eq!(
            parse_args(["addr"]),
            Ok(Args {
                prefix: Some("addr".to_string())
            }),
        );
    }

    #[test]
    fn rejects_two_positionals() {
        assert_eq!(
            parse_args(["addr", "extra"]),
            Err(ParseError::TooManyPositionals),
        );
    }

    #[test]
    fn detects_help_flag_long() {
        assert_eq!(parse_args(["--help"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_help_flag_short() {
        assert_eq!(parse_args(["-h"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_version_flag_long() {
        assert_eq!(parse_args(["--version"]), Err(ParseError::VersionRequested));
    }

    #[test]
    fn detects_version_flag_short() {
        assert_eq!(parse_args(["-v"]), Err(ParseError::VersionRequested));
    }

    #[test]
    fn rejects_unknown_flag() {
        assert_eq!(
            parse_args(["--unknown"]),
            Err(ParseError::UnknownFlag("--unknown".to_string())),
        );
    }

    /// Pin the help-text fixture against drift. If upstream's
    /// `--help` output ever changes (e.g. a new bech32 release adds
    /// flags), refreshing the fixture and updating the placeholder
    /// types in `lib.rs` should be a single coordinated round.
    #[test]
    fn help_text_starts_with_canonical_usage_line() {
        assert!(HELP_TEXT.starts_with("Usage: bech32 [PREFIX]\n"));
    }

    #[test]
    fn version_text_matches_upstream_bech32_release() {
        // Upstream `bech32 --version` emits `1.1.10\n`. Yggdrasil's
        // port targets that release; the byte-equivalent output is
        // the right semantic claim.
        assert_eq!(VERSION_TEXT, "1.1.10\n");
    }
}
