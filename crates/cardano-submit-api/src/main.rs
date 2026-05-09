//! Binary entry point for the `cardano-submit-api` deployable.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. R327 skeleton — minimal binary wrapper
//! that delegates to `yggdrasil_cardano_submit_api::run()`. The upstream binary's
//! `Main.hs` (or per-app launcher) is mirrored into the lib's
//! `run.rs` module across subsequent rounds.

fn main() -> eyre::Result<()> {
    yggdrasil_cardano_submit_api::run()
}
