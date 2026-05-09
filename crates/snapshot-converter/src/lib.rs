//! Ledger snapshot format converter (mem ↔ lmdb ↔ lsm) — pure-Rust port mirroring upstream snapshot-converter.hs.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R327
//! skeleton entry point for the `snapshot-converter` sister-tool crate. Per-file
//! mirror tree under `src/` will be populated incrementally per the
//! sister-tools port arc plan (R326–R459); each leaf module landed
//! in subsequent rounds carries its own `## Naming parity` block.
//!
//! Upstream source vendored at:
//! `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/app/snapshot-converter.hs`.

/// Placeholder run-loop entry called by the binary `main`.
///
/// Subsequent rounds replace this stub with the concrete subcommand
/// dispatcher matching the upstream binary's CLI surface.
pub fn run() -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-snapshot-converter: not yet implemented (R327 skeleton); \
         see docs/operational-runs/ for the snapshot-converter port progress."
    ))
}
