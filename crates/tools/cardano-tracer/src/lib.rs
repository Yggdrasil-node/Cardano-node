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
//! Layout mapping (R380 ships severity.rs + handlers/notifications/types.rs; later rounds populate the rest):
//!
//! | Upstream `.hs`                                       | Yggdrasil `.rs`                          |
//! |------------------------------------------------------|------------------------------------------|
//! | `Cardano/Tracer/Configuration.hs`                    | `configuration.rs`                       |
//! | `Cardano/Tracer/Types.hs`                            | `types.rs`                               |
//! | `Cardano/Tracer/Time.hs`                             | `time.rs`                                |
//! | `Cardano.Logging.SeverityS` (synthesis)              | `severity.rs`                            |
//! | `Cardano/Tracer/CLI.hs`                              | `cli.rs` (pending)                       |
//! | `Cardano/Tracer/Run.hs`                              | `run.rs` (pending)                       |
//! | `Cardano/Tracer/Acceptors/*`                         | `acceptors/*.rs` (pending)               |
//! | `Cardano/Tracer/Handlers/Logs/*`                     | `handlers/logs/*.rs` (pending)           |
//! | `Cardano/Tracer/Handlers/RTView/*`                   | **CARVE-OUT** (synthesis)                |
//! | `Cardano/Tracer/Handlers/Notifications/Types.hs`     | `handlers/notifications/types.rs`        |
//! | `Cardano/Tracer/Handlers/Notifications/{Check,Send,Email,Settings,Timer,Utils}.hs` | `handlers/notifications/*.rs` (pending) |
//! | `Cardano/Tracer/Handlers/Metrics/*`                  | `handlers/metrics/*.rs` (pending)        |

use std::io::Write;
use std::process::ExitCode;

pub mod acceptors;
pub mod configuration;
pub mod environment;
pub mod handlers;
pub mod logging;
pub mod meta_trace;
pub mod metrics_store;
pub mod parser;
pub mod run;
pub mod severity;
pub mod time;
pub mod types;
pub mod utils;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R366 wires the typed parser dispatcher end-to-end. On successful
/// parse the resolved [`parser::Args`] is handed to [`run`]; `--help`
/// and `--version` short-circuit with byte-equivalent upstream output.
pub fn run_main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Idempotent: a second call is a no-op.
    let _ = yggdrasil_telemetry::init_subscriber(&yggdrasil_telemetry::TracingConfig::default());
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
/// Wires argv → [`parser::Args`] → [`run::TracerParams`] →
/// [`run::run_cardano_tracer`]. The trace-objects handler is the
/// canonical [`crate::handlers::logs::trace_objects::trace_objects_handler`]
/// implementation. Earlier rounds shipped a stub returning an
/// "unimplemented" error; R427 replaces that with the real
/// supervisor entry.
pub fn run(args: &parser::Args) -> eyre::Result<()> {
    let params = run::TracerParams {
        tracer_config: args.tracer_config.clone(),
        state_dir: args.state_dir.clone(),
        log_severity: args.log_severity,
    };

    // Build a multi-thread tokio runtime for the supervisor.
    // The default worker count tracks the number of CPU cores,
    // matching upstream's GHC RTS `-N` default.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| eyre::eyre!("failed to build tokio runtime: {e}"))?;

    // R431 wires `run_cardano_tracer_default` which builds the
    // canonical trace-objects handler via
    // `run::default_lo_handler_factory` — the closure dispatches
    // each batch to `handlers::logs::trace_objects::trace_objects_handler`
    // (R401), routing per the operator's `LoggingParams` configuration.
    // Operators wanting custom handlers can call
    // `run::run_cardano_tracer` directly with their own closure.
    rt.block_on(async move { run::run_cardano_tracer_default(params).await })
        .map_err(|e| eyre::eyre!("cardano-tracer supervisor: {e}"))?;
    Ok(())
}
