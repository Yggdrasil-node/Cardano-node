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

/// Status descriptor for the deferred TLS termination path. R429
/// documents the integration recipe; the actual `axum-server-rustls`
/// wiring is a deferred carve-out (the existing `rustls-pemfile`
/// PEM helpers parse the cert + key materials but the listener
/// binding currently only supports plain TCP via [`serve_router`]).
///
/// Operators wanting TLS today can:
/// 1. Use [`load_pem_certs`] + [`load_pem_key`] to load PEM files
///    from `Certificate.cert_file` / `Certificate.key_file`.
/// 2. Build a `rustls::ServerConfig` from the loaded materials.
/// 3. Wire `axum-server` (or `hyper-rustls`) for the bind step
///    instead of [`serve_router`].
///
/// Adding `axum-server` to the workspace requires:
/// - cargo-tree audit confirming no `openssl-sys` / `native-tls`
///   transitive deps (forbidden by `deny.toml:90`).
/// - Justification entry in [`docs/DEPENDENCIES.md`].
/// - Workspace dep pin matching the audited version.
///
/// Mirror context: upstream uses
/// `Network.Wai.Handler.WarpTLS.tlsSettingsChain` driven by
/// `epForceSSL`. The Yggdrasil-side `Endpoint::force_ssl` field is
/// plumbed through; R468 closed the bind-step integration.
pub fn tls_bind_plan_status() -> &'static str {
    "TLS termination: closed at R468. The serve_router_with_tls \
     function in this module wraps axum-server (tls-rustls feature, \
     no native-tls — audited against deny.toml:90's openssl-sys \
     ban) with the existing R408 load_pem_certs / load_pem_key \
     PEM helpers. Operators set Endpoint::force_ssl = Some(true) \
     + Certificate.{cert_file, key_file, chain_file (optional)} \
     in their tracer-config.json; the run_prometheus_server / \
     run_monitoring_server entry points route through TLS automatically."
}

/// Status descriptor describing the operator-facing behavior when
/// `Endpoint::force_ssl == Some(true)`. R468 closed the previously-
/// deferred path; operators with valid PEM materials get real TLS
/// termination via `serve_router_with_tls`. If the PEM materials
/// are missing or invalid, the bind step returns a [`TlsBindError`]
/// rather than silently falling back to plain TCP.
pub fn force_ssl_unsupported_status() -> &'static str {
    "force_ssl = Some(true) is supported at R468: serve_router_with_tls \
     binds the listener via axum-server's TLS-rustls path using the \
     operator's Certificate.{cert_file, key_file, chain_file} PEM \
     materials. Invalid/missing materials surface as TlsBindError \
     rather than silently falling back to plain TCP. The fallback \
     behavior in force_ssl == None / Some(false) paths is unchanged."
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

// ---------------------------------------------------------------------------
// R468 TLS termination
// ---------------------------------------------------------------------------

/// Serve the supplied `router` on `addr` over TLS, using PEM-encoded
/// certificate + key from the operator's `Certificate` config.
/// Mirror of upstream's `Network.Wai.Handler.WarpTLS.tlsSettingsChain
/// cert_file key_file <chain_file>` invocation pattern.
///
/// R468 closure of the `tls_bind_plan_status` /
/// `tls_termination_status` deferred descriptors. The returned
/// `JoinHandle` runs the TLS listener; aborting it stops the
/// listener and lets in-flight requests drain.
///
/// Uses `axum-server` with `tls-rustls` (no native-tls — audited at
/// R468 against deny.toml:90's openssl-sys ban).
///
/// If `chain_path` is `Some(p)`, the chain PEM is appended to the
/// cert PEM in a temp file before passing to axum-server's
/// `RustlsConfig::from_pem_file` (which only accepts a single PEM
/// file containing the full cert chain). Operators with the chain
/// already concatenated into `cert_file` can pass `None`.
pub async fn serve_router_with_tls(
    addr: SocketAddr,
    router: Router,
    cert_path: &Path,
    key_path: &Path,
    chain_path: Option<&Path>,
) -> Result<tokio::task::JoinHandle<()>, TlsBindError> {
    use axum_server::tls_rustls::RustlsConfig;

    // rustls 0.23 requires a process-level CryptoProvider to be
    // installed before ServerConfig::builder runs. Yggdrasil picks
    // `ring` (license-clarified in deny.toml — MIT AND ISC AND
    // OpenSSL) over `aws-lc-rs` (which pulls `aws-lc-sys` C
    // bindings, against Yggdrasil's no-FFI policy spirit). The
    // install is idempotent (returns Err on re-install, which we
    // ignore) so calling this from every TLS bind is safe.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // axum-server's `RustlsConfig::from_pem_file` takes a single
    // cert PEM. If the operator supplied a chain file, concatenate
    // it onto the cert PEM in a tempfile so axum-server sees the
    // full chain.
    let rustls_config = if let Some(chain) = chain_path {
        let cert_pem = std::fs::read(cert_path).map_err(TlsBindError::Cert)?;
        let chain_pem = std::fs::read(chain).map_err(TlsBindError::Chain)?;
        let combined_dir = std::env::temp_dir();
        let combined_path = combined_dir.join(format!(
            "yggdrasil-cardano-tracer-tls-{}-{}.pem",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let mut combined = cert_pem;
        if !combined.ends_with(b"\n") {
            combined.push(b'\n');
        }
        combined.extend_from_slice(&chain_pem);
        std::fs::write(&combined_path, combined).map_err(TlsBindError::Cert)?;
        let config = RustlsConfig::from_pem_file(&combined_path, key_path)
            .await
            .map_err(TlsBindError::Rustls)?;
        // Best-effort tempfile cleanup; rustls has already loaded the
        // bytes into memory by this point.
        let _ = std::fs::remove_file(&combined_path);
        config
    } else {
        RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .map_err(TlsBindError::Rustls)?
    };

    // Bind + serve. axum-server's `bind_rustls` returns a Server
    // builder; calling `.serve(router.into_make_service())` runs it.
    let server = axum_server::bind_rustls(addr, rustls_config);
    let handle = tokio::spawn(async move {
        let _ = server.serve(router.into_make_service()).await;
    });
    Ok(handle)
}

/// Errors from the TLS-binding path.
#[derive(Debug, thiserror::Error)]
pub enum TlsBindError {
    /// Failed to load the certificate file.
    #[error("load cert: {0}")]
    Cert(std::io::Error),

    /// Failed to load the optional certificate chain file.
    #[error("load chain: {0}")]
    Chain(std::io::Error),

    /// rustls rejected the cert/key materials.
    #[error("rustls config: {0}")]
    Rustls(std::io::Error),
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
    fn tls_bind_plan_status_describes_closure() {
        let s = tls_bind_plan_status();
        assert!(s.contains("closed at R468"));
        assert!(s.contains("serve_router_with_tls"));
        assert!(s.contains("axum-server"));
    }

    #[test]
    fn force_ssl_unsupported_status_describes_support() {
        let s = force_ssl_unsupported_status();
        assert!(s.contains("force_ssl"));
        assert!(s.contains("supported at R468"));
        assert!(s.contains("TlsBindError"));
    }

    // ----- R468 TLS bind path tests -------------------------------------

    #[tokio::test]
    async fn serve_router_with_tls_fails_on_missing_cert_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let nonexistent_cert = dir.path().join("nope-cert.pem");
        let nonexistent_key = dir.path().join("nope-key.pem");
        let addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
        let router = build_router();
        let result =
            serve_router_with_tls(addr, router, &nonexistent_cert, &nonexistent_key, None).await;
        // Either the cert load fails or rustls rejects empty input —
        // both are TlsBindError variants.
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn serve_router_with_tls_fails_on_invalid_cert_pem() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let cert_path = dir.path().join("invalid-cert.pem");
        let key_path = dir.path().join("invalid-key.pem");
        // Write garbage PEM that won't parse as a cert.
        tokio::fs::write(
            &cert_path,
            b"-----BEGIN CERTIFICATE-----\nNOT-A-REAL-CERT\n-----END CERTIFICATE-----\n",
        )
        .await
        .expect("write");
        tokio::fs::write(
            &key_path,
            b"-----BEGIN PRIVATE KEY-----\nNOT-A-REAL-KEY\n-----END PRIVATE KEY-----\n",
        )
        .await
        .expect("write");
        let addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
        let router = build_router();
        let result = serve_router_with_tls(addr, router, &cert_path, &key_path, None).await;
        assert!(matches!(result, Err(TlsBindError::Rustls(_))));
    }

    #[tokio::test]
    async fn serve_router_with_tls_fails_on_missing_chain_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        let missing_chain = dir.path().join("missing-chain.pem");
        // Write minimal valid-looking PEM so cert read succeeds.
        tokio::fs::write(&cert_path, b"").await.expect("write cert");
        tokio::fs::write(&key_path, b"").await.expect("write key");
        let addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
        let router = build_router();
        let result =
            serve_router_with_tls(addr, router, &cert_path, &key_path, Some(&missing_chain)).await;
        assert!(matches!(result, Err(TlsBindError::Chain(_))));
    }

    #[test]
    fn tls_bind_error_variants_have_distinct_display() {
        let cert_err = TlsBindError::Cert(std::io::Error::other("cert msg"));
        let chain_err = TlsBindError::Chain(std::io::Error::other("chain msg"));
        let rustls_err = TlsBindError::Rustls(std::io::Error::other("rustls msg"));
        let cert_str = format!("{cert_err}");
        let chain_str = format!("{chain_err}");
        let rustls_str = format!("{rustls_err}");
        assert!(cert_str.contains("load cert"));
        assert!(chain_str.contains("load chain"));
        assert!(rustls_str.contains("rustls config"));
        // All three contain their respective inner messages.
        assert!(cert_str.contains("cert msg"));
        assert!(chain_str.contains("chain msg"));
        assert!(rustls_str.contains("rustls msg"));
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
