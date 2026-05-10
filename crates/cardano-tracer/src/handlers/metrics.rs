//! Metrics-handler subsystem — parent shell for the
//! `handlers/metrics/` sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side parent shell for the
//! `Cardano.Tracer.Handlers.Metrics.*` namespace. The upstream
//! namespace has no aggregate `Metrics.hs` file — it's a directory
//! of peer leaves (`Servers.hs`, `Utils.hs`, `Monitoring.hs`,
//! `TimeseriesServer.hs`, `Prometheus.hs`) — so this file exists
//! only to declare the sub-modules below it.
//!
//! Layout mapping:
//!
//! | Upstream                                           | Yggdrasil                |
//! |----------------------------------------------------|--------------------------|
//! | `Metrics/Utils.hs`                                 | `utils.rs`               |
//! | `Metrics/Servers.hs`                               | `servers.rs` (pending)   |
//! | `Metrics/Monitoring.hs`                            | `monitoring.rs` (pending)|
//! | `Metrics/TimeseriesServer.hs`                      | `timeseries_server.rs` (pending) |
//! | `Metrics/Prometheus.hs`                            | `prometheus.rs`          |

pub mod prometheus;
pub mod utils;
