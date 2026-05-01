## Round 223 — Phase D.2 second slice: wire lifetime stats + aggregate Prometheus exposition

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: D.2 (multi-session peer accounting — second slice)

### Goal

R222 added the `PeerLifetimeStats` data structure + accessor
methods on `GovernorState`.  R223 wires the first concrete update
points and exposes aggregate counters via `/metrics`.

### Wiring

Two production update points in
[`node/src/runtime.rs::PeerSessionManager::promote_to_warm`](../../node/src/runtime.rs):

1. **Successful handshake → `Ok(session)` branch**: after the
   existing `governor_state.record_success(peer)` (which resets the
   session-keyed `failures` map), call
   `governor_state.record_lifetime_session_started(peer)` to bump
   the lifetime `sessions` and `successful_handshakes` counters.

2. **Connection failure → `Err(err)` branch**: after the existing
   `governor_state.record_failure(peer)` (which feeds the
   exponential-backoff state), call
   `governor_state.record_lifetime_session_failure(peer)` to bump
   the lifetime `failures_total` counter.

### Aggregate Prometheus exposition

Two new aggregate counters on `NodeMetrics`:

```
yggdrasil_peer_lifetime_sessions_total  (counter)
yggdrasil_peer_lifetime_failures_total  (counter)
```

Both fields published via the existing snapshot flow in
[`node/src/tracer.rs`](../../node/src/tracer.rs) — extended
`NodeMetrics` struct, `MetricsSnapshot`, `to_prometheus_text`
rendering, plus two new setters:

- `set_peer_lifetime_sessions_total(total)`
- `set_peer_lifetime_failures_total(total)`

The runtime governor tick at
[`node/src/runtime.rs:~2490`](../../node/src/runtime.rs)
(alongside `set_peer_selection_counters`) now also folds across
`governor_state.lifetime_stats.values()` to compute totals and
calls the new setters.

### Mainnet verification

```
$ rm -rf /tmp/ygg-r223-mainnet-db /tmp/ygg-r223-mainnet.sock
$ ./target/release/yggdrasil-node run --network mainnet \
    --database-path /tmp/ygg-r223-mainnet-db \
    --socket-path /tmp/ygg-r223-mainnet.sock \
    --peer 3.135.125.51:3001 \
    --metrics-port 12470 \
    --max-concurrent-block-fetch-peers 4 &
$ sleep 60
$ curl -s http://127.0.0.1:12470/metrics | grep yggdrasil_peer_lifetime
```

Result:

```
# HELP yggdrasil_peer_lifetime_sessions_total ...
# TYPE yggdrasil_peer_lifetime_sessions_total counter
yggdrasil_peer_lifetime_sessions_total 2
# HELP yggdrasil_peer_lifetime_failures_total ...
# TYPE yggdrasil_peer_lifetime_failures_total counter
yggdrasil_peer_lifetime_failures_total 0
```

Live counts at the same moment:
```
yggdrasil_known_peers       3
yggdrasil_established_peers 3
yggdrasil_active_peers      1
```

The lifetime counter (2 sessions) is **distinct** from the live
active gauge (1) — exactly the observability win Phase D.2 was
designed for.  If the active peer churned and reconnected,
`active_peers` would stay at 1 but
`yggdrasil_peer_lifetime_sessions_total` would tick to 3.
Operators can compute peer-churn rate as
`rate(yggdrasil_peer_lifetime_sessions_total[5m])`, which the
session-keyed gauges cannot expose.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 746 passed / 0 failed / 1 ignored
                                 # (R222 baseline preserved)
cargo build --release            # clean (38.61 s)
```

### Strategic significance

R223 closes the foundational + observability slice of Phase D.2:
the lifetime stats accumulate in the right places, are aggregated
across peers, and are exposed via the standard Prometheus
endpoint.  Operator dashboards can now graph real peer churn
distinct from the live session counts.

### Open follow-ups (Phase D.2 remaining slices)

R222 + R223 cover the **session counters**.  The remaining
`PeerLifetimeStats` fields (`bytes_in`, `bytes_out`) require
plumbing the existing `BlockFetchInstrumentation::note_success`
byte counters into the lifetime stats, plus per-peer instrumentation
in ChainSync and TxSubmission2 byte accounting.  This is a
follow-up slice — the data model is already in place per R222.

Plus the unchanged Phase D.1 deep cross-epoch rollback recovery,
Phase E.1 cardano-base coordinated fixture refresh, and Phase E.2
24h+ mainnet sync rehearsal items.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md)
  step D.2.
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §6.
- Previous round: [R222](2026-04-30-round-222-peer-lifetime-stats-foundation.md).
- Captures: `/tmp/ygg-r223-mainnet.log`.
- Touched files (2):
  - `node/src/runtime.rs` — promote_to_warm wiring + aggregate
    governor-tick update.
  - `node/src/tracer.rs` — `NodeMetrics` fields, `MetricsSnapshot`
    fields, two setters, Prometheus rendering for both counters.
- Upstream reference: same as R222 (`KnownPeers.knownPeerInfo`
  parallel).
