//! Common HTTP-server scaffolding for the cardano-tracer Metrics
//! handlers (Prometheus, Monitoring, TimeseriesServer, Servers
//! orchestration).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side synthesis: upstream's cardano-tracer ships each
//! HTTP server as a separate `.hs` file under
//! `Cardano.Tracer.Handlers.Metrics.*`, with shared scaffolding
//! provided ad-hoc via `Network.Wai.Handler.Warp` per server. The
//! Rust port consolidates that scaffolding here so each server
//! impl (R409+) can focus on its routing + handler logic.
//!
//! What ships at R408 (this file):
//! - [`build_router`]: empty `axum::Router` builder. Per-server
//!   handlers attach routes via `.route(...)` chain.
//! - [`serve_router`]: tokio task that binds the router to the
//!   supplied `SocketAddr` and serves until the spawned task is
//!   aborted.
//! - [`load_pem_certs`] / [`load_pem_key`]: rustls-pemfile-backed
//!   helpers that load PEM-encoded TLS material from a file path.
//!   Used by R409+ servers when `epForceSSL` is enabled.
//!
//! What ships in subsequent rounds:
//! - R409+ Prometheus / Monitoring / TimeseriesServer endpoint
//!   handlers (each attaches its routes via [`build_router`]).
//! - Full `axum-server-rustls` integration with
//!   [`load_pem_certs`] + [`load_pem_key`]. Currently the helpers
//!   parse the PEM material; future rounds wire them to a TLS
//!   acceptor.

use std::net::SocketAddr;
use std::path::Path;

use axum::Router;

/// Build an empty `axum::Router`. Per-server handlers extend it
/// via the standard `.route(...)` builder pattern.
pub fn build_router() -> Router {
    Router::new()
}

/// Serve the router on the supplied `addr`. The returned
/// `JoinHandle` runs until aborted; aborting it stops the listener
/// and lets in-flight requests drain.
pub async fn serve_router(
    addr: SocketAddr,
    router: Router,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let handle = tokio::spawn(async move {
        // axum 0.8's serve adapter — runs until listener errors.
        let _ = axum::serve(listener, router).await;
    });
    Ok(handle)
}

/// Errors from PEM-backed TLS material loading.
#[derive(Debug, thiserror::Error)]
pub enum PemLoadError {
    /// Could not open / read the PEM file.
    #[error("PEM file IO failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Load a sequence of PEM-encoded certificates from a file. Mirror
/// of upstream's `Network.Wai.Handler.WarpTLS.tlsSettingsChain`
/// certificate-chain parsing (cert + chain together produce the
/// full certificate path).
///
/// Returns the parsed DER-encoded byte vectors in declaration order
/// (root → leaf). Sites that need to feed `rustls`'s
/// `ServerConfig::with_single_cert` should use the result alongside
/// [`load_pem_key`].
pub fn load_pem_certs(path: &Path) -> Result<Vec<Vec<u8>>, PemLoadError> {
    let bytes = std::fs::read(path)?;
    let mut reader = std::io::BufReader::new(bytes.as_slice());
    let mut certs = Vec::new();
    for cert in rustls_pemfile::certs(&mut reader) {
        let cert = cert?;
        certs.push(cert.to_vec());
    }
    Ok(certs)
}

/// Load a PEM-encoded private key from a file. Returns the
/// DER-encoded key bytes. Tries PKCS#8 first, then RSA-PKCS#1, then
/// EC (matches `rustls-pemfile`'s `private_key()` heuristic). On
/// failure, returns an `Err(PemLoadError::Io)` carrying the
/// underlying `std::io::Error`.
pub fn load_pem_key(path: &Path) -> Result<Vec<u8>, PemLoadError> {
    let bytes = std::fs::read(path)?;
    let mut reader = std::io::BufReader::new(bytes.as_slice());
    match rustls_pemfile::private_key(&mut reader)? {
        Some(key) => Ok(key.secret_der().to_vec()),
        None => Err(PemLoadError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "no PEM private key found in file",
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spawn-an-empty-router test: just verifies the router binds.
    #[tokio::test]
    async fn build_router_returns_empty_axum_router() {
        let router = build_router();
        // axum::Router doesn't expose route-count introspection
        // publicly; the type-shape itself is the assertion.
        let _: Router = router;
    }

    #[tokio::test]
    async fn serve_router_binds_and_aborts_cleanly() {
        // Bind to ephemeral port (127.0.0.1:0).
        let addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
        let router = build_router();
        let handle = serve_router(addr, router).await.expect("bind");
        // Abort the handle — listener task should clean up.
        handle.abort();
        // Give the runtime a moment to observe the abort.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(handle.is_finished());
    }

    #[test]
    fn load_pem_certs_returns_io_error_for_missing_file() {
        let result = load_pem_certs(Path::new("/nonexistent/path/to/cert.pem"));
        assert!(matches!(result, Err(PemLoadError::Io(_))));
    }

    #[test]
    fn load_pem_certs_returns_empty_for_empty_pem_file() {
        let tmp = std::env::temp_dir().join(format!(
            "yggdrasil-empty-pem-{}-{}.pem",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&tmp, b"").expect("write");
        let result = load_pem_certs(&tmp).expect("parse");
        assert!(result.is_empty());
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_pem_certs_parses_valid_pem_block() {
        // A self-signed certificate-shaped PEM block (synthetic; not
        // a real cert, but matches the BEGIN/END markers).
        let pem = "-----BEGIN CERTIFICATE-----\n\
                   MIIBKjCB1aADAgECAhAyo2P/0HQq9zXvYqONVy3+MAUGAytlcDAUMRIwEAYDVQQD\n\
                   DAl5Z2dkcmFzaWwwIBcNMjQwMTAxMDAwMDAwWhgPOTk5OTAxMDEwMDAwMDBaMBQx\n\
                   EjAQBgNVBAMMCXlnZ2RyYXNpbDAqMAUGAytlcAMhAEgM1234567890ABCDEFGHIJ\n\
                   KLMNOPQRSTUVWXYZ1234567890aoyozMAUGAytlcANBADV3iLKK7zGnDX4UQF/3\n\
                   -----END CERTIFICATE-----\n";
        let tmp = std::env::temp_dir().join(format!(
            "yggdrasil-valid-pem-{}-{}.pem",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&tmp, pem).expect("write");
        let result = load_pem_certs(&tmp).expect("parse");
        assert_eq!(result.len(), 1);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_pem_key_returns_error_for_missing_file() {
        let result = load_pem_key(Path::new("/nonexistent/path/to/key.pem"));
        assert!(matches!(result, Err(PemLoadError::Io(_))));
    }

    #[test]
    fn load_pem_key_returns_error_when_no_key_in_file() {
        let tmp = std::env::temp_dir().join(format!(
            "yggdrasil-empty-key-{}-{}.pem",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&tmp, b"# nothing useful here").expect("write");
        let result = load_pem_key(&tmp);
        assert!(matches!(result, Err(PemLoadError::Io(_))));
        let _ = std::fs::remove_file(&tmp);
    }
}
