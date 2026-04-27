//! LocalStateQuery mini-protocol server driver.
//!
//! The LocalStateQuery protocol lets a local client acquire a ledger-state
//! snapshot and issue one or more typed queries against it.  This driver
//! wraps a [`ProtocolHandle`] and exposes typed methods for each server-agency
//! state.
//!
//! The query and result payloads are intentionally kept opaque (`Vec<u8>`) at
//! this layer.  The node layer decodes query bytes, executes the query against
//! a `LedgerStateSnapshot`, and encodes the result.
//!
//! Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Server`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::local_state_query::{
    AcquireFailure, AcquireTarget, LocalStateQueryMessage, LocalStateQueryState,
    LocalStateQueryTransitionError,
};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the LocalStateQuery server driver.
#[derive(Debug, thiserror::Error)]
pub enum LocalStateQueryServerError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] LocalStateQueryTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the client.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// Typed request from client
// ---------------------------------------------------------------------------

/// A request received from the client in the `StIdle` state.
#[derive(Clone, Debug)]
pub enum LocalStateQueryIdleRequest {
    /// Client wants to acquire a ledger snapshot at the given target.
    Acquire(AcquireTarget),
    /// Client terminates the protocol.
    Done,
}

/// A request received while a snapshot is acquired (`StAcquired`).
#[derive(Clone, Debug)]
pub enum LocalStateQueryAcquiredRequest {
    /// Client issues a query; payload is opaque CBOR.
    Query(Vec<u8>),
    /// Client releases the snapshot.
    Release,
    /// Client re-acquires at a new target.
    ReAcquire(AcquireTarget),
}

// ---------------------------------------------------------------------------
// LocalStateQueryServer
// ---------------------------------------------------------------------------

/// A LocalStateQuery server driver maintaining the protocol state machine.
///
/// The server loop:
/// 1. Receive an [`LocalStateQueryIdleRequest`] — either `Acquire` or `Done`.
/// 2. Attempt to acquire the requested snapshot; respond with
///    [`acquired`] or [`failure`].
/// 3. Loop receiving [`LocalStateQueryAcquiredRequest`]s until `Release`
///    or `ReAcquire`:
///    - For each `Query`: call [`recv_query`], compute a result, call
///      [`send_result`].
///    - `Release`: returns to idle (loop from step 1).
///    - `ReAcquire`: attempt the new acquisition (loop from step 2).
/// 4. When `Done` is received in step 1, the session ends.
///
/// [`acquired`]: Self::acquired
/// [`failure`]: Self::failure
/// [`recv_query`]: Self::recv_acquired_request
/// [`send_result`]: Self::send_result
pub struct LocalStateQueryServer {
    channel: MessageChannel,
    state: LocalStateQueryState,
}

impl LocalStateQueryServer {
    /// Create a new server driver from a LocalStateQuery `ProtocolHandle`.
    ///
    /// The protocol starts in `StIdle` — the server waits for the first
    /// client request.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: LocalStateQueryState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> LocalStateQueryState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(
        &mut self,
        msg: &LocalStateQueryMessage,
    ) -> Result<(), LocalStateQueryServerError> {
        self.state = self.state.transition(msg)?;
        let bytes = msg.to_cbor();
        if std::env::var("YGG_NTC_DEBUG").is_ok_and(|v| v != "0") {
            let preview_len = bytes.len().min(256);
            let preview: String = bytes[..preview_len]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            eprintln!(
                "[ygg-ntc-debug] LSQ send state={:?} raw_len={} preview={}",
                self.state,
                bytes.len(),
                preview
            );
        }
        self.channel
            .send(bytes)
            .await
            .map_err(LocalStateQueryServerError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<LocalStateQueryMessage, LocalStateQueryServerError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(LocalStateQueryServerError::ConnectionClosed)?;
        if std::env::var("YGG_NTC_DEBUG").is_ok_and(|v| v != "0") {
            let preview_len = raw.len().min(256);
            let preview: String = raw[..preview_len]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            eprintln!(
                "[ygg-ntc-debug] LSQ recv state={:?} raw_len={} preview={}",
                self.state,
                raw.len(),
                preview
            );
        }
        let msg = LocalStateQueryMessage::from_cbor(&raw).map_err(|e| {
            if std::env::var("YGG_NTC_DEBUG").is_ok_and(|v| v != "0") {
                eprintln!("[ygg-ntc-debug] LSQ decode failed: {e}");
            }
            LocalStateQueryServerError::Decode(e.to_string())
        })?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Wait for the next idle-state request from the client.
    ///
    /// Returns either an `Acquire` request (with the target chain point)
    /// or `Done` (the client closed the protocol).
    ///
    /// Must be called when the server is in `StIdle`.
    pub async fn recv_idle_request(
        &mut self,
    ) -> Result<LocalStateQueryIdleRequest, LocalStateQueryServerError> {
        match self.recv_msg().await? {
            LocalStateQueryMessage::MsgAcquire { target } => {
                Ok(LocalStateQueryIdleRequest::Acquire(target))
            }
            LocalStateQueryMessage::MsgDone => Ok(LocalStateQueryIdleRequest::Done),
            msg => Err(LocalStateQueryServerError::UnexpectedMessage(format!(
                "{msg:?}"
            ))),
        }
    }

    /// Confirm successful snapshot acquisition.
    ///
    /// Sends `MsgAcquired` and transitions to `StAcquired`.
    /// Must be called when the server is in `StAcquiring`.
    pub async fn acquired(&mut self) -> Result<(), LocalStateQueryServerError> {
        self.send_msg(&LocalStateQueryMessage::MsgAcquired).await
    }

    /// Signal that the requested snapshot could not be acquired.
    ///
    /// Sends `MsgFailure(reason)` and returns to `StIdle`.
    /// Must be called when the server is in `StAcquiring`.
    pub async fn failure(
        &mut self,
        reason: AcquireFailure,
    ) -> Result<(), LocalStateQueryServerError> {
        self.send_msg(&LocalStateQueryMessage::MsgFailure { reason })
            .await
    }

    /// Wait for the next request from the client while a snapshot is acquired.
    ///
    /// Returns a query payload (`Query`), a release signal (`Release`), or a
    /// re-acquire target (`ReAcquire`).
    ///
    /// Must be called when the server is in `StAcquired`.
    pub async fn recv_acquired_request(
        &mut self,
    ) -> Result<LocalStateQueryAcquiredRequest, LocalStateQueryServerError> {
        match self.recv_msg().await? {
            LocalStateQueryMessage::MsgQuery { query } => {
                Ok(LocalStateQueryAcquiredRequest::Query(query))
            }
            LocalStateQueryMessage::MsgRelease => Ok(LocalStateQueryAcquiredRequest::Release),
            LocalStateQueryMessage::MsgReAcquire { target } => {
                Ok(LocalStateQueryAcquiredRequest::ReAcquire(target))
            }
            msg => Err(LocalStateQueryServerError::UnexpectedMessage(format!(
                "{msg:?}"
            ))),
        }
    }

    /// Send a query result to the client.
    ///
    /// Sends `MsgResult(result_bytes)` and transitions back to `StAcquired`.
    /// Must be called when the server is in `StQuerying`.
    pub async fn send_result(&mut self, result: Vec<u8>) -> Result<(), LocalStateQueryServerError> {
        self.send_msg(&LocalStateQueryMessage::MsgResult { result })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_lsq_server_connection_closed() {
        let s = format!("{}", LocalStateQueryServerError::ConnectionClosed);
        assert!(s.to_lowercase().contains("connection closed"));
    }

    #[test]
    fn display_lsq_server_decode_propagates_inner() {
        let e = LocalStateQueryServerError::Decode("acquire-point CBOR malformed".into());
        let s = format!("{e}");
        assert!(s.contains("CBOR decode"));
        assert!(s.contains("acquire-point CBOR malformed"));
    }

    #[test]
    fn display_lsq_server_unexpected_message_propagates_inner() {
        let e = LocalStateQueryServerError::UnexpectedMessage("MsgQuery in StIdle".into());
        let s = format!("{e}");
        assert!(s.contains("unexpected message"));
        assert!(s.contains("MsgQuery in StIdle"));
    }
}
