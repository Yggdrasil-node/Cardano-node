//! Transaction-stream generator runtime surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx.hs`.
//! Hosts the Rust modules that mirror `Cardano.Benchmarking.GeneratorTx.*`.
//! The current concrete slice is `SizedMetadata.hs`; wallet benchmark
//! scheduling and node-to-node submission clients land in later strict
//! slices.

pub mod sized_metadata;
