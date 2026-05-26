#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `cardano-testnet`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell for the
//! `cardano-testnet` sister-tool crate plus local Rust surfaces mirroring the
//! era-free Testnet records, path/default helpers, and parser composition.
//!
//! Layout mapping (post-R839):
//!
//! | Upstream `.hs`                                       | Yggdrasil `.rs`              |
//! |------------------------------------------------------|------------------------------|
//! | `Testnet/Start/Types.hs` (option records)            | `types.rs`                   |
//! | `Testnet/Types.hs` (runtime/key/process records)     | `runtime_types.rs`           |
//! | `Cardano.Node.Testnet.Paths`                         | `paths.rs`                   |
//! | `Testnet/Filepath.hs`                                | `filepath.rs`                |
//! | `Testnet/Defaults.hs`                                | `defaults.rs`                |
//! | `Testnet/Components/{Query,Configuration}.hs`        | `components/*.rs`            |
//! | `Parsers/{Run,Cardano}.hs`                           | `parser.rs`                  |
//! | `Testnet/Start/{Byron,Cardano}.hs` (era startup)     | `start/*.rs` (pending)       |
//! | `Testnet/Process/Cli/Keys.hs` (key command builders) | `process/cli/keys.rs`        |
//! | `Testnet/Process/Cli/Transaction.hs` (sign/submit/txid + spend-output txbody builders) | `process/cli/transaction.rs` |
//! | `Testnet/Process/Cli/DRep.hs` (key/cert/vote builders) | `process/cli/drep.rs`       |
//! | `Testnet/Process/Cli/SPO.hs` (certificate/vote builders) | `process/cli/spo.rs`        |
//! | `Testnet/Process/Run.hs` (flexible process execution) | `process/run.rs`              |
//! | `Testnet/Process/RunIO.hs` (plan-json process planning + execution helpers) | `process/run_io.rs`          |
//! | `Testnet/Process/Cli/SPO.hs` (registration/check workflows) | `process/cli/spo.rs` (pending runtime layer) |
//! | `Testnet/Process/Cli/Transaction.hs` (UTxO/script-address runtime execution) | `process/cli/transaction.rs` (pending runtime layer) |
//! | `Testnet/Process/Cli/DRep.hs` (runtime workflows)     | `process/cli/drep.rs` (pending runtime layer) |
//! | `Testnet/Property/Assert.hs` (pure + CLI-backed assertion helpers) | `property/assert.rs`         |
//! | `Testnet/Property/Util.hs` (pure harness helpers)     | `property/util.rs`           |
//! | `Testnet/Property/Run.hs` (pure harness-control helpers + testnetProperty planning + runtime message rendering) | `property/run.rs` |

use std::io::Write;
use std::process::ExitCode;

pub mod components;
pub mod defaults;
pub mod filepath;
pub mod parser;
pub mod paths;
pub mod process;
pub mod property;
pub mod runtime_types;
pub mod status;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R367 wires the top-level parser dispatcher. R818-R823 add the
/// Parsers/Cardano option parsers and composition helpers; R825 carries those
/// typed payloads through [`parser::Command`]. R826 adds the
/// `Testnet/Types.hs` runtime record carriers used by future execution, and
/// R827-R839 add pure Process/Cli key, transaction, DRep, and SPO command
/// builders, including spend-output txbody plans, plus Process/Run flexible
/// execution wrappers, Process/RunIO process-planning/execution helpers,
/// Property/Util pure harness helpers, Property/Assert pure/CLI-backed
/// assertion helpers, and Property/Run pure harness-control/planning helpers.
/// `--help` / `--version` short-circuit with byte-equivalent upstream output.
pub fn run_main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Idempotent: a second call is a no-op.
    let _ = yggdrasil_telemetry::init_subscriber(&yggdrasil_telemetry::TracingConfig::default());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let command = match parser::parse_args(&argv) {
        Ok(cmd) => cmd,
        Err(parser::ParseError::HelpRequested) => {
            let _ = std::io::stdout().write_all(parser::HELP_TEXT.as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(parser::ParseError::VersionRequested) => {
            let _ = std::io::stdout().write_all(parser::VERSION_TEXT.as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            return ExitCode::FAILURE;
        }
    };
    match run(&command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry.
///
/// R367 introduced argv → [`parser::Command`] subcommand dispatch. R772-R823 ship
/// the era-free option records and the Parsers/Cardano option-composition
/// helpers. The typed `Command` payloads, version subcommand, and runtime
/// record carriers plus Process/Cli key, transaction, DRep, SPO,
/// Process/Run, Process/RunIO, Property/Util, and Property/Assert helper
/// surfaces are wired, including the stake-pools query assertion wrapper and
/// Property/Run pure harness-control/planning helpers; node/KES spawning,
/// era-genesis, runtime/query workflows, and the remaining Process/Property
/// harness bodies remain deferred.
pub fn run(command: &parser::Command) -> eyre::Result<()> {
    let subcommand = match command {
        parser::Command::Version(_) => {
            std::io::stdout().write_all(parser::VERSION_TEXT.as_bytes())?;
            return Ok(());
        }
        parser::Command::Cardano(_) => status::Subcommand::Cardano,
        parser::Command::CreateEnv(_) => status::Subcommand::CreateEnv,
    };
    Err(RunError::SubcommandEraDispatchDeferred { subcommand }.into())
}

/// Errors from the cardano-testnet `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Per-subcommand era-aware dispatch is deferred. Mirror of
    /// upstream `cardano-testnet/src/Testnet/{Defaults, Runtime,
    /// Start, Components, Process}.hs` and `Parsers/{Run,Cardano}.hs`.
    #[error(
        "yggdrasil-cardano-testnet: `{subcommand}' subcommand era-aware dispatch deferred (see \
         crates/tools/cardano-testnet/src/status.rs::era_dispatch_status for the full deferral \
         rationale)."
    )]
    SubcommandEraDispatchDeferred {
        /// The subcommand the operator invoked.
        subcommand: status::Subcommand,
    },
}

#[cfg(test)]
mod property_util_tests {
    use crate::property::util::{
        aeson_object_lookup, disable_retries_from_env, integration_retry_workspace_names,
        is_linux_os,
    };
    use serde_json::json;

    #[test]
    fn property_util_retry_workspace_names_match_upstream_disable_retries_branch() {
        assert_eq!(
            integration_retry_workspace_names(3, "testnet", false),
            vec!["testnet-0", "testnet-1", "testnet-2"]
        );
        assert_eq!(
            integration_retry_workspace_names(3, "testnet", true),
            vec!["testnet-no-retries"]
        );
        assert!(disable_retries_from_env(
            |name| (name == "DISABLE_RETRIES").then(|| "1".into())
        ));
        assert!(!disable_retries_from_env(|_| None));
    }

    #[test]
    fn property_util_os_and_json_lookup_match_upstream_shape() {
        assert!(is_linux_os("linux"));
        assert!(!is_linux_os("darwin"));
        assert!(!is_linux_os("mingw32"));

        let value = json!({ "slot": 42, "nullish": null });
        assert_eq!(
            aeson_object_lookup(&value, "slot").expect("object lookup"),
            Some(json!(42))
        );
        assert_eq!(
            aeson_object_lookup(&value, "missing").expect("missing key"),
            None
        );

        let err = aeson_object_lookup(&json!(["not", "object"]), "slot")
            .expect_err("non-object is rejected");
        assert!(
            err.to_string()
                .contains("Expected an Aeson Object but got:")
        );
    }
}

#[cfg(test)]
mod property_assert_tests {
    use crate::process::run::{ExecConfig, ProcessRunError};
    use crate::property::assert::{
        assert_by_deadline_custom, assert_eras_equal, assert_expected_spos_in_ledger_state_value,
        assert_expected_spos_in_ledger_state_with_executor, get_relevant_slots_from_values,
        read_json_lines_from_slice, stake_pools_query_args,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    #[test]
    fn property_assert_json_lines_and_relevant_slots_match_upstream_shape() {
        let raw = br#"{"data":{"val":{"kind":"TraceNodeIsLeader","slot":8}}}
not-json
{"data":{"val":{"kind":"TraceNodeNotLeader","slot":10}}}
{"data":{"val":{"kind":"TraceMempool","slot":11}}}
{"data":{"val":{"kind":"TraceNodeIsLeader","slot":12}}}
"#;
        let values = read_json_lines_from_slice(raw);

        assert_eq!(values.len(), 4);
        let slots = get_relevant_slots_from_values(&values, 9);
        assert_eq!(slots.leader_slots, vec![12]);
        assert_eq!(slots.not_leader_slots, vec![10]);
    }

    #[test]
    fn property_assert_deadline_spos_and_era_errors_match_upstream_messages() {
        assert_by_deadline_custom(
            "node ready",
            SystemTime::now() - Duration::from_secs(1),
            || Ok(true),
        )
        .expect("true condition succeeds");
        let deadline = assert_by_deadline_custom(
            "node ready",
            SystemTime::now() - Duration::from_secs(1),
            || Ok(false),
        )
        .expect_err("past deadline fails immediately");
        assert_eq!(
            deadline.to_string(),
            "Condition not met by deadline: node ready"
        );

        assert_expected_spos_in_ledger_state_value(&json!(["pool-a", "pool-b", "pool-a"]), 2)
            .expect("stake pool set semantics dedupe pools");
        let spo_err = assert_expected_spos_in_ledger_state_value(&json!(["pool-a"]), 2)
            .expect_err("wrong SPO count fails");
        assert!(
            spo_err
                .to_string()
                .contains("Expected number of stake pools not found in ledger state")
        );

        assert_eras_equal("Conway", "Conway").expect("same era succeeds");
        let era_err = assert_eras_equal("Conway", "Babbage").expect_err("mismatched era fails");
        assert_eq!(
            era_err.to_string(),
            "Eras mismatch! expected: Conway, received era: Babbage"
        );
    }

    #[test]
    fn property_assert_stake_pool_query_wrapper_matches_upstream_cli_shape() {
        let output_path = std::env::temp_dir().join(format!(
            "yggdrasil-cardano-testnet-r837-stake-pools-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("system time after epoch")
                .as_nanos()
        ));
        let exec_config = ExecConfig {
            env: Some(vec![(
                "CARDANO_NODE_NETWORK_ID".to_string(),
                "42".to_string(),
            )]),
            cwd: Some(PathBuf::from("workdir")),
        };
        let mut captured = None;

        assert_expected_spos_in_ledger_state_with_executor(
            &output_path,
            2,
            &exec_config,
            |config, args| -> Result<String, ProcessRunError> {
                captured = Some((config.clone(), args.to_vec()));
                std::fs::write(&output_path, r#"["pool-a","pool-b","pool-a"]"#)?;
                Ok(String::new())
            },
        )
        .expect("stake-pool wrapper succeeds after CLI writes JSON");

        let (captured_config, captured_args) = captured.expect("executor was called");
        assert_eq!(captured_config, exec_config);
        assert_eq!(captured_args, stake_pools_query_args(&output_path));
        assert_eq!(
            captured_args,
            vec![
                "latest".to_string(),
                "query".to_string(),
                "stake-pools".to_string(),
                "--out-file".to_string(),
                output_path.to_string_lossy().into_owned(),
            ]
        );

        let _ = std::fs::remove_file(output_path);
    }
}

#[cfg(test)]
mod property_run_tests {
    use crate::filepath::Sprocket;
    use crate::property::run::{
        KEEPALIVE_DELAY_MICROS, PropertyDisposition, TESTNET_WORKSPACE_NAME,
        TestnetPropertyWorkspace, UserProvidedEnv, UserProvidedEnvAction, disabled, ignore_on,
        ignore_on_mac, ignore_on_mac_and_windows, ignore_on_windows,
        no_user_provided_env_testnet_property_plan, render_run_testnet_result,
        render_running_testnet_message, user_provided_env_testnet_property_plan,
    };
    use crate::runtime_types::{
        KeyPair, PaymentKeyInfo, SpoNodeKeys, StakeKey, StakePoolKey, TESTNET_DEFAULT_IPV4_ADDRESS,
        TestnetNode, TestnetProcessHandle, TestnetRuntime, TestnetStdinHandle, VrfKey,
    };

    #[test]
    fn property_run_user_env_and_ignore_helpers_match_upstream_shape() {
        assert_eq!(UserProvidedEnv::NoUserProvidedEnv.workspace_hint(), None);
        assert_eq!(
            UserProvidedEnv::UserProvidedEnv("existing-env".into()).workspace_hint(),
            Some(std::path::Path::new("existing-env"))
        );

        assert_eq!(
            ignore_on("Windows", "runs-on-linux").reason,
            "IGNORED on Windows"
        );
        assert_eq!(disabled("manual-only").reason, "IGNORED on Disabled");

        assert!(matches!(
            ignore_on_windows("portable-test", false),
            PropertyDisposition::Run { .. }
        ));
        assert_eq!(
            ignore_on_windows("portable-test", true).ignored_reason(),
            Some("IGNORED on Windows")
        );
        assert_eq!(
            ignore_on_mac("portable-test", "darwin").ignored_reason(),
            Some("IGNORED on MacOS")
        );
        assert_eq!(
            ignore_on_mac_and_windows("portable-test", "mingw32").ignored_reason(),
            Some("IGNORED on MacOS and Windows")
        );
        assert!(matches!(
            ignore_on_mac_and_windows("portable-test", "linux"),
            PropertyDisposition::Run { .. }
        ));
    }

    #[test]
    fn property_run_running_message_matches_upstream_spo_operator_guidance() {
        let runtime = test_runtime(vec![test_node("node-spo1", Some(test_pool_keys()))]);
        let message = render_running_testnet_message(&runtime);

        assert!(message.contains("Please disregard the message above implying a failure."));
        assert!(message.contains("Testnet is running with config file configuration.json"));
        assert!(message.contains("Logs of the SPO node can be found at node-spo1.stdout.log"));
        assert!(
            message
                .contains("export CARDANO_NODE_SOCKET_PATH=/tmp/ygg-testnet/run/socket/node-spo1")
        );
        assert!(message.contains("export CARDANO_NODE_NETWORK_ID=42"));
        assert!(message.ends_with("Type CTRL-C to exit.\n"));
    }

    #[test]
    fn property_run_running_message_matches_upstream_missing_spo_branch() {
        let runtime = test_runtime(vec![test_node("node-relay1", None)]);
        let message = render_running_testnet_message(&runtime);

        assert!(message.contains("Testnet is running with config file configuration.json"));
        assert!(message.contains("Failed to find any SPO node in the testnet"));
        assert!(!message.contains("CARDANO_NODE_SOCKET_PATH"));
        assert!(message.ends_with("Type CTRL-C to exit.\n"));
    }

    #[test]
    fn property_run_testnet_property_plan_matches_upstream_workspace_branches() {
        let no_env = no_user_provided_env_testnet_property_plan();
        assert_eq!(
            no_env.workspace,
            TestnetPropertyWorkspace::IntegrationWorkspace {
                workspace_name: TESTNET_WORKSPACE_NAME.to_string(),
            }
        );
        assert_eq!(no_env.keepalive_delay_micros, KEEPALIVE_DELAY_MICROS);
        assert!(no_env.intentional_failure_after_run);
        assert_eq!(no_env.note(), None);

        let output_dir = std::path::PathBuf::from("C:/tmp/yggdrasil-testnet-env");
        let reuse = user_provided_env_testnet_property_plan(output_dir.clone(), true);
        assert_eq!(
            reuse.workspace,
            TestnetPropertyWorkspace::UserProvided {
                output_dir: output_dir.clone(),
                action: UserProvidedEnvAction::ReuseExisting,
            }
        );
        assert_eq!(
            reuse.note(),
            Some(format!("Reusing {}", output_dir.display()))
        );

        let create = user_provided_env_testnet_property_plan(output_dir.clone(), false);
        assert_eq!(
            create.workspace,
            TestnetPropertyWorkspace::UserProvided {
                output_dir: output_dir.clone(),
                action: UserProvidedEnvAction::CreateDirectory,
            }
        );
        assert_eq!(
            create.note(),
            Some(format!("Created {}", output_dir.display()))
        );
    }

    #[test]
    fn property_run_result_renderer_matches_failed_start_branch() {
        assert_eq!(
            render_run_testnet_result(None),
            "Failed to start testnet.\n"
        );

        let runtime = test_runtime(vec![test_node("node-spo1", Some(test_pool_keys()))]);
        assert_eq!(
            render_run_testnet_result(Some(&runtime)),
            render_running_testnet_message(&runtime)
        );
    }

    fn test_runtime(nodes: Vec<TestnetNode>) -> TestnetRuntime {
        TestnetRuntime {
            configuration_file: "configuration.json".into(),
            shelley_genesis_file: "shelley-genesis.json".into(),
            testnet_magic: 42,
            testnet_nodes: nodes,
            wallets: vec![PaymentKeyInfo {
                payment_key_info_pair: KeyPair::new("pay.vkey", "pay.skey"),
                payment_key_info_addr: "addr_test1abc".to_string(),
            }],
            delegators: Vec::new(),
        }
    }

    fn test_node(name: &str, pool_keys: Option<SpoNodeKeys>) -> TestnetNode {
        TestnetNode {
            node_name: name.to_string(),
            pool_keys,
            node_ipv4: TESTNET_DEFAULT_IPV4_ADDRESS,
            node_port: 30_000,
            node_sprocket: Sprocket {
                base: "/tmp/ygg-testnet/".to_string(),
                name: format!("run/socket/{name}"),
            },
            node_stdin_handle: TestnetStdinHandle::placeholder(),
            node_stdout: format!("{name}.stdout.log").into(),
            node_stderr: format!("{name}.stderr.log").into(),
            node_process_handle: TestnetProcessHandle::placeholder(),
        }
    }

    fn test_pool_keys() -> SpoNodeKeys {
        SpoNodeKeys {
            pool_node_keys_cold: KeyPair::<StakePoolKey>::new("cold.vkey", "cold.skey"),
            pool_node_keys_vrf: KeyPair::<VrfKey>::new("vrf.vkey", "vrf.skey"),
            pool_node_keys_staking: KeyPair::<StakeKey>::new("stake.vkey", "stake.skey"),
        }
    }
}
