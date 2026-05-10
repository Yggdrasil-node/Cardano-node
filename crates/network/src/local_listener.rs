//! Unix-pipe listener for inbound trace-forwarder connections.
//!
//! Wraps a bound [`tokio::net::UnixListener`] and exposes:
//!
//! * [`LocalPeerListener::accept_unix`] — accept the next inbound
//!   Unix-domain-socket connection without performing any
//!   protocol-level work. Cheap and never blocks on a misbehaving
//!   peer's data; the trace-forwarder handshake gets layered on top
//!   in subsequent rounds (R420+ `Acceptors/Server.hs`).
//! * Path-based addressing with stale-socket cleanup on bind
//!   (mirrors `node/src/local_server/accept.rs`'s pattern) and
//!   socket-file removal on drop.
//! * `chmod 0o660` permission gate so a non-root user on a multi-
//!   tenant host cannot speak the trace-forward protocol against
//!   a tracer running as a privileged user.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side Unix-domain-socket
//! listener foundation for the trace-forwarder Acceptors mini-arc
//! (R416-R424). Mirrors the operational shape of
//! `Ouroboros.Network.Snocket.localSnocket` +
//! `Ouroboros.Network.Server.Simple.with` (used by upstream
//! `Cardano.Tracer.Acceptors.Server::doListenToForwarderLocal`,
//! Server.hs:114-143) collapsed to a `LocalPeerListener` pair
//! (`bind` + `accept_unix`). Upstream's `Server.with` additionally
//! threads `HandshakeArguments` + `OuroborosApplicationWithMinimalCtx`
//! through the snocket — Yggdrasil splits those concerns into a
//! handshake-on-stream surface that lands in R420+.
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                              |
//! |---------------------------------------------------------|----------------------------------------|
//! | `localSnocket :: IOManager -> Snocket IO LocalSocket _`  | (collapsed — Yggdrasil uses tokio's `UnixListener` directly without an IO-manager indirection) |
//! | `localAddressFromPath :: FilePath -> LocalAddress`       | `LocalPeerListener::bind(path)`        |
//! | `Server.with` (snocket + bearer + handshake + app)       | (split: bind here; handshake in R420+; app in R421+) |
//! | per-bind `removeFile` on stale socket                    | [`LocalPeerListener::bind`] preflight  |
//! | `Server` blocks until async exception                    | [`LocalPeerListener::accept_unix`] returns one connection per call (caller owns the loop, matching `listener.rs`'s `accept_tcp`) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Snocket` typeclass abstraction**: upstream's `Snocket` is a
//!   record-of-functions IO-manager indirection (`createSocket`,
//!   `bind`, `accept`, `close`, etc.) that lets the simulator inject
//!   in-memory replacements during property tests. Yggdrasil's
//!   property tests use `tokio::net::UnixStream::pair()` directly
//!   (see `chainsync_client.rs::tests`), so the typeclass shim
//!   collapses.
//! - **Handshake threading via `HandshakeArguments`**: the
//!   trace-forwarder handshake codec lands on top of this listener
//!   in R420+. This module is intentionally limited to the Unix-
//!   pipe listener primitive so R417-R419 can wire the per-protocol
//!   responder paths against a stable foundation.

use std::path::{Path, PathBuf};
use tokio::net::{UnixListener, UnixStream};

/// Default Unix-socket permission applied at bind. `0o660` allows
/// owner + group read/write but blocks world access — same gate
/// applied to the NtC socket in `node/src/local_server/accept.rs`.
pub const SOCKET_PERMISSIONS: u32 = 0o660;

// ---------------------------------------------------------------------------
// LocalPeerListener
// ---------------------------------------------------------------------------

/// A Unix-domain-socket listener that accepts inbound trace-forwarder
/// connections.
///
/// ```text
/// bind(path) → LocalPeerListener
///   ↓
/// accept_unix() → UnixStream                ← cheap, returns immediately
///   ↓                                         on inbound connect
/// (rate-limit / tracer-env admission goes here)
///   ↓
/// trace-forwarder handshake (R420+)
/// ```
#[derive(Debug)]
pub struct LocalPeerListener {
    listener: UnixListener,
    path: PathBuf,
}

impl LocalPeerListener {
    /// Bind a Unix-domain-socket listener at the given path.
    ///
    /// Removes any stale socket file at `path` before binding so
    /// clean restarts succeed. Sets the socket file's permissions to
    /// [`SOCKET_PERMISSIONS`] (`0o660`) so a non-root user on a
    /// multi-tenant host cannot speak trace-forward against a
    /// tracer running as a privileged user.
    pub async fn bind(path: impl AsRef<Path>) -> Result<Self, LocalPeerListenerError> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        let listener = UnixListener::bind(&path).map_err(|e| LocalPeerListenerError::Bind {
            path: path.clone(),
            source: e,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(SOCKET_PERMISSIONS))
                .map_err(|e| LocalPeerListenerError::SetPermissions {
                    path: path.clone(),
                    source: e,
                })?;
        }
        Ok(Self { listener, path })
    }

    /// Construct a listener from an already-bound `UnixListener`
    /// without performing any preflight cleanup or permission set.
    /// Caller retains responsibility for the path's lifecycle.
    pub fn from_listener(listener: UnixListener, path: PathBuf) -> Self {
        Self { listener, path }
    }

    /// Returns the filesystem path this listener is bound to.
    pub fn local_path(&self) -> &Path {
        &self.path
    }

    /// Accept the next inbound Unix-domain-socket connection without
    /// performing the trace-forwarder handshake.
    ///
    /// This is the appropriate primitive for an accept loop that
    /// wants to enforce admission control or per-tenant rate
    /// limiting *before* spending CPU and memory on handshake
    /// decoding. The trace-forwarder handshake codec lands in R420+.
    pub async fn accept_unix(&self) -> Result<UnixStream, LocalPeerListenerError> {
        let (stream, _addr) = self
            .listener
            .accept()
            .await
            .map_err(LocalPeerListenerError::Accept)?;
        Ok(stream)
    }
}

impl Drop for LocalPeerListener {
    fn drop(&mut self) {
        // Best-effort socket-file removal on shutdown so a subsequent
        // bind on the same path succeeds without manual cleanup.
        // Mirrors upstream's `Server.with` bracket pattern that
        // releases the snocket on async-exception unwind.
        let _ = std::fs::remove_file(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the Unix-pipe peer listener.
#[derive(Debug, thiserror::Error)]
pub enum LocalPeerListenerError {
    /// Failed to bind the Unix-domain-socket listener.
    #[error("bind error on {path:?}: {source}")]
    Bind {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to set permissions on the bound socket file.
    #[error("set-permissions error on {path:?}: {source}")]
    SetPermissions {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to accept a Unix-domain-socket connection.
    #[error("accept error: {0}")]
    Accept(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn temp_socket_path(label: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join(format!("{label}.sock"));
        (dir, path)
    }

    #[tokio::test]
    async fn bind_creates_socket_file() {
        let (_dir, path) = temp_socket_path("bind_creates");
        let listener = LocalPeerListener::bind(&path).await.expect("bind");
        assert!(path.exists(), "socket file should exist after bind");
        assert_eq!(listener.local_path(), path.as_path());
    }

    #[tokio::test]
    async fn bind_removes_stale_socket() {
        let (_dir, path) = temp_socket_path("stale");
        // Pre-create a stale file at the target path; bind should
        // remove it transparently rather than failing.
        std::fs::write(&path, b"stale-leftover").expect("seed stale");
        let _listener = LocalPeerListener::bind(&path).await.expect("bind");
        assert!(path.exists(), "socket file should be present after bind");
    }

    #[tokio::test]
    async fn bind_sets_socket_permissions_0o660() {
        let (_dir, path) = temp_socket_path("perms");
        let _listener = LocalPeerListener::bind(&path).await.expect("bind");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        // Strip the file-type bits; only compare the access bits.
        assert_eq!(mode & 0o777, SOCKET_PERMISSIONS);
    }

    #[tokio::test]
    async fn accept_unix_returns_connected_stream() {
        let (_dir, path) = temp_socket_path("accept");
        let listener = LocalPeerListener::bind(&path).await.expect("bind");

        // Spawn a client that connects to the listener.
        let connect_path = path.clone();
        let client_task = tokio::spawn(async move {
            tokio::net::UnixStream::connect(&connect_path)
                .await
                .expect("client connect")
        });

        let server_stream = listener.accept_unix().await.expect("accept");
        let _client_stream = client_task.await.expect("client task");
        // Both ends are connected — readiness sanity check.
        assert!(server_stream.peer_addr().is_ok());
    }

    #[tokio::test]
    async fn drop_removes_socket_file() {
        let (_dir, path) = temp_socket_path("drop");
        {
            let _listener = LocalPeerListener::bind(&path).await.expect("bind");
            assert!(path.exists(), "socket present mid-scope");
        }
        // Drop ran when the scope ended.
        assert!(
            !path.exists(),
            "socket file should be removed when listener drops"
        );
    }

    #[tokio::test]
    async fn from_listener_round_trip() {
        let (_dir, path) = temp_socket_path("from");
        let raw = UnixListener::bind(&path).expect("raw bind");
        let listener = LocalPeerListener::from_listener(raw, path.clone());
        assert_eq!(listener.local_path(), path.as_path());
    }

    #[test]
    fn socket_permissions_constant_is_0o660() {
        // Lock down the constant value — operators rely on this for
        // multi-tenant host hardening (audit M-3 parallel).
        assert_eq!(SOCKET_PERMISSIONS, 0o660);
    }

    #[tokio::test]
    async fn bind_error_carries_path() {
        // Bind to a path inside a non-existent directory to provoke
        // a real filesystem error and verify the error carries the
        // offending path for operator diagnosis.
        let bad = PathBuf::from("/nonexistent-dir-yggdrasil-r416/x.sock");
        let err = LocalPeerListener::bind(&bad).await.expect_err("bind err");
        match err {
            LocalPeerListenerError::Bind { path, .. } => assert_eq!(path, bad),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
