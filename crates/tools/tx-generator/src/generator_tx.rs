//! Transaction-stream generator runtime surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx.hs`.
//! Hosts the Rust modules that mirror `Cardano.Benchmarking.GeneratorTx.*`.
//! The current concrete leaves are `SizedMetadata.hs` and the
//! `SubmissionClient.hs` request-state core. Wallet benchmark
//! scheduling and node-to-node socket wiring land in later strict
//! slices.

pub mod sized_metadata;
pub mod submission_client;
