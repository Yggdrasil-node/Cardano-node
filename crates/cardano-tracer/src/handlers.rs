//! Per-subsystem dispatch handlers used by the cardano-tracer
//! supervisor — parent shell.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side parent shell for the `handlers/` sub-tree, which
//! mirrors the upstream `Cardano.Tracer.Handlers.*` namespace. The
//! upstream namespace has no aggregate `Handlers.hs` file — it's a
//! directory of peer subsystem-specific modules — so this file
//! exists only to declare the sub-modules below it.
//!
//! Layout mapping:
//!
//! | Upstream                                        | Yggdrasil                          |
//! |-------------------------------------------------|------------------------------------|
//! | `Cardano/Tracer/Handlers/Notifications/*.hs`    | `notifications/*.rs`               |
//! | `Cardano/Tracer/Handlers/Logs/*.hs`             | `logs/*.rs` (pending)              |
//! | `Cardano/Tracer/Handlers/Metrics/*.hs`          | `metrics/*.rs` (pending)           |
//! | `Cardano/Tracer/Handlers/State/*.hs`            | `state/*.rs` (pending)             |
//! | `Cardano/Tracer/Handlers/RTView/*.hs`           | **CARVE-OUT** (synthesis per plan) |
//! | `Cardano/Tracer/Handlers/Utils.hs`              | `utils.rs` (pending)               |

pub mod logs;
pub mod notifications;
