//! Read hot key.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Read/Committee/HotKey.hs`.
//! R294 lands the file with the API skeleton. Concrete
//! command implementations port from upstream over subsequent
//! rounds + R295 integration tests.

/// Read hot key placeholder.
///
/// Mirrors upstream `Cardano.CLI.Read.Committee.HotKey` types; the Rust port lands
/// as concrete subcommand implementations come online.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HotKeyPlaceholder {}
