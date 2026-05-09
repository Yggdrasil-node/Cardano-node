//! HTTP API for transaction submission — pure-Rust port mirroring upstream cardano-submit-api/src/Cardano/TxSubmit/*.hs.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R327
//! skeleton entry point for the `cardano-submit-api` sister-tool crate. Per-file
//! mirror tree under `src/` will be populated incrementally per the
//! sister-tools port arc plan (R326–R459); each leaf module landed
//! in subsequent rounds carries its own `## Naming parity` block.
//!
//! Upstream source vendored at:
//! `.reference-haskell-cardano-node/cardano-submit-api/`.

/// Placeholder run-loop entry called by the binary `main`.
///
/// Subsequent rounds replace this stub with the concrete subcommand
/// dispatcher matching the upstream binary's CLI surface.
pub fn run() -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-cardano-submit-api: not yet implemented (R327 skeleton); \
         see docs/operational-runs/ for the cardano-submit-api port progress."
    ))
}
