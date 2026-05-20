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
/// `NtoM`; full transaction construction and submission execution
/// still land in later strict slices.
pub fn run(command: command::Command) -> eyre::Result<()> {
    match &command {
        Command::Json(file) => {
            let script = parse_script_file_aeson(file)?;
            let mut env = Env::empty_env();
            run_script(&mut env, &script)?;
            return Ok(());
        }
        Command::JsonHighLevel(cmd) => {
            let raw = std::fs::read_to_string(&cmd.config_file)?;
            let opts = if let Some(testnet_config) = &cmd.testnet_config {
                let user_config = serde_json::from_str(&raw)?;
                discover_testnet_config(testnet_config, user_config)?
            } else {
                parse_nix_service_options_str(&raw)?
            };
            let opts = mangle_node_config(opts, cmd.node_config.clone())?;
            let _final_opts = mangle_tracer_config(opts, cmd.cardano_tracer.clone());
            let _script = compile_options(&_final_opts)?;
        }
        Command::Compile(file) => {
            let raw = std::fs::read_to_string(file)?;
            let opts = parse_nix_service_options_str(&raw)?;
            let script = compile_options(&opts)?;
            let rendered = pretty_print(&script)?;
            std::io::stdout().write_all(rendered.as_bytes())?;
            return Ok(());
        }
        Command::Selftest(_) | Command::Version => {}
    }

    Err(eyre::eyre!(
        "yggdrasil-tx-generator: `{}` command execution not yet implemented \
         (R540 Script/Core NtC query slice). Help/version compatibility, typed \
         subcommand parsing, json_highlevel testnet discovery, and high-level \
         NixServiceOptions parsing/compilation plus low-level script JSON \
         decoding plus deterministic state-only action execution and Script/Core \
         NtC query helpers and sized-metadata construction are wired; full transaction generation \
         and submission land in later strict slices of the tx-generator port arc.",
        command.name()
    ))
}
