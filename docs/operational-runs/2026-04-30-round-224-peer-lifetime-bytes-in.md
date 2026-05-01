## Round 224 — Phase D.2 third slice: lifetime bytes-in counter

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: D.2 (multi-session peer accounting — third slice)

### Goal

R222 + R223 covered session counters; R224 wires the byte-counter
slice that completes the major Phase D.2 deliverable.  The
existing `BlockFetchInstrumentation::peer_state(peer)
.bytes_delivered` already accumulates per-peer cumulative bytes
across reconnects (the pool registry survives session changes),
so we mirror it into `PeerLifetimeStats.bytes_in` at each
governor tick — no new accounting paths needed.

### Implementation

`crates/network/src/governor.rs`:

```rust
/// R224 — overwrite the lifetime `bytes_in` total for `peer`
/// from an external cumulative source (e.g. `BlockFetchInstrumentation
/// ::peer_state(peer).bytes_delivered`).  Use this instead of
/// `record_lifetime_traffic` when the source is already cumulative;
/// mixing the two would double-count.  Creates the lifetime entry
/// if absent.
pub fn set_lifetime_bytes_in(&mut self, peer: SocketAddr, total: u64) { … }
```

`node/src/runtime.rs` (governor-tick site, alongside R223 aggregate):

```rust
if let Some(pool) = config.block_fetch_pool.as_ref() {
    if let Ok(p) = pool.lock() {
        for (peer, state) in p.peers.iter() {
            governor_state.set_lifetime_bytes_in(*peer, state.bytes_delivered);
        }
    }
}
let (sessions_total, failures_total, bytes_in_total) = governor_state
    .lifetime_stats
    .values()
    .fold((0u64, 0u64, 0u64), |(s, f, b), e| {
        (s + e.sessions as u64, f + e.failures_total as u64, b + e.bytes_in)
    });
m.set_peer_lifetime_sessions_total(sessions_total);
m.set_peer_lifetime_failures_total(failures_total);
m.set_peer_lifetime_bytes_in_total(bytes_in_total);
```

`node/src/tracer.rs`:

- New `peer_lifetime_bytes_in_total: AtomicU64` field on `NodeMetrics`.
- Mirror field on `MetricsSnapshot`.
- New `set_peer_lifetime_bytes_in_total(total)` setter.
- Prometheus rendering adds
  `yggdrasil_peer_lifetime_bytes_in_total` counter alongside the
  existing R223 `yggdrasil_peer_lifetime_{sessions,failures}_total`.

### Mainnet verification

```
$ rm -rf /tmp/ygg-r224-mainnet-db /tmp/ygg-r224-mainnet.sock
$ ./target/release/yggdrasil-node run --network mainnet \
    --database-path /tmp/ygg-r224-mainnet-db \
    --socket-path /tmp/ygg-r224-mainnet.sock \
    --peer 3.135.125.51:3001 --metrics-port 12480 \
    --max-concurrent-block-fetch-peers 4 &
$ sleep 75
$ curl -s http://127.0.0.1:12480/metrics | grep yggdrasil_peer_lifetime
```

Result:

```
yggdrasil_peer_lifetime_sessions_total 2
yggdrasil_peer_lifetime_failures_total 0
yggdrasil_peer_lifetime_bytes_in_total 2511595
```

Live counts at the same moment:
```
yggdrasil_blocks_synced 299
yggdrasil_known_peers 3
yggdrasil_established_peers 3
yggdrasil_active_peers 3
```

**2.5 MB of cumulative blocks fetched** during a 75-second
mainnet sync window with 3 active warm workers — the byte counter
matches the order of magnitude expected from R218's per-batch
fetch numbers (~50 KB/batch × ~50 batches in the window).

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 746 passed / 0 failed / 1 ignored
                                 # (no new tests; existing R222 test
                                 #  + extended drift-guard cover the
                                 #  new field)
cargo build --release            # clean (35.92 s)
```

### Strategic significance

R222 + R223 + R224 together complete the **major Phase D.2
deliverable**: a parallel-tracking shadow data structure for
lifetime peer stats with three monotonic Prometheus counters
(`sessions`, `failures`, `bytes_in`) exposed via `/metrics`.  The
remaining `bytes_out` field is intentionally left at 0 in this
slice because there's no equivalent server-emit-side cumulative
source — adding it requires per-mini-protocol byte accounting on
the egress path, which is a larger architectural change than the
mirror approach used here.

### Open follow-ups

1. **Phase D.2 final slice (deferred)**: per-mini-protocol bytes-out
   accounting on the egress path (ChainSync header bytes,
   BlockFetch served-block bytes, TxSubmission2 reply bytes).
   This is a larger architectural change than the R224
   mirror-from-pool approach.
2. Phase D.1 — deep cross-epoch rollback recovery.
3. Phase E.1 — cardano-base coordinated vendored fixture refresh.
4. Phase E.2 — 24h+ mainnet sync rehearsal.
5. (de-prioritised by R217) Phase C.2 pipelined fetch+apply.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md)
  step D.2.
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §6.
- Previous round: [R223](2026-04-30-round-223-peer-lifetime-stats-wiring.md).
- Captures: `/tmp/ygg-r224-mainnet.log`.
- Touched files (3):
  - `crates/network/src/governor.rs` — new `set_lifetime_bytes_in`
    method.
  - `node/src/runtime.rs` — pool-iter at governor tick + new
    `bytes_in_total` aggregate setter call.
  - `node/src/tracer.rs` — `NodeMetrics` field, `MetricsSnapshot`
    field, setter, Prometheus rendering.
- Upstream reference: `Ouroboros.Network.PeerSelection.State.KnownPeers`
  byte-tracking pattern (per-peer cumulative counters).
