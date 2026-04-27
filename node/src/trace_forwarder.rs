//! TraceForwarder: forwards trace events to a Unix domain socket as CBOR.
//!
//! Compatible with cardano-tracer (Haskell) Forwarder backend.

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
