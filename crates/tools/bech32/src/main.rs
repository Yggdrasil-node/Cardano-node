//! Binary entry point for the `bech32` deployable.
//!
//! ## Naming parity
//!
//! **Strict mirror:** bech32/app/Main.hs. The Rust `main.rs` is the
//! canonical 1:1 mirror of upstream `bech32/bech32/app/Main.hs` —
//! the executable entry point that parses command-line arguments
//! (`HRP` + base16 input on stdin), dispatches to the encoder /
//! decoder, and prints the resulting Bech32 / base16 string.
//!
//! R332 lands the CLI parser + byte-equivalent `--help` / `--version`
//! handling. R333 will replace `run()`'s sentinel with the concrete
//! encode/decode dispatch.

use std::io::Write;
use std::process::ExitCode;

use yggdrasil_bech32::parser::{HELP_TEXT, ParseError, VERSION_TEXT, parse_args};

fn main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Idempotent: a second call is a no-op.
    let _ = yggdrasil_telemetry::init_subscriber(&yggdrasil_telemetry::TracingConfig::default());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&argv) {
        Ok(args) => match yggdrasil_bech32::run_with(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                let _ = writeln!(std::io::stderr(), "Error: {err:?}");
                ExitCode::FAILURE
            }
        },
        Err(ParseError::HelpRequested) => {
            // Upstream emits help on stdout; mirror byte-for-byte.
            let _ = std::io::stdout().write_all(HELP_TEXT.as_bytes());
            ExitCode::SUCCESS
        }
        Err(ParseError::VersionRequested) => {
            // Upstream emits "1.1.10\n" on stdout.
            let _ = std::io::stdout().write_all(VERSION_TEXT.as_bytes());
            ExitCode::SUCCESS
        }
        Err(err) => {
            // Unknown flag / too many positionals — upstream prints a
            // short error to stderr and exits 1.
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}
