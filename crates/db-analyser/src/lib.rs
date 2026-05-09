#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `db-analyser`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `db-analyser` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R351 ships types.rs; later rounds populate the rest):
//!
//! | Upstream `.hs`                                       | Yggdrasil `.rs`              |
//! |------------------------------------------------------|------------------------------|
//! | `Tools/DBAnalyser/Types.hs`                          | `types.rs`                   |
//! | `app/DBAnalyser/Parsers.hs`                          | `parser.rs`                  |
//! | `Tools/DBAnalyser/HasAnalysis.hs`                    | `has_analysis.rs` (pending)  |
//! | `Tools/DBAnalyser/Analysis.hs`                       | `analysis.rs` (pending)      |
//! | `Tools/DBAnalyser/CSV.hs`                            | `csv.rs` (pending)           |
//! | `Tools/DBAnalyser/Run.hs`                            | `run.rs` (pending)           |

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
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
    }
}

/// Concrete run-loop entry. R335-pattern skeleton: returns the
/// "not-yet-implemented" sentinel pending later round implementation.
/// The CLI parser surface (--help / --version) IS functional and
/// byte-equivalent to upstream.
pub fn run() -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-db-analyser: subcommand dispatch not yet implemented          (R335-pattern skeleton). Help/version output IS byte-equivalent          to upstream; concrete subcommand implementations land in          later rounds of the sister-tools port arc."
    ))
}
