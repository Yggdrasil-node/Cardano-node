//! EraIndependent hash shared helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Hash/Internal/Common.hs`.
//! The upstream module hosts shared anchor-data/script hash helpers;
//! the R518 Rust slice implements only `hash genesis-file`, which
//! does not need shared helper state. Keep this file as the strict
//! mirror location for those helpers when anchor-data/script hashing
//! is ported.
