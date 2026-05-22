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
//! The era-aware option parsers of `Parsers/Cardano.hs` land
//! incrementally from R818 (`parse_runtime_options`,
//! `parse_genesis_options` so far) — the earlier "blocked on
//! yggdrasil-ledger's era surface" carve-out was resolved by the
//! R783-R786 era-type ports (`CardanoEra` / `ShelleyBasedEra` and
//! the option records). The `Command` variants still carry
//! `PassthroughArgs` until those parsers compose into the full
//! `CardanoTestnetCliOptions` / `CardanoTestnetCreateEnvOptions`.
//!
//! Carve-out:
//!
//! - **`Cardano.CLI.Environment.EnvCli`** (env-var threading): the
//!   Yggdrasil port is environment-blind for this binary; future
//!   rounds can layer env-var threading on top.
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `cardano-testnet` binary; fixtures captured at R335 live at
//! `crates/tools/cardano-testnet/tests/fixtures/upstream-{help,version}.txt`.

/// Byte-for-byte mirror of upstream `cardano-testnet --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `cardano-testnet --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

use std::path::PathBuf;
use std::str::FromStr;

use crate::types::{
    CardanoTestnetCliOptions, CardanoTestnetCreateEnvOptions, GenesisOptions,
    NoUserProvidedEnvOptions, NodeOption, NumDReps, PraosCredentialsSource, RpcSupport,
    StartFromEnvOptions, TestnetCreationOptions, TestnetEnvOptions, TestnetOnChainParams,
    TestnetRuntimeOptions, UpdateTimestamps, cardano_default_testnet_node_options,
};

/// Look up the value following a `--flag` in an argument list.
///
/// Returns `Ok(None)` when the flag is absent, `Ok(Some(value))` when
/// it is present with a following value, and
/// [`ParseError::MissingFlagValue`] when the flag is the last
/// argument (present with no value).
fn flag_with_value<'a>(args: &'a [String], flag: &str) -> Result<Option<&'a str>, ParseError> {
    match args.iter().position(|arg| arg == flag) {
        None => Ok(None),
        Some(idx) => args
            .get(idx + 1)
            .map(|value| Some(value.as_str()))
            .ok_or_else(|| ParseError::MissingFlagValue {
                flag: flag.to_string(),
            }),
    }
}

/// Parse a `--flag value` option, falling back to `default` when the
/// flag is absent.
fn flag_or_default<T: FromStr>(args: &[String], flag: &str, default: T) -> Result<T, ParseError> {
    match flag_with_value(args, flag)? {
        None => Ok(default),
        Some(value) => value.parse().map_err(|_| ParseError::InvalidFlagValue {
            flag: flag.to_string(),
            value: value.to_string(),
        }),
    }
}

/// Parse the `GenesisOptions` flags from a `cardano` / `create-env`
/// argument list — `--testnet-magic`, `--epoch-length`,
/// `--slot-length`, and `--active-slots-coeff`, each defaulting to
/// `GenesisOptions::default()`.
///
/// Mirror of upstream `pGenesisOptions` (`Parsers/Cardano.hs`).
pub fn parse_genesis_options(args: &[String]) -> Result<GenesisOptions, ParseError> {
    let default = GenesisOptions::default();
    Ok(GenesisOptions {
        genesis_testnet_magic: flag_or_default(
            args,
            "--testnet-magic",
            default.genesis_testnet_magic,
        )?,
        genesis_epoch_length: flag_or_default(
            args,
            "--epoch-length",
            default.genesis_epoch_length,
        )?,
        genesis_slot_length: flag_or_default(args, "--slot-length", default.genesis_slot_length)?,
        genesis_active_slots_coeff: flag_or_default(
            args,
            "--active-slots-coeff",
            default.genesis_active_slots_coeff,
        )?,
    })
}

/// Parse the testnet node set from the `--num-pool-nodes` flag.
///
/// Mirror of upstream `pTestnetNodeOptions` (`Parsers/Cardano.hs`):
/// `--num-pool-nodes N` yields `N` stake-pool-operator nodes; absent,
/// the default one-SPO / two-relay set. At least one SPO node is
/// required.
pub fn parse_testnet_node_options(args: &[String]) -> Result<Vec<NodeOption>, ParseError> {
    match flag_with_value(args, "--num-pool-nodes")? {
        None => Ok(cardano_default_testnet_node_options()),
        Some(value) => {
            let invalid = || ParseError::InvalidFlagValue {
                flag: "--num-pool-nodes".to_string(),
                value: value.to_string(),
            };
            let count: i64 = value.parse().map_err(|_| invalid())?;
            if count < 1 {
                return Err(invalid());
            }
            Ok((0..count)
                .map(|_| NodeOption::SpoNodeOptions(Vec::new()))
                .collect())
        }
    }
}

/// Parse the testnet's starting on-chain protocol parameters.
///
/// Mirror of upstream `pOnChainParams` (`Parsers/Cardano.hs`):
/// `--params-file FILEPATH` yields `OnChainParamsFile`,
/// `--params-mainnet` yields `OnChainParamsMainnet`, and absent both,
/// `DefaultParams`.
pub fn parse_on_chain_params(args: &[String]) -> Result<TestnetOnChainParams, ParseError> {
    if let Some(path) = flag_with_value(args, "--params-file")? {
        Ok(TestnetOnChainParams::OnChainParamsFile(PathBuf::from(path)))
    } else if args.iter().any(|arg| arg == "--params-mainnet") {
        Ok(TestnetOnChainParams::OnChainParamsMainnet)
    } else {
        Ok(TestnetOnChainParams::DefaultParams)
    }
}

/// Parse the genesis-timestamp policy from a `cardano` / `create-env`
/// argument list.
///
/// Mirror of upstream `pUpdateTimestamps` (`Parsers/Cardano.hs`):
/// `--preserve-timestamps` yields `DontUpdateTimestamps`;
/// `--update-time` or neither flag yields `UpdateTimestamps` (the
/// parser default, kept for backward compatibility — note this
/// differs from the `UpdateTimestamps` type's own `Default`, which
/// is `DontUpdateTimestamps`).
pub fn parse_update_timestamps(args: &[String]) -> UpdateTimestamps {
    if args.iter().any(|arg| arg == "--preserve-timestamps") {
        UpdateTimestamps::DontUpdateTimestamps
    } else {
        UpdateTimestamps::UpdateTimestamps
    }
}

/// Parse a `TestnetEnvOptions` — a pre-existing testnet environment —
/// from a `cardano` argument list.
///
/// Mirror of upstream `pFromEnv` (`Parsers/Cardano.hs`): the required
/// `--node-env FILEPATH` plus the genesis-timestamp policy.
pub fn parse_from_env(args: &[String]) -> Result<TestnetEnvOptions, ParseError> {
    let env_path =
        flag_with_value(args, "--node-env")?.ok_or_else(|| ParseError::MissingRequiredFlag {
            flag: "--node-env".to_string(),
        })?;
    Ok(TestnetEnvOptions {
        env_path: PathBuf::from(env_path),
        env_update_timestamps: parse_update_timestamps(args),
    })
}

/// Parse the `TestnetCreationOptions` from a `cardano` / `create-env`
/// argument list.
///
/// Mirror of upstream `pCreationOptions` (`Parsers/Cardano.hs`):
/// composes the node set, the `--max-lovelace-supply` and
/// `--num-dreps` field flags, the genesis options, and the on-chain
/// params. The era is not a CLI flag — upstream's
/// `pure (AnyShelleyBasedEra defaultEra)` is the `Default`'s
/// `creation_era` (Conway).
pub fn parse_creation_options(args: &[String]) -> Result<TestnetCreationOptions, ParseError> {
    let default = TestnetCreationOptions::default();
    Ok(TestnetCreationOptions {
        creation_nodes: parse_testnet_node_options(args)?,
        creation_era: default.creation_era,
        creation_max_supply: flag_or_default(
            args,
            "--max-lovelace-supply",
            default.creation_max_supply,
        )?,
        creation_num_dreps: NumDReps(flag_or_default(
            args,
            "--num-dreps",
            default.creation_num_dreps.0,
        )?),
        creation_genesis_options: parse_genesis_options(args)?,
        creation_on_chain_params: parse_on_chain_params(args)?,
    })
}

/// Parse the `TestnetRuntimeOptions` flags from a `cardano` /
/// `create-env` argument list.
///
/// Mirror of upstream `pRuntimeOptions` (`Parsers/Cardano.hs`): the
/// `--enable-new-epoch-state-logging` switch, the `--enable-grpc`
/// flag (`RpcEnabled` when present, else `RpcDisabled`), and the
/// `--use-kes-agent` flag (`UseKesSocket` when present, else
/// `UseKesKeyFile`). Each defaults to off when its flag is absent.
pub fn parse_runtime_options(args: &[String]) -> TestnetRuntimeOptions {
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    TestnetRuntimeOptions {
        runtime_enable_new_epoch_state_logging: has("--enable-new-epoch-state-logging"),
        runtime_enable_rpc: if has("--enable-grpc") {
            RpcSupport::RpcEnabled
        } else {
            RpcSupport::RpcDisabled
        },
        runtime_kes_source: if has("--use-kes-agent") {
            PraosCredentialsSource::UseKesSocket
        } else {
            PraosCredentialsSource::UseKesKeyFile
        },
    }
}

/// Parse the full `cardano`-subcommand options.
///
/// Mirror of upstream `optsTestnet` (`Parsers/Cardano.hs`): the
/// presence of `--node-env` selects the start-from-environment mode;
/// otherwise a new environment is created (with an optional
/// `--output-dir`). Either mode carries the runtime options.
pub fn opts_testnet(args: &[String]) -> Result<CardanoTestnetCliOptions, ParseError> {
    let runtime = parse_runtime_options(args);
    if args.iter().any(|arg| arg == "--node-env") {
        Ok(CardanoTestnetCliOptions::StartFromEnv(
            StartFromEnvOptions {
                from_env_options: parse_from_env(args)?,
                from_env_runtime_options: runtime,
            },
        ))
    } else {
        Ok(CardanoTestnetCliOptions::NoUserProvidedEnv(
            NoUserProvidedEnvOptions {
                no_env_creation_options: parse_creation_options(args)?,
                no_env_output_dir: flag_with_value(args, "--output-dir")?.map(PathBuf::from),
                no_env_runtime_options: runtime,
            },
        ))
    }
}

/// Parse the full `create-env`-subcommand options.
///
/// Mirror of upstream `optsCreateTestnet` (`Parsers/Cardano.hs`): the
/// creation options plus the required `--output` directory.
pub fn opts_create_testnet(args: &[String]) -> Result<CardanoTestnetCreateEnvOptions, ParseError> {
    let output =
        flag_with_value(args, "--output")?.ok_or_else(|| ParseError::MissingRequiredFlag {
            flag: "--output".to_string(),
        })?;
    Ok(CardanoTestnetCreateEnvOptions {
        create_env_creation_options: parse_creation_options(args)?,
        create_env_output_dir: PathBuf::from(output),
    })
}

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
    /// A flag was given a value that could not be parsed.
    #[error("invalid value for {flag}: {value}")]
    InvalidFlagValue {
        /// The flag whose value could not be parsed.
        flag: String,
        /// The offending value.
        value: String,
    },
    /// A flag that requires a value was supplied without one.
    #[error("missing value for {flag}")]
    MissingFlagValue {
        /// The flag missing its value.
        flag: String,
    },
    /// A required flag was not supplied.
    #[error("missing required flag {flag}")]
    MissingRequiredFlag {
        /// The required flag that was absent.
        flag: String,
    },
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
    fn testnet_node_options_default_when_no_flag() {
        let nodes = parse_testnet_node_options(&[]).expect("default node set");
        assert_eq!(nodes, cardano_default_testnet_node_options());
    }

    #[test]
    fn testnet_node_options_count_yields_spo_nodes() {
        let args: Vec<String> = ["--num-pool-nodes", "3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let nodes = parse_testnet_node_options(&args).expect("three nodes");
        assert_eq!(nodes.len(), 3);
        assert!(nodes.iter().all(|n| n.is_spo()));
    }

    #[test]
    fn testnet_node_options_reject_zero() {
        let args: Vec<String> = ["--num-pool-nodes", "0"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(matches!(
            parse_testnet_node_options(&args),
            Err(ParseError::InvalidFlagValue { .. })
        ));
    }

    #[test]
    fn on_chain_params_default_file_and_mainnet() {
        assert_eq!(
            parse_on_chain_params(&[]),
            Ok(TestnetOnChainParams::DefaultParams)
        );
        let file_args: Vec<String> = ["--params-file", "/tmp/p.json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_on_chain_params(&file_args),
            Ok(TestnetOnChainParams::OnChainParamsFile(PathBuf::from(
                "/tmp/p.json"
            )))
        );
        let mainnet_args: Vec<String> = ["--params-mainnet".to_string()].to_vec();
        assert_eq!(
            parse_on_chain_params(&mainnet_args),
            Ok(TestnetOnChainParams::OnChainParamsMainnet)
        );
    }

    #[test]
    fn update_timestamps_flag_branches() {
        assert_eq!(
            parse_update_timestamps(&[]),
            UpdateTimestamps::UpdateTimestamps
        );
        let preserve: Vec<String> = ["--preserve-timestamps".to_string()].to_vec();
        assert_eq!(
            parse_update_timestamps(&preserve),
            UpdateTimestamps::DontUpdateTimestamps
        );
        let update: Vec<String> = ["--update-time".to_string()].to_vec();
        assert_eq!(
            parse_update_timestamps(&update),
            UpdateTimestamps::UpdateTimestamps
        );
    }

    #[test]
    fn from_env_requires_node_env() {
        assert_eq!(
            parse_from_env(&[]),
            Err(ParseError::MissingRequiredFlag {
                flag: "--node-env".to_string(),
            })
        );
        let args: Vec<String> = ["--node-env", "/tmp/env", "--preserve-timestamps"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let env = parse_from_env(&args).expect("node-env supplied");
        assert_eq!(env.env_path, PathBuf::from("/tmp/env"));
        assert_eq!(
            env.env_update_timestamps,
            UpdateTimestamps::DontUpdateTimestamps
        );
    }

    #[test]
    fn opts_testnet_new_env_mode_by_default() {
        let opts = opts_testnet(&[]).expect("default new-env");
        match opts {
            CardanoTestnetCliOptions::NoUserProvidedEnv(o) => {
                assert_eq!(o.no_env_creation_options, TestnetCreationOptions::default());
                assert_eq!(o.no_env_output_dir, None);
            }
            CardanoTestnetCliOptions::StartFromEnv(_) => panic!("expected new-env mode"),
        }
    }

    #[test]
    fn opts_testnet_from_env_mode_with_node_env() {
        let args: Vec<String> = ["--node-env", "/tmp/env"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let opts = opts_testnet(&args).expect("from-env");
        match opts {
            CardanoTestnetCliOptions::StartFromEnv(o) => {
                assert_eq!(o.from_env_options.env_path, PathBuf::from("/tmp/env"));
            }
            CardanoTestnetCliOptions::NoUserProvidedEnv(_) => {
                panic!("expected from-env mode")
            }
        }
    }

    #[test]
    fn opts_create_testnet_requires_output() {
        assert_eq!(
            opts_create_testnet(&[]),
            Err(ParseError::MissingRequiredFlag {
                flag: "--output".to_string(),
            })
        );
        let args: Vec<String> = ["--output", "/tmp/sandbox"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let opts = opts_create_testnet(&args).expect("output supplied");
        assert_eq!(opts.create_env_output_dir, PathBuf::from("/tmp/sandbox"));
    }

    #[test]
    fn creation_options_default_when_no_flags() {
        assert_eq!(
            parse_creation_options(&[]),
            Ok(TestnetCreationOptions::default())
        );
    }

    #[test]
    fn creation_options_compose_each_flag() {
        let args: Vec<String> = [
            "--num-pool-nodes",
            "2",
            "--max-lovelace-supply",
            "42000000",
            "--num-dreps",
            "5",
            "--params-mainnet",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let opts = parse_creation_options(&args).expect("valid creation flags");
        assert_eq!(opts.creation_nodes.len(), 2);
        assert_eq!(opts.creation_max_supply, 42_000_000);
        assert_eq!(opts.creation_num_dreps, NumDReps(5));
        assert_eq!(
            opts.creation_on_chain_params,
            TestnetOnChainParams::OnChainParamsMainnet
        );
        // The era is fixed (not a CLI flag) — the default Conway.
        assert_eq!(
            opts.creation_era,
            TestnetCreationOptions::default().creation_era
        );
    }

    #[test]
    fn runtime_options_default_when_no_flags() {
        let opts = parse_runtime_options(&[]);
        assert!(!opts.runtime_enable_new_epoch_state_logging);
        assert_eq!(opts.runtime_enable_rpc, RpcSupport::RpcDisabled);
        assert_eq!(
            opts.runtime_kes_source,
            PraosCredentialsSource::UseKesKeyFile
        );
    }

    #[test]
    fn genesis_options_default_when_no_flags() {
        assert_eq!(parse_genesis_options(&[]), Ok(GenesisOptions::default()));
    }

    #[test]
    fn genesis_options_parse_each_flag() {
        let args: Vec<String> = [
            "--testnet-magic",
            "7",
            "--epoch-length",
            "600",
            "--slot-length",
            "0.2",
            "--active-slots-coeff",
            "0.1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let opts = parse_genesis_options(&args).expect("valid flags");
        assert_eq!(opts.genesis_testnet_magic, 7);
        assert_eq!(opts.genesis_epoch_length, 600);
        assert_eq!(opts.genesis_slot_length, 0.2);
        assert_eq!(opts.genesis_active_slots_coeff, 0.1);
    }

    #[test]
    fn genesis_options_reject_a_bad_value() {
        let args: Vec<String> = ["--testnet-magic", "not-a-number"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_genesis_options(&args),
            Err(ParseError::InvalidFlagValue {
                flag: "--testnet-magic".to_string(),
                value: "not-a-number".to_string(),
            })
        );
    }

    #[test]
    fn genesis_options_reject_a_flag_without_a_value() {
        let args: Vec<String> = ["--epoch-length"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            parse_genesis_options(&args),
            Err(ParseError::MissingFlagValue {
                flag: "--epoch-length".to_string(),
            })
        );
    }

    #[test]
    fn runtime_options_pick_up_each_flag() {
        let args: Vec<String> = [
            "--enable-new-epoch-state-logging",
            "--enable-grpc",
            "--use-kes-agent",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let opts = parse_runtime_options(&args);
        assert!(opts.runtime_enable_new_epoch_state_logging);
        assert_eq!(opts.runtime_enable_rpc, RpcSupport::RpcEnabled);
        assert_eq!(
            opts.runtime_kes_source,
            PraosCredentialsSource::UseKesSocket
        );
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
