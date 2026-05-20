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
use setup::testnet_discovery::discover_testnet_config;

pub mod command;
pub mod parser;
pub mod setup;

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
/// by running the upstream-shaped testnet discovery merge. Individual
/// command execution still lands in the later `Script`, `Compiler`, and
/// `GeneratorTx` slices.
pub fn run(command: command::Command) -> eyre::Result<()> {
    if let Command::JsonHighLevel(cmd) = &command
        && let Some(testnet_config) = &cmd.testnet_config
    {
        let raw = std::fs::read_to_string(&cmd.config_file)?;
        let user_config = serde_json::from_str(&raw)?;
        let _merged_config = discover_testnet_config(testnet_config, user_config)?;
    }

    Err(eyre::eyre!(
        "yggdrasil-tx-generator: `{}` command execution not yet implemented \
         (R534 setup discovery slice). Help/version compatibility, typed \
         subcommand parsing, and json_highlevel testnet discovery are wired; \
         concrete command implementations land in later strict slices of the \
         tx-generator port arc.",
        command.name()
    ))
}
