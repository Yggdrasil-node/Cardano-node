//! No-op stand-in for the journal sink on non-systemd platforms.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Logs/Journal/NoSystemd.hs.
//!
//! Direct port of the upstream no-systemd implementation:
//!
//! ```haskell
//! writeTraceObjectsToJournal :: LogFormat -> NodeName -> [TraceObject] -> IO ()
//! writeTraceObjectsToJournal _ _ _ = pure ()
//! ```
//!
//! Yggdrasil's no-FFI policy means this is the **only** journal
//! impl — see [`super`]'s docstring for the carve-out rationale on
//! the upstream Systemd-bound `Journal/Systemd.hs`.
//!
//! Mapping summary:
//!
//! | Upstream                                          | Yggdrasil                          |
//! |---------------------------------------------------|------------------------------------|
//! | `writeTraceObjectsToJournal :: LogFormat -> NodeName -> [TraceObject] -> IO ()` | [`write_trace_objects_to_journal`] |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Cardano.Logging.TraceObject`**: now upgraded to a real
//!   6-field record at [`crate::logging::TraceObject`] (R399 inline
//!   port). This file re-exports that canonical type via
//!   `pub use` so callers that imported it from the original
//!   `journal::no_systemd` location keep working — the underlying
//!   shape is no longer a unit struct.

use crate::configuration::LogFormat;
use crate::types::NodeName;

/// Re-export of the canonical [`crate::logging::TraceObject`]
/// 6-field record. Originally lived here as a unit-struct
/// placeholder (R382-R398); upgraded to the real shape at R399 per
/// the audit logged in `docs/operational-runs/2026-05-10-round-398-dep-audit-tracerenv-decision.md`
/// (D3 inline port).
pub use crate::logging::TraceObject;

/// Write a list of trace objects to the systemd journal. Mirror of
/// upstream `writeTraceObjectsToJournal`. On Yggdrasil this is
/// always a no-op (per the no-FFI / no-systemd-binding policy
/// documented in [`super`]).
///
/// Returns `Ok(())` unconditionally so callers can chain with `?`
/// or treat the result as `()` exactly like upstream's `IO ()`.
pub fn write_trace_objects_to_journal(
    _log_format: LogFormat,
    _node_name: &NodeName,
    _trace_objects: &[TraceObject],
) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_trace_objects_to_journal_is_a_no_op_returning_ok() {
        let result =
            write_trace_objects_to_journal(LogFormat::ForMachine, &"test-node".to_string(), &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn write_trace_objects_to_journal_accepts_for_human_format() {
        let result =
            write_trace_objects_to_journal(LogFormat::ForHuman, &"test-node".to_string(), &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn write_trace_objects_to_journal_handles_non_empty_object_list() {
        let objects = vec![
            TraceObject::default(),
            TraceObject::default(),
            TraceObject::default(),
        ];
        let result = write_trace_objects_to_journal(
            LogFormat::ForMachine,
            &"test-node".to_string(),
            &objects,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn trace_object_default_constructs() {
        let _: TraceObject = TraceObject::default();
    }
}
