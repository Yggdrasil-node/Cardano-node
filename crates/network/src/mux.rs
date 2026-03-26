//! Multiplexer / demultiplexer — routes SDU-framed mini-protocol segments
//! between a bearer connection and per-protocol channel handles.
//!
//! The Ouroboros multiplexer runs two concurrent loops over a single
//! bidirectional transport:
//!
//! - **Demuxer (reader)**: reads SDU frames from the transport, dispatches
//!   each payload to the ingress channel of the corresponding mini-protocol.
//! - **Muxer (writer)**: collects outgoing payloads from all protocol
//!   channels and frames them as SDUs on the wire.
//!
//! Reference: `network-mux/src/Network/Mux.hs`.

use std::collections::HashMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::multiplexer::{MiniProtocolDir, MiniProtocolNum, SduHeader, SDU_HEADER_SIZE};

/// Maximum payload size per outgoing SDU segment.
///
/// Matches upstream `network-mux/src/Network/Mux/Types.hs` — `sduSize = 12288`.
/// The mux writer splits outgoing messages larger than this into multiple SDU
/// frames; the [`MessageChannel`] wrapper reassembles them on the receive side.
pub const MAX_SEGMENT_SIZE: usize = 12288;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors arising from multiplexer operation.
#[derive(Debug, thiserror::Error)]
pub enum MuxError {
    /// Underlying transport I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The remote peer closed the connection.
    #[error("connection closed by remote peer")]
    ConnectionClosed,

    /// Received an SDU for a mini-protocol that was not registered.
    #[error("unknown mini-protocol number: {0}")]
    UnknownProtocol(u16),

    /// The ingress channel for a protocol was closed (receiver dropped).
    #[error("ingress channel closed for protocol {0}")]
    IngressClosed(u16),

    /// All egress senders were dropped — clean shutdown.
    #[error("all egress senders closed")]
    EgressClosed,

    /// A payload exceeds the maximum SDU payload size.
    #[error("payload too large: {0} bytes")]
    PayloadTooLarge(usize),
}

// ---------------------------------------------------------------------------
// ProtocolHandle — per-protocol send / receive surface
// ---------------------------------------------------------------------------

/// Handle for exchanging payload bytes with a single mini-protocol through
/// the multiplexer.
///
/// Each registered protocol receives one handle.  The handle's lifetime
/// is independent from the mux tasks — dropping it signals the muxer that
/// this protocol has no more outgoing data, and the demuxer will return
/// [`MuxError::IngressClosed`] if a payload arrives for a dropped handle.
pub struct ProtocolHandle {
    /// Tagged egress sender (shared across all protocols).
    egress: mpsc::Sender<(MiniProtocolNum, Vec<u8>)>,
    /// Per-protocol ingress receiver.
    ingress: mpsc::Receiver<Vec<u8>>,
    /// Protocol number, used to tag outgoing payloads.
    protocol_num: MiniProtocolNum,
}

impl ProtocolHandle {
    /// Send a complete protocol message payload to the remote peer.
    ///
    /// The multiplexer will frame the payload as an SDU with the correct
    /// protocol number and direction.
    pub async fn send(&self, payload: Vec<u8>) -> Result<(), MuxError> {
        self.egress
            .send((self.protocol_num, payload))
            .await
            .map_err(|_| MuxError::EgressClosed)
    }

    /// Receive the next protocol message payload from the remote peer.
    ///
    /// Returns `None` when the demuxer shuts down or the connection closes.
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.ingress.recv().await
    }

    /// The mini-protocol number this handle is bound to.
    pub fn protocol_num(&self) -> MiniProtocolNum {
        self.protocol_num
    }
}

// ---------------------------------------------------------------------------
// MuxHandle — background task control
// ---------------------------------------------------------------------------

/// Handle to the running mux/demux background tasks.
///
/// Both fields are public so callers can `tokio::select!`, `tokio::join!`,
/// or abort them individually.
pub struct MuxHandle {
    /// Demuxer (reader) background task.
    pub reader: JoinHandle<Result<(), MuxError>>,
    /// Muxer (writer) background task.
    pub writer: JoinHandle<Result<(), MuxError>>,
}

impl MuxHandle {
    /// Abort both background tasks immediately.
    pub fn abort(&self) {
        self.reader.abort();
        self.writer.abort();
    }
}

// ---------------------------------------------------------------------------
// start — entry point
// ---------------------------------------------------------------------------

/// Start the multiplexer over a TCP connection.
///
/// Splits the stream into read/write halves and spawns background tasks
/// for reading (demux) and writing (mux).
///
/// # Arguments
///
/// * `stream` — An already-connected TCP stream.
/// * `role` — The direction bit used on outgoing SDUs: `Initiator` for the
///   side that opened the connection, `Responder` for the side that accepted.
/// * `protocols` — The set of mini-protocol numbers to register.  Incoming
///   SDUs for unregistered protocols cause the demuxer to return
///   [`MuxError::UnknownProtocol`].
/// * `buffer_size` — Capacity of each per-protocol channel.
///
/// # Returns
///
/// A map from protocol number to [`ProtocolHandle`], and a [`MuxHandle`]
/// for the background tasks.
///
/// Reference: `network-mux/src/Network/Mux.hs` — `mux` / `demux`.
pub fn start(
    stream: tokio::net::TcpStream,
    role: MiniProtocolDir,
    protocols: &[MiniProtocolNum],
    buffer_size: usize,
) -> (HashMap<MiniProtocolNum, ProtocolHandle>, MuxHandle) {
    let (read_half, write_half) = stream.into_split();
    start_from_halves(read_half, write_half, role, protocols, buffer_size)
}

/// Start the multiplexer over a Unix-domain socket.
///
/// Identical to [`start`] but accepts a [`tokio::net::UnixStream`] instead
/// of a [`tokio::net::TcpStream`].  Used by the Node-to-Client local server.
#[cfg(unix)]
pub fn start_unix(
    stream: tokio::net::UnixStream,
    role: MiniProtocolDir,
    protocols: &[MiniProtocolNum],
    buffer_size: usize,
) -> (HashMap<MiniProtocolNum, ProtocolHandle>, MuxHandle) {
    let (read_half, write_half) = stream.into_split();
    start_from_halves(read_half, write_half, role, protocols, buffer_size)
}

/// Internal generic entry-point: split read/write halves into mux/demux loops.
fn start_from_halves<R, W>(
    read_half: R,
    write_half: W,
    role: MiniProtocolDir,
    protocols: &[MiniProtocolNum],
    buffer_size: usize,
) -> (HashMap<MiniProtocolNum, ProtocolHandle>, MuxHandle)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{

    // Shared egress channel: all protocol handles send tagged payloads here.
    let (egress_tx, egress_rx) = mpsc::channel::<(MiniProtocolNum, Vec<u8>)>(buffer_size);

    // Per-protocol ingress channels + handles.
    let mut handles = HashMap::new();
    let mut ingress_senders: HashMap<MiniProtocolNum, mpsc::Sender<Vec<u8>>> = HashMap::new();

    for &proto in protocols {
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(buffer_size);
        ingress_senders.insert(proto, in_tx);
        handles.insert(
            proto,
            ProtocolHandle {
                egress: egress_tx.clone(),
                ingress: in_rx,
                protocol_num: proto,
            },
        );
    }

    // Drop the builder's clone — only ProtocolHandle clones remain.
    // When every handle is dropped, the writer task's receiver will close.
    drop(egress_tx);

    let reader = tokio::spawn(demux_loop(read_half, ingress_senders));
    let writer = tokio::spawn(mux_loop(write_half, egress_rx, role));

    (handles, MuxHandle { reader, writer })
}

// ---------------------------------------------------------------------------
// Demuxer (reader) loop
// ---------------------------------------------------------------------------

/// Read SDU frames from the transport and dispatch payloads to the
/// per-protocol ingress channels.
async fn demux_loop<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    ingress: HashMap<MiniProtocolNum, mpsc::Sender<Vec<u8>>>,
) -> Result<(), MuxError> {
    loop {
        // Read the 8-byte SDU header.
        let mut hdr_buf = [0u8; SDU_HEADER_SIZE];
        match reader.read_exact(&mut hdr_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(MuxError::ConnectionClosed);
            }
            Err(e) => return Err(MuxError::Io(e)),
        }

        // Decode — never fails on an 8-byte buffer.
        let header = SduHeader::decode(&hdr_buf)
            .expect("SDU header decode cannot fail on 8-byte buffer");

        let len = header.payload_length as usize;

        // Read payload bytes.
        let mut payload = vec![0u8; len];
        if len > 0 {
            match reader.read_exact(&mut payload).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Err(MuxError::ConnectionClosed);
                }
                Err(e) => return Err(MuxError::Io(e)),
            }
        }

        // Dispatch by mini-protocol number.
        let proto = header.protocol_num;
        match ingress.get(&proto) {
            Some(tx) => {
                if tx.send(payload).await.is_err() {
                    return Err(MuxError::IngressClosed(proto.0));
                }
            }
            None => return Err(MuxError::UnknownProtocol(proto.0)),
        }
    }
}

// ---------------------------------------------------------------------------
// Muxer (writer) loop
// ---------------------------------------------------------------------------

/// Collect tagged payloads from protocol handles and write them as SDU
/// frames on the transport.
async fn mux_loop<W: tokio::io::AsyncWrite + Unpin>(
    mut writer: W,
    mut egress: mpsc::Receiver<(MiniProtocolNum, Vec<u8>)>,
    role: MiniProtocolDir,
) -> Result<(), MuxError> {
    while let Some((proto, payload)) = egress.recv().await {
        if payload.len() <= MAX_SEGMENT_SIZE {
            // Common fast path: single SDU.
            let header = SduHeader {
                timestamp: 0,
                protocol_num: proto,
                direction: role,
                payload_length: payload.len() as u16,
            };
            let hdr_bytes = header.encode();
            writer.write_all(&hdr_bytes).await?;
            writer.write_all(&payload).await?;
        } else {
            // Large payload: segment into MAX_SEGMENT_SIZE chunks.
            for chunk in payload.chunks(MAX_SEGMENT_SIZE) {
                let header = SduHeader {
                    timestamp: 0,
                    protocol_num: proto,
                    direction: role,
                    payload_length: chunk.len() as u16,
                };
                let hdr_bytes = header.encode();
                writer.write_all(&hdr_bytes).await?;
                writer.write_all(chunk).await?;
            }
        }
        writer.flush().await?;
    }

    // All egress senders dropped — clean shutdown.
    Ok(())
}

// ---------------------------------------------------------------------------
// CBOR item length detection
// ---------------------------------------------------------------------------

/// Determine the byte length of one complete CBOR data item starting at
/// the beginning of `buf`.
///
/// Returns `Some(n)` if bytes `0..n` form one complete, well-formed CBOR
/// value, or `None` if the buffer does not contain enough data (or the
/// encoding is not supported, e.g. indefinite-length containers).
///
/// This is used by [`MessageChannel`] to detect message boundaries when
/// reassembling multi-SDU protocol messages, matching the upstream approach
/// where CBOR encoding is self-delimiting.
///
/// Only definite-length encodings are supported because all Ouroboros
/// mini-protocol messages use definite-length CBOR arrays.
pub fn cbor_item_length(buf: &[u8]) -> Option<usize> {
    let len = buf.len();
    if len == 0 {
        return None;
    }

    let mut pos: usize = 0;
    // Number of CBOR data items still to consume.
    let mut remaining: u64 = 1;

    while remaining > 0 {
        if pos >= len {
            return None;
        }
        remaining -= 1;

        let initial = buf[pos];
        let major = initial >> 5;
        let additional = initial & 0x1f;
        pos += 1;

        // Decode the argument value from the additional-info field.
        let arg: u64 = match additional {
            0..=23 => additional as u64,
            24 => {
                if pos >= len {
                    return None;
                }
                let v = buf[pos] as u64;
                pos += 1;
                v
            }
            25 => {
                if pos + 2 > len {
                    return None;
                }
                let v = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as u64;
                pos += 2;
                v
            }
            26 => {
                if pos + 4 > len {
                    return None;
                }
                let v = u32::from_be_bytes(
                    buf[pos..pos + 4]
                        .try_into()
                        .expect("slice is exactly 4 bytes"),
                ) as u64;
                pos += 4;
                v
            }
            27 => {
                if pos + 8 > len {
                    return None;
                }
                let v = u64::from_be_bytes(
                    buf[pos..pos + 8]
                        .try_into()
                        .expect("slice is exactly 8 bytes"),
                );
                pos += 8;
                v
            }
            31 if major == 7 => {
                // Break code (0xFF) — terminates indefinite-length containers.
                continue;
            }
            31 => {
                // Indefinite-length container — not supported.
                return None;
            }
            // Additional-info values 28–30 are reserved.
            _ => return None,
        };

        match major {
            // Unsigned integer / negative integer — value is fully encoded.
            0 | 1 => {}
            // Byte string / text string — `arg` bytes of content follow.
            2 | 3 => {
                let end = pos.checked_add(arg as usize)?;
                if end > len {
                    return None;
                }
                pos = end;
            }
            // Array — `arg` items follow.
            4 => {
                remaining = remaining.checked_add(arg)?;
            }
            // Map — `arg` key-value pairs ⇒ 2 × arg items follow.
            5 => {
                remaining = remaining.checked_add(arg.checked_mul(2)?)?;
            }
            // Tag — one tagged data item follows.
            6 => {
                remaining = remaining.checked_add(1)?;
            }
            // Simple value / float — fully encoded by the header + arg.
            7 => {}
            _ => return None,
        }
    }

    Some(pos)
}

// ---------------------------------------------------------------------------
// MessageChannel — CBOR-aware reassembly wrapper
// ---------------------------------------------------------------------------

/// A protocol message channel that handles SDU segmentation on send and
/// CBOR-aware message reassembly on receive.
///
/// Wraps a [`ProtocolHandle`] and is the recommended interface for
/// mini-protocol client drivers.  On the send side, large messages are
/// transparently segmented into multiple SDUs by the mux writer.  On the
/// receive side, SDU payloads are buffered and reassembled into complete
/// CBOR messages before being returned.
///
/// Reference: upstream `network-mux` uses CBOR self-delimiting encoding
/// for message framing at the codec layer.
pub struct MessageChannel {
    handle: ProtocolHandle,
    read_buf: Vec<u8>,
}

impl MessageChannel {
    /// Create a new message channel from a raw `ProtocolHandle`.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            handle,
            read_buf: Vec::new(),
        }
    }

    /// Send a complete protocol message payload to the remote peer.
    ///
    /// If the payload exceeds [`MAX_SEGMENT_SIZE`] the mux writer
    /// automatically splits it into multiple SDU frames on the wire.
    pub async fn send(&self, payload: Vec<u8>) -> Result<(), MuxError> {
        self.handle.send(payload).await
    }

    /// Receive the next complete protocol message from the remote peer.
    ///
    /// SDU payloads are buffered internally.  The method inspects the
    /// buffer after each SDU and extracts complete CBOR values, which it
    /// returns as whole messages.
    ///
    /// Returns `None` when the demuxer shuts down or the connection closes.
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        loop {
            // Try to extract a complete CBOR message from the buffer.
            if let Some(n) = cbor_item_length(&self.read_buf) {
                let message: Vec<u8> = self.read_buf.drain(..n).collect();
                return Some(message);
            }

            // Read the next SDU payload from the mux.
            let chunk = self.handle.recv().await?;

            // Empty SDU on an empty buffer: deliver as an empty message.
            // This preserves backward-compatible semantics for protocols
            // that send zero-length payloads.
            if chunk.is_empty() && self.read_buf.is_empty() {
                return Some(chunk);
            }

            self.read_buf.extend_from_slice(&chunk);
        }
    }

    /// The mini-protocol number this channel is bound to.
    pub fn protocol_num(&self) -> MiniProtocolNum {
        self.handle.protocol_num()
    }
}
