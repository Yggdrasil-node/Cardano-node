//! Minimal bidirectional Mux dispatcher.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side bare-bones SDU
//! demultiplexer for the upstream Network.Mux multiplexer at
//! `.reference-haskell-cardano-node/deps/ouroboros-network/network-mux/src/Network/Mux.hs`.
//! Upstream's Mux runs every mini-protocol concurrently on a
//! shared bearer with per-mini-protocol ingress queues, an egress
//! scheduler that round-robins among ready writers, and a bearer-
//! task lifecycle that supervises both halves. Yggdrasil's
//! [`MuxConnection`] is intentionally smaller: a write-API that
//! serializes outbound SDUs through a tokio mutex, plus a
//! read-task that reads SDUs and routes the payload to per-mini-
//! protocol byte-capped ingress queues (slice (a) below). The
//! egress scheduler and bearer-task supervision remain follow-on
//! rounds.
//!
//! ## Read/write are independently lockable
//!
//! The bearer is [`Bearer::split`]-ed into a [`BearerReader`] and a
//! [`BearerWriter`], each behind its **own** tokio mutex.
//! [`MuxConnection::spawn_read_task`] locks only the reader; a
//! concurrent [`MuxConnection::send_sdu`] locks only the writer, so
//! the two never contend on a single lock. Upstream `Network.Mux`
//! likewise drives ingress and egress on separate threads sharing
//! one socket FD. An earlier revision wrapped the whole bearer in
//! one `Mutex`: the read-task held that lock for the full duration
//! of a `read_sdu().await`, so while a read was pending (no inbound
//! bytes) every `send_sdu` caller blocked forever — a deadlock,
//! latent only because the cardano-tracer conversation is
//! sequential. The split removes the shared lock.
//!
//! What this is good for: a one-mini-protocol-at-a-time conversation
//! over the same bearer (cardano-tracer use case: the Handshake
//! initiator finishes first, then the TraceObject forwarder runs
//! until shutdown). What it isn't good for: concurrent
//! mini-protocol activity with egress fairness / cancel semantics.
//!
//! ## Per-mini-protocol ingress queue limits (slice a)
//!
//! Each subscriber now carries a bounded ingress queue whose
//! capacity mirrors upstream `Network.Mux.Types.MiniProtocolLimits`'
//! `maximumIngressQueue` field. Upstream `Network.Mux.Ingress.demuxer`
//! sums the **payload bytes** queued for a `(MiniProtocolNum,
//! MiniProtocolDir)` pair and, when a freshly-read SDU would push
//! `len' > qMax`, throws `IngressQueueOverRun` — the bearer is torn
//! down. Yggdrasil mirrors that: [`MiniProtocolLimits`] holds the
//! byte cap, [`MuxConnection::subscribe`] /
//! [`MuxConnection::subscribe_with_limits`] register it, the
//! read-task byte-accounts every dispatched payload (an
//! [`std::sync::atomic::AtomicUsize`] per mini-protocol), and an
//! over-cap SDU surfaces [`MuxConnectionError::IngressQueueOverRun`]
//! from the read-loop's `Result` instead of being silently dropped
//! or blocking the producer.
//!
//! The default cap is [`MiniProtocolLimits::CARDANO_TRACER_DEFAULT`]
//! = `i32::MAX as usize`, the Rust analogue of the `maxBound :: Int`
//! that the real `cardano-tracer` acceptor uses for **every**
//! sub-protocol (`Cardano.Tracer.Acceptors.Client.hs:102` /
//! `Server.hs:101`: `MiniProtocolLimits { maximumIngressQueue =
//! maxBound }`). The node-to-node bearer uses much smaller
//! per-protocol caps (`Cardano.Network.NodeToNode`'s
//! `chainSyncProtocolLimits`, `blockFetchProtocolLimits`, …) but the
//! cardano-tracer bearer — the one this crate live-conforms against
//! — does not, so the default stays effectively-unbounded and
//! conformance is preserved. Tests drive the over-cap path through
//! [`MuxConnection::subscribe_with_limits`] with a small explicit
//! cap.
//!
//! Byte accounting: bytes are charged when the read-task dispatches
//! an SDU to a registered subscriber and **freed** when the
//! subscriber drains the SDU through the [`IngressReceiver`] wrapper
//! returned by `subscribe`. An SDU on a mini-protocol with **no**
//! registered subscriber is dropped without ever being accounted
//! (upstream keeps a queue per `(num,dir)` regardless of reader
//! progress; Yggdrasil's smaller dispatcher has no queue until a
//! subscriber exists — documented divergence).
//!
//! The egress side is **not** queue-limited in this slice:
//! [`MuxConnection::send_sdu`] is a direct
//! `writer.lock() + write_sdu` round-trip with no intervening queue.
//! Introducing an egress queue requires the egress scheduler
//! (upstream `Network.Mux.Egress`: `EgressQueue`,
//! `TranslocationServiceRequest`, `Wanton`, the `muxer` task) — that
//! is a follow-on round, tracked in `docs/TECH-DEBT.md`
//! "cardano-tracer Mux Layer 2/3". Bearer-task supervision
//! (cohesive shutdown when any sub-task fails) is a further
//! follow-on round.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use super::bearer::{Bearer, BearerError, BearerReader, BearerWriter};
use super::mux::{MiniProtocolDir, SduHeader};

/// One full received SDU exposed to per-mini-protocol consumers
/// through the channel returned by [`MuxConnection::subscribe`].
#[derive(Clone, Debug)]
pub struct InboundSdu {
    /// The original SDU header — operators care about the
    /// direction and timestamp.
    pub header: SduHeader,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

/// Errors surfaced from `MuxConnection::send_sdu` or returned from
/// the read-loop's outcome.
#[derive(Debug)]
pub enum MuxConnectionError {
    /// Bearer-level read or write failure.
    Bearer(BearerError),
    /// A freshly-read SDU would push a mini-protocol's ingress queue
    /// past its [`MiniProtocolLimits::maximum_ingress_queue`] byte
    /// cap. Mirrors upstream `Network.Mux.Trace.IngressQueueOverRun`
    /// (thrown by `Network.Mux.Ingress.demuxer` as `throwSTM $
    /// IngressQueueOverRun (msNum sdu) (msDir sdu)`): a protocol
    /// violation that tears the bearer down. The read-task returns
    /// this from its `Result` and exits — it does **not** block the
    /// producer the way a bounded `mpsc::Sender` would.
    IngressQueueOverRun {
        /// Mini-protocol number whose ingress queue overflowed.
        mini_protocol_num: u16,
        /// Direction of the offending SDU.
        direction: MiniProtocolDir,
    },
}

impl core::fmt::Display for MuxConnectionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bearer(e) => write!(f, "mux connection bearer error: {e}"),
            Self::IngressQueueOverRun {
                mini_protocol_num,
                direction,
            } => write!(
                f,
                "ingress queue over-run on mini-protocol {mini_protocol_num} \
                 ({direction:?}): SDU exceeds maximumIngressQueue"
            ),
        }
    }
}

impl std::error::Error for MuxConnectionError {}

/// Per-mini-protocol ingress queue limits.
///
/// Mirrors upstream `Network.Mux.Types.MiniProtocolLimits`
/// (`.reference-haskell-cardano-node/deps/ouroboros-network/network-mux/src/Network/Mux/Types.hs`),
/// whose sole field `maximumIngressQueue :: Int` is "the maximum
/// number of **bytes** that can be queued in the miniprotocol's
/// ingress queue". `Network.Mux.Ingress.demuxer` enforces it by
/// summing `BL.length (msBlob sdu)` — the SDU payload bytes — across
/// every queued SDU and throwing `IngressQueueOverRun` when a new
/// SDU would push the running total over the cap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiniProtocolLimits {
    /// Maximum number of payload bytes that may sit un-drained in
    /// this mini-protocol's ingress queue. Upstream
    /// `maximumIngressQueue`.
    pub maximum_ingress_queue: usize,
}

impl MiniProtocolLimits {
    /// The ingress cap the real `cardano-tracer` acceptor applies to
    /// **every** sub-protocol on the trace-forwarding bearer
    /// (Handshake=0, EKG=1, TraceObject=2, DataPoint=3).
    ///
    /// Upstream `Cardano.Tracer.Acceptors.Client.hs:102` and
    /// `Cardano.Tracer.Acceptors.Server.hs:101` both build every
    /// `MiniProtocol` with `miniProtocolLimits = MiniProtocolLimits
    /// { maximumIngressQueue = maxBound }`. `maxBound :: Int` on a
    /// 64-bit GHC is `2^63 - 1`; the value's operational meaning is
    /// "effectively unbounded". Yggdrasil pins it to `i32::MAX as
    /// usize` — large enough that no real trace SDU stream
    /// approaches it, small enough to stay a tidy constant, and
    /// faithful to the upstream intent that the cardano-tracer
    /// bearer applies **no** practical ingress cap. (The
    /// node-to-node bearer is different: `Cardano.Network.NodeToNode`
    /// gives ChainSync/BlockFetch/TxSubmission/KeepAlive/PeerSharing
    /// each a tight per-protocol cap. This crate forwards to
    /// cardano-tracer, not over NtN, so the unbounded default is the
    /// parity-correct one.)
    pub const CARDANO_TRACER_DEFAULT: Self = Self {
        maximum_ingress_queue: i32::MAX as usize,
    };
}

impl Default for MiniProtocolLimits {
    fn default() -> Self {
        Self::CARDANO_TRACER_DEFAULT
    }
}

/// One registered subscriber: the channel sender plus its
/// byte-accounted ingress-queue state.
struct Subscriber {
    /// Sender half handed to the read-task. Unbounded — backpressure
    /// is **not** the upstream semantic; the byte cap is enforced
    /// separately and an over-cap SDU tears the bearer down.
    sender: mpsc::UnboundedSender<InboundSdu>,
    /// Byte cap for this mini-protocol's ingress queue
    /// (upstream `maximumIngressQueue`).
    limits: MiniProtocolLimits,
    /// Running count of payload bytes dispatched into this queue but
    /// not yet drained by the subscriber. Charged by the read-task
    /// on dispatch, freed by the [`IngressReceiver`] wrapper when the
    /// subscriber pulls an SDU. Shared with the `IngressReceiver` so
    /// the two stay in lock-step.
    queued_bytes: Arc<AtomicUsize>,
}

/// Subscriber registry for inbound SDUs — protected by a tokio
/// Mutex so the read-task and the subscribe API can mutate it
/// independently.
type SubscriberMap = HashMap<u16, Subscriber>;

/// Receiver half of a per-mini-protocol ingress queue.
///
/// Wraps the inner `mpsc::UnboundedReceiver<InboundSdu>` so that
/// every SDU the subscriber drains **frees** its payload bytes from
/// the shared `queued_bytes` counter — keeping the read-task's
/// byte-accounting accurate as the consumer makes progress. Drop
/// semantics are unchanged: dropping this receiver un-subscribes the
/// mini-protocol exactly as dropping a bare `UnboundedReceiver` did.
pub struct IngressReceiver {
    /// Inner channel receiver.
    inner: mpsc::UnboundedReceiver<InboundSdu>,
    /// Shared byte counter — `recv` subtracts the drained SDU's
    /// payload length so the read-task's `queued <= cap` check stays
    /// honest.
    queued_bytes: Arc<AtomicUsize>,
}

impl IngressReceiver {
    /// Receive the next inbound SDU for this mini-protocol, freeing
    /// its payload bytes from the ingress-queue accounting. Returns
    /// `None` once the read-task has dropped the matching sender
    /// (connection closed / read-loop exited).
    pub async fn recv(&mut self) -> Option<InboundSdu> {
        let sdu = self.inner.recv().await?;
        // Free this SDU's payload bytes. `saturating_sub` guards the
        // (should-be-impossible) case of a drain without a matching
        // charge.
        let freed = sdu.payload.len();
        let mut current = self.queued_bytes.load(Ordering::Acquire);
        loop {
            let next = current.saturating_sub(freed);
            match self.queued_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
        Some(sdu)
    }

    /// Bytes currently charged to this mini-protocol's ingress
    /// queue (dispatched but not yet drained). Test/diagnostic
    /// accessor.
    pub fn queued_bytes(&self) -> usize {
        self.queued_bytes.load(Ordering::Acquire)
    }
}

/// Multiplexer connection: wraps a [`Bearer<S>`] with a per-mini-
/// protocol dispatch table.
///
/// The bearer is split into independent read/write halves
/// ([`Bearer::split`]) so the read-task and the SDU writer hold
/// **separate** mutexes and never deadlock against each other (see
/// the module docs).
pub struct MuxConnection<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    /// Write half of the bearer. Wrapped in its own tokio Mutex so
    /// concurrent `send_sdu` callers serialize against each other —
    /// a single outbound SDU is never partially interleaved with
    /// another — without ever blocking the read-task.
    writer: Arc<Mutex<BearerWriter<S>>>,
    /// Read half of the bearer. Wrapped in its own tokio Mutex,
    /// locked only by [`Self::spawn_read_task`]'s read-loop and by
    /// [`Self::run_initiator_handshake`]. Independent of `writer`,
    /// so a pending `read_sdu` never starves an outbound write.
    reader: Arc<Mutex<BearerReader<S>>>,
    /// Per-mini-protocol channel registry. `subscribe(num)` /
    /// `subscribe_with_limits(num, limits)` inserts a new entry; the
    /// read-task reads from `reader.read_sdu()`, byte-accounts the
    /// payload against the subscriber's
    /// [`MiniProtocolLimits::maximum_ingress_queue`] cap, and
    /// forwards to the corresponding Sender (silently dropping the
    /// SDU if no subscriber is registered; returning
    /// [`MuxConnectionError::IngressQueueOverRun`] if the cap would
    /// be exceeded).
    subscribers: Arc<Mutex<SubscriberMap>>,
}

impl<S> MuxConnection<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    /// Construct a new MuxConnection from a Bearer. The bearer is
    /// immediately split into independent read/write halves. Call
    /// [`Self::spawn_read_task`] separately to start dispatching
    /// inbound SDUs to subscribers; until that's spawned, the
    /// bearer never reads.
    pub fn new(bearer: Bearer<S>) -> Self {
        let (reader, writer) = bearer.split();
        Self {
            writer: Arc::new(Mutex::new(writer)),
            reader: Arc::new(Mutex::new(reader)),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send one outbound SDU. Serializes against other `send_sdu`
    /// callers through the **writer** mutex so SDUs hit the wire as
    /// atomic units. Never contends with the read-task: the
    /// read-loop locks the separate reader mutex.
    pub async fn send_sdu(
        &self,
        header: &SduHeader,
        payload: &[u8],
    ) -> Result<(), MuxConnectionError> {
        let mut writer = self.writer.lock().await;
        writer
            .write_sdu(header, payload)
            .await
            .map_err(MuxConnectionError::Bearer)
    }

    /// Subscribe to inbound SDUs on the given mini-protocol number
    /// with the default cardano-tracer ingress-queue cap
    /// ([`MiniProtocolLimits::CARDANO_TRACER_DEFAULT`], the upstream
    /// `maxBound`-equivalent — effectively unbounded).
    ///
    /// Returns an [`IngressReceiver`]; the read-task pushes each
    /// inbound SDU with a matching `mini_protocol_num` into it. If
    /// multiple subscribers register for the same number, the LATEST
    /// one wins (previous receiver gets dropped on the inserting
    /// side); callers should subscribe at most once per
    /// mini-protocol.
    pub async fn subscribe(&self, mini_protocol_num: u16) -> IngressReceiver {
        self.subscribe_with_limits(mini_protocol_num, MiniProtocolLimits::default())
            .await
    }

    /// Subscribe to inbound SDUs on the given mini-protocol number
    /// with an explicit [`MiniProtocolLimits`] ingress-queue cap.
    ///
    /// The read-task byte-accounts every dispatched payload against
    /// `limits.maximum_ingress_queue`; an SDU that would push the
    /// running total over the cap makes the read-loop return
    /// [`MuxConnectionError::IngressQueueOverRun`] and exit (upstream
    /// `Network.Mux.Ingress.demuxer`'s `IngressQueueOverRun` throw).
    /// Operators normally want the default cap via [`Self::subscribe`];
    /// this entry point exists so a small explicit cap can drive the
    /// over-run path deterministically in tests.
    pub async fn subscribe_with_limits(
        &self,
        mini_protocol_num: u16,
        limits: MiniProtocolLimits,
    ) -> IngressReceiver {
        let (tx, rx) = mpsc::unbounded_channel();
        let queued_bytes = Arc::new(AtomicUsize::new(0));
        let mut subscribers = self.subscribers.lock().await;
        subscribers.insert(
            mini_protocol_num,
            Subscriber {
                sender: tx,
                limits,
                queued_bytes: Arc::clone(&queued_bytes),
            },
        );
        IngressReceiver {
            inner: rx,
            queued_bytes,
        }
    }

    /// Run the initiator side of the Handshake mini-protocol
    /// (mini-protocol num 0) over this Mux connection.
    ///
    /// Equivalent to
    /// [`super::handshake_driver::run_initiator_handshake`] but
    /// composes cleanly with the per-mini-protocol channel-based
    /// subscription instead of taking a raw `&mut Bearer<S>` —
    /// useful when other mini-protocols share the same bearer
    /// (cardano-tracer use case: handshake → TraceObject forwarding,
    /// both on the same Unix socket).
    ///
    /// MUST be called BEFORE [`Self::spawn_read_task`] starts the
    /// read-loop — this function performs its own blocking
    /// `bearer.read_sdu()` to consume the responder's Accept /
    /// Refuse reply. After it returns, spawn the read-task to
    /// dispatch subsequent SDUs.
    pub async fn run_initiator_handshake(
        &self,
        versions: std::collections::BTreeMap<u32, Vec<u8>>,
    ) -> Result<super::handshake_driver::AgreedVersion, super::handshake_driver::HandshakeDriverError>
    {
        // Take BOTH half-mutexes for the full handshake duration:
        // the handshake is a strict write-then-read exchange and
        // must run before any other mini-protocol touches the
        // bearer. The read-task is not yet spawned (the doc
        // contract requires this call first), so there is no other
        // lock holder to deadlock against.
        let mut writer = self.writer.lock().await;
        let mut reader = self.reader.lock().await;
        super::handshake_driver::run_initiator_handshake_split(&mut reader, &mut writer, versions)
            .await
    }

    /// Spawn the read-task that dispatches inbound SDUs to
    /// subscribers. Returns the `JoinHandle` so the caller can
    /// await it on shutdown.
    ///
    /// The task runs until the bearer returns `UnexpectedEof` (or
    /// any other bearer error), or until a dispatched SDU would push
    /// a mini-protocol's ingress queue past its
    /// [`MiniProtocolLimits::maximum_ingress_queue`] byte cap — in
    /// which case it returns [`MuxConnectionError::IngressQueueOverRun`]
    /// and exits, mirroring upstream `Network.Mux.Ingress.demuxer`'s
    /// `IngressQueueOverRun` throw + bearer tear-down. On any
    /// mini-protocol that has no registered subscriber the inbound
    /// SDU is silently dropped (and never byte-accounted).
    pub fn spawn_read_task(&self) -> tokio::task::JoinHandle<Result<(), MuxConnectionError>> {
        let reader = Arc::clone(&self.reader);
        let subscribers = Arc::clone(&self.subscribers);
        tokio::spawn(async move {
            loop {
                // Lock the READER half for the duration of one full
                // SDU read (header + payload). This is a separate
                // mutex from the writer half, so a `send_sdu`
                // caller can write concurrently while this read is
                // pending — no read/write deadlock. (An earlier
                // revision shared one bearer mutex; see the module
                // docs.)
                let read_outcome = {
                    let mut reader_guard = reader.lock().await;
                    reader_guard.read_sdu().await
                };
                match read_outcome {
                    Ok((header, payload)) => {
                        let subscribers_guard = subscribers.lock().await;
                        if let Some(subscriber) = subscribers_guard.get(&header.mini_protocol_num) {
                            // Byte-account this SDU's payload against
                            // the mini-protocol's ingress-queue cap,
                            // mirroring `Network.Mux.Ingress.demuxer`:
                            //   len' = len + BL.length (msBlob sdu)
                            //   if len' <= qMax then enqueue
                            //                   else throw IngressQueueOverRun
                            // The check + the charge happen as one
                            // `fetch_update` so a concurrent drain on
                            // the `IngressReceiver` cannot wedge the
                            // total between the read and the write.
                            let added = payload.len();
                            let cap = subscriber.limits.maximum_ingress_queue;
                            let charged = subscriber.queued_bytes.fetch_update(
                                Ordering::AcqRel,
                                Ordering::Acquire,
                                |current| {
                                    let next = current.saturating_add(added);
                                    if next <= cap { Some(next) } else { None }
                                },
                            );
                            match charged {
                                Ok(_) => {
                                    // Within cap: dispatch. A send
                                    // error means the subscriber's
                                    // `IngressReceiver` was dropped
                                    // (unsubscription) — discard the
                                    // SDU and release the bytes we
                                    // just charged so the counter
                                    // does not leak.
                                    if subscriber
                                        .sender
                                        .send(InboundSdu { header, payload })
                                        .is_err()
                                    {
                                        subscriber.queued_bytes.fetch_sub(added, Ordering::AcqRel);
                                    }
                                }
                                Err(_) => {
                                    // Over cap: protocol violation.
                                    // Tear the bearer down — return
                                    // the error from the read-loop,
                                    // exactly as upstream throws and
                                    // unwinds the demuxer.
                                    return Err(MuxConnectionError::IngressQueueOverRun {
                                        mini_protocol_num: header.mini_protocol_num,
                                        direction: header.direction,
                                    });
                                }
                            }
                        }
                        // No subscriber: drop the SDU, no
                        // accounting. A real Mux keeps a queue per
                        // (num,dir) regardless of reader progress,
                        // but the cardano-tracer use case registers
                        // every subscriber before spawning the
                        // read-task.
                    }
                    Err(err) => {
                        return Err(MuxConnectionError::Bearer(err));
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod mux_connection_tests {
    use super::*;
    use crate::trace_forwarder::mux::{
        HANDSHAKE_MINI_PROTOCOL_NUM, MiniProtocolDir, TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
    };
    use std::time::Duration;

    /// Send an SDU on the connection's write half, read it back
    /// off the bearer's read half, confirm header + payload are
    /// preserved.
    #[tokio::test]
    async fn mux_connection_send_round_trips_through_bearer() {
        let (client, server) = tokio::io::duplex(4096);
        let client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);
        let conn = MuxConnection::new(client_bearer);

        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Initiator,
            length: 4,
        };
        conn.send_sdu(&header, b"abcd").await.expect("send_sdu");

        let (got_header, got_payload) = server_bearer.read_sdu().await.expect("read");
        assert_eq!(got_header, header);
        assert_eq!(got_payload, b"abcd");
    }

    /// Spawn the read-task; an SDU arriving on the bearer is
    /// dispatched to the matching subscriber's channel.
    #[tokio::test]
    async fn mux_connection_dispatches_inbound_sdu_to_subscriber() {
        let (client, mut server) = tokio::io::duplex(4096);
        let client_bearer = Bearer::new(client);
        let conn = MuxConnection::new(client_bearer);

        // Subscribe to mini-protocol 2 (TraceObject).
        let mut rx = conn.subscribe(TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM).await;

        // Spawn the read-task.
        let _read_task = conn.spawn_read_task();

        // From the server side, write one Responder-direction SDU
        // on mini-protocol 2.
        let outbound_header = SduHeader {
            timestamp: 42,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 5,
        };
        let outbound_bytes =
            super::super::mux::encode_sdu_header(&outbound_header).expect("encode header");
        use tokio::io::AsyncWriteExt;
        server
            .write_all(&outbound_bytes)
            .await
            .expect("write header");
        server.write_all(b"hello").await.expect("write payload");

        // The subscriber should receive the SDU within a
        // reasonable timeout.
        let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("subscriber receive within 1s")
            .expect("subscriber channel produced an SDU");
        assert_eq!(received.header, outbound_header);
        assert_eq!(received.payload, b"hello");
    }

    /// Compose handshake_driver with MuxConnection: invoke the
    /// handshake via `run_initiator_handshake` (taking the bearer
    /// mutex internally), then spawn the read-task afterwards so
    /// subsequent mini-protocol SDUs dispatch normally.
    #[tokio::test]
    async fn mux_connection_run_initiator_handshake_round_trip() {
        use crate::trace_forwarder::handshake::{HandshakeMessage, decode_message, encode_message};
        use std::collections::BTreeMap;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (client, mut server) = tokio::io::duplex(4096);
        let conn = MuxConnection::new(Bearer::new(client));

        // Fake responder: read Propose, reply Accept on version 2.
        let server_task = tokio::spawn(async move {
            let mut hdr = [0u8; 8];
            server.read_exact(&mut hdr).await.expect("read hdr");
            let hdr_decoded = crate::trace_forwarder::mux::decode_sdu_header(&hdr).expect("decode");
            let mut payload = vec![0u8; hdr_decoded.length as usize];
            server.read_exact(&mut payload).await.expect("read payload");
            // Sanity: it was a ProposeVersions in Idle state.
            let _ = decode_message(&payload, true).expect("decode propose");

            // Reply with AcceptVersion(2, <data>).
            let reply = HandshakeMessage::AcceptVersion {
                version: 2,
                data_cbor: {
                    use yggdrasil_ledger::cbor::Encoder;
                    let mut enc = Encoder::new();
                    enc.unsigned(764_824_073u64);
                    enc.into_bytes()
                },
            };
            let reply_payload = encode_message(&reply);
            let reply_hdr = SduHeader {
                timestamp: 0,
                mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
                direction: MiniProtocolDir::Responder,
                length: reply_payload.len() as u16,
            };
            let reply_hdr_bytes =
                crate::trace_forwarder::mux::encode_sdu_header(&reply_hdr).expect("encode");
            server.write_all(&reply_hdr_bytes).await.expect("write hdr");
            server
                .write_all(&reply_payload)
                .await
                .expect("write payload");
        });

        let mut versions = BTreeMap::new();
        let v_data = {
            use yggdrasil_ledger::cbor::Encoder;
            let mut enc = Encoder::new();
            enc.unsigned(1u64);
            enc.into_bytes()
        };
        versions.insert(1u32, v_data.clone());
        versions.insert(2u32, v_data);
        let agreed = conn
            .run_initiator_handshake(versions)
            .await
            .expect("handshake accept");
        assert_eq!(agreed.version, 2);
        let _ = server_task.await;
    }

    /// Regression test for the Mux bearer-layer read/write
    /// deadlock (task #19, outcome d).
    ///
    /// Setup reproduces the exact deadlock geometry:
    ///
    /// 1. Spawn the read-task. Its read-loop calls `read_sdu()` and
    ///    blocks — the peer (`server`) sends nothing, so the
    ///    8-byte header read never completes.
    /// 2. `yield_now()` so the read-task is *guaranteed* to have
    ///    acquired its bearer lock and parked inside the pending
    ///    `read_sdu` before we proceed. Without this yield,
    ///    `send_sdu` could win the lock race on a single-threaded
    ///    runtime and the test would falsely pass on the broken
    ///    code.
    /// 3. Call `send_sdu` concurrently, wrapped in a 2s timeout.
    ///
    /// On the **old** single-`Mutex<Bearer>` design the read-task
    /// held the one bearer lock for the whole duration of the
    /// pending read, so `send_sdu` could never acquire it →
    /// `send_sdu` hangs and the timeout fires. After the
    /// [`Bearer::split`] fix the read-task holds only the reader
    /// mutex and `send_sdu` takes the independent writer mutex, so
    /// the write completes well inside the timeout.
    ///
    /// `keep_alive` holds the server end open for the whole test:
    /// dropping it would EOF the read and let the read-task release
    /// its lock, masking the deadlock.
    #[tokio::test]
    async fn mux_connection_send_sdu_not_blocked_by_pending_read() {
        let (client, keep_alive) = tokio::io::duplex(4096);
        let conn = MuxConnection::new(Bearer::new(client));

        // 1. Read-task: blocks inside `read_sdu` — no inbound bytes.
        let _read_task = conn.spawn_read_task();

        // 2. Let the read-task acquire its lock and park in the
        //    pending read before we attempt the concurrent write.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        // 3. Concurrent `send_sdu` — must NOT be starved by the
        //    pending read. On the pre-split code this hung forever.
        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Initiator,
            length: 4,
        };
        let send_result =
            tokio::time::timeout(Duration::from_secs(2), conn.send_sdu(&header, b"abcd")).await;
        assert!(
            send_result.is_ok(),
            "send_sdu was starved by the read-task's pending read — \
             the Mux bearer-layer deadlock has regressed"
        );
        send_result
            .expect("send_sdu completed within the timeout")
            .expect("send_sdu succeeded");

        // The write reached the peer: read it back off the
        // still-open server end to confirm it actually hit the wire
        // (not just acquired a lock).
        let mut server_bearer = Bearer::new(keep_alive);
        let (got_header, got_payload) =
            tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                .await
                .expect("server read of the concurrently-sent SDU did not time out")
                .expect("server read the SDU");
        assert_eq!(got_header, header);
        assert_eq!(got_payload, b"abcd");
    }

    /// SDUs arriving on a mini-protocol that has no subscriber are
    /// silently dropped (the read-task doesn't error).
    #[tokio::test]
    async fn mux_connection_drops_unsubscribed_sdu() {
        let (client, mut server) = tokio::io::duplex(4096);
        let client_bearer = Bearer::new(client);
        let conn = MuxConnection::new(client_bearer);
        // Subscribe to handshake (0) only.
        let mut handshake_rx = conn.subscribe(HANDSHAKE_MINI_PROTOCOL_NUM).await;
        let _read_task = conn.spawn_read_task();

        // Send an SDU on TRACE_OBJECT mini-protocol (no subscriber).
        let trace_header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 3,
        };
        let trace_hdr_bytes = super::super::mux::encode_sdu_header(&trace_header).expect("encode");
        use tokio::io::AsyncWriteExt;
        server.write_all(&trace_hdr_bytes).await.expect("write");
        server.write_all(b"xxx").await.expect("write payload");

        // Now send one on handshake (which IS subscribed).
        let hs_header = SduHeader {
            timestamp: 0,
            mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 1,
        };
        let hs_bytes = super::super::mux::encode_sdu_header(&hs_header).expect("encode");
        server.write_all(&hs_bytes).await.expect("write");
        server.write_all(b"y").await.expect("write payload");

        // The handshake subscriber must receive the handshake SDU,
        // not the trace-object SDU.
        let received = tokio::time::timeout(Duration::from_secs(1), handshake_rx.recv())
            .await
            .expect("hs receive within 1s")
            .expect("subscriber channel produced an SDU");
        assert_eq!(
            received.header.mini_protocol_num,
            HANDSHAKE_MINI_PROTOCOL_NUM
        );
        assert_eq!(received.payload, b"y");
    }

    // ------------------------------------------------------------------
    // Slice (a): per-mini-protocol ingress queue limits.
    // ------------------------------------------------------------------

    /// The default ingress cap is the cardano-tracer `maxBound`
    /// analogue — effectively unbounded. A unit test pins it so a
    /// future regression that picks a smaller default (e.g. an
    /// NtN-style per-protocol cap) — which could regress the live
    /// conformance test — fails here first.
    #[test]
    fn default_ingress_limit_is_effectively_unbounded() {
        assert_eq!(
            MiniProtocolLimits::default(),
            MiniProtocolLimits::CARDANO_TRACER_DEFAULT
        );
        assert_eq!(
            MiniProtocolLimits::CARDANO_TRACER_DEFAULT.maximum_ingress_queue,
            i32::MAX as usize
        );
    }

    /// Write a 16 KiB payload through the default cap and confirm it
    /// is dispatched, not rejected. This pins the property that the
    /// cardano-tracer default stays large enough that no real trace
    /// SDU stream trips the over-run path — the conformance-test
    /// safety net.
    #[tokio::test]
    async fn default_cap_admits_a_large_sdu() {
        let (client, mut server) = tokio::io::duplex(64 * 1024);
        let conn = MuxConnection::new(Bearer::new(client));
        let mut rx = conn.subscribe(TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM).await;
        let _read_task = conn.spawn_read_task();

        // 16 KiB payload — comfortably larger than any single trace
        // SDU, comfortably smaller than the default cap.
        let payload = vec![0xABu8; 16 * 1024];
        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: payload.len() as u16,
        };
        let hdr_bytes = super::super::mux::encode_sdu_header(&header).expect("encode");
        use tokio::io::AsyncWriteExt;
        server.write_all(&hdr_bytes).await.expect("write header");
        server.write_all(&payload).await.expect("write payload");

        let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("receive within 1s")
            .expect("subscriber produced the SDU");
        assert_eq!(received.payload.len(), 16 * 1024);
    }

    /// Push past an explicit small ingress cap: the read-task must
    /// return [`MuxConnectionError::IngressQueueOverRun`] and exit,
    /// mirroring upstream `Network.Mux.Ingress.demuxer`'s
    /// `IngressQueueOverRun` throw + bearer tear-down.
    ///
    /// Setup: cap = 10 bytes; the subscriber's `IngressReceiver` is
    /// held but never drained, so dispatched bytes stay charged. The
    /// first 6-byte SDU fits (6 <= 10); the second 6-byte SDU would
    /// make the running total 12 > 10 and must trip the over-run.
    #[tokio::test]
    async fn ingress_queue_over_run_tears_down_read_task() {
        let (client, mut server) = tokio::io::duplex(4096);
        let conn = MuxConnection::new(Bearer::new(client));

        // Subscribe with a deliberately tiny 10-byte cap. Hold the
        // receiver so dispatched bytes are NOT freed.
        let _rx = conn
            .subscribe_with_limits(
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolLimits {
                    maximum_ingress_queue: 10,
                },
            )
            .await;
        let read_task = conn.spawn_read_task();

        use tokio::io::AsyncWriteExt;
        let write_sdu = |bytes: &'static [u8]| {
            let header = SduHeader {
                timestamp: 0,
                mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                direction: MiniProtocolDir::Responder,
                length: bytes.len() as u16,
            };
            let hdr = super::super::mux::encode_sdu_header(&header).expect("encode");
            (hdr, bytes)
        };

        // SDU 1: 6 bytes — fits (running total 6 <= 10).
        let (hdr1, p1) = write_sdu(b"aaaaaa");
        server.write_all(&hdr1).await.expect("write hdr1");
        server.write_all(p1).await.expect("write payload1");

        // SDU 2: 6 bytes — would make the running total 12 > 10.
        let (hdr2, p2) = write_sdu(b"bbbbbb");
        server.write_all(&hdr2).await.expect("write hdr2");
        server.write_all(p2).await.expect("write payload2");

        // The read-task must return IngressQueueOverRun and exit.
        let outcome = tokio::time::timeout(Duration::from_secs(2), read_task)
            .await
            .expect("read-task terminated within the timeout")
            .expect("read-task did not panic");
        match outcome {
            Err(MuxConnectionError::IngressQueueOverRun {
                mini_protocol_num,
                direction,
            }) => {
                assert_eq!(mini_protocol_num, TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM);
                assert_eq!(direction, MiniProtocolDir::Responder);
            }
            other => panic!("expected IngressQueueOverRun; got {other:?}"),
        }
    }

    /// Draining the `IngressReceiver` **frees** the queued bytes, so
    /// a steadily-consumed mini-protocol never trips the cap even
    /// when the cumulative byte volume far exceeds it. This pins the
    /// upstream semantic that the cap bounds the *un-drained* queue
    /// depth, not lifetime throughput.
    #[tokio::test]
    async fn draining_frees_ingress_bytes() {
        let (client, mut server) = tokio::io::duplex(4096);
        let conn = MuxConnection::new(Bearer::new(client));

        // 8-byte cap. Each SDU is 6 bytes — two un-drained SDUs (12
        // bytes) would overflow, but we drain after every SDU.
        let mut rx = conn
            .subscribe_with_limits(
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolLimits {
                    maximum_ingress_queue: 8,
                },
            )
            .await;
        let read_task = conn.spawn_read_task();

        use tokio::io::AsyncWriteExt;
        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 6,
        };
        let hdr = super::super::mux::encode_sdu_header(&header).expect("encode");

        // Send-then-drain five 6-byte SDUs = 30 bytes total, far over
        // the 8-byte cap, but never more than 6 un-drained at once.
        for i in 0..5u8 {
            server.write_all(&hdr).await.expect("write header");
            server.write_all(&[i; 6]).await.expect("write payload");
            let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("receive within 1s")
                .expect("subscriber produced the SDU");
            assert_eq!(received.payload, [i; 6]);
            // After draining, the queue is back to empty.
            assert_eq!(
                rx.queued_bytes(),
                0,
                "draining must free the SDU's payload bytes"
            );
        }

        // The read-task is still alive (no over-run): drop the
        // server end to EOF it cleanly.
        drop(server);
        let outcome = tokio::time::timeout(Duration::from_secs(2), read_task)
            .await
            .expect("read-task terminated within the timeout")
            .expect("read-task did not panic");
        assert!(
            matches!(outcome, Err(MuxConnectionError::Bearer(_))),
            "expected a clean bearer EOF, not an over-run; got {outcome:?}"
        );
    }

    /// The cap boundary is inclusive: an SDU whose payload exactly
    /// fills the cap is admitted (upstream check is `len' <= qMax`),
    /// and a subsequent single byte tips the running total over and
    /// over-runs.
    ///
    /// The `IngressReceiver` is held but **never** `recv()`-ed —
    /// `recv()` would free the bytes — so the exact-cap SDU stays
    /// charged at 5 and the next byte makes 6 > 5.
    #[tokio::test]
    async fn ingress_cap_boundary_is_inclusive() {
        let (client, mut server) = tokio::io::duplex(4096);
        let conn = MuxConnection::new(Bearer::new(client));
        // Hold the receiver without draining it: dispatched bytes
        // stay charged.
        let rx = conn
            .subscribe_with_limits(
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolLimits {
                    maximum_ingress_queue: 5,
                },
            )
            .await;
        let read_task = conn.spawn_read_task();

        use tokio::io::AsyncWriteExt;
        // Exactly-5-byte SDU: 5 <= 5, admitted.
        let header5 = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 5,
        };
        let hdr5 = super::super::mux::encode_sdu_header(&header5).expect("encode");
        server.write_all(&hdr5).await.expect("write header");
        server.write_all(b"fffff").await.expect("write payload");

        // Spin until the read-task has dispatched the exact-cap SDU
        // (charged 5/5). The receiver is never drained, so once it
        // reaches 5 it stays there.
        loop {
            if rx.queued_bytes() == 5 {
                break;
            }
            tokio::task::yield_now().await;
        }
        // The exact-cap SDU was admitted, not rejected: the read-task
        // is still alive.
        assert!(!read_task.is_finished(), "exact-cap SDU must be admitted");

        // One more byte tips the running total to 6 > 5.
        let header1 = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 1,
        };
        let hdr1 = super::super::mux::encode_sdu_header(&header1).expect("encode");
        server.write_all(&hdr1).await.expect("write header");
        server.write_all(b"g").await.expect("write payload");

        let outcome = tokio::time::timeout(Duration::from_secs(2), read_task)
            .await
            .expect("read-task terminated within the timeout")
            .expect("read-task did not panic");
        assert!(
            matches!(outcome, Err(MuxConnectionError::IngressQueueOverRun { .. })),
            "one byte past the cap must over-run; got {outcome:?}"
        );
        drop(rx);
    }
}
