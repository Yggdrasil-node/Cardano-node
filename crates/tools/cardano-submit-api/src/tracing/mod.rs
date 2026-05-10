//! Tracing module umbrella for tracing/trace_submit_api.rs.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell that
//! re-exports the per-concern leaf modules to keep call sites
//! unchanged from upstream's `Cardano.TxSubmit.Tracing.<Leaf>`
//! qualified names. R335 file-mirror skeleton.

pub mod trace_submit_api;
