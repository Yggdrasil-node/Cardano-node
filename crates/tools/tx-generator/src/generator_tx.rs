//! Transaction-stream generator runtime surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx.hs`.
//! Hosts the Rust modules that mirror `Cardano.Benchmarking.GeneratorTx.*`.
//! The current concrete slice is `SizedMetadata.hs`; wallet benchmark
//! scheduling and node-to-node submission clients land in strict slices.
//! R561 starts that foundation with the upstream `Benchmarking.Types`
//! and `TpsThrottle` mirrors under [`crate::benchmarking`].

pub mod sized_metadata;
