//! Shared `Cardano.Benchmarking.*` support modules for tx-generator.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//! This Rust module is a namespace shell for upstream
//! `Cardano.Benchmarking.*` leaves whose basenames would otherwise
//! collide with existing `Cardano.TxGenerator.*` mirrors.

pub mod log_types;
pub mod tps_throttle;
pub mod types;
