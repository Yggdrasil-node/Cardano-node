//! Multiplexer / demultiplexer — routes SDU-framed mini-protocol segments
//! between a bearer connection and per-protocol channel handles.
//!
//! The Ouroboros multiplexer runs two concurrent loops over a single
//! bidirectional transport:
//!
//! - **Demuxer (reader)**: reads SDU frames from the transport, dispatches
//!   each payload to the ingress channel of the corresponding mini-protocol.
//!   Per-protocol ingress byte limits are enforced; exceeding the limit
//!   raises [`MuxError::IngressQueueOverRun`] and kills the connection.
//! - **Muxer (writer)**: collects outgoing payloads from per-protocol
//!   egress channels using weighted round-robin scheduling, and frames
//!   them as SDUs on the wire.  Each protocol gets `weight` segments
//!   per round before the scheduler advances, preventing head-of-line
//!   blocking across mini-protocols.
//!
//! Backpressure:
//! - Ingress: upstream `maximumIngressQueue` — byte counter per protocol,
//!   incremented by the demuxer, decremented by [`ProtocolHandle::recv`].
//! - Egress: upstream `egressSoftBufferLimit` — byte counter per protocol,
//!   incremented by [`ProtocolHandle::send`], decremented by the muxer
//!   after writing.  Exceeding the limit returns
//!   [`MuxError::EgressBufferOverflow`].
//!
//! Reference: `network-mux/src/Network/Mux.hs`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::multiplexer::{MiniProtocolDir, MiniProtocolNum, SDU_HEADER_SIZE, SduHeader};

/// Maximum payload size per outgoing SDU segment.
///
/// Matches upstream `network-mux/src/Network/Mux/Types.hs` — `sduSize = 12288`.
/// The mux writer splits outgoing messages larger than this into multiple SDU
/// frames; the [`MessageChannel`] wrapper reassembles them on the receive side.
pub const MAX_SEGMENT_SIZE: usize = 12288;

/// Default per-protocol ingress queue byte limit.
///
/// Matches upstream `maximumIngressQueue` default of 2 MB.
///
/// Reference: `Network.Mux.Types` — `MiniProtocolLimits`.
pub const DEFAULT_INGRESS_LIMIT: usize = 2_000_000;

/// Per-protocol egress soft buffer limit.
///
/// Upstream: `egressSoftBufferLimit = 0x3ffff` (~262 KB) from
/// `network-mux`.  This is a **back-pressure** threshold on
/// accumulated pending egress bytes — when the protocol's egress
/// queue has already accumulated more than this, [`ProtocolHandle::send`]
/// returns [`MuxError::EgressBufferOverflow`] and the runtime should
/// tear down the connection.  R213 — single payloads larger than the
/// limit are *not* rejected when the buffer is empty (e.g. mainnet's
/// `query utxo --whole-utxo` LSQ response is ~1.3 MB; rejecting it
/// at send time would prevent any operator from running this query).
/// Matches upstream `network-mux`'s semantic: the limit gates buffer
/// accumulation under writer back-pressure, not single-message size.
pub const EGRESS_SOFT_LIMIT: usize = 0x3ffff; // 262_143 bytes

/// SDU read timeout on the bearer connection.
///
/// If no SDU header bytes arrive within this window the demuxer terminates
/// the connection with [`MuxError::SduTimeout`].  This matches upstream
/// `network-mux` idle-timeout behaviour (30 seconds).
///
/// Reference: `Network.Mux.Bearer` — `bearerAsChannel` read timeout.
pub const SDU_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Default protocol scheduling weight for the egress round-robin.
///
/// All protocols start with weight 1 (uniform scheduling).  Hot-tier
/// protocols (ChainSync, BlockFetch) can be assigned higher weights
/// via [`ProtocolConfig`] to get proportionally more write slots per
/// round, matching upstream hot-protocol priority.
pub const DEFAULT_PROTOCOL_WEIGHT: u8 = 1;

// ---------------------------------------------------------------------------
// WeightHandle — shared dynamic scheduling weight
// ---------------------------------------------------------------------------

/// Shared handle for dynamically adjusting a protocol's egress scheduling
/// weight at runtime.
///
/// The [`start_configured`] entry point returns one `WeightHandle` per
/// protocol (embedded in each [`ProtocolHandle`]).  The mux writer reads
/// the current weight atomically each scheduling round, so updates take
/// effect immediately without restarting the multiplexer.
///
/// Typical use: bump ChainSync and BlockFetch weights when a peer is
/// promoted to hot-tier, and reset them to 1 on demotion to warm.
#[derive(Clone, Debug)]
pub struct WeightHandle(Arc<AtomicU8>);

impl WeightHandle {
    /// Update the scheduling weight (floor-clamped to 1).
    pub fn set(&self, w: u8) {
        self.0.store(w.max(1), Ordering::Relaxed);
    }

    /// Read the current scheduling weight.
    pub fn get(&self) -> u8 {
        self.0.load(Ordering::Relaxed)
    }
}

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

    /// Per-protocol ingress queue byte limit exceeded.
    ///
    /// Upstream: `MuxIngressQueueOverRun` from `Network.Mux.Trace`.
    /// The demuxer terminates the connection when the accumulated
    /// ingress bytes for a protocol exceed `maximumIngressQueue`.
    #[error("ingress queue overrun for protocol {protocol}: {bytes} bytes exceeds limit {limit}")]
    IngressQueueOverRun {
        /// Mini-protocol number.
        protocol: u16,
        /// Byte count that triggered the overrun.
        bytes: usize,
        /// Configured limit.
        limit: usize,
    },

    /// Per-protocol egress soft buffer limit exceeded.
    ///
    /// Upstream: Wanton buffer check against `egressSoftBufferLimit`.
    /// The connection should be torn down when a protocol tries to
    /// buffer more egress data than the limit allows.
    #[error("egress buffer overflow for protocol {protocol}: {bytes} bytes exceeds limit {limit}")]
    EgressBufferOverflow {
        /// Mini-protocol number.
        protocol: u16,
        /// Byte count that triggered the overflow.
        bytes: usize,
        /// Configured limit.
        limit: usize,
    },

    /// No SDU bytes arrived within [`SDU_READ_TIMEOUT`].
    ///
    /// Upstream: bearer read timeout in `Network.Mux.Bearer.bearerAsChannel`.
    #[error("SDU read timeout after {0:?}")]
    SduTimeout(Duration),
}

// ---------------------------------------------------------------------------
// ProtocolConfig — per-protocol mux configuration
// ---------------------------------------------------------------------------

/// Configuration for a single mini-protocol channel in the multiplexer.
///
/// Used by [`start_configured`] to specify per-protocol ingress limits
/// and egress scheduling weights.  [`start`] uses
/// [`ProtocolConfig::default_for`] for every protocol.
#[derive(Clone, Debug)]
pub struct ProtocolConfig {
    /// Protocol number.
    pub num: MiniProtocolNum,
    /// Maximum ingress queue bytes (default: [`DEFAULT_INGRESS_LIMIT`]).
    pub ingress_limit: usize,
    /// Scheduling weight for egress round-robin (default: [`DEFAULT_PROTOCOL_WEIGHT`]).
    pub weight: u8,
}

impl ProtocolConfig {
    /// Create a config with default limits and weight.
    pub fn default_for(num: MiniProtocolNum) -> Self {
        Self {
            num,
            ingress_limit: DEFAULT_INGRESS_LIMIT,
            weight: DEFAULT_PROTOCOL_WEIGHT,
        }
    }
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
/// Payloads for dropped protocol handles are discarded without tearing down
/// the entire connection.
///
/// Backpressure:
/// - Egress: [`send`](Self::send) checks `pending_egress_bytes` against
///   the per-protocol soft limit before queueing.
/// - Ingress: the demuxer increments `ingress_bytes` when dispatching an
///   SDU; [`recv`](Self::recv) decrements after delivery.
pub struct ProtocolHandle {
    /// Per-protocol egress sender.
    egress: mpsc::Sender<Vec<u8>>,
    /// Per-protocol ingress receiver.
    ingress: mpsc::Receiver<Vec<u8>>,
    /// Protocol number, used to tag outgoing payloads.
    protocol_num: MiniProtocolNum,
    /// Shared ingress byte counter (incremented by demuxer, decremented
    /// by [`recv`](Self::recv)).
    ingress_bytes: Arc<AtomicUsize>,
    /// Shared egress byte counter (incremented by [`send`](Self::send),
    /// decremented by the mux writer after writing).
    egress_bytes: Arc<AtomicUsize>,
    /// Per-protocol egress soft limit.
    egress_limit: usize,
    /// Notification handle — wakes the mux writer when new egress data
    /// is available (replaces the old single shared channel).
    notify: Arc<tokio::sync::Notify>,
    /// Shared scheduling weight (also held by the mux writer's `EgressSlot`).
    weight: WeightHandle,
}

impl ProtocolHandle {
    /// Send a complete protocol message payload to the remote peer.
    ///
    /// Returns [`MuxError::EgressBufferOverflow`] when the protocol's
    /// already-pending egress bytes exceed the soft limit — i.e. when
    /// the writer has fallen behind and the buffer is accumulating
    /// faster than the bearer can drain it.  R213 — single large
    /// payloads are *always* allowed through (the check guards against
    /// buffer accumulation, not single-message size); without this,
    /// LSQ responses larger than `EGRESS_SOFT_LIMIT` (~262 KB) such as
    /// mainnet's `query utxo --whole-utxo` (~1.3 MB) trip the limit
    /// even when the buffer is empty.  Upstream `network-mux` uses the
    /// same back-pressure semantic (the limit gates accumulated
    /// bytes, not new sends).
    ///
    /// The multiplexer will frame the payload as an SDU with the
    /// correct protocol number and direction.
    pub async fn send(&self, payload: Vec<u8>) -> Result<(), MuxError> {
        let len = payload.len();
        let current = self.egress_bytes.load(Ordering::Relaxed);
        if current > self.egress_limit {
            return Err(MuxError::EgressBufferOverflow {
                protocol: self.protocol_num.0,
                bytes: current,
                limit: self.egress_limit,
            });
        }
        self.egress
            .send(payload)
            .await
            .map_err(|_| MuxError::EgressClosed)?;
        self.egress_bytes.fetch_add(len, Ordering::Relaxed);
        self.notify.notify_one();
        Ok(())
    }

    /// Receive the next protocol message payload from the remote peer.
    ///
    /// Returns `None` when the demuxer shuts down or the connection closes.
    /// Decrements the ingress byte counter for this protocol.
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        let data = self.ingress.recv().await;
        if let Some(ref d) = data {
            self.ingress_bytes.fetch_sub(d.len(), Ordering::Relaxed);
        }
        data
    }

    /// The mini-protocol number this handle is bound to.
    pub fn protocol_num(&self) -> MiniProtocolNum {
        self.protocol_num
    }

    /// Clone the shared weight handle so the caller can adjust this
    /// protocol's egress scheduling weight after the [`ProtocolHandle`]
    /// has been consumed.
    pub fn weight_handle(&self) -> WeightHandle {
        self.weight.clone()
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
/// All protocols get default ingress limits ([`DEFAULT_INGRESS_LIMIT`]),
/// egress limits ([`EGRESS_SOFT_LIMIT`]), and weight
/// ([`DEFAULT_PROTOCOL_WEIGHT`]).  Use [`start_configured`] for custom
/// per-protocol settings.
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
    let configs: Vec<ProtocolConfig> = protocols
        .iter()
        .map(|&p| ProtocolConfig::default_for(p))
        .collect();
    let (read_half, write_half) = stream.into_split();
    start_from_halves(read_half, write_half, role, &configs, buffer_size)
}

/// Start the multiplexer over a TCP connection with custom per-protocol
/// configuration (ingress limits, egress limits, scheduling weights).
pub fn start_configured(
    stream: tokio::net::TcpStream,
    role: MiniProtocolDir,
    protocols: &[ProtocolConfig],
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
    let configs: Vec<ProtocolConfig> = protocols
        .iter()
        .map(|&p| ProtocolConfig::default_for(p))
        .collect();
    let (read_half, write_half) = stream.into_split();
    start_from_halves(read_half, write_half, role, &configs, buffer_size)
}

/// Start the multiplexer over a Unix-domain socket with custom per-protocol
/// configuration.
#[cfg(unix)]
pub fn start_unix_configured(
    stream: tokio::net::UnixStream,
    role: MiniProtocolDir,
    protocols: &[ProtocolConfig],
    buffer_size: usize,
) -> (HashMap<MiniProtocolNum, ProtocolHandle>, MuxHandle) {
    let (read_half, write_half) = stream.into_split();
    start_from_halves(read_half, write_half, role, protocols, buffer_size)
}

// ---------------------------------------------------------------------------
// EgressSlot — internal per-protocol egress source for the mux writer
// ---------------------------------------------------------------------------

/// Per-protocol egress receiver slot used by the round-robin mux writer.
struct EgressSlot {
    protocol_num: MiniProtocolNum,
    receiver: mpsc::Receiver<Vec<u8>>,
    weight: WeightHandle,
    pending_bytes: Arc<AtomicUsize>,
    closed: bool,
}

/// Internal generic entry-point: split read/write halves into mux/demux loops.
fn start_from_halves<R, W>(
    read_half: R,
    write_half: W,
    role: MiniProtocolDir,
    protocols: &[ProtocolConfig],
    buffer_size: usize,
) -> (HashMap<MiniProtocolNum, ProtocolHandle>, MuxHandle)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let notify = Arc::new(tokio::sync::Notify::new());

    let mut handles = HashMap::new();
    let mut ingress_senders: HashMap<MiniProtocolNum, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut ingress_bytes_map: HashMap<MiniProtocolNum, Arc<AtomicUsize>> = HashMap::new();
    let mut ingress_limits: HashMap<MiniProtocolNum, usize> = HashMap::new();
    let mut egress_slots = Vec::new();

    for cfg in protocols {
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(buffer_size);
        let (eg_tx, eg_rx) = mpsc::channel::<Vec<u8>>(buffer_size);

        let ingress_bytes = Arc::new(AtomicUsize::new(0));
        let egress_bytes = Arc::new(AtomicUsize::new(0));
        let weight = WeightHandle(Arc::new(AtomicU8::new(cfg.weight.max(1))));

        ingress_senders.insert(cfg.num, in_tx);
        ingress_bytes_map.insert(cfg.num, Arc::clone(&ingress_bytes));
        ingress_limits.insert(cfg.num, cfg.ingress_limit);

        egress_slots.push(EgressSlot {
            protocol_num: cfg.num,
            receiver: eg_rx,
            weight: weight.clone(),
            pending_bytes: Arc::clone(&egress_bytes),
            closed: false,
        });

        handles.insert(
            cfg.num,
            ProtocolHandle {
                egress: eg_tx,
                ingress: in_rx,
                protocol_num: cfg.num,
                ingress_bytes,
                egress_bytes,
                egress_limit: EGRESS_SOFT_LIMIT,
                notify: Arc::clone(&notify),
                weight,
            },
        );
    }

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let reader = tokio::spawn(demux_loop(
        read_half,
        ingress_senders,
        ingress_bytes_map,
        ingress_limits,
        cancel_tx.clone(),
        cancel_rx.clone(),
    ));
    let writer = tokio::spawn(mux_loop(
        write_half,
        egress_slots,
        role,
        notify,
        cancel_tx,
        cancel_rx,
    ));

    (handles, MuxHandle { reader, writer })
}

// ---------------------------------------------------------------------------
// Demuxer (reader) loop
// ---------------------------------------------------------------------------

/// Read SDU frames from the transport and dispatch payloads to the
/// per-protocol ingress channels.
///
/// Enforces per-protocol ingress byte limits: if the accumulated ingress
/// bytes for a protocol exceed the configured limit, returns
/// [`MuxError::IngressQueueOverRun`] and terminates the connection.
async fn demux_loop<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    ingress: HashMap<MiniProtocolNum, mpsc::Sender<Vec<u8>>>,
    ingress_bytes: HashMap<MiniProtocolNum, Arc<AtomicUsize>>,
    ingress_limits: HashMap<MiniProtocolNum, usize>,
    cancel_tx: tokio::sync::watch::Sender<bool>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), MuxError> {
    let result = demux_loop_inner(
        &mut reader,
        &ingress,
        &ingress_bytes,
        &ingress_limits,
        &mut cancel_rx,
    )
    .await;

    if std::env::var("YGG_SYNC_DEBUG").is_ok_and(|v| v != "0") {
        if let Err(ref err) = result {
            eprintln!("[ygg-sync-debug] demux-exit error={err}");
        }
    }

    // Signal the writer to shut down when the reader exits with an error.
    if result.is_err() {
        let _ = cancel_tx.send(true);
    }

    result
}

/// Inner demux loop, extracted so the outer wrapper can signal on error.
async fn demux_loop_inner<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    ingress: &HashMap<MiniProtocolNum, mpsc::Sender<Vec<u8>>>,
    ingress_bytes: &HashMap<MiniProtocolNum, Arc<AtomicUsize>>,
    ingress_limits: &HashMap<MiniProtocolNum, usize>,
    cancel_rx: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<(), MuxError> {
    loop {
        // Read the 8-byte SDU header, aborting if the peer task cancelled.
        let mut hdr_buf = [0u8; SDU_HEADER_SIZE];
        tokio::select! {
            biased;
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow_and_update() {
                    return Err(MuxError::ConnectionClosed);
                }
            }
            result = tokio::time::timeout(SDU_READ_TIMEOUT, reader.read_exact(&mut hdr_buf)) => {
                match result {
                    Err(_elapsed) => return Err(MuxError::SduTimeout(SDU_READ_TIMEOUT)),
                    Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        return Err(MuxError::ConnectionClosed);
                    }
                    Ok(Err(e)) => return Err(MuxError::Io(e)),
                    Ok(Ok(_)) => {}
                }
            }
        }

        // Decode — never fails on an 8-byte buffer.
        let header =
            SduHeader::decode(&hdr_buf).expect("SDU header decode cannot fail on 8-byte buffer");

        let len = header.payload_length as usize;
        let proto = header.protocol_num;

        // Ingress byte-limit check (upstream `maximumIngressQueue`) is
        // performed BEFORE allocating the payload buffer, so that a peer
        // who has already filled this protocol's queue cannot keep
        // forcing per-frame allocations at line rate (audit finding M-1).
        if let Some(counter) = ingress_bytes.get(&proto) {
            let limit = ingress_limits
                .get(&proto)
                .copied()
                .unwrap_or(DEFAULT_INGRESS_LIMIT);
            let current = counter.load(Ordering::Relaxed);
            if current + len > limit {
                return Err(MuxError::IngressQueueOverRun {
                    protocol: proto.0,
                    bytes: current + len,
                    limit,
                });
            }
            counter.fetch_add(len, Ordering::Relaxed);
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
        match ingress.get(&proto) {
            Some(tx) => {
                if tx.send(payload).await.is_err() {
                    // The protocol handle was dropped locally. Discard the
                    // payload and keep the connection alive for other
                    // protocols.
                    if let Some(counter) = ingress_bytes.get(&proto) {
                        counter.fetch_sub(len, Ordering::Relaxed);
                    }
                    continue;
                }
            }
            None => {
                // Undo byte increment for unknown protocol.
                if let Some(counter) = ingress_bytes.get(&proto) {
                    counter.fetch_sub(len, Ordering::Relaxed);
                }
                return Err(MuxError::UnknownProtocol(proto.0));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Muxer (writer) loop
// ---------------------------------------------------------------------------

/// Weighted round-robin egress writer.
///
/// Each protocol has its own egress channel and a scheduling weight.
/// Per round, each protocol gets up to `weight` segments written before
/// the scheduler advances to the next protocol, preventing head-of-line
/// blocking.  When all channels are empty the writer blocks on a shared
/// [`tokio::sync::Notify`] until a [`ProtocolHandle::send`] wakes it.
///
/// Upstream reference: `network-mux` per-Wanton round-robin writer.
async fn mux_loop<W: tokio::io::AsyncWrite + Unpin>(
    mut writer: W,
    mut slots: Vec<EgressSlot>,
    role: MiniProtocolDir,
    notify: Arc<tokio::sync::Notify>,
    cancel_tx: tokio::sync::watch::Sender<bool>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), MuxError> {
    let result = mux_loop_inner(&mut writer, &mut slots, role, &notify, &mut cancel_rx).await;

    if std::env::var("YGG_SYNC_DEBUG").is_ok_and(|v| v != "0") {
        if let Err(ref err) = result {
            eprintln!("[ygg-sync-debug] mux-exit error={err}");
        }
    }

    // Signal the reader to shut down when the writer exits with an error.
    if result.is_err() {
        let _ = cancel_tx.send(true);
    }

    result
}

/// Inner mux loop, extracted so the outer wrapper can signal on error.
async fn mux_loop_inner<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    slots: &mut Vec<EgressSlot>,
    role: MiniProtocolDir,
    notify: &tokio::sync::Notify,
    cancel_rx: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<(), MuxError> {
    let bearer_start = std::time::Instant::now();
    loop {
        // Remove fully-disconnected slots.
        slots.retain(|s| !s.closed);
        if slots.is_empty() {
            return Ok(());
        }

        let mut any_written = false;

        for slot in slots.iter_mut() {
            let mut written = 0u8;
            while written < slot.weight.get() {
                match slot.receiver.try_recv() {
                    Ok(payload) => {
                        let len = payload.len();
                        write_sdu(writer, slot.protocol_num, role, &payload, bearer_start).await?;
                        slot.pending_bytes.fetch_sub(len, Ordering::Relaxed);
                        any_written = true;
                        written += 1;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        slot.closed = true;
                        break;
                    }
                }
            }
        }

        if !any_written {
            // Check again after marking closed slots.
            if slots.iter().all(|s| s.closed) {
                slots.retain(|s| !s.closed);
                return Ok(());
            }
            // All channels empty — wait for notification from a sender,
            // or cancellation from the peer (demux) task.
            tokio::select! {
                biased;
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow_and_update() {
                        return Err(MuxError::ConnectionClosed);
                    }
                }
                _ = notify.notified() => {}
            }
        }
    }
}

/// Compute the monotonic SDU timestamp per upstream
/// `Network.Mux.Bearer.Pipe.makeBearer` — lower 32 bits of microseconds
/// elapsed since `start`.  Each frame on the bearer carries this so
/// the peer can monitor relative timing for back-pressure / liveness.
/// Pre-fix yggdrasil sent literal `0` for the timestamp; upstream
/// `cardano-cli`'s mux layer rejects all-zero timestamps which is why
/// `BlockQuery (QueryHardFork GetCurrentEra)` MsgResults arrived at the
/// CLI with `DeserialiseFailure 2 "expected list len or indef"` — the
/// ENVELOPE was malformed before the result content was even examined.
/// Reference: 2026-04-27 socat capture comparing yggdrasil's
/// `00 00 00 00 80 07 00 03 82 04 01` against the upstream Haskell
/// node's `56 8b ae ed 80 07 00 03 82 04 02` — only difference is the
/// SDU timestamp being non-zero on the upstream side.
fn sdu_timestamp_micros(start: std::time::Instant) -> u32 {
    start.elapsed().as_micros() as u32
}

/// Write one payload as one or more SDU frames on the transport.
async fn write_sdu<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    proto: MiniProtocolNum,
    role: MiniProtocolDir,
    payload: &[u8],
    bearer_start: std::time::Instant,
) -> Result<(), MuxError> {
    if payload.len() <= MAX_SEGMENT_SIZE {
        // Common fast path: single SDU.
        let header = SduHeader {
            timestamp: sdu_timestamp_micros(bearer_start),
            protocol_num: proto,
            direction: role,
            payload_length: payload.len() as u16,
        };
        writer.write_all(&header.encode()).await?;
        writer.write_all(payload).await?;
    } else {
        // Large payload: segment into MAX_SEGMENT_SIZE chunks.
        for chunk in payload.chunks(MAX_SEGMENT_SIZE) {
            let header = SduHeader {
                timestamp: sdu_timestamp_micros(bearer_start),
                protocol_num: proto,
                direction: role,
                payload_length: chunk.len() as u16,
            };
            writer.write_all(&header.encode()).await?;
            writer.write_all(chunk).await?;
        }
    }
    writer.flush().await?;
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
