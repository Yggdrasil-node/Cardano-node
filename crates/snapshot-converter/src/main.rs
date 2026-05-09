//! Binary entry point for the `snapshot-converter` deployable.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. R327 skeleton — minimal binary wrapper
//! that delegates to `yggdrasil_snapshot_converter::run()`. The upstream binary's
//! `Main.hs` (or per-app launcher) is mirrored into the lib's
//! `run.rs` module across subsequent rounds.

fn main() -> eyre::Result<()> {
    yggdrasil_snapshot_converter::run()
}
