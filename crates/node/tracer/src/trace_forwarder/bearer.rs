//! AF_UNIX SOCK_STREAM bearer for the cardano-tracer forwarder Mux.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `tokio`-backed bearer
//! adapter for the upstream `Network.Mux.Bearer` and
//! `Network.Mux.Bearer.Pipe` surfaces at
//! `.reference-haskell-cardano-node/deps/ouroboros-network/network-mux/`.
//! Upstream's `makeSocketBearer` accepts a `Network.Socket.Socket`
//! and returns a `Bearer { write, read, sduSize, name, … }` record;
//! Yggdrasil exposes a smaller `Bearer<S>` async surface that
//! covers the SDU read/write halves of that record. Timeouts,
//! tracing, and the `sduSize` knob land when the Mux state-machine
//! driver actually consumes the bearer.
//!
//! ## What this module ships
//!
//! [`Bearer<S>`] is a generic wrapper over any tokio `AsyncRead`
//! `AsyncWrite` `Unpin` `Send` transport (typically
//! `tokio::net::UnixStream` for the cardano-tracer Unix-socket
//! path, or `tokio::io::DuplexStream` for tests). [`Bearer::read_sdu`]
//! reads exactly 8 header bytes via [`super::mux::decode_sdu_header`],
//! then exactly `length` payload bytes; it returns the parsed
//! header plus an owned `Vec<u8>` payload. [`Bearer::write_sdu`]
//! encodes the header via [`super::mux::encode_sdu_header`] and
//! writes the header-followed-by-payload in two `write_all` calls
//! (tokio buffers coalesce them). [`BearerError`] surfaces the
//! failure modes operators need to distinguish: I/O failure,
//! mux-codec failure (malformed SDU header), unexpected EOF
//! mid-frame, and caller-side header-length mismatch.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::mux::{MuxError, SduHeader, decode_sdu_header, encode_sdu_header};

/// AF_UNIX SOCK_STREAM bearer (or any other `AsyncRead + AsyncWrite`
/// transport — the only assumption is reliable byte-stream
/// semantics).
///
/// `S` is the underlying transport. For cardano-tracer the operator-
/// canonical instantiation is `Bearer<tokio::net::UnixStream>`; for
/// unit tests it's typically `Bearer<tokio::io::DuplexStream>` (an
/// in-memory pipe).
pub struct Bearer<S> {
    stream: S,
}

impl<S> Bearer<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    /// Wrap an `AsyncRead + AsyncWrite` transport.
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    /// Consume the bearer and return the underlying transport.
    pub fn into_inner(self) -> S {
        self.stream
    }

    /// Read one SDU off the wire: 8-byte header + variable payload
    /// (length carried in the header). The payload is returned as
    /// an owned `Vec<u8>` so callers can hand it to per-mini-
    /// protocol decoders without lifetime ceremony.
    ///
    /// Errors: [`BearerError::Io`] for transport-level failure
    /// (broken pipe, connection closed, etc);
    /// [`BearerError::UnexpectedEof`] when the stream returned fewer
    /// bytes than the header promised (length field was N but only
    /// K < N payload bytes received before EOF); [`BearerError::Mux`]
    /// when the 8-byte header itself didn't parse (malformed
    /// direction bit, out-of-range protocol number, short read).
    pub async fn read_sdu(&mut self) -> Result<(SduHeader, Vec<u8>), BearerError> {
        let mut header_buf = [0_u8; 8];
        self.stream
            .read_exact(&mut header_buf)
            .await
            .map_err(map_eof_to_unexpected_eof)?;
        let header = decode_sdu_header(&header_buf).map_err(BearerError::Mux)?;
        let mut payload = vec![0_u8; header.length as usize];
        if header.length > 0 {
            self.stream
                .read_exact(&mut payload)
                .await
                .map_err(map_eof_to_unexpected_eof)?;
        }
        Ok((header, payload))
    }

    /// Write one SDU to the wire: the encoded 8-byte header
    /// followed by the payload bytes. The two `write_all` calls
    /// are coalesced by tokio's internal buffer so this still hits
    /// the kernel as one or two `write(2)` syscalls per SDU.
    ///
    /// `header.length` MUST equal `payload.len()`; the encoder
    /// trusts the caller's bookkeeping and the decoder will fail
    /// later if a peer's bearer doesn't honor it.
    pub async fn write_sdu(
        &mut self,
        header: &SduHeader,
        payload: &[u8],
    ) -> Result<(), BearerError> {
        if header.length as usize != payload.len() {
            return Err(BearerError::HeaderLengthMismatch {
                declared: header.length,
                actual: payload.len(),
            });
        }
        let encoded_header = encode_sdu_header(header).map_err(BearerError::Mux)?;
        self.stream
            .write_all(&encoded_header)
            .await
            .map_err(BearerError::Io)?;
        if !payload.is_empty() {
            self.stream
                .write_all(payload)
                .await
                .map_err(BearerError::Io)?;
        }
        Ok(())
    }
}

/// Errors surfaced from `Bearer::read_sdu` and `Bearer::write_sdu`.
#[derive(Debug)]
pub enum BearerError {
    /// Transport-level I/O failure (broken pipe, connection closed
    /// mid-write, refused, …).
    Io(std::io::Error),
    /// Stream returned fewer bytes than the SDU header promised.
    UnexpectedEof,
    /// Mux-level encoder/decoder failure (malformed header, out-of-
    /// range protocol number).
    Mux(MuxError),
    /// Caller's `header.length` did not equal `payload.len()`.
    HeaderLengthMismatch {
        /// What the header advertised.
        declared: u16,
        /// What the payload buffer actually contained.
        actual: usize,
    },
}

impl core::fmt::Display for BearerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "bearer I/O error: {e}"),
            Self::UnexpectedEof => f.write_str("bearer read returned EOF mid-frame"),
            Self::Mux(e) => write!(f, "SDU codec error: {e}"),
            Self::HeaderLengthMismatch { declared, actual } => write!(
                f,
                "SDU header length ({declared}) does not match payload size ({actual})"
            ),
        }
    }
}

impl std::error::Error for BearerError {}

/// Map `std::io::ErrorKind::UnexpectedEof` to the dedicated
/// [`BearerError::UnexpectedEof`] variant so callers can tell a
/// clean EOF mid-frame apart from a generic transport error.
fn map_eof_to_unexpected_eof(err: std::io::Error) -> BearerError {
    if err.kind() == std::io::ErrorKind::UnexpectedEof {
        BearerError::UnexpectedEof
    } else {
        BearerError::Io(err)
    }
}

#[cfg(test)]
mod bearer_tests {
    use super::*;
    use crate::trace_forwarder::mux::{MiniProtocolDir, TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM};

    /// Round-trip an SDU through an in-memory duplex pipe: write
    /// SDU on one half, read SDU on the other; header + payload
    /// come back byte-identical.
    #[tokio::test]
    async fn bearer_round_trips_sdu_over_duplex() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);

        let header = SduHeader {
            timestamp: 0x_1234_5678,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Initiator,
            length: 11,
        };
        let payload = b"hello world".to_vec();

        client_bearer
            .write_sdu(&header, &payload)
            .await
            .expect("write");
        let (got_header, got_payload) = server_bearer.read_sdu().await.expect("read");
        assert_eq!(got_header, header);
        assert_eq!(got_payload, payload);
    }

    /// Round-trip an empty-payload SDU. Edge case the payload-
    /// length-zero short-circuit must handle without spurious EOF.
    #[tokio::test]
    async fn bearer_round_trips_empty_payload_sdu() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);

        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: 1,
            direction: MiniProtocolDir::Responder,
            length: 0,
        };
        client_bearer
            .write_sdu(&header, &[])
            .await
            .expect("write zero-length sdu");
        let (got_header, got_payload) = server_bearer.read_sdu().await.expect("read");
        assert_eq!(got_header, header);
        assert!(got_payload.is_empty());
    }

    /// HeaderLengthMismatch fires when the caller's bookkeeping is
    /// wrong (header says N bytes; payload buffer is M ≠ N).
    #[tokio::test]
    async fn bearer_rejects_header_length_mismatch() {
        let (client, _server) = tokio::io::duplex(1024);
        let mut bearer = Bearer::new(client);
        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: 0,
            direction: MiniProtocolDir::Initiator,
            length: 10, // claims 10 bytes
        };
        let result = bearer.write_sdu(&header, b"only 4").await;
        assert!(
            matches!(
                result,
                Err(BearerError::HeaderLengthMismatch {
                    declared: 10,
                    actual: 6
                })
            ),
            "expected HeaderLengthMismatch{{10,6}}; got {result:?}"
        );
    }

    /// EOF mid-frame surfaces as `UnexpectedEof`, not a generic IO
    /// error. Drop the writer half before sending the payload bytes
    /// to drive the case.
    #[tokio::test]
    async fn bearer_read_returns_unexpected_eof_on_truncated_payload() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);

        // Write just the header, then drop the writer so the
        // payload read times out / EOFs.
        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: 0,
            direction: MiniProtocolDir::Initiator,
            length: 16,
        };
        let header_bytes = encode_sdu_header(&header).expect("encode");
        // Write only the header (no payload), then drop the writer.
        client_bearer
            .stream
            .write_all(&header_bytes)
            .await
            .expect("write header");
        drop(client_bearer);

        let result = server_bearer.read_sdu().await;
        assert!(
            matches!(result, Err(BearerError::UnexpectedEof)),
            "expected UnexpectedEof; got {result:?}"
        );
    }
}
