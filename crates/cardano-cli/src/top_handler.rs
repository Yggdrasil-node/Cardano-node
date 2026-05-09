//! Top-level error handler for the cardano-cli binary.
//!
//! Mirrors upstream `Cardano.CLI.TopHandler` — the wrapper that
//! catches panics and structured errors at the binary's main entry
//! point and renders them with a consistent format before exit.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/TopHandler.hs`.
//! R295 sweeper closes the missing top-level mirror surfaced when
//! tallying upstream files vs Yggdrasil's R289 + R294 coverage.

use eyre::Result;

/// Wrap a `main`-style entry point with the cardano-cli top-level
/// error handler. Catches panics, prints structured errors, and
/// returns the appropriate process exit code.
///
/// Mirrors upstream `toplevelExceptionHandler` from
/// `Cardano.CLI.TopHandler`.
pub fn top_handler<F>(entry: F) -> i32
where
    F: FnOnce() -> Result<()>,
{
    match entry() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("yggdrasil-cardano-cli: {err}");
            1
        }
    }
}
