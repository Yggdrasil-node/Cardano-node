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

use crate::bearer::MAX_SDU_PAYLOAD;
use crate::multiplexer::{MiniProtocolDir, MiniProtocolNum, SduHeader, SDU_HEADER_SIZE};

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
    #[error("payload too large: {0} bytes (max {MAX_SDU_PAYLOAD})")]
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
async fn demux_loop(
    mut reader: tokio::net::tcp::OwnedReadHalf,
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
        if len > MAX_SDU_PAYLOAD {
            return Err(MuxError::PayloadTooLarge(len));
        }

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
async fn mux_loop(
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    mut egress: mpsc::Receiver<(MiniProtocolNum, Vec<u8>)>,
    role: MiniProtocolDir,
) -> Result<(), MuxError> {
    while let Some((proto, payload)) = egress.recv().await {
        if payload.len() > MAX_SDU_PAYLOAD {
            return Err(MuxError::PayloadTooLarge(payload.len()));
        }

        let header = SduHeader {
            timestamp: 0,
            protocol_num: proto,
            direction: role,
            payload_length: payload.len() as u16,
        };

        let hdr_bytes = header.encode();
        writer.write_all(&hdr_bytes).await?;
        writer.write_all(&payload).await?;
        writer.flush().await?;
    }

    // All egress senders dropped — clean shutdown.
    Ok(())
}
