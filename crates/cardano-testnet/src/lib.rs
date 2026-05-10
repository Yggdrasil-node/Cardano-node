#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `cardano-testnet`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `cardano-testnet` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R359 ships types.rs covering simple option types;
//! later rounds populate the deeper era-aware records):
//!
//! | Upstream `.hs`                                       | Yggdrasil `.rs`              |
//! |------------------------------------------------------|------------------------------|
//! | `Testnet/Start/Types.hs` (simple option types)       | `types.rs`                   |
//! | `Testnet/Types.hs` (runtime/key types)               | `runtime_types.rs` (pending) |
//! | `Testnet/Start/{Byron,Cardano}.hs` (era startup)     | `start/*.rs` (pending)       |
//! | `Testnet/Components/{Query,Configuration}.hs`        | `components/*.rs` (pending)  |
//! | `Testnet/Process/Cli/*.hs` (SPO/Tx/Keys/DRep dispatch) | `process/cli/*.rs` (pending) |
//! | `Testnet/Property/*.hs`                              | **CARVE-OUT** (Hedgehog → proptest synthesis) |
//! | `Testnet/Process/{Run,RunIO}.hs`                     | **CARVE-OUT** (Hedgehog → tokio::process synthesis) |

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
        "yggdrasil-cardano-testnet: subcommand dispatch not yet implemented          (R335-pattern skeleton). Help/version output IS byte-equivalent          to upstream; concrete subcommand implementations land in          later rounds of the sister-tools port arc."
    ))
}
