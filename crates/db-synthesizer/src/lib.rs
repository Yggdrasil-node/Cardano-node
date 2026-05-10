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
//! Layout mapping (R354 ships types.rs; later rounds populate the rest):
//!
//! | Upstream `.hs`                                | Yggdrasil `.rs`              |
//! |-----------------------------------------------|------------------------------|
//! | `Tools/DBSynthesizer/Types.hs`                | `types.rs`                   |
//! | `app/DBSynthesizer/Parsers.hs`                | `parser.rs`                  |
//! | `Tools/DBSynthesizer/Forging.hs`              | `forging.rs` (pending)       |
//! | `Tools/DBSynthesizer/Run.hs`                  | `run.rs` (pending)           |
//! | `Tools/DBSynthesizer/Orphans.hs`              | `orphans.rs` (synthesis)     |

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
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
    Err(eyre::eyre!(
        "yggdrasil-db-synthesizer: forge loop not yet implemented \
         (R364 ships argv → Args dispatch; Forging.hs + Run.hs land in \
         subsequent rounds gated on Phase C entry). Resolved: \
         config={}, db={}, limit={}, mode={}.",
        args.paths.config.display(),
        args.paths.chain_db.display(),
        limit,
        mode,
    ))
}
