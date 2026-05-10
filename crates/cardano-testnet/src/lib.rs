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

pub mod parser;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R367 wires the typed parser dispatcher end-to-end. On successful
/// parse the resolved [`parser::Command`] is handed to [`run`];
/// `--help` / `--version` short-circuit with byte-equivalent
/// upstream output.
pub fn run_main() -> ExitCode {
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
    use parser::Command;
    let subcommand_name = match command {
        Command::Cardano(_) => "cardano",
        Command::CreateEnv(_) => "create-env",
        Command::Version(_) => "version",
    };
    Err(eyre::eyre!(
        "yggdrasil-cardano-testnet: `{subcommand_name}' subcommand era-aware \
         dispatch not yet implemented (R367 ships argv → Command subcommand \
         recognition; per-subcommand era-aware option records land in \
         subsequent rounds gated on yggdrasil-ledger's era surface)."
    ))
}
