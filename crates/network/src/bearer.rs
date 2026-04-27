//! Async multiplexer bearer — abstraction over a transport that carries
//! SDU-framed mini-protocol segments.
//!
//! The Ouroboros multiplexer operates over a *bearer*, which is any
//! bidirectional byte stream that can send and receive SDU frames (8-byte
//! header + variable-length payload).
//!
//! Reference: `network-mux/src/Network/Mux/Bearer.hs`.

use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::multiplexer::{MiniProtocolDir, MiniProtocolNum, SDU_HEADER_SIZE, SduHeader};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors arising from bearer I/O operations.
#[derive(Debug, thiserror::Error)]
pub enum BearerError {
    /// The remote peer closed the connection before a complete SDU was read.
    #[error("connection closed by remote peer")]
    ConnectionClosed,

    /// The payload length declared in the SDU header exceeds the protocol
    /// limit.  Upstream caps this at 0xFFFF (65535) bytes, which the `u16`
    /// field enforces automatically, but an implementation may choose a
    /// tighter limit.
    #[error("SDU payload too large: {0} bytes (limit {1})")]
    PayloadTooLarge(usize, usize),

    /// Underlying I/O error from the transport.
    #[error(transparent)]
    Io(#[from] io::Error),
}

// ---------------------------------------------------------------------------
// Sdu — owned SDU frame
// ---------------------------------------------------------------------------

/// An owned SDU frame consisting of a decoded header and its payload bytes.
#[derive(Clone, Debug)]
pub struct Sdu {
    /// Decoded SDU header.
    pub header: SduHeader,
    /// Payload bytes (length matches `header.payload_length`).
    pub payload: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Bearer trait
// ---------------------------------------------------------------------------

/// Maximum payload size per SDU segment (upstream `network-mux` default).
///
/// Reference: `network-mux/src/Network/Mux/Types.hs` — `sduSize`.
pub const MAX_SDU_PAYLOAD: usize = 0xFFFF;

/// An async transport that sends and receives multiplexed SDU frames.
///
/// Implementations must ensure that each `send` / `recv` transfers a
/// complete SDU atomically — partial reads or writes should be retried
/// internally.
///
/// Reference: `network-mux/src/Network/Mux/Bearer.hs` — `MuxBearer`.
pub trait Bearer: Send {
    /// Send a complete SDU frame (header + payload).
    fn send(
        &mut self,
        sdu: &Sdu,
    ) -> impl std::future::Future<Output = Result<(), BearerError>> + Send;

    /// Receive the next complete SDU frame.
    ///
    /// Returns [`BearerError::ConnectionClosed`] when the remote peer has
    /// shut down the connection.
    fn recv(&mut self) -> impl std::future::Future<Output = Result<Sdu, BearerError>> + Send;
}

// ---------------------------------------------------------------------------
// TcpBearer — tokio TCP socket bearer
// ---------------------------------------------------------------------------

/// A bearer backed by a TCP stream using `tokio::net::TcpStream`.
///
/// Implements the Ouroboros multiplexer bearer protocol: each SDU is
/// prefixed with an 8-byte header encoding the timestamp, mini-protocol
/// number, direction, and payload length.
///
/// Reference: `network-mux/src/Network/Mux/Bearer/Socket.hs`.
pub struct TcpBearer {
    stream: tokio::net::TcpStream,
}

impl TcpBearer {
    /// Wrap an already-connected `TcpStream` as a multiplexer bearer.
    pub fn new(stream: tokio::net::TcpStream) -> Self {
        Self { stream }
    }

    /// Connect to a remote peer and return a bearer.
    pub async fn connect(addr: impl tokio::net::ToSocketAddrs) -> Result<Self, BearerError> {
        let stream = tokio::net::TcpStream::connect(addr).await?;
        Ok(Self::new(stream))
    }
}

impl Bearer for TcpBearer {
    async fn send(&mut self, sdu: &Sdu) -> Result<(), BearerError> {
        let header_bytes = sdu.header.encode();
        self.stream.write_all(&header_bytes).await?;
        self.stream.write_all(&sdu.payload).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Sdu, BearerError> {
        // Read the 8-byte SDU header.
        let mut hdr_buf = [0u8; SDU_HEADER_SIZE];
        match self.stream.read_exact(&mut hdr_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                return Err(BearerError::ConnectionClosed);
            }
            Err(e) => return Err(BearerError::Io(e)),
        }

        // Decode the header.  `SduHeader::decode` only fails on short
        // buffers, and we always supply exactly `SDU_HEADER_SIZE` bytes.
        let header = SduHeader::decode(&hdr_buf)
            .expect("SDU header decode should not fail on 8-byte buffer");

        let len = header.payload_length as usize;
        if len > MAX_SDU_PAYLOAD {
            return Err(BearerError::PayloadTooLarge(len, MAX_SDU_PAYLOAD));
        }

        // Read the payload.
        let mut payload = vec![0u8; len];
        if len > 0 {
            match self.stream.read_exact(&mut payload).await {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Err(BearerError::ConnectionClosed);
                }
                Err(e) => return Err(BearerError::Io(e)),
            }
        }

        Ok(Sdu { header, payload })
    }
}

// ---------------------------------------------------------------------------
// Helper constructors
// ---------------------------------------------------------------------------

impl Sdu {
    /// Build an SDU frame for the given mini-protocol, direction, and payload.
    ///
    /// Round 149 — timestamp is the lower 32 bits of microseconds since
    /// the process start (a monotonic counter `Instant::now()` measured
    /// from a fixed reference).  Pre-fix yggdrasil sent literal `0`
    /// timestamps, which upstream `cardano-cli`'s mux layer interprets
    /// as a malformed/replayed frame on the LSQ data path (handshake
    /// SDUs accept zero timestamps; data SDUs reject them).  Reference:
    /// `Network.Mux.Bearer.Pipe` in `ouroboros-network`.
    pub fn new(
        protocol_num: MiniProtocolNum,
        direction: MiniProtocolDir,
        payload: Vec<u8>,
    ) -> Self {
        // Use a fixed `LazyLock<Instant>` so all `Sdu::new` calls in the
        // same process share a monotonic reference.  Microsecond
        // precision wraps every ~71 minutes which matches upstream's
        // u32 wrap convention.
        use std::sync::OnceLock;
        static REFERENCE: OnceLock<std::time::Instant> = OnceLock::new();
        let start = *REFERENCE.get_or_init(std::time::Instant::now);
        let header = SduHeader {
            timestamp: start.elapsed().as_micros() as u32,
            protocol_num,
            direction,
            payload_length: payload.len() as u16,
        };
        Self { header, payload }
    }
}
