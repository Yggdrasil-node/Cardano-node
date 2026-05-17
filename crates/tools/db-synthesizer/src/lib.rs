#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `db-synthesizer`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `db-synthesizer` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (Phase 4 R1 ships forging.rs + run.rs):
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
/// **Phase 4 R2 slice:** wires argv → [`parser::Args`] → the
/// [`run::synthesize_from_config`] supervisor. The synthesizer reads
/// the `--config` node config to resolve the real Shelley-genesis
/// epoch length, opens (or creates) the ChainDB at `--db`, forges the
/// `--blocks N` / `--slots N` / `--epochs N` deterministic structural
/// blocks, then prints upstream-shaped progress lines.
///
/// Mirror of upstream `app/db-synthesizer.hs`'s `main`:
/// `initialize paths creds forgeOpts >>= either die (synthesize ...)`.
///
/// **Carve-out (R3):** upstream's `initialize` also builds the full
/// multi-era `CardanoProtocolParams` and a Praos `BlockForging`
/// credential set. This slice ports the genesis-loading half — the
/// real epoch length is now read from `--config` — but the Praos
/// forge path (`initProtocol` + the VRF/KES/OpCert leader check) is
/// the remaining db-synthesizer R3 carve-out, so the forged blocks
/// are still *non-Praos* structural blocks (see [`forging`]'s module
/// note). The result is a structurally-valid ChainDB that yggdrasil's
/// own `FileImmutable` / `db-analyser` can open and walk — not yet a
/// Praos-valid chain.
pub fn run(args: &parser::Args) -> eyre::Result<()> {
    let outcome =
        run::synthesize_from_config(args.options, &args.paths.config, &args.paths.chain_db)?;

    // Upstream-shaped progress reporting (mirror of Run.hs's putStrLn
    // lines + app/db-synthesizer.hs's "--> done" line).
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
