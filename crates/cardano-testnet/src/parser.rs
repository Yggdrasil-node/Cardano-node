//! CLI argument parser for the `cardano-testnet` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Parsers/Run.hs.
//!
//! R367 lands the top-level subcommand dispatch from upstream's
//! `commands :: EnvCli -> Parser CardanoTestnetCommands`. The 4
//! subcommands map to:
//!
//! | Upstream                     | Yggdrasil                                |
//! |------------------------------|------------------------------------------|
//! | `cardano CardanoTestnetCliOptions` | [`Command::Cardano`] (era-aware payload deferred) |
//! | `create-env CardanoTestnetCreateEnvOptions` | [`Command::CreateEnv`] (era-aware payload deferred) |
//! | `version VersionOptions`     | [`Command::Version`]                     |
//! | `help ...`                   | [`ParseError::HelpRequested`] short-circuit |
//!
//! Carve-outs (NOT ported in R367; tracked under `remaining_work`):
//!
//! - **`CardanoTestnetCliOptions` payload** (era-aware): the deep
//!   record carries `Cardano.Api.AnyShelleyBasedEra` / cluster
//!   topology / per-era genesis options. Yggdrasil's port stops at
//!   the subcommand-recognition layer; the era-aware payload lands
//!   when yggdrasil-ledger's era surface is exposed at crate
//!   boundaries.
//! - **`CardanoTestnetCreateEnvOptions` payload** (era-aware): same
//!   carve-out as above.
//! - **`Cardano.CLI.Environment.EnvCli`** (env-var threading): the
//!   Yggdrasil port is environment-blind for this binary; future
//!   rounds can layer env-var threading on top.
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `cardano-testnet` binary; fixtures captured at R335 live at
//! `crates/cardano-testnet/tests/fixtures/upstream-{help,version}.txt`.

/// Byte-for-byte mirror of upstream `cardano-testnet --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `cardano-testnet --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Top-level subcommand dispatch — partial mirror of upstream
/// `data CardanoTestnetCommands`.
///
/// Each variant currently carries an opaque [`PassthroughArgs`] —
/// the deep era-aware option records (`CardanoTestnetCliOptions`,
/// `CardanoTestnetCreateEnvOptions`, `VersionOptions`) live in
/// upstream's `Testnet.Start.Types` and depend on `Cardano.Api`
/// era machinery that yggdrasil-ledger has not yet exposed at
/// crate boundaries. Subsequent rounds will replace
/// `PassthroughArgs` with the typed records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// `cardano` subcommand — run a testnet using era-aware options
    /// (mirror of upstream `StartCardanoTestnet CardanoTestnetCliOptions`).
    Cardano(PassthroughArgs),
    /// `create-env` subcommand — create a testnet environment for
    /// later use (mirror of upstream `CreateTestnetEnv
    /// CardanoTestnetCreateEnvOptions`).
    CreateEnv(PassthroughArgs),
    /// `version` subcommand — emit the version banner (mirror of
    /// upstream `GetVersion VersionOptions`).
    Version(PassthroughArgs),
}

/// Opaque passthrough wrapping the post-subcommand argv tail. R367
/// preserves the operator-supplied flags verbatim so they can be
/// passed through to the era-aware parser when it lands. Each
/// variant will replace this with its typed record in subsequent
/// rounds.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct PassthroughArgs {
    /// Raw post-subcommand flags + values, in order.
    pub raw: Vec<String>,
}

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen at the top level (or via the `help`
    /// subcommand).
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` was seen at the top level. Note that the upstream
    /// `version` subcommand is a separate dispatch path; this variant
    /// only fires for the top-level `--version` flag.
    #[error("(--version requested)")]
    VersionRequested,
    /// No subcommand was supplied.
    #[error("missing subcommand: expected one of cardano, create-env, version, help")]
    MissingSubcommand,
    /// An unknown subcommand was supplied.
    #[error("unknown subcommand: {0}")]
    UnknownSubcommand(String),
}

/// Parse a slice of command-line arguments into a [`Command`]. R367
/// implementation: top-level `--help`/`--version` short-circuit;
/// otherwise locate the first non-flag positional and treat it as
/// the subcommand keyword. Everything after the subcommand is
/// captured verbatim in [`PassthroughArgs::raw`].
pub fn parse_args<I, S>(args: I) -> Result<Command, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let argv: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();

    // Top-level --help / --version short-circuit anywhere in argv.
    for arg in &argv {
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "--version" => return Err(ParseError::VersionRequested),
            _ => {}
        }
    }

    // Locate the first non-flag positional — the subcommand keyword.
    let subcommand_idx = argv
        .iter()
        .position(|a| !a.starts_with('-'))
        .ok_or(ParseError::MissingSubcommand)?;

    let subcommand = &argv[subcommand_idx];
    let after = argv[subcommand_idx + 1..].to_vec();
    let passthrough = PassthroughArgs { raw: after };

    match subcommand.as_str() {
        "cardano" => Ok(Command::Cardano(passthrough)),
        "create-env" => Ok(Command::CreateEnv(passthrough)),
        "version" => Ok(Command::Version(passthrough)),
        "help" => Err(ParseError::HelpRequested),
        other => Err(ParseError::UnknownSubcommand(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_help_long_at_top_level() {
        assert_eq!(parse_args(["--help"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_help_short_at_top_level() {
        assert_eq!(parse_args(["-h"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_version_at_top_level() {
        assert_eq!(parse_args(["--version"]), Err(ParseError::VersionRequested));
    }

    #[test]
    fn empty_argv_errors_on_missing_subcommand() {
        let argv: Vec<String> = Vec::new();
        assert_eq!(parse_args(&argv), Err(ParseError::MissingSubcommand));
    }

    #[test]
    fn unknown_subcommand_errors() {
        assert_eq!(
            parse_args(["frobnicate"]),
            Err(ParseError::UnknownSubcommand("frobnicate".to_string()))
        );
    }

    #[test]
    fn dispatches_cardano_subcommand() {
        let cmd = parse_args(["cardano"]).expect("parses");
        assert!(matches!(cmd, Command::Cardano(_)));
    }

    #[test]
    fn dispatches_create_env_subcommand() {
        let cmd = parse_args(["create-env"]).expect("parses");
        assert!(matches!(cmd, Command::CreateEnv(_)));
    }

    #[test]
    fn dispatches_version_subcommand() {
        let cmd = parse_args(["version"]).expect("parses");
        assert!(matches!(cmd, Command::Version(_)));
    }

    #[test]
    fn help_subcommand_short_circuits_to_help_requested() {
        assert_eq!(parse_args(["help"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn cardano_subcommand_captures_passthrough_args() {
        let cmd = parse_args(["cardano", "--num-pool-nodes", "3", "--num-relay-nodes", "2"])
            .expect("parses");
        match cmd {
            Command::Cardano(p) => {
                assert_eq!(
                    p.raw,
                    vec![
                        "--num-pool-nodes".to_string(),
                        "3".to_string(),
                        "--num-relay-nodes".to_string(),
                        "2".to_string(),
                    ]
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn create_env_subcommand_captures_passthrough_args() {
        let cmd = parse_args(["create-env", "--output-dir", "/tmp/testnet-env"]).expect("parses");
        match cmd {
            Command::CreateEnv(p) => {
                assert_eq!(
                    p.raw,
                    vec!["--output-dir".to_string(), "/tmp/testnet-env".to_string()]
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn version_subcommand_with_no_args_yields_empty_passthrough() {
        let cmd = parse_args(["version"]).expect("parses");
        match cmd {
            Command::Version(p) => assert!(p.raw.is_empty()),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn help_inside_subcommand_window_short_circuits() {
        // Upstream's `--help` works anywhere in argv; R367 mirrors
        // that behavior since top-level `--help` peek runs before
        // subcommand identification.
        assert_eq!(
            parse_args(["cardano", "--help"]),
            Err(ParseError::HelpRequested)
        );
    }

    #[test]
    fn passthrough_args_default_is_empty() {
        assert_eq!(
            PassthroughArgs::default(),
            PassthroughArgs { raw: Vec::new() }
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
