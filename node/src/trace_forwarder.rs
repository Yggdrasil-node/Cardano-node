//! Lightweight Unix-domain CBOR egress used by the `Forwarder` trace
//! backend.
//!
//! # Parity status
//!
//! This is a **stub**, NOT a faithful reimplementation of upstream
//! `cardano-tracer`'s Forwarder backend.  Upstream
//! `Cardano.Logging.Forwarding` runs two typed mini-protocols
//! (`Trace.Forward.Protocol.TraceObject.Type` and
//! `Trace.Forward.Protocol.DataPoint.Type`) multiplexed by `Network.Mux`
//! over an `AF_UNIX` `SOCK_STREAM` socket.  Each protocol is a
//! request/response state machine (`MsgRequest` / `MsgReply` /
//! `MsgDone`) with explicit version handshake — see
//! `cardano-node:trace-dispatcher/src/Cardano/Logging/Forwarding.hs`
//! and the `trace-forward` package on Hackage.
//!
//! The current implementation:
//! - Uses an `AF_UNIX` `SOCK_DGRAM` socket (wrong type).
//! - Sends bare CBOR-encoded JSON payloads with no SDU framing,
//!   handshake, or message tags.
//! - Has no acknowledgement / backpressure path.
//!
//! Consequence: a real `cardano-tracer` will reject the wire format at
//! transport level; configuring the `Forwarder` backend in
//! `TraceOptions` therefore silently drops events on the floor when no
//! Yggdrasil-aware listener is bound to the configured socket.  Plain
//! stdout backends (`Stdout HumanFormatColoured`, `Stdout HumanFormat`,
//! `StdoutMachine`) are unaffected and remain the operational default.
//!
//! Tracked as a parity gap; a follow-up will replace this module with a
//! mux-framed `TraceObject` + `DataPoint` worker built on top of
//! `yggdrasil-network`'s SDU framing.

use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug)]
pub struct TraceForwarder {
    socket_path: String,
    socket: Mutex<Option<UnixDatagram>>,
}

impl TraceForwarder {
    pub fn new(socket_path: String) -> Self {
        Self {
            socket_path,
            socket: Mutex::new(None),
        }
    }

    /// Returns the configured Unix-socket path.  Used by the runtime to
    /// emit a one-shot parity-gap warning at startup so operators are
    /// not surprised by silently-dropped trace events.
    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    pub fn send(&self, event: &serde_json::Value) {
        // CBOR encoding via ciborium (RFC 8949). Replaces unmaintained
        // serde_cbor (RUSTSEC-2021-0127). Audit finding M-4.
        let mut encoded = Vec::new();
        if ciborium::ser::into_writer(event, &mut encoded).is_err() {
            return;
        }
        let mut sock_guard = self
            .socket
            .lock()
            .expect("trace forwarder socket mutex poisoned");
        if sock_guard.is_none() {
            let sock = UnixDatagram::unbound().ok();
            if let Some(ref s) = sock {
                let _ = s.connect(Path::new(&self.socket_path));
            }
            *sock_guard = sock;
        }
        if let Some(ref sock) = *sock_guard {
            let _ = sock.send(&encoded);
        }
    }
}
