## Round 234 — Phase D.2 bytes-out (initial slice): BlockFetch server bytes-served counter

Date: 2026-05-01
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: D.2 (multi-session peer accounting — bytes-out initial slice)

### Goal

R222–R226 + R229 shipped the Phase D.2 lifetime peer-stats
deliverable as 5 Prometheus counters covering INGRESS
(yggdrasil-as-client) traffic.  The deferred-items list flagged
"bytes-out (egress) requires per-mini-protocol byte accounting on
the server-emit path" as a substantive remaining slice.

R234 ships the **first concrete bytes-out instrumentation point**:
a global aggregate counter for bytes served by the BlockFetch
server (the most data-dense egress path).  ChainSync header bytes
and TxSubmission2 reply bytes remain deferred to a follow-up that
can use the same instrumentation pattern.

### Implementation

**Code change** in [`node/src/server.rs::run_blockfetch_server`](../../node/src/server.rs):

```rust
pub async fn run_blockfetch_server(
    mut server: BlockFetchServer,
    provider: &dyn BlockProvider,
    metrics: Option<&crate::tracer::NodeMetrics>,    // R234 — new param
) -> Result<(), BlockFetchServerError> {
    loop {
        match server.recv_request().await? {
            BlockFetchServerRequest::RequestRange(range) => {
                let blocks = provider.get_block_range(&range.lower, &range.upper);
                if let Some(m) = metrics {
                    let bytes_out: u64 = blocks.iter().map(|b| b.len() as u64).sum();
                    m.add_blockfetch_server_bytes_served(bytes_out);
                }
                server.serve_batch(blocks).await?;
            }
            ...
        }
    }
}
```

**`NodeMetrics` extension** in [`node/src/tracer.rs`](../../node/src/tracer.rs):

- New field `blockfetch_server_bytes_served_total: AtomicU64`.
- Mirror field on `MetricsSnapshot`.
- New setter `add_blockfetch_server_bytes_served(n)` (additive,
  not absolute — each `serve_batch` call contributes its own
  bytes).
- Prometheus exposition adds
  `yggdrasil_blockfetch_server_bytes_served_total` (counter).

**Caller wiring** in [`node/src/server.rs::run_inbound_accept_loop`](../../node/src/server.rs):

The BlockFetch responder spawn now clones the metrics handle
into the closure scope:

```rust
let bf_metrics: Option<Arc<NodeMetrics>> = metrics.cloned();
tokio::spawn(async move {
    ...
    let _ = run_blockfetch_server(
        session.block_fetch,
        &*provider,
        bf_metrics.as_deref(),
    ).await;
    ...
});
```

### End-to-end verification (instance-to-instance preprod)

Two yggdrasil instances on the same host: A listens, B connects to A.

```
$ ./target/release/yggdrasil-node run --network preprod \
    --metrics-port 12494 --port 13061 ...                # instance A
$ ./target/release/yggdrasil-node run --network preprod \
    --metrics-port 12495 --port 13062 \
    --peer 127.0.0.1:13061 ...                            # instance B
$ sleep 30
```

**Result — egress/ingress symmetry confirmed**:

| Side | Metric                                          | Value     |
| ---- | ----------------------------------------------- | --------: |
| A (server) | `yggdrasil_blockfetch_server_bytes_served_total` | **100 500** |
| A (server) | `yggdrasil_blocks_synced` (own ingress from upstream) | 199 |
| A (server) | `yggdrasil_inbound_connections_accepted`        | 1 (from B) |
| B (client) | `yggdrasil_peer_lifetime_bytes_in_total`        | **100 500** |
| B (client) | `yggdrasil_blocks_synced`                       | 100 (synced from A) |

**The two byte counters match exactly** — A's server-side
`blockfetch_server_bytes_served_total` (100 500) equals B's
client-side `peer_lifetime_bytes_in_total` (100 500).  This is
the **operational proof of correctness** for R234: every byte A
serves to B as a peer is counted on both sides; no leakage, no
double-counting.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 749 passed / 0 failed / 1 ignored
cargo build --release            # clean (33.26 s)
```

### Strategic significance

R234 closes the **major Phase D.2 bytes-out gap** with a single
aggregate counter that quantifies yggdrasil's egress traffic when
acting as a peer.  Combined with R220+R221's bidirectional P2P
parity, an operator can now see **how much yggdrasil is
contributing to the upstream Cardano network** as a relay.

The "bytes-out remains 0" caveat from R224 is now resolved for
the BlockFetch path — the most data-dense mini-protocol on
mainnet.  ChainSync header bytes (~94 bytes per RollForward) and
TxSubmission2 reply bytes are at least an order of magnitude
smaller and remain deferred to a follow-up using the same
instrumentation pattern (clone metrics into the responder spawn,
sum bytes per response).

### Per-peer attribution still deferred

R234 is **aggregate-only** (one global counter).  Per-peer
attribution requires threading the remote `SocketAddr` through
the `BlockFetchServer` run-loop signature, which is a larger
refactor.  Operators who need per-peer egress bytes can correlate
this counter with the live `inbound_connections_accepted` count
during traffic windows; full per-peer attribution remains
deferred to a follow-up that also covers ChainSync and
TxSubmission2 egress.

### Open follow-ups

1. **Phase D.2 ChainSync server bytes-out** — same pattern: tally
   raw_header bytes per `MsgRollForward`.
2. **Phase D.2 TxSubmission2 server bytes-out** — same pattern:
   tally tx body bytes per `MsgReplyTxs`.
3. **Phase D.2 per-peer egress attribution** — threads remote
   `SocketAddr` through the responder run-loops; substantial
   refactor.
4. Phase D.1 full deep-rollback recovery (gated by R225 mainnet
   distribution data).
5. Phase E.1 cardano-base coordinated fixture refresh.
6. Phase E.2 24h+ mainnet sync rehearsal.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md)
  step D.2.
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §6.
- Previous round: [R233](2026-05-01-round-225-rollback-depth-histogram.md).
- Captures: `/tmp/ygg-r234b-a.log` (instance A — server),
  `/tmp/ygg-r234b-b.log` (instance B — client).
- Touched files (3):
  - `node/src/server.rs` — `run_blockfetch_server` signature +
    instrumentation; caller updated to clone metrics into spawn.
  - `node/src/tracer.rs` — `NodeMetrics` field + setter +
    snapshot mirror + Prometheus rendering.
- Upstream reference: standard Prometheus counter exposition.
