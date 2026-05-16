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
//! ## Egress scheduler (slice b)
//!
//! The egress side is now routed through a port of upstream
//! `Network.Mux.Egress` — see [`super::egress`]. The lifecycle is a
//! faithful mirror of upstream `Network.Mux.hs`, where the
//! `muxer`/`demuxer` job pair is forked **after** the Handshake
//! mini-protocol has already completed over a plain direct-write
//! `bearerAsChannel`:
//!
//! - [`MuxConnection::run_initiator_handshake`] still does its own
//!   direct `writer.write_sdu` / `reader.read_sdu` exchange. No
//!   scheduler is involved — exactly as upstream runs the handshake
//!   before the muxer exists. The handshake conformance test is
//!   therefore unaffected by this slice.
//! - [`MuxConnection::spawn_muxer_task`] constructs the egress
//!   channel ([`super::egress::egress_channel`]), stashes the
//!   producer-side [`super::egress::EgressDemand`] handle, and spawns
//!   the `muxer` task ([`super::egress::run_muxer`]) on the **writer**
//!   half. It MUST be called after the handshake and before any
//!   [`MuxConnection::send_sdu`].
//! - After `spawn_muxer_task`, [`MuxConnection::send_sdu`] no longer
//!   writes directly: it `enqueue`s a [`super::egress::Wanton`]-style
//!   demand onto the shared FIFO and returns as soon as the demand is
//!   queued. The muxer drains the FIFO, segments to `sdu_size`, and
//!   batches up to `MAX_SDUS_PER_BATCH` SDUs per bearer write.
//!   Round-robin fairness across mini-protocols emerges from the
//!   FIFO + re-enqueue-on-remainder discipline.
//! - Before `spawn_muxer_task` (i.e. during the handshake window),
//!   `send_sdu` falls back to a direct `writer.lock() + write_sdu` so
//!   the API stays usable for callers that never spawn a muxer (the
//!   pre-existing `send_sdu` round-trip unit tests).
//!
//! ## Bearer-task supervision (slice c)
//!
//! [`MuxConnection::run`] is the supervised lifecycle entry point and
//! the last slice of the full `Network.Mux` arc. It mirrors upstream
//! `Network.Mux.run` (`Network/Mux.hs`), which forks the
//! `muxer`/`demuxer` job pair into a `JobPool` and runs a `monitor`
//! loop: the first job to fail tears the whole mux down, and the
//! `withJobPool` `bracket` cancels every still-running sibling.
//!
//! `run` owns a `tokio::task::JoinSet` of exactly two jobs — the
//! read-task ([`Self::spawn_read_task`], the demuxer analogue) and the
//! muxer-task ([`Self::spawn_muxer_task`], the egress muxer). It
//! `select`s the first job to finish:
//!
//! - **First job fails** → the supervisor `abort`s the sibling and
//!   drains it. Mirrors upstream's `MuxerException` / `DemuxerException`
//!   → `Failed` → `throwIO`, plus `withJobPool`'s sibling-cancel. The
//!   per-job outcomes are surfaced in [`MuxRunResult`]; `first_failure`
//!   names the job that triggered the tear-down.
//! - **First job exits `Ok`** → the supervisor waits a bounded grace
//!   period for the sibling to also finish cleanly; if it does, both
//!   outcomes are `Ok`. This is the clean-shutdown path: the producer
//!   drops the egress demand (muxer drains its FIFO and exits `Ok`)
//!   and the bearer EOFs (read-task's `read_sdu` returns
//!   `UnexpectedEof`). A read-task `UnexpectedEof` **after** the muxer
//!   has exited `Ok` is re-classified as a clean shutdown — the
//!   Yggdrasil analogue of upstream's `DemuxerException BearerClosed`
//!   "when all mini-protocols stopped indicates a normal shutdown".
//!
//! `run` does NOT fork one job per mini-protocol — upstream does, but
//! cardano-tracer's two-sub-protocol use (Handshake then TraceObject)
//! does not need it, and that would widen the slice past a
//! risk-bounded change. [`Self::spawn_read_task`] /
//! [`Self::spawn_muxer_task`] stay public as escape hatches for
//! callers (and unit tests) that want to drive the two halves
//! directly.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::bearer::{Bearer, BearerError, BearerReader, BearerWriter};
use super::egress::{EgressConfig, EgressDemand, EgressError, egress_channel, run_muxer};
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
    /// The egress scheduler reported a failure — either a fatal
    /// bearer write inside the muxer or an `enqueue` after the muxer
    /// task already exited. Surfaced from [`MuxConnection::send_sdu`]
    /// once a muxer has been spawned. Mirrors upstream
    /// `Network.Mux.hs`'s "muxer exception is always fatal".
    Egress(EgressError),
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
            Self::Egress(e) => write!(f, "mux connection egress error: {e}"),
        }
    }
}

impl std::error::Error for MuxConnectionError {}

/// Outcome of one supervised bearer job (the read-task or the
/// muxer-task) as seen by [`MuxConnection::run`].
///
/// Mirrors the per-job result discrimination upstream `Network.Mux`'s
/// `monitor` loop draws from a `JobResult`: a job either ran to a
/// clean finish, failed with an error, or was cancelled by the
/// supervisor because its sibling failed first.
#[derive(Debug)]
pub enum JobOutcome<E> {
    /// The job finished cleanly. For the read-task this is a bearer
    /// EOF at a frame boundary after the muxer already stopped (the
    /// Yggdrasil analogue of upstream `DemuxerException BearerClosed`
    /// "normal shutdown"); for the muxer-task this is every
    /// [`EgressDemand`] producer being dropped so the muxer drained
    /// its FIFO and returned `Ok`.
    Completed,
    /// The job returned an error from its `Result`.
    Failed(E),
    /// The supervisor aborted this job because its sibling failed
    /// (or finished) first. Mirrors upstream `withJobPool`'s
    /// `uninterruptibleCancel` of still-running sibling jobs.
    ///
    /// The discriminator is `tokio::task::JoinError::is_cancelled` —
    /// "the abort signal won the race", not merely "the task produced
    /// no result". A job aborted mid-`read_sdu` may instead surface
    /// as [`Self::Failed`] (e.g. a bearer `UnexpectedEof`) if the
    /// in-flight read resolves before the abort fires; both are valid
    /// tear-down signals and `first_failure` is the field that names
    /// the actual culprit.
    Cancelled,
    /// The job's task panicked (a `JoinError` that is not a
    /// cancellation).
    Panicked,
}

/// Which supervised bearer job triggered a [`MuxConnection::run`]
/// tear-down.
///
/// Mirrors the upstream distinction between a `MuxerException` and a
/// `DemuxerException` in `Network.Mux`'s `monitor` loop — both are
/// fatal, but knowing which side failed first is the diagnostic the
/// operator needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailedJob {
    /// The read-task (the demuxer analogue) failed first.
    ReadTask,
    /// The muxer-task (the egress muxer) failed first.
    MuxerTask,
}

/// Result of a supervised [`MuxConnection::run`] lifecycle.
///
/// Mirrors upstream `Network.Mux`'s terminal `Status` (`Stopped` vs
/// `Failed`): when `first_failure` is `None` both jobs finished
/// cleanly (`Stopped`); when it is `Some(job)` that job failed (or
/// panicked) and the supervisor tore the bearer down (`Failed`),
/// aborting the sibling.
#[derive(Debug)]
pub struct MuxRunResult {
    /// Outcome of the read-task (demuxer analogue).
    pub read_outcome: JobOutcome<MuxConnectionError>,
    /// Outcome of the muxer-task (egress muxer). `None` when `run`
    /// was called without a muxer (no [`EgressConfig`] supplied).
    pub muxer_outcome: Option<JobOutcome<EgressError>>,
    /// The job whose failure tore the bearer down, or `None` for a
    /// clean shutdown of both jobs.
    pub first_failure: Option<FailedJob>,
}

impl MuxRunResult {
    /// `true` when both supervised jobs finished cleanly — the
    /// upstream `Stopped` terminal status.
    pub fn is_clean_shutdown(&self) -> bool {
        self.first_failure.is_none()
    }
}

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
    /// Producer-side handle for the egress scheduler — `Some` once
    /// [`Self::spawn_muxer_task`] has been called, `None` before
    /// that (the handshake window). When `Some`, [`Self::send_sdu`]
    /// `enqueue`s onto the shared FIFO instead of writing directly.
    /// Behind a tokio Mutex so `spawn_muxer_task` can install it and
    /// `send_sdu` can read it without a data race.
    ///
    /// [`Self::shutdown`] sets this back to `None`, which drops the
    /// last [`EgressDemand`] and lets the muxer-task drain its FIFO
    /// and exit `Ok` — the producer-drop half of a clean shutdown.
    egress: Arc<Mutex<Option<EgressDemand>>>,
    /// Set by [`Self::shutdown`]. The supervised [`Self::run`] loop
    /// consults this when classifying a read-task `UnexpectedEof`: an
    /// EOF observed *after* an operator-requested shutdown is a clean
    /// stop (upstream `Network.Mux`'s `DemuxerException BearerClosed`
    /// "normal shutdown"), not a fault. Shared (`Arc`) so the
    /// `run` future and `shutdown` callers see the same flag.
    shutdown_initiated: Arc<AtomicBool>,
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
            egress: Arc::new(Mutex::new(None)),
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request a clean shutdown of the supervised bearer lifecycle.
    ///
    /// This is Yggdrasil's analogue of upstream `Network.Mux.stop`
    /// (which writes `CmdShutdown` to the mux control queue; the
    /// `monitor` loop then unwinds the job pool). It does two things:
    ///
    /// 1. Drops the stashed [`EgressDemand`] (sets `self.egress` back
    ///    to `None`). With no producer handle left, the muxer-task's
    ///    wake channel closes; the muxer drains whatever is still on
    ///    its FIFO and returns `Ok` — the producer-drop half of a
    ///    clean shutdown. A `send_sdu` issued after this falls back to
    ///    the direct-write path.
    /// 2. Sets [`Self::shutdown_initiated`] so that when the bearer
    ///    subsequently EOFs and the read-task returns
    ///    `UnexpectedEof`, [`Self::run`]'s supervisor re-classifies
    ///    that EOF as a clean stop ([`JobOutcome::Completed`]) rather
    ///    than a fault.
    ///
    /// Idempotent: calling it twice is harmless. It does NOT itself
    /// close the bearer — the caller still EOFs the transport (or the
    /// peer does) to wind the read-task down. After both halves stop,
    /// [`Self::run`] returns a [`MuxRunResult`] with
    /// `first_failure == None`.
    pub async fn shutdown(&self) {
        self.shutdown_initiated.store(true, Ordering::Release);
        *self.egress.lock().await = None;
    }

    /// `true` once the egress scheduler (`muxer`) has been installed
    /// — i.e. [`Self::spawn_muxer_task`] (directly, or via
    /// [`Self::run`]) has stashed the [`EgressDemand`]. Test-only
    /// introspection: lets a test deterministically wait for the
    /// muxer to be live before issuing a [`Self::send_sdu`] that must
    /// route through the scheduler rather than the direct-write
    /// fallback.
    #[cfg(test)]
    pub(crate) async fn muxer_installed(&self) -> bool {
        self.egress.lock().await.is_some()
    }

    /// Send one outbound SDU.
    ///
    /// **After [`Self::spawn_muxer_task`]** this `enqueue`s the
    /// payload as a [`super::egress`] demand onto the shared egress
    /// FIFO and returns as soon as the demand is queued — the bytes
    /// are **not** yet on the wire when this returns. The muxer task
    /// drains the FIFO, segments to `sdu_size`, batches, and writes.
    /// Round-robin fairness across mini-protocols comes from the
    /// FIFO + re-enqueue-on-remainder. This is the upstream
    /// `muxChannel.send` semantic.
    ///
    /// **Before `spawn_muxer_task`** (the handshake window, or any
    /// caller that never spawns a muxer) this falls back to a direct
    /// `writer.lock() + write_sdu` round-trip, serialised against
    /// other direct callers through the writer mutex. Never contends
    /// with the read-task: the read-loop locks the separate reader
    /// mutex.
    ///
    /// The `header.timestamp` is honoured only on the direct-write
    /// path; on the scheduler path the muxer stamps its own header
    /// (timestamp 0, mirroring upstream `processSingleWanton` which
    /// sets `RemoteClockModel 0`) — the trace-forward protocol treats
    /// the SDU timestamp as informational, so this is parity-correct.
    pub async fn send_sdu(
        &self,
        header: &SduHeader,
        payload: &[u8],
    ) -> Result<(), MuxConnectionError> {
        // Fast-path: a muxer has been spawned — enqueue the demand.
        {
            let egress = self.egress.lock().await;
            if let Some(demand) = egress.as_ref() {
                return demand
                    .enqueue(header.mini_protocol_num, header.direction, payload.to_vec())
                    .await
                    .map_err(MuxConnectionError::Egress);
            }
        }
        // No muxer yet: direct write (handshake window / muxer-less
        // callers).
        let mut writer = self.writer.lock().await;
        writer
            .write_sdu(header, payload)
            .await
            .map_err(MuxConnectionError::Bearer)
    }

    /// Spawn the egress scheduler (`muxer`) task and switch
    /// [`Self::send_sdu`] over to the enqueue path.
    ///
    /// Faithful mirror of upstream `Network.Mux.hs`, which forks the
    /// `muxer` job (`forkJob jobpool (muxerJob egressQueue)`) only
    /// **after** the Handshake mini-protocol has finished over a
    /// plain direct-write channel. Therefore this MUST be called
    /// **after** [`Self::run_initiator_handshake`] and **before** the
    /// first scheduler-routed [`Self::send_sdu`].
    ///
    /// Returns the muxer's `JoinHandle` so the caller can await it on
    /// shutdown. The muxer runs until every [`super::egress::EgressDemand`]
    /// is dropped (clean shutdown — it drains the FIFO first) or a
    /// bearer write fails (fatal). The muxer locks only the **writer**
    /// half, so it never contends with the read-task.
    ///
    /// `config` selects the segmentation `sdu_size` and the per-batch
    /// byte budget; pass [`EgressConfig::default`] for the
    /// trace-forwarder bearer (`u16::MAX` SDUs — no segmentation of a
    /// real trace SDU).
    ///
    /// Calling this twice replaces the stashed [`super::egress::EgressDemand`]
    /// with a fresh channel; the previous muxer's producer side is
    /// dropped, so the previous muxer drains its FIFO and exits.
    /// Callers should spawn the muxer exactly once per connection.
    pub fn spawn_muxer_task(
        &self,
        config: EgressConfig,
    ) -> tokio::task::JoinHandle<Result<(), EgressError>> {
        let (demand, muxer) = egress_channel();
        // Install the producer handle synchronously so a `send_sdu`
        // immediately after this call already routes through the
        // scheduler. The documented call contract is "after the
        // handshake, before the first scheduler-routed send_sdu", so
        // no other task holds the `egress` lock at this point — the
        // `try_lock` always succeeds. The `else` arm is a
        // defence-in-depth detached install (the muxer is already
        // spawned below either way, so no demand is ever lost).
        match self.egress.try_lock() {
            Ok(mut slot) => *slot = Some(demand),
            Err(_) => {
                let egress = Arc::clone(&self.egress);
                tokio::spawn(async move {
                    *egress.lock().await = Some(demand);
                });
            }
        }
        let writer = Arc::clone(&self.writer);
        tokio::spawn(run_muxer(muxer, writer, config))
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

    /// Run the supervised bearer lifecycle: fork the read-task and the
    /// muxer-task into one [`JoinSet`], wait for the first to finish,
    /// and tear the other down cohesively.
    ///
    /// This is the slice-(c) entry point and a faithful mirror of
    /// upstream `Network.Mux.run` (`Network/Mux.hs`): `run` forks the
    /// `muxer`/`demuxer` job pair into a `JobPool` and a `monitor`
    /// loop watches them — the first job to fail propagates its
    /// exception and `withJobPool`'s `bracket` `uninterruptibleCancel`s
    /// the still-running sibling. Yggdrasil's [`JoinSet`] gives the
    /// same "first-to-finish wins, abort the rest" structure.
    ///
    /// Call contract — the same ordering upstream uses (handshake over
    /// a direct-write channel, *then* fork the job pair):
    ///
    /// 1. [`Self::run_initiator_handshake`] FIRST (direct bearer
    ///    write/read; no supervised job exists yet).
    /// 2. [`Self::subscribe`] / [`Self::subscribe_with_limits`] for
    ///    every inbound mini-protocol — BEFORE `run`, because the
    ///    read-task drops SDUs with no registered subscriber.
    /// 3. `run(Some(config))` — forks both jobs and supervises them.
    ///
    /// `egress_config`:
    /// - `Some(config)` — fork the muxer-task; [`Self::send_sdu`]
    ///   routes through the egress scheduler. This is the
    ///   cardano-tracer path.
    /// - `None` — supervise the read-task only; `send_sdu` keeps the
    ///   direct-write fallback. `muxer_outcome` in the result is
    ///   `None`.
    ///
    /// ### Outcome classification
    ///
    /// - **A job returns `Err` or panics** → the supervisor `abort`s
    ///   the sibling, drains the [`JoinSet`], and reports
    ///   `first_failure = Some(job)`; the aborted sibling's outcome is
    ///   [`JobOutcome::Cancelled`]. Mirrors upstream's `MuxerException`
    ///   / `DemuxerException` → `Failed` tear-down.
    /// - **A job returns `Ok`** → the supervisor waits a bounded grace
    ///   period ([`MUX_RUN_SHUTDOWN_GRACE`]) for the sibling to also
    ///   finish. If the sibling finishes `Ok` within the grace,
    ///   `first_failure = None` (clean shutdown — upstream `Stopped`).
    ///   If it does not, it is aborted and reported `Cancelled` with
    ///   `first_failure` naming the *sibling* (it failed to shut down
    ///   in time — a soft failure).
    /// - **Read-task `UnexpectedEof` after the muxer already exited
    ///   `Ok`, or after [`Self::shutdown`] was called** →
    ///   re-classified [`JobOutcome::Completed`], not `Failed`. This
    ///   is the clean producer-drop + bearer-EOF path: the Yggdrasil
    ///   analogue of upstream treating a `DemuxerException
    ///   BearerClosed` "when all mini-protocols stopped" as a normal
    ///   shutdown rather than a fault.
    pub async fn run(&self, egress_config: Option<EgressConfig>) -> MuxRunResult {
        // One JoinSet of both jobs. The two jobs have different
        // `Result` error types, so each spawned future maps its
        // outcome into the shared `JobReport` discriminant before the
        // JoinSet ever sees it.
        let mut jobs: JoinSet<JobReport> = JoinSet::new();

        // Demuxer analogue — the read-task.
        let read_handle = self.spawn_read_task();
        jobs.spawn(async move {
            match read_handle.await {
                Ok(Ok(())) => JobReport::Read(JobTermination::Ok),
                Ok(Err(e)) => JobReport::Read(JobTermination::Err(e)),
                Err(join_err) => JobReport::Read(join_termination(join_err)),
            }
        });

        // Egress muxer — only when a config was supplied.
        let has_muxer = egress_config.is_some();
        if let Some(config) = egress_config {
            let muxer_handle = self.spawn_muxer_task(config);
            jobs.spawn(async move {
                match muxer_handle.await {
                    Ok(Ok(())) => JobReport::Muxer(JobTermination::Ok),
                    Ok(Err(e)) => JobReport::Muxer(JobTermination::Err(e)),
                    Err(join_err) => JobReport::Muxer(join_termination(join_err)),
                }
            });
        }

        supervise(jobs, has_muxer, Arc::clone(&self.shutdown_initiated)).await
    }
}

/// Bounded grace period the supervisor waits for the *sibling* job to
/// finish after the first job has exited cleanly.
///
/// Upstream `Network.Mux`'s `monitor` waits 2 s for the egress queue
/// to drain on a `CmdShutdown` (`timeout 2 $ … tryPeekTBQueue`).
/// Yggdrasil uses the same 2 s budget: a clean producer-drop has the
/// muxer drain its FIFO and the read-task observe bearer EOF promptly,
/// so 2 s is generous; a sibling that overshoots it is aborted.
pub const MUX_RUN_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// How one supervised job terminated, with its error type erased into
/// the shared report.
enum JobTermination<E> {
    /// The job's `Result` was `Ok(())`.
    Ok,
    /// The job's `Result` was `Err`.
    Err(E),
    /// The job's task was cancelled (aborted) — `JoinError::is_cancelled`.
    Cancelled,
    /// The job's task panicked.
    Panicked,
}

/// Classify a `tokio::task::JoinError` into the [`JobTermination`]
/// discriminant. A cancelled task (`abort`) is NOT a failure — it is
/// the supervisor's own doing — so it maps to `Cancelled`, not
/// `Panicked`.
fn join_termination<E>(join_err: tokio::task::JoinError) -> JobTermination<E> {
    if join_err.is_cancelled() {
        JobTermination::Cancelled
    } else {
        JobTermination::Panicked
    }
}

/// One supervised job's identity + termination, as the [`JoinSet`]
/// in [`MuxConnection::run`] yields it.
enum JobReport {
    /// The read-task (demuxer analogue) terminated.
    Read(JobTermination<MuxConnectionError>),
    /// The muxer-task (egress muxer) terminated.
    Muxer(JobTermination<EgressError>),
}

/// Convert a read-task [`JobTermination`] into the public
/// [`JobOutcome`]. `eof_is_clean` carries the slice-(c)
/// re-classification: a bearer `UnexpectedEof` is a clean shutdown —
/// not a fault — when the muxer has already finished `Ok`, when an
/// operator [`MuxConnection::shutdown`] was requested, or when there
/// is no muxer at all (a read-only `run`). It is then reported
/// [`JobOutcome::Completed`] rather than [`JobOutcome::Failed`]
/// (upstream `BearerClosed` "normal shutdown").
fn read_outcome(
    term: JobTermination<MuxConnectionError>,
    eof_is_clean: bool,
) -> JobOutcome<MuxConnectionError> {
    match term {
        JobTermination::Ok => JobOutcome::Completed,
        JobTermination::Cancelled => JobOutcome::Cancelled,
        JobTermination::Panicked => JobOutcome::Panicked,
        JobTermination::Err(MuxConnectionError::Bearer(BearerError::UnexpectedEof))
            if eof_is_clean =>
        {
            JobOutcome::Completed
        }
        JobTermination::Err(e) => JobOutcome::Failed(e),
    }
}

/// Convert a muxer-task [`JobTermination`] into the public
/// [`JobOutcome`].
fn muxer_outcome(term: JobTermination<EgressError>) -> JobOutcome<EgressError> {
    match term {
        JobTermination::Ok => JobOutcome::Completed,
        JobTermination::Cancelled => JobOutcome::Cancelled,
        JobTermination::Panicked => JobOutcome::Panicked,
        JobTermination::Err(e) => JobOutcome::Failed(e),
    }
}

/// `true` when a [`JobTermination`] represents a fault (an `Err` or a
/// panic) — i.e. a tear-down trigger. A cancellation is the
/// supervisor's own doing and is NOT a fault.
fn termination_is_fault<E>(term: &JobTermination<E>) -> bool {
    matches!(term, JobTermination::Err(_) | JobTermination::Panicked)
}

/// `true` when a read-task [`JobTermination`] is a bearer
/// `UnexpectedEof` AND an operator shutdown has been requested — i.e.
/// the EOF is the expected end of a clean shutdown, not a fault.
/// Mirrors upstream `Network.Mux` treating `DemuxerException
/// BearerClosed` as a normal stop "when all mini-protocols stopped".
fn read_eof_is_clean_shutdown(
    term: &JobTermination<MuxConnectionError>,
    shutdown_initiated: &AtomicBool,
) -> bool {
    matches!(
        term,
        JobTermination::Err(MuxConnectionError::Bearer(BearerError::UnexpectedEof))
    ) && shutdown_initiated.load(Ordering::Acquire)
}

/// Drive the two-job [`JoinSet`] supervision loop. Factored out of
/// [`MuxConnection::run`] so the body is independent of the generic
/// `S` transport parameter — the [`JoinSet`] only ever yields the
/// already-type-erased [`JobReport`].
///
/// `shutdown_initiated` is consulted when classifying a read-task
/// `UnexpectedEof`: an EOF after [`MuxConnection::shutdown`] is a
/// clean stop, never a fault — so the supervisor must not treat it as
/// a tear-down trigger even when the read-task is the FIRST job to
/// finish.
async fn supervise(
    mut jobs: JoinSet<JobReport>,
    has_muxer: bool,
    shutdown_initiated: Arc<AtomicBool>,
) -> MuxRunResult {
    // First job to finish. `join_next` yields `None` only if the set
    // is empty — `run` always spawns at least the read-task, so the
    // first `join_next` is always `Some`.
    let first = match jobs.join_next().await {
        Some(Ok(report)) => report,
        // The JoinSet wrapper futures never panic and are never
        // aborted before this point, so a `JoinError` here is not
        // reachable in practice; treat it conservatively as a
        // read-task panic so the function stays total.
        Some(Err(_)) | None => {
            return MuxRunResult {
                read_outcome: JobOutcome::Panicked,
                muxer_outcome: has_muxer.then_some(JobOutcome::Cancelled),
                first_failure: Some(FailedJob::ReadTask),
            };
        }
    };

    // Did the first job fault? If so we tear the sibling down. A
    // read-task `UnexpectedEof` is NOT a fault when it is the
    // expected end of a clean stop: after an operator shutdown, or —
    // for a muxer-less `run(None)` — any bearer EOF (there is no
    // egress side that could still be mid-shutdown).
    let read_eof_clean = |t: &JobTermination<MuxConnectionError>| {
        read_eof_is_clean_shutdown(t, &shutdown_initiated)
            || (!has_muxer
                && matches!(
                    t,
                    JobTermination::Err(MuxConnectionError::Bearer(BearerError::UnexpectedEof))
                ))
    };
    let first_faulted = match &first {
        JobReport::Read(t) => termination_is_fault(t) && !read_eof_clean(t),
        JobReport::Muxer(t) => termination_is_fault(t),
    };

    let mut read_term: Option<JobTermination<MuxConnectionError>> = None;
    let mut muxer_term: Option<JobTermination<EgressError>> = None;
    // Which job triggered the tear-down (a fault, or an overshoot of
    // the shutdown grace period). `None` once both jobs are accounted
    // for cleanly.
    let mut trigger: Option<FailedJob> = None;
    record_report(first, &mut read_term, &mut muxer_term);

    if first_faulted {
        // First job FAILED — name it as the trigger, abort the
        // sibling, and drain. Mirrors upstream `withJobPool`'s
        // `uninterruptibleCancel` of the still-running sibling once
        // one job throws.
        trigger = Some(match (&read_term, &muxer_term) {
            (Some(_), _) => FailedJob::ReadTask,
            (_, Some(_)) => FailedJob::MuxerTask,
            // Unreachable: `record_report` of the faulted first job
            // filled exactly one slot.
            (None, None) => FailedJob::ReadTask,
        });
        jobs.abort_all();
        drain_aborted(&mut jobs, &mut read_term, &mut muxer_term).await;
        return finalize(
            read_term,
            muxer_term,
            trigger,
            has_muxer,
            &shutdown_initiated,
        );
    }

    // First job finished CLEANLY. If `run` was called without a
    // muxer there is no sibling — the single read-task finishing is
    // the whole shutdown.
    if !has_muxer {
        return finalize(
            read_term,
            muxer_term,
            trigger,
            has_muxer,
            &shutdown_initiated,
        );
    }

    // Wait a bounded grace period for the sibling to also finish.
    match tokio::time::timeout(MUX_RUN_SHUTDOWN_GRACE, jobs.join_next()).await {
        Ok(Some(Ok(report))) => {
            // Sibling finished on its own. If it faulted, name it as
            // the trigger; `finalize` then reports it `Failed`. A
            // read-task `UnexpectedEof` is a clean stop — not a fault
            // — when the FIRST job was the muxer finishing `Ok` (the
            // bearer then closed as part of that clean shutdown) or
            // after an operator [`MuxConnection::shutdown`].
            let muxer_finished_ok = matches!(muxer_term, Some(JobTermination::Ok));
            let sibling_faulted = match &report {
                JobReport::Read(t) => {
                    termination_is_fault(t)
                        && !read_eof_is_clean_shutdown(t, &shutdown_initiated)
                        && !(muxer_finished_ok
                            && matches!(
                                t,
                                JobTermination::Err(MuxConnectionError::Bearer(
                                    BearerError::UnexpectedEof
                                ))
                            ))
                }
                JobReport::Muxer(t) => termination_is_fault(t),
            };
            if sibling_faulted {
                trigger = Some(match report {
                    JobReport::Read(_) => FailedJob::ReadTask,
                    JobReport::Muxer(_) => FailedJob::MuxerTask,
                });
            }
            record_report(report, &mut read_term, &mut muxer_term);
        }
        Ok(Some(Err(_))) | Ok(None) => {
            // Sibling's wrapper future panicked or the set drained
            // unexpectedly — fill the empty slot as a panic and name
            // it the trigger.
            if read_term.is_none() {
                read_term = Some(JobTermination::Panicked);
                trigger = Some(FailedJob::ReadTask);
            } else if muxer_term.is_none() {
                muxer_term = Some(JobTermination::Panicked);
                trigger = Some(FailedJob::MuxerTask);
            }
        }
        Err(_elapsed) => {
            // The sibling overshot the grace period — IT is the
            // trigger (failing to shut down in time is a soft
            // fault). Abort it and drain.
            trigger = Some(if read_term.is_none() {
                FailedJob::ReadTask
            } else {
                FailedJob::MuxerTask
            });
            jobs.abort_all();
            drain_aborted(&mut jobs, &mut read_term, &mut muxer_term).await;
        }
    }
    finalize(
        read_term,
        muxer_term,
        trigger,
        has_muxer,
        &shutdown_initiated,
    )
}

/// Drain a [`JoinSet`] after [`JoinSet::abort_all`], recording each
/// yielded job into the matching termination slot. A yielded job that
/// already mapped its own outcome keeps that outcome (it finished in
/// the abort race); a job whose wrapper future was aborted before it
/// could map a result is recorded [`JobTermination::Cancelled`] on
/// whichever slot is still empty.
async fn drain_aborted(
    jobs: &mut JoinSet<JobReport>,
    read_term: &mut Option<JobTermination<MuxConnectionError>>,
    muxer_term: &mut Option<JobTermination<EgressError>>,
) {
    while let Some(joined) = jobs.join_next().await {
        match joined {
            Ok(report) => record_report(report, read_term, muxer_term),
            Err(_) => {
                if read_term.is_none() {
                    *read_term = Some(JobTermination::Cancelled);
                } else if muxer_term.is_none() {
                    *muxer_term = Some(JobTermination::Cancelled);
                }
            }
        }
    }
}

/// Route one [`JobReport`] into the matching `read_term` / `muxer_term`
/// slot.
fn record_report(
    report: JobReport,
    read_term: &mut Option<JobTermination<MuxConnectionError>>,
    muxer_term: &mut Option<JobTermination<EgressError>>,
) {
    match report {
        JobReport::Read(t) => *read_term = Some(t),
        JobReport::Muxer(t) => *muxer_term = Some(t),
    }
}

/// Assemble the final [`MuxRunResult`] from the two collected
/// terminations and the supervisor's `trigger` decision.
///
/// `trigger` is the job the supervisor identified as causing the
/// tear-down — a faulted job, an over-grace-period job, or `None` for
/// a clean shutdown. The read-task's `UnexpectedEof` re-classification
/// is applied here via [`read_outcome`]: the EOF is treated as a
/// clean stop when the muxer finished `Ok`, when an operator
/// [`MuxConnection::shutdown`] was requested, or when there is no
/// muxer (`has_muxer == false`).
fn finalize(
    read_term: Option<JobTermination<MuxConnectionError>>,
    muxer_term: Option<JobTermination<EgressError>>,
    trigger: Option<FailedJob>,
    has_muxer: bool,
    shutdown_initiated: &AtomicBool,
) -> MuxRunResult {
    // Gate for the read-task's `UnexpectedEof` → `Completed`
    // re-classification: the muxer finished `Ok`, an operator
    // shutdown was requested, or there is no muxer at all.
    let eof_is_clean = matches!(muxer_term, Some(JobTermination::Ok))
        || shutdown_initiated.load(Ordering::Acquire)
        || !has_muxer;
    // A missing read termination should be impossible (the read-task
    // is always spawned); default to a panic so the fault surfaces.
    let read_term = read_term.unwrap_or(JobTermination::Panicked);

    MuxRunResult {
        read_outcome: read_outcome(read_term, eof_is_clean),
        muxer_outcome: muxer_term.map(muxer_outcome),
        first_failure: trigger,
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

    // ------------------------------------------------------------------
    // Slice (b): egress scheduler.
    // ------------------------------------------------------------------

    /// After [`MuxConnection::spawn_muxer_task`], `send_sdu` routes
    /// through the egress scheduler: the demand is enqueued, the
    /// muxer drains it, and the SDU reaches the wire byte-identical
    /// to a direct write.
    #[tokio::test]
    async fn send_sdu_via_muxer_reaches_the_wire() {
        use crate::trace_forwarder::egress::EgressConfig;

        let (client, server) = tokio::io::duplex(8192);
        let conn = MuxConnection::new(Bearer::new(client));
        let mut server_bearer = Bearer::new(server);

        // Spawn the muxer — switches send_sdu to the enqueue path.
        let _muxer = conn.spawn_muxer_task(EgressConfig::default());

        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Initiator,
            length: 9,
        };
        conn.send_sdu(&header, b"scheduled")
            .await
            .expect("send_sdu via muxer");

        let (got_header, got_payload) =
            tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                .await
                .expect("read within 2s")
                .expect("read sdu");
        assert_eq!(
            got_header.mini_protocol_num,
            TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM
        );
        assert_eq!(got_header.direction, MiniProtocolDir::Initiator);
        assert_eq!(got_payload, b"scheduled");
    }

    /// Before `spawn_muxer_task`, `send_sdu` still does a direct
    /// write — the pre-scheduler behaviour the handshake window and
    /// muxer-less callers depend on. This is the regression guard
    /// for the handshake-conformance path: the handshake never
    /// spawns a muxer, so its SDUs must keep flowing through the
    /// direct-write fallback.
    #[tokio::test]
    async fn send_sdu_without_muxer_writes_directly() {
        let (client, server) = tokio::io::duplex(8192);
        let conn = MuxConnection::new(Bearer::new(client));
        let mut server_bearer = Bearer::new(server);

        // No spawn_muxer_task → direct-write fallback.
        let header = SduHeader {
            timestamp: 7,
            mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Initiator,
            length: 6,
        };
        conn.send_sdu(&header, b"direct")
            .await
            .expect("direct send_sdu");

        let (got_header, got_payload) =
            tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                .await
                .expect("read within 2s")
                .expect("read sdu");
        // Direct write honours the caller's timestamp.
        assert_eq!(got_header, header);
        assert_eq!(got_payload, b"direct");
    }

    /// Egress fairness through the `MuxConnection` API: with a forced
    /// tiny `sdu_size`, two `send_sdu` calls on different
    /// mini-protocols — issued back-to-back before the muxer has a
    /// chance to drain — have their SDUs round-robin-interleaved on
    /// the wire. This is the connection-level proof of the slice-(b)
    /// fairness property (the unit-level proof lives in
    /// `egress::egress_tests::muxer_round_robins_between_two_mini_protocols`).
    #[tokio::test]
    async fn send_sdu_via_muxer_round_robins_mini_protocols() {
        use crate::trace_forwarder::egress::EgressConfig;

        let (client, server) = tokio::io::duplex(64 * 1024);
        let conn = MuxConnection::new(Bearer::new(client));
        let mut server_bearer = Bearer::new(server);

        // 2-byte SDUs so each 6-byte payload spans 3 SDUs.
        let _muxer = conn.spawn_muxer_task(EgressConfig {
            sdu_size: 2,
            batch_size: u16::MAX as usize,
        });

        const PROTO_A: u16 = 2;
        const PROTO_B: u16 = 3;
        let header = |num: u16| SduHeader {
            timestamp: 0,
            mini_protocol_num: num,
            direction: MiniProtocolDir::Initiator,
            length: 6,
        };
        // Two enqueues back-to-back: FIFO order A then B is fixed.
        conn.send_sdu(&header(PROTO_A), &[0xA1; 6])
            .await
            .expect("send A");
        conn.send_sdu(&header(PROTO_B), &[0xB2; 6])
            .await
            .expect("send B");

        // Wire order must interleave: A,B,A,B,A,B.
        let expected = [PROTO_A, PROTO_B, PROTO_A, PROTO_B, PROTO_A, PROTO_B];
        let mut observed: Vec<u16> = Vec::new();
        for _ in 0..6 {
            let (h, p) = tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                .await
                .expect("read SDU within 2s")
                .expect("read sdu");
            assert_eq!(p.len(), 2, "each SDU is one 2-byte fragment");
            observed.push(h.mini_protocol_num);
        }
        assert_eq!(
            observed, expected,
            "send_sdu via the muxer must round-robin between mini-protocols"
        );
    }

    // ------------------------------------------------------------------
    // Slice (c): bearer-task supervision (`MuxConnection::run`).
    // ------------------------------------------------------------------

    /// A deterministic mock transport for the supervision tests.
    ///
    /// Reads always return `Poll::Pending` — the read-task parks
    /// inside `read_sdu().await` and never makes progress on its own,
    /// so the *only* way it terminates is the supervisor aborting it.
    /// Writes always fail with a broken-pipe `io::Error` — the muxer's
    /// first `write_sdu` faults immediately. There is no async race:
    /// both behaviours are decided synchronously at `poll_*` time.
    struct ReadPendingWriteFails;

    impl tokio::io::AsyncRead for ReadPendingWriteFails {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            // Never readable — the read-task blocks forever until the
            // supervisor aborts it.
            std::task::Poll::Pending
        }
    }

    impl tokio::io::AsyncWrite for ReadPendingWriteFails {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "mock transport: writes always fail",
            )))
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "mock transport: writes always fail",
            )))
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    /// Supervision test (i): a muxer bearer-write failure tears the
    /// read-task down.
    ///
    /// Deterministic — no reliance on async race timing:
    ///
    /// * The mock transport's writes fail synchronously, so the
    ///   muxer's first `write_sdu` faults the instant a demand is
    ///   enqueued.
    /// * The mock transport's reads are `Poll::Pending` forever, so
    ///   the read-task can ONLY terminate by the supervisor aborting
    ///   it — there is no path where the read-task finishes first.
    ///
    /// Expected `MuxRunResult`: `first_failure == Some(MuxerTask)`,
    /// the muxer outcome is a `Failed(EgressError::Bearer)`, and the
    /// read-task outcome is `Cancelled`. Mirrors upstream
    /// `Network.Mux`'s `MuxerException` → `Failed` → `withJobPool`
    /// cancelling the demuxer sibling.
    #[tokio::test]
    async fn run_muxer_write_failure_cancels_read_task() {
        use crate::trace_forwarder::egress::EgressConfig;

        let conn = Arc::new(MuxConnection::new(Bearer::new(ReadPendingWriteFails)));

        // Spawn the supervised lifecycle.
        let run_conn = Arc::clone(&conn);
        let run_handle =
            tokio::spawn(async move { run_conn.run(Some(EgressConfig::default())).await });

        // Deterministically wait for `run` to install the muxer
        // before enqueueing: a `send_sdu` issued before the muxer is
        // live would take the direct-write fallback (which on this
        // mock transport fails immediately) instead of the scheduler
        // path the test means to exercise.
        while !conn.muxer_installed().await {
            tokio::task::yield_now().await;
        }

        // Enqueue one demand: the muxer pops it, calls `write_sdu`,
        // and the mock transport fails the write → muxer faults.
        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Initiator,
            length: 4,
        };
        conn.send_sdu(&header, b"abcd")
            .await
            .expect("send_sdu enqueues the demand");

        let result = tokio::time::timeout(Duration::from_secs(5), run_handle)
            .await
            .expect("run() completed within 5s — supervisor did not hang")
            .expect("run() task did not panic");

        assert_eq!(
            result.first_failure,
            Some(FailedJob::MuxerTask),
            "a muxer write failure must be the tear-down trigger"
        );
        assert!(
            matches!(
                result.muxer_outcome,
                Some(JobOutcome::Failed(EgressError::Bearer(_)))
            ),
            "muxer outcome must be a Failed bearer error; got {:?}",
            result.muxer_outcome
        );
        assert!(
            matches!(result.read_outcome, JobOutcome::Cancelled),
            "the read-task must be cancelled by the supervisor when the \
             muxer fails; got {:?}",
            result.read_outcome
        );
        assert!(!result.is_clean_shutdown());
    }

    /// Supervision test (ii): a read-task `IngressQueueOverRun` tears
    /// the muxer down.
    ///
    /// Deterministic — no reliance on async race timing:
    ///
    /// * Two over-cap SDUs are written to the server end and the
    ///   server end is then dropped, all BEFORE `run()` is spawned —
    ///   the bytes are buffered in the duplex pipe, so the read-task
    ///   reads them as soon as it starts and trips the 10-byte cap on
    ///   the second SDU. `IngressQueueOverRun` is returned with
    ///   certainty.
    /// * The muxer has no demand enqueued, so it sits blocked on its
    ///   wake channel — it can ONLY terminate by the supervisor
    ///   aborting it.
    ///
    /// Expected `MuxRunResult`: `first_failure == Some(ReadTask)`, the
    /// read outcome is `Failed(IngressQueueOverRun)`, the muxer
    /// outcome is `Cancelled`. Mirrors upstream `Network.Mux`'s
    /// `DemuxerException` (a non-`BearerClosed` demuxer fault) →
    /// `Failed` → `withJobPool` cancelling the muxer sibling.
    #[tokio::test]
    async fn run_read_task_over_run_cancels_muxer() {
        use crate::trace_forwarder::egress::EgressConfig;
        use tokio::io::AsyncWriteExt;

        let (client, mut server) = tokio::io::duplex(4096);
        let conn = Arc::new(MuxConnection::new(Bearer::new(client)));

        // Subscribe with a tiny 10-byte ingress cap. Hold the
        // receiver so dispatched bytes stay charged (not drained).
        let _rx = conn
            .subscribe_with_limits(
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolLimits {
                    maximum_ingress_queue: 10,
                },
            )
            .await;

        // Pre-load two 6-byte SDUs into the pipe BEFORE spawning
        // `run`: 6 fits (<= 10), 6 more makes 12 > 10 → over-run.
        let write_one = |bytes: &[u8]| {
            let header = SduHeader {
                timestamp: 0,
                mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                direction: MiniProtocolDir::Responder,
                length: bytes.len() as u16,
            };
            super::super::mux::encode_sdu_header(&header).expect("encode header")
        };
        server.write_all(&write_one(b"aaaaaa")).await.expect("hdr1");
        server.write_all(b"aaaaaa").await.expect("payload1");
        server.write_all(&write_one(b"bbbbbb")).await.expect("hdr2");
        server.write_all(b"bbbbbb").await.expect("payload2");
        drop(server);

        // Spawn the supervised lifecycle. The read-task drains the
        // pre-loaded SDUs and over-runs on the second; the muxer is
        // idle (no demand enqueued).
        let run_conn = Arc::clone(&conn);
        let run_handle =
            tokio::spawn(async move { run_conn.run(Some(EgressConfig::default())).await });

        let result = tokio::time::timeout(Duration::from_secs(5), run_handle)
            .await
            .expect("run() completed within 5s — supervisor did not hang")
            .expect("run() task did not panic");

        assert_eq!(
            result.first_failure,
            Some(FailedJob::ReadTask),
            "an ingress over-run must be the tear-down trigger"
        );
        assert!(
            matches!(
                result.read_outcome,
                JobOutcome::Failed(MuxConnectionError::IngressQueueOverRun { .. })
            ),
            "read outcome must be a Failed IngressQueueOverRun; got {:?}",
            result.read_outcome
        );
        assert!(
            matches!(result.muxer_outcome, Some(JobOutcome::Cancelled)),
            "the muxer must be cancelled by the supervisor when the \
             read-task over-runs; got {:?}",
            result.muxer_outcome
        );
        assert!(!result.is_clean_shutdown());
    }

    /// Supervision test (iii): a clean producer-drop + bearer-EOF
    /// shuts both jobs down `Ok`.
    ///
    /// Deterministic — no reliance on async race timing:
    ///
    /// * `shutdown()` is called explicitly: it drops the stashed
    ///   `EgressDemand` (the muxer's wake channel closes → the muxer
    ///   drains its empty FIFO and returns `Ok`) and sets the
    ///   shutdown flag.
    /// * `drop(server)` then EOFs the bearer: the read-task's
    ///   `read_sdu` returns `UnexpectedEof`. Because the shutdown flag
    ///   is set (and the muxer already finished `Ok`), the supervisor
    ///   re-classifies that EOF as a clean stop, not a fault.
    ///
    /// Both signals are issued by the test itself, in a fixed order —
    /// nothing depends on which job the runtime happens to schedule
    /// first. Expected `MuxRunResult`: `is_clean_shutdown()`, both
    /// outcomes `Completed`. Mirrors upstream `Network.Mux` reaching
    /// the `Stopped` terminal status on an orderly shutdown.
    #[tokio::test]
    async fn run_clean_shutdown_exits_both_ok() {
        use crate::trace_forwarder::egress::EgressConfig;

        let (client, server) = tokio::io::duplex(4096);
        let conn = Arc::new(MuxConnection::new(Bearer::new(client)));

        let run_conn = Arc::clone(&conn);
        let run_handle =
            tokio::spawn(async move { run_conn.run(Some(EgressConfig::default())).await });

        // Deterministically wait for `run` to install both jobs
        // before we signal shutdown — otherwise `shutdown()` could
        // clear the egress slot before `spawn_muxer_task` fills it.
        while !conn.muxer_installed().await {
            tokio::task::yield_now().await;
        }

        // Producer-drop: drop the EgressDemand → the muxer drains its
        // FIFO and exits Ok. Also sets the shutdown flag.
        conn.shutdown().await;
        // Bearer-EOF: the read-task's pending read returns
        // UnexpectedEof, re-classified clean because the flag is set.
        drop(server);

        let result = tokio::time::timeout(Duration::from_secs(5), run_handle)
            .await
            .expect("run() completed within 5s — clean shutdown did not hang")
            .expect("run() task did not panic");

        assert!(
            result.is_clean_shutdown(),
            "producer-drop + bearer-EOF must be a clean shutdown; \
             first_failure = {:?}",
            result.first_failure
        );
        assert!(
            matches!(result.read_outcome, JobOutcome::Completed),
            "read-task must complete cleanly; got {:?}",
            result.read_outcome
        );
        assert!(
            matches!(result.muxer_outcome, Some(JobOutcome::Completed)),
            "muxer-task must complete cleanly; got {:?}",
            result.muxer_outcome
        );
    }

    /// `run(None)` supervises the read-task alone — the muxer-less
    /// path. A clean bearer EOF is `Completed` (no muxer means the
    /// EOF is vacuously a clean stop), and `muxer_outcome` is `None`.
    #[tokio::test]
    async fn run_without_muxer_supervises_read_task_only() {
        let (client, server) = tokio::io::duplex(4096);
        let conn = Arc::new(MuxConnection::new(Bearer::new(client)));

        let run_conn = Arc::clone(&conn);
        let run_handle = tokio::spawn(async move { run_conn.run(None).await });

        tokio::task::yield_now().await;
        // EOF the bearer — the sole supervised job (read-task) ends.
        drop(server);

        let result = tokio::time::timeout(Duration::from_secs(5), run_handle)
            .await
            .expect("run() completed within 5s")
            .expect("run() task did not panic");

        assert!(result.is_clean_shutdown());
        assert!(
            matches!(result.read_outcome, JobOutcome::Completed),
            "muxer-less run: a bearer EOF is a clean read-task stop; got {:?}",
            result.read_outcome
        );
        assert!(
            result.muxer_outcome.is_none(),
            "run(None) has no muxer outcome"
        );
    }
}
