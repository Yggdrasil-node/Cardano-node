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
//! exactly) for drop-in deployment via `node/scripts/run-tools.sh`.

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
/// R340 lands argv → [`cli::types::TxSubmitCommand`] validation. The
/// [`run`] entrypoint now produces a fully-resolved
/// [`cli::types::TxSubmitNodeParams`] before failing with the
/// "web server not yet implemented" sentinel — operators still see the
/// previous behavior at the binary level (no HTTP listener) but
/// `--config`/`--socket-path`/`--mainnet|--testnet-magic` are now
/// validated and missing-flag errors are surfaced clearly.
///
/// R341 will replace the trailing sentinel with the real axum server.
pub fn run() -> eyre::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parser::parse_args(&argv) {
        Ok(args) => args,
        Err(parser::ParseError::HelpRequested | parser::ParseError::VersionRequested) => {
            return Ok(());
        }
        Err(err) => return Err(eyre::eyre!("CLI parse error: {err}")),
    };
    let _command = cli::parsers::into_command(&args)
        .map_err(|err| eyre::eyre!("CLI validation error: {err}"))?;
    Err(eyre::eyre!(
        "yggdrasil-cardano-submit-api: web server not yet implemented \
         (R340 ships argv → TxSubmitCommand validation; R341 lands the \
         axum HTTP listener)."
    ))
}
