//! Linux journal sink — re-export wrapper for the no-systemd
//! implementation.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Logs/Journal.hs.
//!
//! Direct port of the upstream CPP-conditional dispatcher:
//!
//! ```haskell
//! #ifdef SYSTEMD
//! import           Cardano.Tracer.Handlers.Logs.Journal.Systemd as Impl
//! #else
//! import           Cardano.Tracer.Handlers.Logs.Journal.NoSystemd as Impl
//! #endif
//! ```
//!
//! Yggdrasil always selects the `NoSystemd` implementation per the
//! workspace policy banning systemd-specific dependencies. The
//! re-export keeps the Haskell-side dispatcher's external surface
//! intact: callers `use crate::handlers::logs::journal::write_trace_objects_to_journal`
//! exactly as they would `import Cardano.Tracer.Handlers.Logs.Journal
//! (writeTraceObjectsToJournal)` — the underlying impl is a no-op
//! that returns `Ok(())` without doing any I/O.
//!
//! Mapping summary:
//!
//! | Upstream                                          | Yggdrasil                                       |
//! |---------------------------------------------------|-------------------------------------------------|
//! | `module Impl` (CPP-conditional re-export)         | `pub use no_systemd::write_trace_objects_to_journal` |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Cardano.Tracer.Handlers.Logs.Journal.Systemd`**: upstream
//!   has a Systemd-only implementation invoking the
//!   `libsystemd-journal` Haskell binding (which itself wraps the
//!   C `libsystemd-journal` library). Yggdrasil's no-FFI policy
//!   forbids that path; structured log output is instead emitted
//!   via the file/Prometheus/forwarder paths in
//!   [`super::super::notifications`] / future `handlers/logs/file`
//!   / future `handlers/metrics/prometheus`. Operators who require
//!   journald output can run the Yggdrasil tracer behind systemd's
//!   own log-redirect (which implicitly converts stdout/stderr to
//!   journal entries when `StandardOutput=journal` is set in the
//!   service unit).

pub mod no_systemd;

pub use no_systemd::write_trace_objects_to_journal;
