//! Pure-Rust port of upstream `tx-generator`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell for the
//! `tx-generator` sister-tool crate. Per-leaf modules carry upstream
//! mirrors for the command parser and later strict slices.

use std::io::Write;
use std::process::ExitCode;

pub mod command;
pub mod parser;

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
/// dispatch boundary. Individual command execution still lands in the
/// later `Script`, `Compiler`, `Setup`, and `GeneratorTx` slices.
pub fn run(command: command::Command) -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-tx-generator: `{}` command execution not yet implemented \
         (R533 command parser slice). Help/version compatibility and typed \
         subcommand parsing are wired; concrete command implementations land \
         in later strict slices of the tx-generator port arc.",
        command.name()
    ))
}
