//! Top-level event handlers wired by `run_node`.
//!
//! Mirrors upstream `Cardano.Node.Handlers.*`. Currently only
//! `shutdown` is broken out as its own submodule; future
//! Phase D work will add `top_level` (mirroring upstream
//! `Cardano.Node.Handlers.TopLevel`, 138 lines / 5 KB) when the
//! top-level handler dispatch lands.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/src/Cardano/Node/Handlers>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over
//! `handlers/shutdown.rs` (the only sub-module currently). The
//! shell exists to allow further runtime-handler sub-modules to
//! land without churning the public path. Upstream wires
//! shutdown / OS-signal handling inline in `Cardano.Node.Run.runNode`.

pub mod shutdown;
