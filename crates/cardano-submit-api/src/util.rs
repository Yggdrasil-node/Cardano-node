//! Utility helpers shared across the binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Util.hs.
//!
//! Direct port: [`log_exception`] mirrors upstream `logException`.
//!
//! Upstream signature:
//!
//! ```haskell
//! logException :: Trace IO TraceSubmitApi -> Text -> IO a -> IO a
//! logException tracer txt action = action `catch` logger
//!   where
//!     logger :: SomeException -> IO a
//!     logger e = do
//!       traceWith tracer (EndpointException txt e)
//!       throwIO e
//! ```
//!
//! Rust port: closure-generic wrapper that forwards a single
//! `EndpointException` event to the supplied tracer-fn on `Err`, then
//! propagates the error. The tracer-fn is a `FnOnce` so it can capture
//! the live trace surface without committing this layer to a particular
//! tracing backend (`tracing` vs `slog` vs structured-stdout); R340
//! integration round wires the chosen backend.

use crate::tracing::trace_submit_api::TraceSubmitApi;

/// Run an action, forwarding any error to the tracer as an
/// `EndpointException` event before re-raising.
///
/// Upstream Haskell uses `catch :: IO a -> (SomeException -> IO a) -> IO a`;
/// the Rust port operates on synchronous `Result<T, E>` actions and
/// preserves the rethrow-after-trace semantic. Async tx-submission paths
/// can adapt this helper trivially via `.await` inside the action
/// closure.
///
/// # Parameters
///
/// - `tracer` — sink that receives a single [`TraceSubmitApi::EndpointException`]
///   event when the action errors. Receives a fresh closure-owned event
///   per call (matches upstream's per-call trace emit).
/// - `context` — operator-facing label that becomes the event's `context`
///   field. Mirrors upstream's `txt :: Text` parameter.
/// - `action` — the fallible operation to execute. Returns the same
///   `Result` type unchanged.
///
/// # Behavior on success
///
/// Returns `Ok(value)` without invoking `tracer`.
///
/// # Behavior on failure
///
/// Renders the error via [`std::fmt::Display`], emits a
/// [`TraceSubmitApi::EndpointException`] event with the rendered string
/// in the `exception` field, then returns the original `Err(e)`.
///
/// # Example
///
/// ```
/// use yggdrasil_cardano_submit_api::tracing::trace_submit_api::TraceSubmitApi;
/// use yggdrasil_cardano_submit_api::util::log_exception;
///
/// let mut events: Vec<TraceSubmitApi> = Vec::new();
/// let tracer = |evt| events.push(evt);
///
/// let result: Result<(), std::io::Error> = log_exception(
///     tracer,
///     "submit-tx-handler",
///     || Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "ECONNRESET")),
/// );
///
/// assert!(result.is_err());
/// assert_eq!(events.len(), 1);
/// assert!(matches!(
///     &events[0],
///     TraceSubmitApi::EndpointException { context, exception }
///         if context == "submit-tx-handler" && exception.contains("ECONNRESET")
/// ));
/// ```
pub fn log_exception<T, E, F, L>(tracer: L, context: &str, action: F) -> Result<T, E>
where
    F: FnOnce() -> Result<T, E>,
    E: std::fmt::Display,
    L: FnOnce(TraceSubmitApi),
{
    match action() {
        Ok(value) => Ok(value),
        Err(err) => {
            tracer(TraceSubmitApi::EndpointException {
                context: context.to_string(),
                exception: err.to_string(),
            });
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::io;

    #[test]
    fn ok_path_does_not_invoke_tracer() {
        let events: RefCell<Vec<TraceSubmitApi>> = RefCell::new(Vec::new());
        let tracer = |evt: TraceSubmitApi| events.borrow_mut().push(evt);
        let result: Result<u32, io::Error> = log_exception(tracer, "ctx", || Ok(42));
        assert_eq!(result.expect("ok path"), 42);
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn err_path_emits_endpoint_exception_event() {
        let events: RefCell<Vec<TraceSubmitApi>> = RefCell::new(Vec::new());
        let tracer = |evt: TraceSubmitApi| events.borrow_mut().push(evt);

        let result: Result<u32, io::Error> = log_exception(tracer, "submit-tx", || {
            Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "ECONNREFUSED",
            ))
        });

        assert!(result.is_err());
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        match &events[0] {
            TraceSubmitApi::EndpointException { context, exception } => {
                assert_eq!(context, "submit-tx");
                assert!(exception.contains("ECONNREFUSED"));
            }
            other => panic!("expected EndpointException, got {other:?}"),
        }
    }

    #[test]
    fn err_path_propagates_original_error() {
        let events: RefCell<Vec<TraceSubmitApi>> = RefCell::new(Vec::new());
        let tracer = |evt: TraceSubmitApi| events.borrow_mut().push(evt);

        let result: Result<u32, io::Error> = log_exception(tracer, "x", || {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "EACCES"))
        });

        let err = result.expect_err("err path");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn context_label_propagates_to_event() {
        let events: RefCell<Vec<TraceSubmitApi>> = RefCell::new(Vec::new());
        let tracer = |evt: TraceSubmitApi| events.borrow_mut().push(evt);

        let _: Result<(), io::Error> =
            log_exception(tracer, "my-context-label", || Err(io::Error::other("oops")));

        match &events.borrow()[0] {
            TraceSubmitApi::EndpointException { context, .. } => {
                assert_eq!(context, "my-context-label");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }
}
