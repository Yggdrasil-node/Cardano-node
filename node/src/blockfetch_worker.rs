//! Per-peer BlockFetch worker tasks (Phase 6 step 3 of the
//! [architecture docs](../../docs/ARCHITECTURE.md)).
//!
//! Mirrors upstream `Ouroboros.Network.BlockFetch.ClientRegistry`
//! semantics from `IntersectMBO/ouroboros-network`:
//!
//! - Each peer has a long-lived task that owns its `BlockFetchClient`.
//!   The task drains an `mpsc` queue of [`FetchRequest`]s, dispatches
//!   each on the wire, and sends the result back through a
//!   per-request `oneshot` channel.  Per-peer state stays in the
//!   per-peer task — there is no `Arc<Mutex<BlockFetchClient>>`
//!   wrapper, no lock contention between peers, and no shared
//!   borrow that crosses an `await`.
//! - A central [`FetchWorkerPool`] holds the set of registered
//!   workers keyed on `SocketAddr`.  The runtime adds workers when
//!   peers promote to warm (or hot) and removes them on disconnect.
//! - Multi-peer dispatch is two-phase: send all requests first, then
//!   await all responses.  Workers process their requests in
//!   parallel because each runs in its own task; from the
//!   dispatcher's perspective there is no `tokio::spawn` and no
//!   `'static` closure constraint.
//!
//! ## Why this shape
//!
//! Rust's borrow checker rejects `&mut BlockFetchClient` borrows that
//! cross `await` points when multiple peers must be polled
//! concurrently.  Upstream Haskell solves the equivalent problem
//! with STM-based `FetchClientStateVars`; the Rust analogue is
//! per-peer task ownership plus channel-based message passing.
//! This module is the seam.
//!
//! ## Operational parity with the Haskell node
//!
//! - Each connected peer runs a single fetch worker task — same as
//!   upstream's per-peer `fetchClient` thread.
//! - The fetch decision policy (currently
//!   [`crate::sync::dispatch_range_with_tentative`]) sees per-peer
//!   handles as opaque endpoints, mirroring upstream where
//!   `fetchDecisions` reads per-peer `FetchClientStateVars` via STM
//!   without knowing the underlying connection lifecycle.
//! - On peer disconnect: drop the [`FetchWorkerHandle`] →
//!   `mpsc::Sender` drops → the worker task observes a closed
//!   channel and exits cleanly.  The runtime can `await` the
//!   `JoinHandle` for graceful shutdown signalling.
//! - On peer reconnect: spawn a fresh worker.  No state carries
//!   over; in-flight requests at the moment of disconnect are
//!   reported as errors via the dispatcher's standard error path.
//!
//! Reference:
//! [`Ouroboros.Network.BlockFetch.ClientRegistry`](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/BlockFetch/ClientRegistry.hs);
//! [`Ouroboros.Network.BlockFetch.Decision.fetchDecisions`](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/BlockFetch/Decision.hs);
//! [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) Phase 6.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Instant;

use tokio::sync::{mpsc, oneshot};
use yggdrasil_ledger::{Point, SlotNo};
use yggdrasil_network::{
    BlockFetchClient, BlockFetchInstrumentation, blockfetch_pool::ReorderBuffer,
};

use crate::sync::{BlockFetchAssignment, MultiEraBlock, SyncError};

/// Result type for a fetched range — vector of `(raw_bytes, decoded_block)`
/// tuples in chain order.  Aliased to keep generic signatures readable.
pub type FetchedRange<B> = Vec<(Vec<u8>, B)>;

/// Result alias produced by the per-peer fetch closure and consumed
/// by [`oneshot::Sender`] in [`FetchRequest`].
pub type FetchOutcome<B> = Result<FetchedRange<B>, SyncError>;

/// Default per-peer request queue depth.  Mirrors upstream
/// `bfcMaxConcurrencyDeadline = 1` — one in-flight request per peer
/// at a time.  Bulk-sync mode (`bfcMaxConcurrencyBulkSync = 2`)
/// would set this to 2 by overriding via [`FetchWorkerHandle::spawn_with_queue_depth`].
pub const DEFAULT_WORKER_QUEUE_DEPTH: usize = 1;

/// A request sent to a per-peer fetch worker.
///
/// The worker dispatches the requested range on its
/// `BlockFetchClient` and replies through `response`.  If the worker
/// is shut down before processing the request, `response` is
/// dropped and the awaiting caller observes a `RecvError` which
/// the higher-level helpers translate into a [`SyncError::Recovery`].
pub struct FetchRequest<B> {
    /// Lower bound of the range (inclusive).
    pub lower: Point,
    /// Upper bound of the range (inclusive).
    pub upper: Point,
    /// One-shot reply channel.  Dropped if the worker exits before
    /// processing the request.
    pub response: oneshot::Sender<FetchOutcome<B>>,
}

/// Handle to a per-peer fetch worker task.
///
/// Cloning is intentionally not provided: ownership of the request
/// `Sender` is exclusive so the runtime knows precisely when a
/// worker shuts down.  To dispatch from multiple call sites, route
/// through a [`FetchWorkerPool`] which holds the handle.
///
/// Drop semantics: dropping the handle closes the request channel,
/// which causes the worker task to observe the channel close and
/// exit its loop.  The `JoinHandle` is consumed by
/// [`FetchWorkerHandle::shutdown`] for callers that want graceful
/// shutdown signalling, or implicitly dropped (the task is detached
/// and runs to completion).
pub struct FetchWorkerHandle<B> {
    addr: SocketAddr,
    sender: mpsc::Sender<FetchRequest<B>>,
    join: tokio::task::JoinHandle<()>,
}

impl<B: Send + 'static> FetchWorkerHandle<B> {
    /// Spawn a per-peer fetch worker with the default queue depth
    /// ([`DEFAULT_WORKER_QUEUE_DEPTH`] = 1).
    ///
    /// `fetch_one` is the closure the worker invokes for each
    /// `FetchRequest`.  In production this captures the peer's
    /// `BlockFetchClient` (moved into the closure so no `Arc<Mutex>`
    /// is needed).  In tests, callers pass a synthetic closure that
    /// returns canned data.
    ///
    /// Mirrors upstream `withFetchClient` from
    /// `Ouroboros.Network.BlockFetch.Client`, which spawns the
    /// per-peer fetch thread bound to a fresh
    /// `FetchClientStateVars`.
    pub fn spawn<F, Fut>(addr: SocketAddr, fetch_one: F) -> Self
    where
        F: FnMut(SocketAddr, Point, Point) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Vec<(Vec<u8>, B)>, SyncError>> + Send,
    {
        Self::spawn_with_queue_depth(addr, fetch_one, DEFAULT_WORKER_QUEUE_DEPTH)
    }

    /// Spawn a per-peer fetch worker with an explicit queue depth.
    /// Use this to opt into upstream `bfcMaxConcurrencyBulkSync = 2`
    /// behaviour (queue_depth = 2) on syncing nodes.
    pub fn spawn_with_queue_depth<F, Fut>(
        addr: SocketAddr,
        mut fetch_one: F,
        queue_depth: usize,
    ) -> Self
    where
        F: FnMut(SocketAddr, Point, Point) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Vec<(Vec<u8>, B)>, SyncError>> + Send,
    {
        let queue_depth = queue_depth.max(1);
        let (sender, mut receiver) = mpsc::channel::<FetchRequest<B>>(queue_depth);
        let join = tokio::spawn(async move {
            // Drain requests until the channel closes.  Channel close
            // is the worker's exit signal — same semantics as upstream
            // where the fetch thread exits when its TVar input goes
            // empty + connection terminated.
            while let Some(req) = receiver.recv().await {
                let result = fetch_one(addr, req.lower, req.upper).await;
                // Ignore the send error: the caller dropping the
                // receiver is a normal cancellation.  Mirrors upstream
                // where TVar updates are silently absorbed if no
                // reader is waiting.
                let _ = req.response.send(result);
            }
        });
        Self { addr, sender, join }
    }

    /// The peer address this worker serves.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Round-trip a fetch request to the worker, awaiting the
    /// response.  Returns `Err(SyncError::Recovery)` if the worker
    /// channel is closed (peer disconnected, worker shut down) or
    /// if the worker dropped the response sender (request cancelled
    /// during processing).
    ///
    /// The `await` here is two distinct waits internally:
    /// 1. `mpsc::Sender::send` waits for queue-depth space.
    /// 2. `oneshot::Receiver::await` waits for the worker to
    ///    process and reply.
    ///
    /// Both are cancellation-safe; the caller's outer
    /// `tokio::select!` may abandon the future without leaking
    /// state in the worker.
    pub async fn fetch(&self, lower: Point, upper: Point) -> FetchOutcome<B> {
        let (response_tx, response_rx) = oneshot::channel();
        let req = FetchRequest {
            lower,
            upper,
            response: response_tx,
        };
        self.sender.send(req).await.map_err(|_| {
            SyncError::Recovery(format!(
                "fetch worker channel closed for peer {}",
                self.addr
            ))
        })?;
        response_rx.await.map_err(|_| {
            SyncError::Recovery(format!(
                "fetch worker dropped response for peer {}",
                self.addr
            ))
        })?
    }

    /// Initiate worker shutdown by dropping the request channel and
    /// returning the worker's `JoinHandle` so the caller can `await`
    /// graceful exit.
    ///
    /// Equivalent to dropping `self`, except the caller observes
    /// completion.  Useful for runtime teardown sequences.
    pub fn shutdown(self) -> tokio::task::JoinHandle<()> {
        // Drop sender → channel closes → worker task exits.
        let Self { join, .. } = self;
        join
    }

    /// Returns `true` if the worker's request channel is closed
    /// (worker has exited).  Cheap snapshot with no allocation.
    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

impl FetchWorkerHandle<MultiEraBlock> {
    /// Spawn a per-peer fetch worker that owns a real
    /// [`BlockFetchClient`] and dispatches requests via the
    /// `crate::sync::fetch_range_blocks_multi_era_raw_decoded` helper.
    ///
    /// This is the production wire: the runtime calls this when
    /// promoting a peer to warm so the per-peer fetch task takes
    /// ownership of the negotiated `BlockFetchClient` for the rest
    /// of the connection's lifetime.  On disconnect, the runtime
    /// calls [`FetchWorkerHandle::shutdown`] (or drops the handle)
    /// to gracefully exit the task.
    ///
    /// The closure-based [`FetchWorkerHandle::spawn`] entry point
    /// stays available for tests using synthetic fetch closures.
    /// Both produce a [`FetchWorkerHandle`] with identical channel
    /// semantics, so [`FetchWorkerPool`] can hold a mix.
    ///
    /// Reference: upstream
    /// [`Ouroboros.Network.BlockFetch.Client.fetchClient`](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/BlockFetch/Client.hs)
    /// — the per-peer fetch thread that owns the connection's fetch
    /// state for the connection's lifetime.
    pub fn spawn_with_block_fetch_client(addr: SocketAddr, block_fetch: BlockFetchClient) -> Self {
        Self::spawn_with_block_fetch_client_and_queue_depth(
            addr,
            block_fetch,
            DEFAULT_WORKER_QUEUE_DEPTH,
        )
    }

    /// Variant of [`FetchWorkerHandle::spawn_with_block_fetch_client`]
    /// with explicit queue depth.  Use `2` to opt into upstream's
    /// `bfcMaxConcurrencyBulkSync = 2` for syncing nodes.
    pub fn spawn_with_block_fetch_client_and_queue_depth(
        addr: SocketAddr,
        mut block_fetch: BlockFetchClient,
        queue_depth: usize,
    ) -> Self {
        let queue_depth = queue_depth.max(1);
        let (sender, mut receiver) = mpsc::channel::<FetchRequest<MultiEraBlock>>(queue_depth);
        let join = tokio::spawn(async move {
            // The async-block owns `block_fetch` by value; each loop
            // iteration takes a fresh `&mut` borrow that lives only
            // for the duration of the awaited
            // `fetch_range_blocks_multi_era_raw_decoded` call.  No
            // borrow crosses iterations, so the outer `'static`
            // bound on the spawned task is satisfied.
            while let Some(req) = receiver.recv().await {
                let result = crate::sync::fetch_range_blocks_multi_era_raw_decoded(
                    &mut block_fetch,
                    req.lower,
                    req.upper,
                )
                .await;
                let _ = req.response.send(result);
            }
        });
        Self { addr, sender, join }
    }
}

/// Registry of per-peer [`FetchWorkerHandle`]s keyed on `SocketAddr`.
///
/// Mirrors upstream
/// [`Ouroboros.Network.BlockFetch.ClientRegistry`](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/BlockFetch/ClientRegistry.hs)
/// which holds `Map peer FetchClientStateVars`.  The runtime adds
/// entries on peer promote-to-warm, removes them on disconnect, and
/// dispatches multi-peer plans through [`FetchWorkerPool::dispatch_plan`].
///
/// `B` is the block type — production callers parameterise as
/// `crate::sync::MultiEraBlock`; tests use a `u64` placeholder.
#[derive(Default)]
pub struct FetchWorkerPool<B> {
    workers: BTreeMap<SocketAddr, FetchWorkerHandle<B>>,
}

impl<B> std::fmt::Debug for FetchWorkerPool<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Render only the registered peer addresses + count; the
        // worker handles themselves contain non-Debug runtime
        // resources (mpsc::Sender, JoinHandle).
        f.debug_struct("FetchWorkerPool")
            .field("registered_peers", &self.workers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl<B: Send + 'static> FetchWorkerPool<B> {
    /// Construct an empty pool.
    pub fn new() -> Self {
        Self {
            workers: BTreeMap::new(),
        }
    }

    /// Insert a worker for a peer.  If the peer was already
    /// registered, the previous handle is returned so the caller
    /// can shut it down deterministically.  Mirrors upstream
    /// behaviour where `bracketSyncWithFetchClient` always exits
    /// the previous registration before re-entering.
    pub fn register(&mut self, handle: FetchWorkerHandle<B>) -> Option<FetchWorkerHandle<B>> {
        self.workers.insert(handle.addr, handle)
    }

    /// Remove and return the worker for `addr`.  Used by the runtime
    /// on peer disconnect.  The caller is responsible for awaiting
    /// the returned handle's `shutdown()` if a graceful exit is
    /// required.
    pub fn unregister(&mut self, addr: &SocketAddr) -> Option<FetchWorkerHandle<B>> {
        self.workers.remove(addr)
    }

    /// Borrow the worker for `addr`, if registered.
    pub fn worker(&self, addr: &SocketAddr) -> Option<&FetchWorkerHandle<B>> {
        self.workers.get(addr)
    }

    /// All currently-registered peer addresses, in `BTreeMap` order
    /// (sort by `SocketAddr`).  Stable ordering across ticks so the
    /// dispatcher sees peers in a deterministic sequence — same
    /// invariant the governor's
    /// [`crate::sync::partition_fetch_range_across_peers`] honours.
    pub fn peer_addrs(&self) -> Vec<SocketAddr> {
        self.workers.keys().copied().collect()
    }

    /// Number of registered workers.
    pub fn len(&self) -> usize {
        self.workers.len()
    }

    /// Returns `true` when no workers are registered.
    pub fn is_empty(&self) -> bool {
        self.workers.is_empty()
    }

    /// Garbage-collect workers whose channels have closed (e.g. the
    /// task panicked or completed).  Returns the addresses removed
    /// so the runtime can re-spawn or surface the failure.
    pub fn prune_closed(&mut self) -> Vec<SocketAddr> {
        let to_remove: Vec<SocketAddr> = self
            .workers
            .iter()
            .filter(|(_, h)| h.is_closed())
            .map(|(addr, _)| *addr)
            .collect();
        for addr in &to_remove {
            self.workers.remove(addr);
        }
        to_remove
    }

    /// Dispatch a multi-peer fetch plan through the registered
    /// workers and return the assembled blocks in chain order.
    ///
    /// Two-phase dispatch:
    /// 1. **Send** all `FetchRequest`s synchronously.  This phase
    ///    is fast (no fetch I/O happens here, just channel sends).
    /// 2. **Await** all responses.  Workers run in parallel because
    ///    each is its own task; the await loop aggregates results
    ///    in the same task as the caller.
    ///
    /// Error handling: on the first error response, propagate
    /// immediately.  Pending oneshot receivers are dropped — the
    /// workers complete their fetches but discard the results
    /// (no leaks, just wasted bandwidth).  Workers stay registered
    /// for subsequent iterations.
    ///
    /// Multi-chunk plans against `from_point = Origin` are rejected
    /// upfront — a from-genesis sync cannot anchor intermediate chunk
    /// boundaries (their hashes are placeholder), and out-of-order
    /// release would violate the validator's chain-extension check.
    /// Single-chunk plans from Origin are accepted: the post-fetch
    /// drain seeds the [`ReorderBuffer`] head from the chunk's lower
    /// before draining so the delivered blocks release cleanly.
    /// Reference: `docs/MANUAL_TEST_RUNBOOK.md` §6.5a "Round 91 Gap BN"
    /// closure (Round 144).
    pub async fn dispatch_plan(
        &self,
        plan: &[BlockFetchAssignment],
        from_point: Point,
        pool_instr: Option<&BlockFetchInstrumentation>,
    ) -> FetchOutcome<B> {
        if plan.is_empty() {
            return Ok(Vec::new());
        }
        if plan.len() > 1 && matches!(from_point, Point::Origin) {
            return Err(SyncError::Recovery(
                "multi-chunk BlockFetch dispatch requires non-Origin from_point; \
                 from-genesis sync cannot anchor intermediate chunk boundaries"
                    .to_owned(),
            ));
        }

        // Single-chunk fast path (Round 144 — closes Round 91 Gap BN).
        // A single-chunk plan has no reordering concern: the worker
        // returns blocks in the same chain order the peer delivered
        // them.  Routing through the [`ReorderBuffer`] in this case is
        // pure overhead, AND silently breaks genesis bootstrap because
        // `peek_releasable` short-circuits on Origin head whenever the
        // head_seed is `Origin` (or whenever the chunk's lower-slot is
        // `0`, which produces buffer key `0` ≤ any non-Origin head
        // slot).  Bypass the buffer for single-chunk plans entirely;
        // the multi-peer / out-of-order case still routes through the
        // buffer below.  Reference: `docs/MANUAL_TEST_RUNBOOK.md`
        // §6.5a "Round 91 Gap BN" notice.
        if plan.len() == 1 {
            let asn = plan[0];
            let worker = self.workers.get(&asn.peer).ok_or_else(|| {
                SyncError::Recovery(format!(
                    "fetch worker not registered for peer {} (caller must register before dispatch)",
                    asn.peer
                ))
            })?;
            let (response_tx, response_rx) = oneshot::channel();
            let req = FetchRequest {
                lower: asn.lower,
                upper: asn.upper,
                response: response_tx,
            };
            worker.sender.send(req).await.map_err(|_| {
                SyncError::Recovery(format!(
                    "fetch worker channel closed for peer {} during dispatch",
                    asn.peer
                ))
            })?;
            if let Some(instr) = pool_instr {
                if let Ok(mut g) = instr.lock() {
                    g.note_dispatch(asn.peer);
                }
            }
            let result = response_rx.await.map_err(|_| {
                SyncError::Recovery(format!(
                    "fetch worker dropped response for peer {} mid-dispatch",
                    asn.peer
                ))
            })?;
            return match result {
                Ok(blocks) => {
                    if let Some(instr) = pool_instr {
                        if let Ok(mut g) = instr.lock() {
                            let n = blocks.len() as u64;
                            let bytes: u64 = blocks.iter().map(|(raw, _)| raw.len() as u64).sum();
                            g.note_success(asn.peer, n, bytes, Instant::now());
                        }
                    }
                    Ok(blocks)
                }
                Err(err) => {
                    if let Some(instr) = pool_instr {
                        if let Ok(mut g) = instr.lock() {
                            g.note_failure(asn.peer);
                        }
                    }
                    Err(err)
                }
            };
        }

        // Phase 1 — dispatch.  Each `worker.sender.send()` may briefly
        // wait if the worker's queue is full (queue_depth saturated),
        // but for a freshly-issued plan against idle workers this is
        // O(N) channel sends.
        type Pending<B> = (BlockFetchAssignment, oneshot::Receiver<FetchOutcome<B>>);
        let mut pending: Vec<Pending<B>> = Vec::with_capacity(plan.len());
        for asn in plan {
            let worker = self.workers.get(&asn.peer).ok_or_else(|| {
                SyncError::Recovery(format!(
                    "fetch worker not registered for peer {} (caller must register before dispatch)",
                    asn.peer
                ))
            })?;
            let (response_tx, response_rx) = oneshot::channel();
            let req = FetchRequest {
                lower: asn.lower,
                upper: asn.upper,
                response: response_tx,
            };
            worker.sender.send(req).await.map_err(|_| {
                SyncError::Recovery(format!(
                    "fetch worker channel closed for peer {} during dispatch",
                    asn.peer
                ))
            })?;
            if let Some(instr) = pool_instr {
                if let Ok(mut g) = instr.lock() {
                    g.note_dispatch(asn.peer);
                }
            }
            pending.push((*asn, response_rx));
        }

        // Phase 2 — await.  Workers process in parallel; this loop
        // aggregates results.  On first error: propagate; pending
        // receivers drop, workers complete and discard results.
        let head_seed = head_seed_for_buffer(from_point);
        let mut buffer: ReorderBuffer<(Vec<u8>, B)> = ReorderBuffer::new(head_seed);

        for (asn, rx) in pending {
            let result = rx.await.map_err(|_| {
                SyncError::Recovery(format!(
                    "fetch worker dropped response for peer {} mid-dispatch",
                    asn.peer
                ))
            })?;
            match result {
                Ok(blocks) => {
                    if let Some(instr) = pool_instr {
                        if let Ok(mut g) = instr.lock() {
                            let n = blocks.len() as u64;
                            let bytes: u64 = blocks.iter().map(|(raw, _)| raw.len() as u64).sum();
                            g.note_success(asn.peer, n, bytes, Instant::now());
                        }
                    }
                    buffer.insert(asn.lower, asn.upper, blocks);
                }
                Err(err) => {
                    if let Some(instr) = pool_instr {
                        if let Ok(mut g) = instr.lock() {
                            g.note_failure(asn.peer);
                        }
                    }
                    return Err(err);
                }
            }
        }

        let mut out = Vec::new();
        for (_lower, _upper, blocks) in buffer.drain_releasable() {
            out.extend(blocks);
        }
        Ok(out)
    }
}

/// Compute the `ReorderBuffer` head-seed from a `from_point`.
/// Same logic as `execute_multi_peer_blockfetch_plan` — the buffer
/// requires lower-slot strictly greater than head-slot, but
/// `partition_fetch_range_across_peers` puts the first chunk's lower
/// equal to `from_point`, so seed one slot below.
fn head_seed_for_buffer(from_point: Point) -> Point {
    match from_point {
        Point::Origin => Point::Origin,
        Point::BlockPoint(slot, hash) => {
            if slot.0 == 0 {
                Point::BlockPoint(SlotNo(0), hash)
            } else {
                Point::BlockPoint(SlotNo(slot.0 - 1), hash)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn block_point(slot: u64) -> Point {
        Point::BlockPoint(SlotNo(slot), yggdrasil_ledger::HeaderHash([0u8; 32]))
    }

    fn fake_block(slot: u64) -> (Vec<u8>, u64) {
        (vec![slot as u8; 4], slot)
    }

    /// Closure returning a fixed result for any (peer, lower, upper)
    /// triple.  Used by tests that don't care about the request
    /// shape, only that the worker round-trips.
    type EchoFut = std::pin::Pin<Box<dyn std::future::Future<Output = FetchOutcome<u64>> + Send>>;

    fn echo_closure(
        result: FetchOutcome<u64>,
    ) -> impl FnMut(SocketAddr, Point, Point) -> EchoFut + Send + 'static {
        let result = std::sync::Arc::new(result);
        move |_a, _l, _u| {
            let r = result.clone();
            Box::pin(async move {
                match &*r {
                    Ok(v) => Ok(v.clone()),
                    Err(e) => Err(SyncError::Recovery(format!("{e}"))),
                }
            })
        }
    }

    #[tokio::test]
    async fn worker_round_trips_a_single_request() {
        let p = addr(3001);
        let worker: FetchWorkerHandle<u64> =
            FetchWorkerHandle::spawn(p, echo_closure(Ok(vec![fake_block(50)])));
        let result = worker.fetch(block_point(40), block_point(60)).await;
        assert!(matches!(result, Ok(v) if v.len() == 1 && v[0].1 == 50));
    }

    #[tokio::test]
    async fn worker_propagates_fetch_error() {
        let p = addr(3002);
        let worker: FetchWorkerHandle<u64> = FetchWorkerHandle::spawn(
            p,
            echo_closure(Err(SyncError::Recovery("simulated".into()))),
        );
        let err = worker
            .fetch(block_point(10), block_point(20))
            .await
            .expect_err("error must propagate");
        assert!(matches!(err, SyncError::Recovery(_)));
    }

    #[tokio::test]
    async fn worker_serves_multiple_sequential_requests() {
        // queue_depth = 1 means the worker must process one request
        // fully before accepting the next.  Three sequential fetches
        // exercise that contract.
        let p = addr(3003);
        let mut counter: u64 = 0;
        let closure = move |_addr, _lower, _upper| {
            counter += 1;
            let n = counter;
            async move { Ok(vec![fake_block(n)]) }
        };
        let worker: FetchWorkerHandle<u64> = FetchWorkerHandle::spawn(p, closure);
        for expected in 1..=3u64 {
            let r = worker
                .fetch(block_point(0), block_point(expected))
                .await
                .expect("fetch ok");
            assert_eq!(r[0].1, expected);
        }
    }

    #[tokio::test]
    async fn worker_shutdown_drains_then_exits() {
        let p = addr(3004);
        let worker: FetchWorkerHandle<u64> =
            FetchWorkerHandle::spawn(p, echo_closure(Ok(Vec::new())));
        // Issue one fetch so the loop has run at least once.
        let _ = worker.fetch(block_point(1), block_point(2)).await;
        assert!(!worker.is_closed());
        let join = worker.shutdown();
        // Worker exits cleanly once the channel closes.
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(1), join)
            .await
            .expect("shutdown must complete within 1s");
        assert!(outcome.is_ok(), "worker task must exit without panic");
    }

    #[tokio::test]
    async fn fetch_after_shutdown_returns_channel_closed_error() {
        // Drop the handle (via shutdown), then a second handle to the
        // same peer must report channel-closed.  Simulates the runtime
        // scenario where a peer has disconnected between dispatch and
        // a stale caller's late call.
        let p = addr(3005);
        let worker: FetchWorkerHandle<u64> =
            FetchWorkerHandle::spawn(p, echo_closure(Ok(Vec::new())));
        // Issue fetch with the channel still open.
        let ok = worker.fetch(block_point(1), block_point(2)).await;
        assert!(ok.is_ok());
        // Now close — the next fetch on the SAME handle must error
        // because we drop sender via shutdown.  Use a stable
        // construction: spawn, drop sender by emulating a closed
        // worker via a drop-aware test helper.
        //
        // Simpler: build a worker whose closure panics so the task
        // exits, then send fetch — it observes the channel close.
        let p2 = addr(3006);
        let worker2: FetchWorkerHandle<u64> = FetchWorkerHandle::spawn(p2, |_, _, _| async {
            // Force the worker task to terminate via panic.  In real
            // production code worker termination happens through
            // channel close (sender drop), not panic; this test
            // exercises the *receive* side's error handling when
            // the worker is gone.
            panic!("intentional test panic");
        });
        let _ = worker2.fetch(block_point(1), block_point(2)).await; // panic happens here
        // Give the panic a moment to propagate and close the channel.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let err = worker2
            .fetch(block_point(3), block_point(4))
            .await
            .expect_err("after panic, channel must be closed");
        assert!(matches!(err, SyncError::Recovery(_)));
    }

    #[tokio::test]
    async fn pool_register_replaces_existing_worker() {
        let p = addr(3010);
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        let w1 = FetchWorkerHandle::spawn(p, echo_closure(Ok(Vec::new())));
        assert!(pool.register(w1).is_none());
        // Re-register: previous handle must come back so caller can
        // shut it down deterministically.
        let w2 = FetchWorkerHandle::spawn(p, echo_closure(Ok(Vec::new())));
        let prev = pool.register(w2);
        assert!(prev.is_some());
        assert_eq!(prev.unwrap().addr(), p);
    }

    #[tokio::test]
    async fn pool_unregister_returns_handle_for_graceful_shutdown() {
        let p = addr(3011);
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        pool.register(FetchWorkerHandle::spawn(p, echo_closure(Ok(Vec::new()))));
        assert_eq!(pool.len(), 1);
        let h = pool.unregister(&p).expect("registered");
        assert_eq!(h.addr(), p);
        assert!(pool.is_empty());
        // Drop the handle to detach the worker.  The worker exits
        // cleanly once its mpsc receiver observes the channel close;
        // we don't await the JoinHandle to keep the unit test snappy.
        drop(h);
    }

    #[tokio::test]
    async fn pool_peer_addrs_returns_btreemap_sorted_view() {
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        let p3 = addr(3023);
        let p1 = addr(3021);
        let p2 = addr(3022);
        pool.register(FetchWorkerHandle::spawn(p3, echo_closure(Ok(Vec::new()))));
        pool.register(FetchWorkerHandle::spawn(p1, echo_closure(Ok(Vec::new()))));
        pool.register(FetchWorkerHandle::spawn(p2, echo_closure(Ok(Vec::new()))));
        // BTreeMap ordering by SocketAddr — same invariant the
        // governor's partition planner relies on.
        assert_eq!(pool.peer_addrs(), vec![p1, p2, p3]);
    }

    #[tokio::test]
    async fn pool_dispatch_plan_returns_empty_for_empty_plan() {
        let pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        let result = pool
            .dispatch_plan(&[], block_point(100), None)
            .await
            .expect("empty plan succeeds");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn pool_dispatch_plan_rejects_genesis_multi_peer() {
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        let p1 = addr(3030);
        let p2 = addr(3031);
        pool.register(FetchWorkerHandle::spawn(p1, echo_closure(Ok(Vec::new()))));
        pool.register(FetchWorkerHandle::spawn(p2, echo_closure(Ok(Vec::new()))));
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: Point::Origin,
                upper: block_point(50),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(51),
                upper: block_point(100),
            },
        ];
        let err = pool
            .dispatch_plan(&plan, Point::Origin, None)
            .await
            .expect_err("genesis multi-peer must error");
        assert!(matches!(err, SyncError::Recovery(_)));
    }

    #[tokio::test]
    async fn pool_dispatch_plan_releases_single_chunk_genesis() {
        // Round 144 fix for Round 91 Gap BN.  Pre-fix, a single-chunk
        // plan dispatched with `from_point = Origin` slipped past the
        // `plan.len() > 1` guard, the worker fetched blocks correctly,
        // but the ReorderBuffer silently dropped them at drain time
        // because `peek_releasable` short-circuits on Origin head —
        // dispatch_plan returned `Ok(vec![])`, the runtime advanced
        // `from_point` past the upper bound without ever writing the
        // blocks to volatile storage, and the node livelocked
        // re-syncing from Origin on every session handoff.
        //
        // After the fix, single-chunk plans bypass the ReorderBuffer
        // entirely (no reorder concern with one chunk), so the worker's
        // delivered blocks pass through directly.  Exercises the
        // production shape `partition_fetch_range_across_peers` produces
        // for from-genesis sync: `split_range(Origin, upper, n)` returns
        // `[(Origin, upper)]` so the assignment carries `lower = Origin`.
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        let p = addr(3032);
        pool.register(FetchWorkerHandle::spawn(
            p,
            echo_closure(Ok(vec![fake_block(1), fake_block(50)])),
        ));
        let plan = vec![BlockFetchAssignment {
            peer: p,
            lower: Point::Origin,
            upper: block_point(50),
        }];
        let result = pool
            .dispatch_plan(&plan, Point::Origin, None)
            .await
            .expect("single-chunk genesis dispatch must release");
        let slots: Vec<u64> = result.iter().map(|(_, s)| *s).collect();
        assert_eq!(
            slots,
            vec![1, 50],
            "single-chunk genesis plan must deliver every fetched block in chain order — \
             pre-fix this returned an empty Vec because the ReorderBuffer dropped the chunk"
        );
    }

    #[tokio::test]
    async fn pool_dispatch_plan_single_chunk_records_pool_instrumentation() {
        // The single-chunk fast path must still record dispatch /
        // success / failure on the pool instrumentation so the
        // governor's per-peer accounting (consecutive_failures,
        // blocks_delivered, bytes_delivered) stays accurate.  Without
        // this, a peer serving every batch via the genesis fast path
        // would never count toward the demote-on-failure threshold.
        use std::sync::Arc;
        use std::sync::Mutex;
        use yggdrasil_network::blockfetch_pool::{BlockFetchPool, FetchMode};
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        let p = addr(3033);
        pool.register(FetchWorkerHandle::spawn(
            p,
            echo_closure(Ok(vec![fake_block(7)])),
        ));
        let mut bp = BlockFetchPool::new(FetchMode::FetchModeBulkSync);
        bp.register_peer(p);
        let instr: BlockFetchInstrumentation = Arc::new(Mutex::new(bp));
        let plan = vec![BlockFetchAssignment {
            peer: p,
            lower: Point::Origin,
            upper: block_point(7),
        }];
        let _ = pool
            .dispatch_plan(&plan, Point::Origin, Some(&instr))
            .await
            .expect("single-chunk dispatch must succeed");
        let guard = instr.lock().expect("instrumentation lock");
        let state = guard.peer_state(p).expect("peer must be registered");
        assert_eq!(state.blocks_delivered, 1);
        assert_eq!(state.consecutive_failures, 0);
        assert!(state.last_success.is_some());
    }

    #[tokio::test]
    async fn pool_dispatch_plan_unknown_peer_returns_explicit_error() {
        // Ensure the error message names the offending peer so
        // operators can quickly identify a registry-out-of-sync bug.
        let pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        let p = addr(3040);
        let plan = vec![BlockFetchAssignment {
            peer: p,
            lower: block_point(10),
            upper: block_point(20),
        }];
        let err = pool
            .dispatch_plan(&plan, block_point(5), None)
            .await
            .expect_err("unknown peer must error");
        match err {
            SyncError::Recovery(msg) => {
                assert!(msg.contains(&format!("{p}")));
                assert!(msg.contains("not registered"));
            }
            other => panic!("expected Recovery error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn pool_dispatch_plan_assembles_two_peers_in_chain_order() {
        let p1 = addr(3050);
        let p2 = addr(3051);
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();

        // Per-peer closures return distinct slot ranges.
        pool.register(FetchWorkerHandle::spawn(p1, |_a, _l, _u| async {
            Ok(vec![fake_block(60), fake_block(100)])
        }));
        pool.register(FetchWorkerHandle::spawn(p2, |_a, _l, _u| async {
            Ok(vec![fake_block(150), fake_block(200)])
        }));

        // Plan covers (50, 200) split between the two peers.
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: block_point(50),
                upper: block_point(125),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(126),
                upper: block_point(200),
            },
        ];

        let result = pool
            .dispatch_plan(&plan, block_point(50), None)
            .await
            .expect("two-peer dispatch succeeds");
        assert_eq!(result.len(), 4);
        // ReorderBuffer guarantees chain order regardless of which
        // worker responds first.
        let slots: Vec<u64> = result.iter().map(|(_, s)| *s).collect();
        assert_eq!(slots, vec![60, 100, 150, 200]);
    }

    #[tokio::test]
    async fn pool_dispatch_plan_propagates_first_chunk_error() {
        let p1 = addr(3060);
        let p2 = addr(3061);
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        pool.register(FetchWorkerHandle::spawn(
            p1,
            echo_closure(Err(SyncError::Recovery("simulated p1 failure".into()))),
        ));
        pool.register(FetchWorkerHandle::spawn(
            p2,
            echo_closure(Ok(vec![fake_block(100)])),
        ));
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: block_point(50),
                upper: block_point(125),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(126),
                upper: block_point(200),
            },
        ];
        let err = pool
            .dispatch_plan(&plan, block_point(50), None)
            .await
            .expect_err("p1 failure must propagate");
        assert!(matches!(err, SyncError::Recovery(_)));
    }

    #[tokio::test]
    async fn pool_prune_closed_removes_dead_workers() {
        let mut pool: FetchWorkerPool<u64> = FetchWorkerPool::new();
        let p_alive = addr(3070);
        let p_dead = addr(3071);

        pool.register(FetchWorkerHandle::spawn(
            p_alive,
            echo_closure(Ok(Vec::new())),
        ));
        // Build a worker whose closure panics so its channel closes.
        pool.register(FetchWorkerHandle::spawn(p_dead, |_, _, _| async {
            panic!("intentional");
        }));
        // Drive the dead worker into its panic so the channel closes.
        let dead_handle = pool.worker(&p_dead).expect("registered");
        let _ = dead_handle.fetch(block_point(1), block_point(2)).await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let pruned = pool.prune_closed();
        assert_eq!(pruned, vec![p_dead]);
        assert!(pool.worker(&p_alive).is_some());
        assert!(pool.worker(&p_dead).is_none());
    }
}
