//! Egress scheduler for the cardano-tracer forwarder Mux —
//! a Rust port of upstream `Network.Mux.Egress`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side Rust port of
//! `.reference-haskell-cardano-node/deps/ouroboros-network/network-mux/src/Network/Mux/Egress.hs`.
//! The upstream symbols ported here are `EgressQueue`,
//! `TranslocationServiceRequest` (`TLSRDemand`), `Wanton`, the
//! `muxer` task, and the `processSingleWanton` segment-and-requeue
//! helper. The upstream `muxChannel.send` payload-append path
//! (`Network.Mux.hs`) is mirrored by [`EgressDemand::enqueue`].
//! Field/type names are kept close to upstream so the parity harness
//! can pin behaviour against the Haskell muxer side-by-side.
//!
//! ## Why an egress scheduler
//!
//! Before this slice, [`super::mux_connection::MuxConnection::send_sdu`]
//! was a direct `writer.lock() + write_sdu` round-trip: every caller
//! competed for one writer mutex with no fairness guarantee and no
//! segmentation. Upstream `Network.Mux` instead routes **all**
//! outbound traffic through a single shared FIFO ([`EgressQueue`])
//! drained by one dedicated task (`muxer`). Each mini-protocol may
//! have at most one [`Wanton`] (its payload-being-drained) referenced
//! from the queue at a time; the `muxer` pulls one demand, slices off
//! at most `sduSize` bytes, writes that SDU, and — if the `Wanton`
//! still holds bytes — re-enqueues the demand at the **back** of the
//! FIFO. Round-robin fairness emerges from that re-enqueue: a
//! mini-protocol with a large payload yields the wire to every other
//! ready mini-protocol between each of its SDUs.
//!
//! ## Upstream `muxer` loop, faithfully
//!
//! `Network.Mux.Egress.muxer` (lines 149-186 of `Egress.hs`):
//!
//! ```text
//! forever $ do
//!   TLSRDemand mpc md d <- atomically $ readTBQueue egressQueue
//!   sdu  <- processSingleWanton egressQueue sduSize mpc md d
//!   sdus <- buildBatch [sdu] (sduLength sdu)        -- up to maxSDUsPerBatch / batchSize
//!   void $ writeMany tracer timeout sdus
//! ```
//!
//! Yggdrasil mirrors this: [`run_muxer`] blocks on the queue, calls
//! [`process_single_wanton`] to get one SDU (re-enqueueing the demand
//! if the `Wanton` still has bytes), accumulates a batch of up to
//! [`MAX_SDUS_PER_BATCH`] SDUs or [`EgressConfig::batch_size`] bytes
//! by `try_recv`-ing further ready demands, then writes the whole
//! batch through the [`super::bearer::BearerWriter`] back-to-back
//! (the Rust analogue of `writeMany` — `BearerWriter` has no vectored
//! write, so a "batch" is N sequential `write_sdu` calls with no
//! intervening `yield`, which still lets tokio coalesce them into one
//! kernel write set).
//!
//! ## Segmentation default — [`EgressConfig::sdu_size`]
//!
//! The trace-forwarder default [`EgressConfig::sdu_size`] is
//! `u16::MAX` (the largest a 16-bit SDU length field can carry). With
//! that default, [`process_single_wanton`] never splits a real trace
//! SDU — one `enqueue` call produces exactly one SDU on the wire,
//! byte-identical to the pre-scheduler direct-write behaviour, so the
//! live cardano-tracer conformance tests are preserved. Segmentation
//! is still **structurally present** and is exercised by unit tests
//! that construct an [`EgressConfig`] with a small `sdu_size`. The
//! node-to-node bearer uses a much smaller `sduSize` (12 288 bytes,
//! `Ouroboros.Network.Diffusion`); this crate forwards to
//! cardano-tracer, not over NtN, so the unsegmented default is the
//! parity-correct one for the live bearer.
//!
//! ## Where the muxer fits in the lifecycle
//!
//! Upstream forks the `muxer`/`demuxer` job pair (`Network.Mux.hs`
//! ~lines 248-258: `egressQueue <- newTBQueue 100` then
//! `forkJob muxerJob`) **after** the Handshake mini-protocol has
//! already completed over a plain `bearerAsChannel` direct-write.
//! Yggdrasil mirrors this exactly:
//! [`super::mux_connection::MuxConnection::run_initiator_handshake`]
//! still does its own direct bearer write/read, and
//! [`super::mux_connection::MuxConnection::spawn_muxer_task`] forks
//! the muxer **only after** the handshake returns — so the handshake
//! conformance test still drives a direct write and is unaffected by
//! this slice.

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use super::bearer::{BearerError, BearerWriter};
use super::mux::{MiniProtocolDir, SduHeader};

/// Upstream `maxSDUsPerBatch :: Int = 100` — the hard cap on how
/// many SDUs the muxer accumulates into one `writeMany` batch
/// before flushing to the bearer (`Network.Mux.Egress.muxer`'s
/// `buildBatch`). The egress queue is still processed one SDU at a
/// time inside the batch so a slow mini-protocol cannot monopolise
/// the batch; the batch is purely a write-coalescing optimisation.
pub const MAX_SDUS_PER_BATCH: usize = 100;

/// Upstream egress-queue capacity — `Network.Mux.hs`
/// `newTBQueue 100`. The FIFO holds one
/// [`TranslocationServiceRequest`] per pending mini-protocol demand.
pub const EGRESS_QUEUE_CAPACITY: usize = 100;

/// Configuration knobs for the muxer, mirroring the relevant
/// `Network.Mux.Types.Bearer` record fields (`sduSize`, `batchSize`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EgressConfig {
    /// Maximum payload bytes the muxer puts in a single SDU. Mirrors
    /// upstream `Bearer.sduSize :: SDUSize`. `process_single_wanton`
    /// slices a `Wanton`'s buffer into `sdu_size`-byte fragments;
    /// when a fragment is taken and bytes remain, the demand is
    /// re-enqueued. The trace-forwarder default is `u16::MAX` — large
    /// enough that no real trace SDU is ever split.
    pub sdu_size: usize,
    /// Soft byte budget for one `writeMany` batch. Mirrors upstream
    /// `Bearer.batchSize :: Int`. The muxer stops accumulating into
    /// the current batch once the running byte total reaches this
    /// (or [`MAX_SDUS_PER_BATCH`] SDUs accumulate, whichever first).
    pub batch_size: usize,
}

impl EgressConfig {
    /// The egress configuration the trace-forwarder bearer uses.
    ///
    /// `sdu_size = u16::MAX` so a single `MsgTraceObjectsReply` SDU
    /// is never segmented — `enqueue` → one SDU on the wire, exactly
    /// as the pre-scheduler direct-write path produced, preserving
    /// the live cardano-tracer conformance behaviour.
    ///
    /// `batch_size = u16::MAX` so a single SDU never trips the
    /// per-batch byte budget on its own; the muxer still flushes
    /// each batch promptly because the trace-forwarder rarely has
    /// more than one demand queued at a time.
    pub const CARDANO_TRACER_DEFAULT: Self = Self {
        sdu_size: u16::MAX as usize,
        batch_size: u16::MAX as usize,
    };
}

impl Default for EgressConfig {
    fn default() -> Self {
        Self::CARDANO_TRACER_DEFAULT
    }
}

/// A `Wanton` — the concrete payload being drained for one
/// mini-protocol demand. Mirrors upstream
/// `newtype Wanton m = Wanton { want :: StrictTVar m BL.ByteString }`.
///
/// Upstream represents the not-yet-sent bytes as a `TVar ByteString`
/// shared between the producer (`muxChannel.send` appends to it) and
/// the muxer (`processSingleWanton` slices the front off it). In the
/// trace-forwarder each `enqueue` call carries its own complete
/// payload, so the `Wanton` is constructed fully-formed and is then
/// only ever drained — the producer never appends to a live one.
/// `remaining` holds the bytes still to send; an empty `remaining`
/// is the "last fragment enqueued" signal upstream gets from the
/// `TVar` becoming empty.
#[derive(Debug)]
struct Wanton {
    /// Bytes still to be segmented and written. Drained from the
    /// front by [`process_single_wanton`].
    remaining: Vec<u8>,
}

/// A `TranslocationServiceRequest` — a demand to translocate one
/// mini-protocol message. Mirrors upstream
/// `TLSRDemand !MiniProtocolNum !MiniProtocolDir !(Wanton m)`.
///
/// The multiplexing layer owns segmenting the (arbitrary but
/// bounded) payload into SDUs; the `(mini_protocol_num, direction)`
/// pair is stamped into every SDU header the demand produces.
#[derive(Debug)]
struct TranslocationServiceRequest {
    /// Mini-protocol number for every SDU this demand produces.
    mini_protocol_num: u16,
    /// Direction stamped into every SDU header this demand produces.
    direction: MiniProtocolDir,
    /// The payload being drained.
    wanton: Wanton,
}

/// The shared egress FIFO — upstream `EgressQueue m =
/// StrictTBQueue m (TranslocationServiceRequest m)`.
///
/// A single FIFO shared by every mini-protocol. The muxer reads from
/// the front; producers (`enqueue`) and the muxer's
/// re-enqueue-on-remainder both write to the back. Round-robin
/// fairness emerges purely from this FIFO discipline plus the
/// re-enqueue in [`process_single_wanton`].
type EgressQueue = VecDeque<TranslocationServiceRequest>;

/// Producer-facing handle for the egress scheduler.
///
/// [`super::mux_connection::MuxConnection::send_sdu`] holds one of
/// these. `enqueue` appends a demand to the shared FIFO and signals
/// the muxer; it returns as soon as the demand is queued — it does
/// **not** wait for the bytes to reach the wire. This is the upstream
/// `muxChannel.send` semantic (the STM transaction `writeTBQueue`s
/// the demand and returns) and is what makes fairness possible: a
/// caller never blocks the scheduler.
#[derive(Clone)]
pub struct EgressDemand {
    /// Shared FIFO of pending demands.
    queue: Arc<Mutex<EgressQueue>>,
    /// Wake the muxer when a demand is appended. Capacity-1 so a
    /// burst of `enqueue` calls collapses into one wake-up (the
    /// muxer drains the whole queue per wake).
    notify: mpsc::Sender<()>,
}

impl EgressDemand {
    /// Enqueue one outbound SDU's worth of payload as a demand.
    ///
    /// The `(mini_protocol_num, direction)` pair is stamped into
    /// every SDU header the demand produces; `payload` is the bytes
    /// the muxer segments and writes. Returns once the demand is on
    /// the FIFO — the bytes are **not** yet on the wire.
    ///
    /// Mirrors upstream `muxChannel.send`: append to the `Wanton`
    /// and `writeTBQueue` the `TLSRDemand`. Yggdrasil constructs a
    /// fresh `Wanton` per call rather than appending to a shared one
    /// — the trace-forwarder hands a complete payload per send, so
    /// there is never a live `Wanton` to append to.
    ///
    /// Returns [`EgressError::MuxerStopped`] if the muxer task has
    /// already exited (its notify receiver dropped) — the demand is
    /// still queued but will never be drained, so the caller is told.
    pub async fn enqueue(
        &self,
        mini_protocol_num: u16,
        direction: MiniProtocolDir,
        payload: Vec<u8>,
    ) -> Result<(), EgressError> {
        {
            let mut queue = self.queue.lock().await;
            queue.push_back(TranslocationServiceRequest {
                mini_protocol_num,
                direction,
                wanton: Wanton { remaining: payload },
            });
        }
        // Wake the muxer. A full channel (capacity 1) means a wake is
        // already pending and the muxer has not yet consumed it — the
        // demand we just queued will be seen on that pending drain,
        // so dropping this extra wake is correct, not a lost signal.
        match self.notify.try_send(()) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(())) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(())) => Err(EgressError::MuxerStopped),
        }
    }
}

/// Errors surfaced by the egress scheduler.
#[derive(Debug)]
pub enum EgressError {
    /// A bearer write inside the muxer failed. The muxer task exits
    /// after returning this; mirrors the upstream muxer exception
    /// being fatal (`Network.Mux.hs`: "The muxer exception is always
    /// fatal").
    Bearer(BearerError),
    /// The muxer task has already exited, so an `enqueue`d demand
    /// will never be drained. Surfaced from [`EgressDemand::enqueue`].
    MuxerStopped,
}

impl core::fmt::Display for EgressError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bearer(e) => write!(f, "egress muxer bearer error: {e}"),
            Self::MuxerStopped => f.write_str("egress muxer task has stopped"),
        }
    }
}

impl std::error::Error for EgressError {}

/// Construct the egress scheduler: returns the producer handle
/// ([`EgressDemand`]) and the [`EgressMuxer`] that the
/// `MuxConnection` spawns as a task.
///
/// Mirrors upstream `Network.Mux.hs` `egressQueue <- newTBQueue 100`
/// followed by wiring `muxChannel`s (producers) and `muxerJob`
/// (consumer) onto the one queue.
pub fn egress_channel() -> (EgressDemand, EgressMuxer) {
    let queue = Arc::new(Mutex::new(EgressQueue::new()));
    // Capacity-1 wake channel: enqueue bursts collapse to one wake.
    let (notify_tx, notify_rx) = mpsc::channel(1);
    (
        EgressDemand {
            queue: Arc::clone(&queue),
            notify: notify_tx,
        },
        EgressMuxer {
            queue,
            notify: notify_rx,
        },
    )
}

/// The consumer side of the egress scheduler — the upstream `muxer`
/// task. The `MuxConnection` owns this until it spawns it via
/// [`run_muxer`].
pub struct EgressMuxer {
    /// Shared FIFO of pending demands.
    queue: Arc<Mutex<EgressQueue>>,
    /// Wake-up signal from [`EgressDemand::enqueue`].
    notify: mpsc::Receiver<()>,
}

/// Pull at most `sdu_size` bytes off the front of the demand's
/// `Wanton`, build one SDU, and — if the `Wanton` still has bytes —
/// re-enqueue the demand at the **back** of the FIFO.
///
/// Faithful port of upstream `processSingleWanton`
/// (`Network.Mux.Egress.hs` lines 192-225). Upstream does the
/// `readTVar`/`writeTVar`/`writeTBQueue` inside one STM transaction
/// to preserve byte-stream ordering within a mini-protocol; Yggdrasil
/// holds the `queue` mutex for the equivalent critical section (the
/// demand is *not* on the queue while being processed — the caller
/// already `pop_front`-ed it — so the re-enqueue restores FIFO
/// order). The re-enqueue is what gives every other ready
/// mini-protocol a turn before this one's next fragment: this is the
/// fairness mechanism.
async fn process_single_wanton(
    queue: &Arc<Mutex<EgressQueue>>,
    sdu_size: usize,
    mut demand: TranslocationServiceRequest,
) -> (SduHeader, Vec<u8>) {
    // `sdu_size` is u16::MAX-bounded for the trace-forwarder, but the
    // public `EgressConfig` lets a test pass a tiny value, so clamp
    // defensively: an SDU's length field is a `u16`.
    let take = demand
        .wanton
        .remaining
        .len()
        .min(sdu_size)
        .min(u16::MAX as usize);
    // Split the front fragment off; `rest` is the tail still to send.
    let rest = demand.wanton.remaining.split_off(take);
    let frag = demand.wanton.remaining;
    let header = SduHeader {
        // Upstream `processSingleWanton` stamps `RemoteClockModel 0`
        // here and the bearer's `write` then overwrites it with
        // `getMonotonicTime`. Yggdrasil's `BearerWriter::write_sdu`
        // does NOT rewrite the timestamp, so a fixed 0 reaches the
        // wire — but this matches the pre-scheduler direct-write
        // behaviour exactly (`forwarding_task::build_reply_sdu` also
        // stamps 0) and the trace-forward protocol treats the SDU
        // timestamp as informational, so there is no behavioural
        // change. TODO(round c+): if a `RemoteClockModel` tick source
        // lands on `BearerWriter`, stamp it here too.
        timestamp: 0,
        mini_protocol_num: demand.mini_protocol_num,
        direction: demand.direction,
        length: frag.len() as u16,
    };
    if !rest.is_empty() {
        // Bytes remain: re-enqueue the demand at the BACK of the FIFO
        // so every other ready demand is serviced before this one's
        // next fragment. This is the round-robin fairness mechanism.
        demand.wanton.remaining = rest;
        let mut q = queue.lock().await;
        q.push_back(demand);
    }
    (header, frag)
}

/// Run the muxer loop until the [`EgressDemand`] producer side is
/// dropped (clean shutdown) or a bearer write fails (fatal).
///
/// Faithful port of upstream `Network.Mux.Egress.muxer`. Each
/// iteration:
///
/// 1. Block until a wake-up arrives (a demand was enqueued).
/// 2. `pop_front` one demand, [`process_single_wanton`] it into one
///    SDU (re-enqueueing the remainder if any).
/// 3. Accumulate a batch: keep `pop_front`-ing ready demands and
///    segmenting them until [`MAX_SDUS_PER_BATCH`] SDUs or
///    [`EgressConfig::batch_size`] payload bytes accumulate, or the
///    queue drains. This mirrors upstream `buildBatch`.
/// 4. Write the whole batch to the bearer back-to-back (the Rust
///    analogue of `writeMany` — no `yield` between SDUs so tokio
///    coalesces them).
///
/// Returns `Ok(())` on clean producer-drop shutdown, `Err` on a
/// fatal bearer write failure.
pub async fn run_muxer<S>(
    mut muxer: EgressMuxer,
    writer: Arc<Mutex<BearerWriter<S>>>,
    config: EgressConfig,
) -> Result<(), EgressError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    loop {
        // (1) Block for a wake-up. `None` means every `EgressDemand`
        // was dropped — clean shutdown.
        if muxer.notify.recv().await.is_none() {
            // Drain anything still queued before exiting so a demand
            // enqueued just before the last producer drop is not
            // silently lost.
            drain_remaining(&muxer.queue, &writer, config).await?;
            return Ok(());
        }

        // (2)+(3) Drain the queue: process one demand at a time,
        // building batches, until the queue is empty.
        loop {
            // Pop one demand. Empty queue → back to waiting for a
            // wake-up.
            let first = {
                let mut q = muxer.queue.lock().await;
                q.pop_front()
            };
            let Some(first) = first else {
                break;
            };

            // Segment the first demand into one SDU (re-enqueue
            // remainder).
            let (header, payload) =
                process_single_wanton(&muxer.queue, config.sdu_size, first).await;
            let mut batch: Vec<(SduHeader, Vec<u8>)> = Vec::with_capacity(MAX_SDUS_PER_BATCH);
            let mut batch_bytes = sdu_len(&payload);
            batch.push((header, payload));

            // buildBatch: keep pulling ready demands one SDU at a
            // time until the batch is full.
            while batch.len() < MAX_SDUS_PER_BATCH && batch_bytes < config.batch_size {
                let next = {
                    let mut q = muxer.queue.lock().await;
                    q.pop_front()
                };
                let Some(next) = next else {
                    break;
                };
                let (h, p) = process_single_wanton(&muxer.queue, config.sdu_size, next).await;
                batch_bytes += sdu_len(&p);
                batch.push((h, p));
            }

            // (4) writeMany: flush the whole batch back-to-back.
            write_batch(&writer, &batch).await?;
        }
    }
}

/// Length of one SDU on the wire — 8-byte header + payload — for the
/// `buildBatch` byte budget. Mirrors upstream
/// `sduLength sdu = msHeaderLength + msLength sdu`.
fn sdu_len(payload: &[u8]) -> usize {
    8 + payload.len()
}

/// Write a batch of SDUs back-to-back through the bearer writer.
/// The Rust analogue of upstream `writeMany` — `BearerWriter` has no
/// vectored write, so this is N sequential `write_sdu` calls under
/// one lock acquisition with no intervening `yield`, letting tokio's
/// internal buffering coalesce them into one kernel write set.
async fn write_batch<S>(
    writer: &Arc<Mutex<BearerWriter<S>>>,
    batch: &[(SduHeader, Vec<u8>)],
) -> Result<(), EgressError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut w = writer.lock().await;
    for (header, payload) in batch {
        w.write_sdu(header, payload)
            .await
            .map_err(EgressError::Bearer)?;
    }
    Ok(())
}

/// Drain every demand still on the FIFO at shutdown — used once the
/// last [`EgressDemand`] is dropped so a demand enqueued in the race
/// window before the final drop still reaches the wire.
async fn drain_remaining<S>(
    queue: &Arc<Mutex<EgressQueue>>,
    writer: &Arc<Mutex<BearerWriter<S>>>,
    config: EgressConfig,
) -> Result<(), EgressError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    loop {
        let demand = {
            let mut q = queue.lock().await;
            q.pop_front()
        };
        let Some(demand) = demand else {
            return Ok(());
        };
        let (header, payload) = process_single_wanton(queue, config.sdu_size, demand).await;
        write_batch(writer, &[(header, payload)]).await?;
    }
}

#[cfg(test)]
mod egress_tests {
    use super::*;
    use crate::trace_forwarder::bearer::Bearer;
    use crate::trace_forwarder::mux::TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM;
    use std::time::Duration;

    /// One enqueued demand whose payload fits in one SDU is written
    /// as exactly one SDU on the wire, byte-identical to a direct
    /// `write_sdu`. Pins the property that the default config never
    /// segments a real trace SDU.
    #[tokio::test]
    async fn single_demand_writes_one_sdu() {
        let (client, server) = tokio::io::duplex(8192);
        let (_reader, writer) = Bearer::new(client).split();
        let writer = Arc::new(Mutex::new(writer));
        let mut server_bearer = Bearer::new(server);

        let (demand, muxer) = egress_channel();
        let muxer_task = tokio::spawn(run_muxer(
            muxer,
            Arc::clone(&writer),
            EgressConfig::default(),
        ));

        demand
            .enqueue(
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolDir::Initiator,
                b"hello egress".to_vec(),
            )
            .await
            .expect("enqueue");

        let (header, payload) =
            tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                .await
                .expect("read within 2s")
                .expect("read sdu");
        assert_eq!(
            header.mini_protocol_num,
            TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM
        );
        assert_eq!(header.direction, MiniProtocolDir::Initiator);
        assert_eq!(payload, b"hello egress");

        drop(demand);
        let outcome = tokio::time::timeout(Duration::from_secs(2), muxer_task)
            .await
            .expect("muxer terminated")
            .expect("muxer did not panic");
        assert!(outcome.is_ok(), "muxer returned {outcome:?}");
    }

    /// Segmentation: with a forced tiny `sdu_size`, one large
    /// payload is split into multiple SDUs, and `process_single_wanton`
    /// re-enqueues the remainder so the fragments arrive in order.
    #[tokio::test]
    async fn large_payload_is_segmented_to_sdu_size() {
        let (client, server) = tokio::io::duplex(8192);
        let (_reader, writer) = Bearer::new(client).split();
        let writer = Arc::new(Mutex::new(writer));
        let mut server_bearer = Bearer::new(server);

        let (demand, muxer) = egress_channel();
        // 4-byte SDUs.
        let cfg = EgressConfig {
            sdu_size: 4,
            batch_size: u16::MAX as usize,
        };
        let muxer_task = tokio::spawn(run_muxer(muxer, Arc::clone(&writer), cfg));

        // 10-byte payload → 4 + 4 + 2.
        demand
            .enqueue(
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolDir::Initiator,
                vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            )
            .await
            .expect("enqueue");

        let mut reassembled: Vec<u8> = Vec::new();
        for expected_len in [4usize, 4, 2] {
            let (header, payload) =
                tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                    .await
                    .expect("read fragment within 2s")
                    .expect("read sdu");
            assert_eq!(payload.len(), expected_len, "fragment length");
            assert_eq!(header.length as usize, expected_len);
            reassembled.extend_from_slice(&payload);
        }
        assert_eq!(
            reassembled,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "segments must reassemble to the original payload in order"
        );

        drop(demand);
        let _ = tokio::time::timeout(Duration::from_secs(2), muxer_task).await;
    }

    /// Round-robin fairness — the core property of this round.
    ///
    /// Two demands on different mini-protocols, each with a payload
    /// that spans multiple SDUs at a forced tiny `sdu_size`, are
    /// enqueued back-to-back BEFORE the muxer is given a chance to
    /// run. `process_single_wanton`'s re-enqueue-on-remainder must
    /// then interleave their SDUs: A, B, A, B, … — neither
    /// mini-protocol monopolises the wire. The test is fully
    /// deterministic: enqueue order fixes the FIFO order, and the
    /// muxer drains it without any concurrent producer racing it.
    #[tokio::test]
    async fn muxer_round_robins_between_two_mini_protocols() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (_reader, writer) = Bearer::new(client).split();
        let writer = Arc::new(Mutex::new(writer));
        let mut server_bearer = Bearer::new(server);

        let (demand, muxer) = egress_channel();
        // 2-byte SDUs so each 6-byte payload spans 3 SDUs.
        let cfg = EgressConfig {
            sdu_size: 2,
            batch_size: u16::MAX as usize,
        };

        // Enqueue BOTH demands before the muxer starts: this fixes
        // the FIFO order deterministically.
        const PROTO_A: u16 = 2;
        const PROTO_B: u16 = 3;
        demand
            .enqueue(PROTO_A, MiniProtocolDir::Initiator, vec![0xA1; 6])
            .await
            .expect("enqueue A");
        demand
            .enqueue(PROTO_B, MiniProtocolDir::Initiator, vec![0xB2; 6])
            .await
            .expect("enqueue B");

        // Now start the muxer.
        let muxer_task = tokio::spawn(run_muxer(muxer, Arc::clone(&writer), cfg));

        // Expected interleaving: A is at the front, B behind it.
        //   pop A → SDU A0, re-enqueue A behind B   queue: [B, A]
        //   pop B → SDU B0, re-enqueue B behind A   queue: [A, B]
        //   pop A → SDU A1, re-enqueue A            queue: [B, A]
        //   pop B → SDU B1, re-enqueue B            queue: [A, B]
        //   pop A → SDU A2 (last fragment, no requeue) queue: [B]
        //   pop B → SDU B2 (last fragment)             queue: []
        // → wire order A, B, A, B, A, B.
        let expected_protocols = [PROTO_A, PROTO_B, PROTO_A, PROTO_B, PROTO_A, PROTO_B];
        let mut observed: Vec<u16> = Vec::new();
        for _ in 0..6 {
            let (header, payload) =
                tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                    .await
                    .expect("read SDU within 2s")
                    .expect("read sdu");
            assert_eq!(payload.len(), 2, "each SDU is one 2-byte fragment");
            observed.push(header.mini_protocol_num);
        }
        assert_eq!(
            observed, expected_protocols,
            "muxer must round-robin SDUs between the two mini-protocols \
             (A,B,A,B,A,B) — a non-interleaved order means the egress \
             scheduler's re-enqueue-on-remainder fairness has regressed"
        );

        drop(demand);
        let _ = tokio::time::timeout(Duration::from_secs(2), muxer_task).await;
    }

    /// Many demands enqueued in a burst all reach the wire — pins
    /// that the capacity-1 wake channel never loses a demand even
    /// when several `enqueue` calls collapse onto one wake-up.
    #[tokio::test]
    async fn burst_enqueue_delivers_every_demand() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (_reader, writer) = Bearer::new(client).split();
        let writer = Arc::new(Mutex::new(writer));
        let mut server_bearer = Bearer::new(server);

        let (demand, muxer) = egress_channel();
        let muxer_task = tokio::spawn(run_muxer(
            muxer,
            Arc::clone(&writer),
            EgressConfig::default(),
        ));

        // Burst 50 single-byte demands, each tagged by its index.
        for i in 0..50u8 {
            demand
                .enqueue(
                    TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                    MiniProtocolDir::Initiator,
                    vec![i],
                )
                .await
                .expect("enqueue");
        }

        // Every demand must arrive, in enqueue order (one
        // mini-protocol → FIFO order is preserved).
        for i in 0..50u8 {
            let (_header, payload) =
                tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                    .await
                    .expect("read within 2s")
                    .expect("read sdu");
            assert_eq!(
                payload,
                vec![i],
                "demand {i} arrived out of order or was lost"
            );
        }

        drop(demand);
        let outcome = tokio::time::timeout(Duration::from_secs(2), muxer_task)
            .await
            .expect("muxer terminated")
            .expect("muxer did not panic");
        assert!(outcome.is_ok(), "muxer returned {outcome:?}");
    }

    /// A demand enqueued in the race window just before the last
    /// `EgressDemand` is dropped is still drained by the shutdown
    /// path (`drain_remaining`), not silently lost.
    #[tokio::test]
    async fn shutdown_drains_remaining_demands() {
        let (client, server) = tokio::io::duplex(8192);
        let (_reader, writer) = Bearer::new(client).split();
        let writer = Arc::new(Mutex::new(writer));
        let mut server_bearer = Bearer::new(server);

        let (demand, muxer) = egress_channel();

        // Enqueue, then immediately drop the producer — the muxer is
        // not yet running, so this exercises the drain-on-shutdown
        // path: the wake `recv()` returns `None` but the queue is
        // non-empty.
        demand
            .enqueue(
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolDir::Initiator,
                b"last gasp".to_vec(),
            )
            .await
            .expect("enqueue");
        drop(demand);

        let muxer_task = tokio::spawn(run_muxer(
            muxer,
            Arc::clone(&writer),
            EgressConfig::default(),
        ));

        let (_header, payload) =
            tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                .await
                .expect("read within 2s")
                .expect("the pre-shutdown demand must still be drained");
        assert_eq!(payload, b"last gasp");

        let outcome = tokio::time::timeout(Duration::from_secs(2), muxer_task)
            .await
            .expect("muxer terminated")
            .expect("muxer did not panic");
        assert!(outcome.is_ok(), "muxer returned {outcome:?}");
    }

    /// `enqueue` after the muxer has stopped returns
    /// `EgressError::MuxerStopped` rather than silently queueing into
    /// the void.
    #[tokio::test]
    async fn enqueue_after_muxer_stopped_errors() {
        let (client, _server) = tokio::io::duplex(8192);
        let (_reader, writer) = Bearer::new(client).split();
        let writer = Arc::new(Mutex::new(writer));

        let (demand, muxer) = egress_channel();
        let muxer_task = tokio::spawn(run_muxer(
            muxer,
            Arc::clone(&writer),
            EgressConfig::default(),
        ));

        // No producer activity; the muxer is blocked on its wake
        // channel. We cannot drop the muxer's notify receiver
        // directly, so instead: spawn-then-abort the muxer task to
        // close the receiver.
        muxer_task.abort();
        let _ = muxer_task.await;

        // The notify receiver is now dropped; enqueue must report it.
        let result = demand
            .enqueue(
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolDir::Initiator,
                b"orphan".to_vec(),
            )
            .await;
        assert!(
            matches!(result, Err(EgressError::MuxerStopped)),
            "enqueue after the muxer stopped must report MuxerStopped; got {result:?}"
        );
    }

    /// The default egress config never segments a real trace SDU:
    /// `sdu_size` and `batch_size` are both `u16::MAX`. Pinned so a
    /// future regression that picks a small default — which would
    /// start splitting `MsgTraceObjectsReply` SDUs and could regress
    /// the live conformance test — fails here first.
    #[test]
    fn default_config_does_not_segment_trace_sdus() {
        assert_eq!(
            EgressConfig::default(),
            EgressConfig::CARDANO_TRACER_DEFAULT
        );
        assert_eq!(
            EgressConfig::CARDANO_TRACER_DEFAULT.sdu_size,
            u16::MAX as usize
        );
        assert_eq!(
            EgressConfig::CARDANO_TRACER_DEFAULT.batch_size,
            u16::MAX as usize
        );
        assert_eq!(MAX_SDUS_PER_BATCH, 100);
        assert_eq!(EGRESS_QUEUE_CAPACITY, 100);
    }
}
