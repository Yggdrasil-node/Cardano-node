#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `snapshot-converter`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `snapshot-converter` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R353 ships types.rs; later rounds populate the rest):
//!
//! | Upstream `app/snapshot-converter.hs` section          | Yggdrasil `.rs`                 |
//! |-------------------------------------------------------|---------------------------------|
//! | `data Config` / `data Snapshot'` / supporting types   | `types.rs`                      |
//! | `parseConfig` (optparse-applicative)                  | `parser.rs`                     |
//! | `convertSnapshot` (LedgerDB conversion logic)         | `convert.rs` (pending; carve-out)|
//! | `withManager` / `watchTree` daemon                    | `daemon.rs` (pending)           |
//! | `main`                                                | `main.rs`                       |

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
pub mod status;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R363 wires the typed parser dispatcher end-to-end. On successful
/// parse the resolved [`types::Config`] is handed to [`run`]; `--help`
/// and `--version` short-circuit with byte-equivalent upstream output.
pub fn run_main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Idempotent: a second call is a no-op.
    let _ = yggdrasil_telemetry::init_subscriber(&yggdrasil_telemetry::TracingConfig::default());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let config = match parser::parse_args(&argv) {
        Ok(config) => config,
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
    match run(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry.
///
/// R363 wires argv → [`types::Config`] dispatch. The actual
/// mem↔lsm conversion logic + filesystem-watcher daemon are
/// documented carve-outs (see [`status::ConvertSnapshotStatus`])
/// — gated on yggdrasil-format LedgerStore reader/writer being
/// available, which is itself a separate parity arc.
///
/// R439 surfaces the deferral via the [`RunError`] enum +
/// [`status::convert_snapshot_status`] introspection helper rather
/// than a raw `eyre::eyre!` string. Callers can match on the
/// specific deferral variant for programmatic dispatch.
pub fn run(config: &types::Config) -> eyre::Result<()> {
    let mode = match config {
        types::Config::Daemon { .. } => RunMode::Daemon,
        types::Config::Oneshot { .. } => RunMode::Oneshot,
    };
    Err(RunError::ConvertSnapshotDeferred { mode }.into())
}

/// Operating-mode tag for [`RunError::ConvertSnapshotDeferred`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RunMode {
    /// Daemon mode — `--monitor-lsm-snapshots-in` filesystem-watcher loop.
    Daemon,
    /// Oneshot mode — single `--input-mem`/`--input-lsm` →
    /// `--output-mem`/`--output-lsm` conversion.
    Oneshot,
}

impl std::fmt::Display for RunMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Daemon => "daemon",
            Self::Oneshot => "oneshot",
        })
    }
}

/// Errors from the snapshot-converter `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The `convertSnapshot` mem↔lsm logic + filesystem-watcher
    /// daemon are deferred carve-outs gated on yggdrasil-format
    /// LedgerStore reader/writer being available. Mirror of upstream
    /// `Ouroboros.Consensus.Cardano.SnapshotConversion.convertSnapshot`
    /// — the conversion operates on the upstream ledger-DB on-disk
    /// format, which differs from yggdrasil's storage layout.
    #[error(
        "yggdrasil-snapshot-converter: {mode} mode dispatch deferred — convertSnapshot LSM/Mem \
         logic + filesystem-watcher daemon land when the yggdrasil-format LedgerStore reader/\
         writer is wired (see crates/tools/snapshot-converter/src/status.rs::convert_snapshot_status \
         for the full deferral rationale)."
    )]
    ConvertSnapshotDeferred {
        /// The mode the operator's config selected.
        mode: RunMode,
    },
}
