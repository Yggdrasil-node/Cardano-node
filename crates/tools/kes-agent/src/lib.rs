//! Pure-Rust port of upstream `kes-agent`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `kes-agent` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
pub mod status;

/// Process-exit-code wrapper around the run-loop dispatch.
pub fn run_main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Idempotent: a second call is a no-op.
    let _ = yggdrasil_telemetry::init_subscriber(&yggdrasil_telemetry::TracingConfig::default());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parser::parse_args(&argv) {
        Ok(_args) => match run() {
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
    }
}

/// Concrete run-loop entry. R335-pattern skeleton with R443
/// structured deferral. The CLI parser surface (--help /
/// --version) IS functional and byte-equivalent to upstream.
pub fn run() -> eyre::Result<()> {
    Err(RunError::DaemonDispatchDeferred.into())
}

/// Errors from the kes-agent `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Daemon dispatch is deferred. Mirror of upstream's
    /// `Cardano.KESAgent.Processes.{ServiceMain, ServiceClient, RunCommands}`
    /// — gated on the named kes-agent mini-arc per the
    /// playful-tickling-plum.md plan (R344-R354).
    #[error(
        "yggdrasil-kes-agent: daemon dispatch deferred (see crates/tools/kes-agent/src/status.rs::\
         daemon_status for the full deferral rationale). Help/version output IS \
         byte-equivalent to upstream."
    )]
    DaemonDispatchDeferred,
}
