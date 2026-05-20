//! Pure-Rust port of upstream `Cardano.TxSubmit` — HTTP transaction-
//! submission web API for Cardano nodes.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit.hs.
//! The Rust crate root (`lib.rs`) is the canonical 1:1 mirror of
//! upstream `Cardano.TxSubmit.hs` — the public library API surface
//! that wires together the CLI, REST, tracing, web, and metrics
//! modules.
//!
//! Upstream layout → Yggdrasil mapping:
//!
//! | Upstream `.hs`                                          | Yggdrasil `.rs`                     |
//! |---------------------------------------------------------|-------------------------------------|
//! | `Cardano/TxSubmit.hs`                                   | `lib.rs`                            |
//! | `Cardano/TxSubmit/CLI/{Types,Parsers}.hs`               | `cli/{types,parsers}.rs`            |
//! | `Cardano/TxSubmit/Rest/{Types,Parsers,Web}.hs`          | `rest/{types,parsers,web}.rs`       |
//! | `Cardano/TxSubmit/Tracing/TraceSubmitApi.hs`            | `tracing/trace_submit_api.rs`       |
//! | `Cardano/TxSubmit/{Types,Util,Orphans,Metrics,Web}.hs`  | `{types,util,orphans,metrics,web}.rs` |
//! | `app/Main.hs`                                           | `main.rs`                           |
//!
//! Yggdrasil's binary is named `cardano-submit-api` (matching upstream
//! exactly) for drop-in deployment via `scripts/run-tools.sh`.

use std::io::Write;
use std::process::ExitCode;

pub mod cli;
pub mod metrics;
pub mod orphans;
pub mod parser;
pub mod rest;
pub mod tracing;
pub mod types;
pub mod util;
pub mod web;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// Used by `main.rs` to translate CLI parse outcomes (HelpRequested
/// / VersionRequested / unknown-flag / good-args) into the right
/// exit code + stdout/stderr output.
pub fn run_main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Operators piping stdout into Promtail / fluentd
    // get a uniform field schema across the whole Yggdrasil binary
    // surface. The init is idempotent — a second call is a no-op.
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
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry.
///
/// R343 wires the tokio runtime + real LocalTxSubmission integration.
/// argv → [`cli::types::TxSubmitCommand`] validation runs first; on
/// `TxSubmitRun(params)` the runtime spins
/// [`web::run_tx_submit_server_from_params`] which binds the HTTP
/// listener, routes `POST /api/submit/tx` to a NtC LocalTxSubmission
/// client, and runs until the listener exits or the operator sends
/// SIGINT/SIGTERM.
///
/// Tracer routing: events are forwarded to stderr via
/// [`TraceSubmitApi::render_human`]. R344 will swap stderr forwarding
/// for the cardano-tracer NtN protocol once that crate ships its
/// receiver.
pub fn run() -> eyre::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parser::parse_args(&argv) {
        Ok(args) => args,
        Err(parser::ParseError::HelpRequested | parser::ParseError::VersionRequested) => {
            return Ok(());
        }
        Err(err) => return Err(eyre::eyre!("CLI parse error: {err}")),
    };
    let command = cli::parsers::into_command(&args)
        .map_err(|err| eyre::eyre!("CLI validation error: {err}"))?;

    let params = match command {
        cli::types::TxSubmitCommand::TxSubmitRun(params) => params,
        cli::types::TxSubmitCommand::TxSubmitVersion => {
            // Already handled by parser::ParseError::VersionRequested
            // above; this branch is unreachable in practice.
            return Ok(());
        }
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| eyre::eyre!("failed to build tokio runtime: {err}"))?;

    let tracer: rest::web::Tracer = std::sync::Arc::new(|evt| {
        let line = evt.render_human();
        let _ = writeln!(std::io::stderr(), "[cardano-submit-api] {line}");
    });

    runtime.block_on(async {
        web::run_tx_submit_server_from_params(tracer, params)
            .await
            .map_err(|err| eyre::eyre!("tx-submit server: {err}"))
    })
}
