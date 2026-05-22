#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `cardano-testnet`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `cardano-testnet` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R359 ships types.rs covering simple option types;
//! later rounds populate the deeper era-aware records):
//!
//! | Upstream `.hs`                                       | Yggdrasil `.rs`              |
//! |------------------------------------------------------|------------------------------|
//! | `Testnet/Start/Types.hs` (simple option types)       | `types.rs`                   |
//! | `Testnet/Types.hs` (runtime/key types)               | `runtime_types.rs` (pending) |
//! | `Testnet/Start/{Byron,Cardano}.hs` (era startup)     | `start/*.rs` (pending)       |
//! | `Testnet/Components/{Query,Configuration}.hs`        | `components/*.rs` (pending)  |
//! | `Testnet/Process/Cli/*.hs` (SPO/Tx/Keys/DRep dispatch) | `process/cli/*.rs` (pending) |
//! | `Testnet/Property/*.hs`                              | **CARVE-OUT** (Hedgehog → proptest synthesis) |
//! | `Testnet/Process/{Run,RunIO}.hs`                     | **CARVE-OUT** (Hedgehog → tokio::process synthesis) |

use std::io::Write;
use std::process::ExitCode;

pub mod components;
pub mod defaults;
pub mod filepath;
pub mod parser;
pub mod paths;
pub mod runtime_types;
pub mod status;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R367 wires the typed parser dispatcher end-to-end. On successful
/// parse the resolved [`parser::Command`] is handed to [`run`];
/// `--help` / `--version` short-circuit with byte-equivalent
/// upstream output.
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
/// R367 lands argv → [`parser::Command`] subcommand dispatch. The
/// per-subcommand era-aware option records + Process/Property
/// modules + multi-node testnet runtime land in subsequent rounds
/// (gated on yggdrasil-ledger's era surface being exposed at crate
/// boundaries; Process/Property carve-out per the plan).
pub fn run(command: &parser::Command) -> eyre::Result<()> {
    let subcommand = match command {
        parser::Command::Cardano(_) => status::Subcommand::Cardano,
        parser::Command::CreateEnv(_) => status::Subcommand::CreateEnv,
        parser::Command::Version(_) => status::Subcommand::Version,
    };
    Err(RunError::SubcommandEraDispatchDeferred { subcommand }.into())
}

/// Errors from the cardano-testnet `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Per-subcommand era-aware dispatch is deferred. Mirror of
    /// upstream `cardano-testnet/src/Testnet/{Defaults, Runtime,
    /// Start, Components, Process}.hs` — gated on yggdrasil-
    /// ledger's era surface being exposed at crate boundaries.
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
