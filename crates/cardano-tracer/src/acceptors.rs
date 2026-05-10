//! Trace-forwarder mini-protocol acceptors — entry points for
//! cardano-tracer's pull-side wiring against running cardano-node
//! forwarders.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell module.
//! Mirror of upstream's `Cardano.Tracer.Acceptors` namespace
//! (`Acceptors/{Server, Client, Utils, Run}.hs`); the leaves carry
//! their own strict-mirror declarations:
//!
//! - [`utils`] mirrors `Cardano.Tracer.Acceptors.Utils`.
//! - `server` (R424 pending) will mirror
//!   `Cardano.Tracer.Acceptors.Server`.
//! - `client` (R425 pending) will mirror
//!   `Cardano.Tracer.Acceptors.Client`.
//! - `run` (R426 pending) will mirror
//!   `Cardano.Tracer.Acceptors.Run`.

pub mod server;
pub mod utils;
