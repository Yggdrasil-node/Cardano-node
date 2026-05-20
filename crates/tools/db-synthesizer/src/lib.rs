#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `db-synthesizer`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell for the
//! `db-synthesizer` sister-tool crate. The leaf modules below carry the
//! upstream file mirrors for the typed config, parser, forge, run, and
//! orphan-instance surfaces.
//!
//! Layout mapping:
//!
//! | Upstream `.hs`                                | Yggdrasil `.rs`              |
//! |-----------------------------------------------|------------------------------|
//! | `Tools/DBSynthesizer/Types.hs`                | `types.rs`                   |
//! | `app/DBSynthesizer/Parsers.hs`                | `parser.rs`                  |
//! | `Tools/DBSynthesizer/Forging.hs`              | `forging.rs`                 |
//! | `Tools/DBSynthesizer/Run.hs`                  | `run.rs`                     |
//! | `Tools/DBSynthesizer/Orphans.hs`              | `orphans.rs`                 |

use std::io::Write;
use std::process::ExitCode;

pub mod forging;
pub mod orphans;
pub mod parser;
pub mod run;
pub mod status;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R364 wires the typed parser dispatcher end-to-end. On successful
/// parse the resolved [`parser::Args`] (NodeFilePaths +
/// NodeCredentials + DBSynthesizerOptions) is handed to [`run`];
/// `--help` and `--version` short-circuit with byte-equivalent
/// upstream output.
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
/// Wires argv → [`parser::Args`] → the [`run::synthesize_from_config`]
/// supervisor. The synthesizer reads `--config`, resolves the consensus
/// protocol and leader credentials, opens (or creates) the ChainDB at
/// `--db`, forges `--blocks N` / `--slots N` / `--epochs N` through the
/// shared Praos leader-check + KES block forge path, then prints
/// upstream-shaped progress lines.
///
/// Mirror of upstream `app/db-synthesizer.hs`'s `main`:
/// `initialize paths creds forgeOpts >>= either die (synthesize ...)`.
///
/// **Remaining gate:** the production forge path now derives per-forger
/// stake from the rotating ledger-view snapshots. Final operator swap-in
/// remains gated on the upstream ChainDB byte-equivalence soak.
pub fn run(args: &parser::Args) -> eyre::Result<()> {
    let outcome = run::synthesize_from_config(
        args.options,
        &args.credentials,
        &args.paths.config,
        &args.paths.chain_db,
    )?;

    // Upstream-shaped progress reporting (mirror of Run.hs's putStrLn
    // lines + app/db-synthesizer.hs's "--> done" line).
    if !outcome.chain_db_opened {
        println!("--> no forgers found; leaving possibly existing ChainDB untouched");
        println!("--> done; result: ForgeResult {{resultForged = 0}}");
        return Ok(());
    }

    let mode = match args.options.open_mode {
        types::DBSynthesizerOpenMode::OpenCreate => "OpenCreate",
        types::DBSynthesizerOpenMode::OpenCreateForce => "OpenCreateForce",
        types::DBSynthesizerOpenMode::OpenAppend => "OpenAppend",
    };
    println!("--> opening ChainDB on file system with mode: {mode}");
    println!("--> starting at: SlotNo {}", outcome.resumed_from.0);
    println!(
        "--> forged and adopted {} blocks; reached SlotNo {}",
        outcome.forge.result.forged, outcome.forge.final_state.current_slot.0
    );
    println!(
        "--> done; result: ForgeResult {{resultForged = {}}}",
        outcome.forge.result.forged
    );
    Ok(())
}

/// Errors from the db-synthesizer `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The synthesize supervisor failed (ChainDB open / mode check /
    /// block append). Mirror of upstream `Cardano.Tools.DBSynthesizer.Run`
    /// surfacing a `preOpenChainDB` `fail` or a ChainDB-write error.
    #[error(transparent)]
    Synthesize(#[from] run::RunError),
}
