//! Binary entry point for the `db-truncater` deployable.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. R327 skeleton — minimal binary wrapper
//! that delegates to `yggdrasil_db_truncater::run()`. The upstream binary's
//! `Main.hs` (or per-app launcher) is mirrored into the lib's
//! `run.rs` module across subsequent rounds.

fn main() -> eyre::Result<()> {
    yggdrasil_db_truncater::run()
}
