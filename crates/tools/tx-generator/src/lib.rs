//! Pure-Rust port of upstream `tx-generator`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell for the
//! `tx-generator` sister-tool crate. Per-leaf modules carry upstream
//! mirrors for the command parser and later strict slices.

use std::io::Write;
use std::process::ExitCode;

use command::Command;
use compiler::compile_options;
use script::aeson::{parse_script_file_aeson, pretty_print};
use script::env::Env;
use script::run_script;
use setup::nix_service::{mangle_node_config, mangle_tracer_config, parse_nix_service_options_str};
use setup::testnet_discovery::discover_testnet_config;
use tx_generator::plutus_context::read_script_data;
use types::TxGenPlutusParams;

pub mod command;
pub mod compiler;
pub mod generator_tx;
pub mod parser;
pub mod script;
pub mod setup;
pub mod tx_generator;
pub mod types;
pub mod wallet;

/// Process-exit-code wrapper around the run-loop dispatch.
pub fn run_main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Idempotent: a second call is a no-op.
    let _ = yggdrasil_telemetry::init_subscriber(&yggdrasil_telemetry::TracingConfig::default());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parser::parse_args(&argv) {
        Ok(args) => match run(args.command) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                let _ = writeln!(std::io::stderr(), "Error: {err}");
                ExitCode::FAILURE
            }
        },
        Err(parser::ParseError::HelpRequested) => {
            let _ = std::io::stdout().write_all(parser::HELP_TEXT.as_bytes());
            ExitCode::SUCCESS
        }
        Err(parser::ParseError::VersionRequested) => {
            let _ = std::io::stdout().write_all(parser::VERSION_TEXT.as_bytes());
            ExitCode::SUCCESS
        }
        Err(parser::ParseError::Invalid(err)) => {
            let _ = writeln!(std::io::stderr(), "{err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry.
///
/// R533 wires the upstream-shaped [`command::Command`] parser and
/// dispatch boundary. R534 prepares `json_highlevel --testnet-config-dir`
/// by running the upstream-shaped testnet discovery merge. R535 parses
/// high-level config into `NixServiceOptions` and applies CLI overrides.
/// R536 compiles high-level options to script actions and makes
/// `compile` emit that script. R537 adds the upstream-shaped
/// `Script/Aeson.hs` parser for `json` scripts. R538 starts the
/// `Script/Env.hs` and `Script/Action.hs` runtime boundary for
/// deterministic state-only actions. R540 wires the `Script/Core.hs`
/// node-to-client current-era and protocol-parameter query path. R541
/// adds the `GeneratorTx/SizedMetadata.hs` sizing helper used by
/// `NtoM`; R542/R543 add wallet queues and upstream value-splitting
/// preflight; R544-R547 add UTxO output builders and static Plutus
/// context loading. R548/R549 add key-spend transaction construction
/// and finite submitInEra execution. R550 wires `json_highlevel`
/// command execution through the compiled script runner. R551 wires
/// `StartProtocol` config-derived env state so high-level execution
/// advances beyond the protocol/bootstrap action. R553 wires the
/// `selftest` command for the upstream no-output-file path.
pub fn run(command: command::Command) -> eyre::Result<()> {
    match command {
        Command::Json(file) => {
            let script = parse_script_file_aeson(&file)?;
            let mut env = Env::empty_env();
            run_script(&mut env, &script)?;
            Ok(())
        }
        Command::JsonHighLevel(cmd) => {
            let raw = std::fs::read_to_string(&cmd.config_file)?;
            let opts = if let Some(testnet_config) = &cmd.testnet_config {
                let user_config = serde_json::from_str(&raw)?;
                discover_testnet_config(testnet_config, user_config)?
            } else {
                parse_nix_service_options_str(&raw)?
            };
            let initial_opts = opts.clone();
            let opts = mangle_node_config(opts, cmd.node_config.clone())?;
            let final_opts = mangle_tracer_config(opts, cmd.cardano_tracer.clone());
            println!("--> initial options:\n{initial_opts:?}\n--> final options:\n{final_opts:?}");
            quick_test_plutus_data_or_die(&final_opts)?;
            let script = compile_options(&final_opts)?;
            let mut env = Env::empty_env();
            run_script(&mut env, &script)?;
            Ok(())
        }
        Command::Compile(file) => {
            let raw = std::fs::read_to_string(&file)?;
            let opts = parse_nix_service_options_str(&raw)?;
            let script = compile_options(&opts)?;
            let rendered = pretty_print(&script)?;
            std::io::stdout().write_all(rendered.as_bytes())?;
            Ok(())
        }
        Command::Version => {
            std::io::stdout().write_all(parser::VERSION_TEXT.as_bytes())?;
            Ok(())
        }
        Command::Selftest(out_file) => {
            script::selftest::run_selftest(out_file.as_deref())?;
            Ok(())
        }
    }
}

/// Mirror of upstream `quickTestPlutusDataOrDie`.
fn quick_test_plutus_data_or_die(opts: &setup::nix_service::NixServiceOptions) -> eyre::Result<()> {
    let Some(TxGenPlutusParams::PlutusOn {
        plutus_datum,
        plutus_redeemer,
        ..
    }) = opts.nix_plutus.as_ref()
    else {
        println!("--> success: quickTestPlutusDataOrDie []");
        return Ok(());
    };

    let files = [plutus_datum.as_ref(), plutus_redeemer.as_ref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    for file in &files {
        read_script_data(file)
            .map_err(|err| eyre::eyre!("quickTestPlutusDataOrDie ({}): {err}", file.display()))?;
    }
    println!("--> success: quickTestPlutusDataOrDie {files:?}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::JsonHighLevelCommand;
    use crate::setup::nix_service::parse_nix_service_options_value;
    use crate::types::{PlutusScriptRef, TxGenPlutusType};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-{name}-{}-{nanos}",
            std::process::id(),
        ))
    }

    fn write(path: &Path, content: &str) {
        std::fs::write(path, content).expect("write temp file");
    }

    fn write_node_config(path: &Path) {
        let mut config = yggdrasil_node_config::default_config();
        config.network_magic = 42;
        config.protocol = Some("Cardano".to_string());
        config.shelley_genesis_file = None;
        config.shelley_genesis_hash = None;
        write(
            path,
            &serde_json::to_string(&config).expect("render node config"),
        );
    }

    fn high_level_config(config_path: &Path, node_config_path: &Path, sig_key_path: &Path) {
        write(
            config_path,
            &serde_json::to_string(&json!({
                "debugMode": true,
                "tx_count": 1,
                "tps": 10.0,
                "inputs_per_tx": 1,
                "outputs_per_tx": 1,
                "tx_fee": 212345,
                "min_utxo_value": 1000000,
                "add_tx_size": 0,
                "init_cooldown": 0.0,
                "era": "Conway",
                "keepalive": 45,
                "localNodeSocketPath": "node.socket",
                "nodeConfigFile": node_config_path,
                "sigKey": sig_key_path,
                "targetNodes": [
                    {"addr": "127.0.0.1", "port": 30000, "name": "node0"}
                ],
                "plutus": null
            }))
            .expect("render config"),
        );
    }

    #[test]
    fn json_highlevel_runs_compiled_script_until_runtime_boundary() {
        let config_path = unique_temp_path("highlevel-config.json");
        let node_config_path = unique_temp_path("node-config.json");
        let missing_sig_key_path = unique_temp_path("missing-genesis-utxo.skey");
        high_level_config(&config_path, &node_config_path, &missing_sig_key_path);
        write_node_config(&node_config_path);

        let err = run(Command::JsonHighLevel(JsonHighLevelCommand {
            config_file: config_path.clone(),
            testnet_config: None,
            node_config: None,
            cardano_tracer: None,
        }))
        .expect_err("missing genesis signing key is reached after StartProtocol");

        assert!(err.to_string().contains("action #10 failed"));
        assert!(err.to_string().contains("readSigningKeyFile"));
        assert!(!err.to_string().contains("startProtocol"));

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(node_config_path);
    }

    #[test]
    fn quick_test_plutus_data_reports_bad_datum_before_compile_run() {
        let datum_path = unique_temp_path("bad-datum.json");
        write(&datum_path, "{\"notDetailedSchema\": true}");
        let mut opts = parse_nix_service_options_value(json!({
            "debugMode": true,
            "tx_count": 1,
            "tps": 10.0,
            "inputs_per_tx": 1,
            "outputs_per_tx": 1,
            "tx_fee": 212345,
            "min_utxo_value": 1000000,
            "add_tx_size": 0,
            "init_cooldown": 0.0,
            "era": "Conway",
            "keepalive": 45,
            "localNodeSocketPath": "node.socket",
            "nodeConfigFile": "config.json",
            "sigKey": "genesis-utxo.skey",
            "targetNodes": [
                {"addr": "127.0.0.1", "port": 30000, "name": "node0"}
            ],
            "plutus": null
        }))
        .expect("config parses");
        opts.nix_plutus = Some(TxGenPlutusParams::PlutusOn {
            plutus_type: TxGenPlutusType::CustomScript,
            plutus_script: PlutusScriptRef::Named("Loop".to_string()),
            plutus_datum: Some(datum_path.clone()),
            plutus_redeemer: None,
            plutus_exec_memory: Some(1),
            plutus_exec_steps: Some(1),
        });

        let err = quick_test_plutus_data_or_die(&opts).expect_err("datum preflight fails");

        assert!(err.to_string().contains("quickTestPlutusDataOrDie"));
        assert!(err.to_string().contains("expected one of"));

        let _ = std::fs::remove_file(datum_path);
    }

    #[test]
    fn selftest_command_dispatches_to_static_script() {
        let output_path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-command-{}.out",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&output_path);

        run(Command::Selftest(Some(output_path.clone()))).expect("selftest command");

        let rendered = std::fs::read_to_string(&output_path).expect("selftest dump");
        let _ = std::fs::remove_file(&output_path);
        assert!(rendered.starts_with("\nShelleyTx ShelleyBasedEraAllegra"));
        assert_eq!(
            rendered.lines().filter(|line| !line.is_empty()).count(),
            4_000
        );
    }
}
