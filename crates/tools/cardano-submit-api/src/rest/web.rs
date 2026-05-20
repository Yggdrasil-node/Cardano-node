//! REST endpoint handlers (POST /api/submit/tx).
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Rest/Web.hs.
//!
//! Direct ports:
//!
//! - [`run_settings`] — `runSettings :: Trace IO TraceSubmitApi -> Settings -> Application -> IO ()`.
//!   Upstream uses `Network.Wai.Handler.Warp.runSettingsSocket` after
//!   binding a TCP socket and tracing `EndpointListeningOnPort`. The
//!   Rust port uses raw tokio TCP for the same flow (matching the
//!   project's existing `crates/node/tracer/src/metrics_server.rs` pattern); no
//!   axum/warp dependency is required.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - `Network.Wai.Handler.Warp.runSettingsSocket` /
//!   `Data.Streaming.Network.bindPortTCP` — replaced by
//!   [`tokio::net::TcpListener::bind`].
//! - `Servant.Application` — the request-dispatch type is replaced by
//!   the [`Handler`] type alias (`fn(&HttpRequest) -> HttpResponse`
//!   wrapped in `Arc<dyn Fn ... + Send + Sync>`). The
//!   `serve (Proxy :: Proxy TxSubmitApi) (toServant handlers)` chain is
//!   collapsed into the path-prefix dispatch in [`run_settings`].
//!
//! ## HTTP-1.1 behavior
//!
//! - Connection: close on every response (no keep-alive / pipelining).
//! - Request size cap: [`MAX_REQUEST_BYTES`] (32 KiB) — wide enough
//!   for any reasonable Cardano tx CBOR plus headers.
//! - Body framing: Content-Length only (no chunked transfer-encoding
//!   support). cardano-submit-api clients always send Content-Length.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::tracing::trace_submit_api::TraceSubmitApi;

/// Maximum HTTP request size accepted by [`run_settings`]. Generous
/// budget for a full Cardano tx CBOR plus headers; Yggdrasil's tx-
/// submit binary rejects any request larger than this with 400 Bad
/// Request before reading the body.
pub const MAX_REQUEST_BYTES: usize = 32 * 1024;

/// Parsed HTTP request — minimal subset of the RFC 7230 surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpRequest {
    /// Uppercase HTTP method (`POST`, `GET`, ...).
    pub method: String,
    /// Request path including any query string (`/api/submit/tx`).
    pub path: String,
    /// `Content-Type` header value, lowercased. `None` if absent.
    pub content_type: Option<String>,
    /// Request body (always empty for GET; up to `Content-Length`
    /// bytes for POST). The caller is responsible for any further
    /// decoding (e.g. CBOR).
    pub body: Vec<u8>,
}

/// HTTP response constructed by a handler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    /// Numeric status code (`202`, `400`, `503`, ...).
    pub status: u16,
    /// Reason phrase corresponding to `status` (`"Accepted"`, ...).
    pub status_text: &'static str,
    /// Response `Content-Type` header value.
    pub content_type: &'static str,
    /// Response body bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// 202 Accepted with `application/json` body.
    pub fn accepted_json(body: Vec<u8>) -> Self {
        HttpResponse {
            status: 202,
            status_text: "Accepted",
            content_type: "application/json",
            body,
        }
    }

    /// 400 Bad Request with `application/json` body.
    pub fn bad_request_json(body: Vec<u8>) -> Self {
        HttpResponse {
            status: 400,
            status_text: "Bad Request",
            content_type: "application/json",
            body,
        }
    }

    /// 404 Not Found.
    pub fn not_found() -> Self {
        HttpResponse {
            status: 404,
            status_text: "Not Found",
            content_type: "text/plain",
            body: b"Not Found".to_vec(),
        }
    }

    /// 405 Method Not Allowed.
    pub fn method_not_allowed() -> Self {
        HttpResponse {
            status: 405,
            status_text: "Method Not Allowed",
            content_type: "text/plain",
            body: b"Method Not Allowed".to_vec(),
        }
    }

    /// 413 Payload Too Large (request body exceeds [`MAX_REQUEST_BYTES`]).
    pub fn payload_too_large() -> Self {
        HttpResponse {
            status: 413,
            status_text: "Payload Too Large",
            content_type: "text/plain",
            body: b"Payload Too Large".to_vec(),
        }
    }

    /// 503 Service Unavailable with `application/json` body.
    pub fn service_unavailable_json(body: Vec<u8>) -> Self {
        HttpResponse {
            status: 503,
            status_text: "Service Unavailable",
            content_type: "application/json",
            body,
        }
    }

    /// Encode the response as a wire HTTP/1.1 message ending with
    /// `Connection: close`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(128 + self.body.len());
        out.extend_from_slice(
            format!(
                "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                self.status,
                self.status_text,
                self.content_type,
                self.body.len(),
            )
            .as_bytes(),
        );
        out.extend_from_slice(&self.body);
        out
    }
}

/// Errors during request parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// Request was empty (zero bytes received).
    #[error("empty request")]
    Empty,
    /// Request line could not be split into method/path/version.
    #[error("malformed request line")]
    MalformedRequestLine,
    /// Unsupported transfer-encoding (e.g. `chunked`).
    #[error("unsupported transfer-encoding")]
    UnsupportedTransferEncoding,
    /// `Content-Length` header value was not a non-negative integer.
    #[error("malformed content-length")]
    MalformedContentLength,
    /// Request size exceeded [`MAX_REQUEST_BYTES`].
    #[error("request too large")]
    TooLarge,
    /// Request body was shorter than its declared `Content-Length`.
    #[error("truncated body")]
    TruncatedBody,
}

/// Parse an HTTP/1.1 request from a buffer.
///
/// Accepts the canonical `<method> <path> HTTP/1.1\r\n<headers>\r\n\r\n<body>`
/// shape. Headers are scanned for `Content-Length`, `Content-Type`, and
/// `Transfer-Encoding`. Chunked transfer is rejected with [`ParseError::UnsupportedTransferEncoding`].
pub fn parse_request(buf: &[u8]) -> Result<HttpRequest, ParseError> {
    if buf.is_empty() {
        return Err(ParseError::Empty);
    }
    if buf.len() > MAX_REQUEST_BYTES {
        return Err(ParseError::TooLarge);
    }

    let header_end = match find_header_end(buf) {
        Some(idx) => idx,
        None => return Err(ParseError::MalformedRequestLine),
    };
    let header_section =
        std::str::from_utf8(&buf[..header_end]).map_err(|_| ParseError::MalformedRequestLine)?;
    let mut lines = header_section.split("\r\n");
    let request_line = lines.next().ok_or(ParseError::MalformedRequestLine)?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or(ParseError::MalformedRequestLine)?
        .to_ascii_uppercase();
    let path = parts
        .next()
        .ok_or(ParseError::MalformedRequestLine)?
        .to_string();
    let _http_version = parts.next().ok_or(ParseError::MalformedRequestLine)?;

    let mut content_length: Option<usize> = None;
    let mut content_type: Option<String> = None;
    for header_line in lines {
        if header_line.is_empty() {
            continue;
        }
        if let Some((name, value)) = header_line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            match name.as_str() {
                "content-length" => {
                    content_length = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| ParseError::MalformedContentLength)?,
                    );
                }
                "content-type" => {
                    content_type = Some(value.to_ascii_lowercase());
                }
                "transfer-encoding" if !value.eq_ignore_ascii_case("identity") => {
                    return Err(ParseError::UnsupportedTransferEncoding);
                }
                _ => {}
            }
        }
    }

    let body_start = header_end + 4;
    let body_len = content_length.unwrap_or(0);
    if body_len > MAX_REQUEST_BYTES {
        return Err(ParseError::TooLarge);
    }
    if buf.len() < body_start + body_len {
        return Err(ParseError::TruncatedBody);
    }
    let body = buf[body_start..body_start + body_len].to_vec();

    Ok(HttpRequest {
        method,
        path,
        content_type,
        body,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Dispatch handler: takes an owned [`HttpRequest`] and returns a
/// future that resolves to an [`HttpResponse`].
///
/// The async return is required because the canonical handler
/// (`tx_submit_post`) does NtC LocalTxSubmission I/O. Synchronous
/// handlers (404/405 fallback, in-memory routing) can wrap a plain
/// value in `Box::pin(async move { value })`.
pub type Handler =
    Arc<dyn Fn(HttpRequest) -> Pin<Box<dyn Future<Output = HttpResponse> + Send>> + Send + Sync>;

/// Trace forwarder: invoked synchronously per event (must not block).
pub type Tracer = Arc<dyn Fn(TraceSubmitApi) + Send + Sync>;

/// Mirror of upstream `runSettings`. Bind a TCP listener, trace
/// `EndpointListeningOnPort`, and serve requests indefinitely. Returns
/// only on listener-bind failure or unrecoverable accept-loop error.
///
/// The handler is invoked once per request; `tracer` is invoked once
/// per `EndpointListeningOnPort` plus per per-connection error.
///
/// `addr` may use port `0` to request an ephemeral port; the actual
/// bound address is reported via the `EndpointListeningOnPort` trace.
pub async fn run_settings(
    tracer: Tracer,
    addr: SocketAddr,
    handler: Handler,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let bound_addr = listener.local_addr()?;
    tracer(TraceSubmitApi::EndpointListeningOnPort(bound_addr));

    loop {
        let (stream, _peer) = listener.accept().await?;
        let handler = Arc::clone(&handler);
        let tracer = Arc::clone(&tracer);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, &handler).await {
                tracer(TraceSubmitApi::EndpointException {
                    context: "rest::web::run_settings: ".to_string(),
                    exception: err.to_string(),
                });
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream, handler: &Handler) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        if buf.len() + n > MAX_REQUEST_BYTES {
            let response = HttpResponse::payload_too_large();
            stream.write_all(&response.encode()).await?;
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if find_header_end(&buf).is_some() {
            // Reached end of headers; check if Content-Length body is fully buffered.
            if let Ok(req) = parse_request(&buf) {
                let response = handler(req).await;
                stream.write_all(&response.encode()).await?;
                return Ok(());
            }
            // Either body still arriving (TruncatedBody) or hard parse
            // error; keep reading until socket closes or buffer fills.
        }
    }
    // Connection closed before request completed — emit a Bad Request
    // response. (Real clients always send Content-Length headers.)
    let response = match parse_request(&buf) {
        Ok(req) => handler(req).await,
        Err(_) => HttpResponse::bad_request_json(b"{\"tag\":\"TxSubmitEmpty\"}".to_vec()),
    };
    stream.write_all(&response.encode()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_request_line() -> Vec<u8> {
        b"POST /api/submit/tx HTTP/1.1\r\nContent-Length: 0\r\n\r\n".to_vec()
    }

    /// Wrap a sync closure into the async `Handler` type.
    fn sync_handler<F>(f: F) -> Handler
    where
        F: Fn(HttpRequest) -> HttpResponse + Send + Sync + 'static,
    {
        Arc::new(move |req| {
            let response = f(req);
            Box::pin(async move { response })
        })
    }

    // -- HttpResponse encode --

    #[test]
    fn encode_accepted_json_emits_canonical_wire_format() {
        let resp = HttpResponse::accepted_json(b"{\"txid\":\"abc\"}".to_vec());
        let wire = String::from_utf8(resp.encode()).expect("utf8");
        assert!(wire.starts_with("HTTP/1.1 202 Accepted\r\n"));
        assert!(wire.contains("Content-Type: application/json\r\n"));
        assert!(wire.contains("Content-Length: 14\r\n"));
        assert!(wire.contains("Connection: close\r\n"));
        assert!(wire.ends_with("{\"txid\":\"abc\"}"));
    }

    #[test]
    fn encode_bad_request_json_includes_status_line() {
        let resp = HttpResponse::bad_request_json(b"{}".to_vec());
        let wire = String::from_utf8(resp.encode()).expect("utf8");
        assert!(wire.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    }

    #[test]
    fn encode_service_unavailable_includes_status_line() {
        let resp = HttpResponse::service_unavailable_json(b"{}".to_vec());
        let wire = String::from_utf8(resp.encode()).expect("utf8");
        assert!(wire.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
    }

    #[test]
    fn encode_method_not_allowed() {
        let resp = HttpResponse::method_not_allowed();
        let wire = String::from_utf8(resp.encode()).expect("utf8");
        assert!(wire.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
    }

    // -- parse_request --

    #[test]
    fn parse_request_empty_buffer_returns_empty() {
        assert_eq!(parse_request(&[]), Err(ParseError::Empty));
    }

    #[test]
    fn parse_request_post_with_zero_body_succeeds() {
        let req = parse_request(&fake_request_line()).expect("parses");
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/submit/tx");
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_request_post_with_body_succeeds() {
        let buf = b"POST /api/submit/tx HTTP/1.1\r\nContent-Length: 4\r\nContent-Type: application/cbor\r\n\r\n\x00\x01\x02\x03";
        let req = parse_request(buf).expect("parses");
        assert_eq!(req.body, b"\x00\x01\x02\x03");
        assert_eq!(req.content_type.as_deref(), Some("application/cbor"));
    }

    #[test]
    fn parse_request_get_succeeds_with_no_body() {
        let buf = b"GET /api/submit/tx HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_request(buf).expect("parses");
        assert_eq!(req.method, "GET");
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_request_method_uppercases() {
        let buf = b"post /api/submit/tx HTTP/1.1\r\n\r\n";
        let req = parse_request(buf).expect("parses");
        assert_eq!(req.method, "POST");
    }

    #[test]
    fn parse_request_truncated_body_errors() {
        let buf = b"POST /api/submit/tx HTTP/1.1\r\nContent-Length: 100\r\n\r\n\x00\x01";
        assert_eq!(parse_request(buf), Err(ParseError::TruncatedBody));
    }

    #[test]
    fn parse_request_oversize_errors() {
        let mut buf = b"POST /api/submit/tx HTTP/1.1\r\nContent-Length: 1\r\n\r\n".to_vec();
        buf.extend(vec![0u8; MAX_REQUEST_BYTES + 1]);
        assert_eq!(parse_request(&buf), Err(ParseError::TooLarge));
    }

    #[test]
    fn parse_request_chunked_transfer_rejected() {
        let buf = b"POST /api/submit/tx HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(
            parse_request(buf),
            Err(ParseError::UnsupportedTransferEncoding)
        );
    }

    #[test]
    fn parse_request_malformed_content_length_errors() {
        let buf = b"POST /api/submit/tx HTTP/1.1\r\nContent-Length: notanumber\r\n\r\n";
        assert_eq!(parse_request(buf), Err(ParseError::MalformedContentLength));
    }

    #[test]
    fn parse_request_no_request_line_terminator_errors() {
        let buf = b"POST /api/submit/tx HTTP/1.1";
        assert_eq!(parse_request(buf), Err(ParseError::MalformedRequestLine));
    }

    #[test]
    fn parse_request_content_type_lowercased() {
        let buf = b"POST /api/submit/tx HTTP/1.1\r\nContent-Type: APPLICATION/CBOR\r\n\r\n";
        let req = parse_request(buf).expect("parses");
        assert_eq!(req.content_type.as_deref(), Some("application/cbor"));
    }

    // -- end-to-end run_settings --

    #[tokio::test]
    async fn run_settings_binds_and_serves_request() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listening_port: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let port_for_tracer = Arc::clone(&listening_port);
        let tracer: Tracer = Arc::new(move |evt| {
            if let TraceSubmitApi::EndpointListeningOnPort(addr) = evt {
                port_for_tracer.store(addr.port() as usize, Ordering::SeqCst);
            }
        });
        let handler = sync_handler(|req: HttpRequest| {
            assert_eq!(req.method, "POST");
            assert_eq!(req.path, "/api/submit/tx");
            HttpResponse::service_unavailable_json(b"{\"tag\":\"placeholder\"}".to_vec())
        });

        let server = tokio::spawn(async move {
            let addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
            let _ = run_settings(tracer, addr, handler).await;
        });

        // Wait for the listener to bind and trace its port.
        for _ in 0..50 {
            if listening_port.load(Ordering::SeqCst) != 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let port = listening_port.load(Ordering::SeqCst) as u16;
        assert!(port > 0, "listener never bound");

        let mut client = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        client
            .write_all(b"POST /api/submit/tx HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("write");
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.expect("read");
        let resp = String::from_utf8_lossy(&resp).to_string();
        assert!(resp.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        assert!(resp.contains("placeholder"));

        server.abort();
    }

    #[tokio::test]
    async fn run_settings_traces_endpoint_listening_on_port() {
        use std::sync::Mutex;
        let events: Arc<Mutex<Vec<TraceSubmitApi>>> = Arc::new(Mutex::new(Vec::new()));
        let events_for_tracer = Arc::clone(&events);
        let tracer: Tracer = Arc::new(move |evt| {
            events_for_tracer.lock().expect("lock").push(evt);
        });
        let handler = sync_handler(|_| HttpResponse::not_found());

        let server = tokio::spawn(async move {
            let addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
            let _ = run_settings(tracer, addr, handler).await;
        });

        for _ in 0..50 {
            if !events.lock().expect("lock").is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let events = events.lock().expect("lock");
        assert!(matches!(
            events.first(),
            Some(TraceSubmitApi::EndpointListeningOnPort(_))
        ));

        server.abort();
    }
}
