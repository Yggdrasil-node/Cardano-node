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
//! Layout mapping (R378 ships orphans.rs; later rounds populate the rest):
//!
//! | Upstream `.hs`                                | Yggdrasil `.rs`              |
//! |-----------------------------------------------|------------------------------|
//! | `Tools/DBSynthesizer/Types.hs`                | `types.rs`                   |
//! | `app/DBSynthesizer/Parsers.hs`                | `parser.rs`                  |
//! | `Tools/DBSynthesizer/Forging.hs`              | `forging.rs` (pending)       |
//! | `Tools/DBSynthesizer/Run.hs`                  | `run.rs` (pending)           |
//! | `Tools/DBSynthesizer/Orphans.hs`              | `orphans.rs`                 |

use std::io::Write;
use std::process::ExitCode;

pub mod orphans;
pub mod parser;
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
/// R364 lands argv → [`parser::Args`] dispatch. The actual forging
/// loop (Forging.hs port leveraging node/src/block_producer.rs) +
/// Run.hs supervisor land in subsequent rounds per the per-tool
/// roadmap (gated on Phase C entry per the plan's Phase C
/// authorization checkpoint).
pub fn run(args: &parser::Args) -> eyre::Result<()> {
    let limit = match args.options.limit {
        types::ForgeLimit::Slot(s) => format!("slots={}", s.0),
        types::ForgeLimit::Block(b) => format!("blocks={b}"),
        types::ForgeLimit::Epoch(e) => format!("epochs={e}"),
    };
    let mode = match args.options.open_mode {
        types::DBSynthesizerOpenMode::OpenCreate => "create",
        types::DBSynthesizerOpenMode::OpenCreateForce => "create-force",
        types::DBSynthesizerOpenMode::OpenAppend => "append",
    };
    Err(RunError::ForgeLoopDeferred {
        config: args.paths.config.display().to_string(),
        chain_db: args.paths.chain_db.display().to_string(),
        limit,
        mode: mode.to_string(),
    }
    .into())
}

/// Errors from the db-synthesizer `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Forge loop + Run.hs supervisor are deferred. Mirror of
    /// upstream `Cardano.Tools.DBSynthesizer.{Forging, Run}`.
    /// Yggdrasil's port is gated on the Phase C authorization
    /// checkpoint per the playful-tickling-plum.md plan (the
    /// cardano-cli MVS in the parallel C-arc must complete first).
    #[error(
        "yggdrasil-db-synthesizer: forge loop deferred — gated on Phase C authorization \
         checkpoint (see crates/db-synthesizer/src/status.rs::forge_loop_status for the \
         full deferral rationale). Resolved CLI: config={config}, db={chain_db}, \
         limit={limit}, mode={mode}."
    )]
    ForgeLoopDeferred {
        /// Path to the config file the operator supplied.
        config: String,
        /// Path to the ChainDB the operator supplied.
        chain_db: String,
        /// Forge-limit rendering (slots / blocks / epochs).
        limit: String,
        /// DB-open-mode rendering (create / create-force / append).
        mode: String,
    },
}
