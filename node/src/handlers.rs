//! Top-level event handlers wired by `run_node`.
//!
//! Mirrors upstream `Cardano.Node.Handlers.*`. Currently only
//! `shutdown` is broken out as its own submodule; future
//! Phase D work will add `top_level` (mirroring upstream
//! `Cardano.Node.Handlers.TopLevel`, 138 lines / 5 KB) when the
//! top-level handler dispatch lands.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/src/Cardano/Node/Handlers>

pub mod shutdown;
