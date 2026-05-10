//! cardano-cli helper utilities.
//!
//! Mirrors upstream `Cardano.CLI.Helper` — assorted helpers used
//! across multiple runners (text-envelope writers, hex encoders/
//! decoders, JSON file readers, file-mode validators).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Helper.hs`.
//! R289 lands the file as a skeleton with the `version_info` helper.
//! Concrete helpers (text-envelope writers, key-file validators)
//! land with the per-cluster rounds.

/// Return the cardano-cli compatibility version string.
///
/// Mirrors upstream `displayVersion` from `Cardano.CLI.Helper`. The
/// version string is the Yggdrasil crate version with a "(pure-rust)"
/// suffix to disambiguate from the upstream Haskell binary.
pub fn version_info() -> String {
    format!(
        "yggdrasil-cardano-cli (pure-rust) {}",
        env!("CARGO_PKG_VERSION")
    )
}
