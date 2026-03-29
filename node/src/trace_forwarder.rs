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
        let encoded = match serde_cbor::to_vec(event) {
            Ok(bytes) => bytes,
            Err(_) => return,
        };
        let mut sock_guard = self.socket.lock().unwrap();
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
