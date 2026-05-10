#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `cardano-tracer`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `cardano-tracer` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R358 ships configuration.rs; later rounds populate the rest):
//!
//! | Upstream `.hs`                                       | Yggdrasil `.rs`              |
//! |------------------------------------------------------|------------------------------|
//! | `Cardano/Tracer/Configuration.hs`                    | `configuration.rs`           |
//! | `Cardano/Tracer/Types.hs`                            | `types.rs` (pending)         |
//! | `Cardano/Tracer/CLI.hs`                              | `cli.rs` (pending)           |
//! | `Cardano/Tracer/Run.hs`                              | `run.rs` (pending)           |
//! | `Cardano/Tracer/Acceptors/*`                         | `acceptors/*.rs` (pending)   |
//! | `Cardano/Tracer/Handlers/Logs/*`                     | `handlers/logs/*.rs` (pending)|
//! | `Cardano/Tracer/Handlers/RTView/*`                   | **CARVE-OUT** (synthesis)    |
//! | `Cardano/Tracer/Handlers/Notifications/*`            | `handlers/notifications/*.rs` (pending) |
//! | `Cardano/Tracer/Handlers/Metrics/*`                  | `handlers/metrics/*.rs` (pending) |

use std::io::Write;
use std::process::ExitCode;

pub mod configuration;
pub mod parser;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R366 wires the typed parser dispatcher end-to-end. On successful
/// parse the resolved [`parser::Args`] is handed to [`run`]; `--help`
/// and `--version` short-circuit with byte-equivalent upstream output.
pub fn run_main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parser::parse_args(&argv) {
        Ok(args) => args,
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
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry.
///
/// R366 lands argv → [`parser::Args`] dispatch. The actual config-file
/// load + Acceptors/Handlers/Logs/Metrics wiring lands in subsequent
/// rounds per the per-tool roadmap.
pub fn run(args: &parser::Args) -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-cardano-tracer: config-file load + Acceptors/Handlers \
         not yet implemented (R366 ships argv → Args dispatch; later \
         rounds wire the trace-forwarder mini-protocol acceptor + log \
         writers + metrics endpoints + notifications dispatcher). \
         Resolved: config={}, state-dir={}, min-log-severity={:?}.",
        args.tracer_config.display(),
        args.state_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        args.log_severity,
    ))
}
