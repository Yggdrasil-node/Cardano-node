## Round 226 — Phase D.2 enhancement: unique-peers + handshakes-total counters

Date: 2026-05-01
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: D.2 (multi-session peer accounting — fourth slice)

### Goal

R222–R224 delivered Phase D.2's major scope: sessions, failures,
bytes_in lifetime counters.  R226 adds two additional aggregate
counters that are cheap to compute and operationally informative:

1. `yggdrasil_peer_lifetime_unique_peers` (gauge) — cardinality
   of `governor_state.lifetime_stats` map.  Counts distinct peer
   addresses ever observed during this process lifetime.
2. `yggdrasil_peer_lifetime_handshakes_total` (counter) — sum of
   `PeerLifetimeStats::successful_handshakes` across all peers.
   Already tracked per-peer by R222; R226 surfaces the aggregate.

The combination lets operators detect "peer entries created but
never promoted to warm" — useful for debugging configuration
issues (e.g. unreachable bootstrap peer that nonetheless lives in
the registry).

### Implementation

`node/src/tracer.rs`:
- Two new atomic fields: `peer_lifetime_unique_peers: AtomicU64`,
  `peer_lifetime_handshakes_total: AtomicU64`.
- Mirror fields on `MetricsSnapshot`.
- Two new setters
  `set_peer_lifetime_unique_peers(total)` and
  `set_peer_lifetime_handshakes_total(total)`.
- Prometheus rendering adds the two counters alongside the
  existing R222–R224 lifetime counters.

`node/src/runtime.rs` (governor-tick fold):
- Extends the existing fold to collect
  `successful_handshakes` alongside sessions/failures/bytes_in.
- Calls
  `m.set_peer_lifetime_unique_peers(governor_state.lifetime_stats.len() as u64)`
  and
  `m.set_peer_lifetime_handshakes_total(handshakes_total)`.

### Mainnet verification

```
$ ./target/release/yggdrasil-node run --network mainnet \
    --metrics-port 12491 --max-concurrent-block-fetch-peers 4 ... &
$ sleep 60
$ curl -s http://127.0.0.1:12491/metrics | grep yggdrasil_peer_lifetime
```

Result:

```
yggdrasil_peer_lifetime_sessions_total 2
yggdrasil_peer_lifetime_failures_total 0
yggdrasil_peer_lifetime_bytes_in_total 1548246
yggdrasil_peer_lifetime_unique_peers 3
yggdrasil_peer_lifetime_handshakes_total 2
```

**Operator-relevant observation**: `unique_peers (3) > sessions (2)`
reveals that 3 peer addresses are tracked but only 2 successfully
promoted to warm.  The third peer entry exists in
`lifetime_stats` (likely from byte-stats refresh via R224's
pool-iter, since the BlockFetch pool's peer registry creates
entries during outbound dispatch that don't always succeed) but
hasn't completed a successful handshake yet.

`handshakes_total = sessions_total` in this run because we're not
yet exercising the rare case where a session disconnects after
handshake but before any mini-protocol traffic — when that
happens, `handshakes` would tick higher than `sessions`.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 746 passed / 0 failed / 1 ignored
cargo build --release            # clean (42.84 s)
```

### Strategic significance

R226 closes the cumulative Phase D.2 deliverable to **5 monotonic
Prometheus counters / gauges** exposing per-process lifetime peer
behaviour:

| Metric                                    | Type    | Source                              |
| ----------------------------------------- | ------- | ----------------------------------- |
| `peer_lifetime_sessions_total`            | counter | R222 — promote_to_warm success      |
| `peer_lifetime_failures_total`            | counter | R222 — promote_to_warm error        |
| `peer_lifetime_bytes_in_total`            | counter | R224 — BlockFetch bytes_delivered   |
| `peer_lifetime_unique_peers`              | gauge   | R226 — lifetime_stats cardinality   |
| `peer_lifetime_handshakes_total`          | counter | R226 — successful_handshakes        |

Operators can now compute peer-health derived signals:

```promql
# Peer reliability ratio (failures per session)
yggdrasil_peer_lifetime_failures_total
  / yggdrasil_peer_lifetime_sessions_total

# Average bytes per session
yggdrasil_peer_lifetime_bytes_in_total
  / yggdrasil_peer_lifetime_sessions_total

# Handshake-but-no-session rate
1 - (yggdrasil_peer_lifetime_sessions_total
     / yggdrasil_peer_lifetime_handshakes_total)

# Unique-peer-but-no-session rate (registry leakage indicator)
1 - (yggdrasil_peer_lifetime_sessions_total
     / yggdrasil_peer_lifetime_unique_peers)
```

### Open follow-ups (Phase D.2 final remaining)

- Bytes-out aggregate counter (requires per-mini-protocol egress
  byte accounting on the server-emit path; substantive change).
- Per-peer labelled metrics (cardinality concern for large
  topologies; defer until needed).

Other deferred items unchanged: Phase D.1 full deep-rollback
recovery; Phase E.1 cardano-base coordinated fixture refresh;
Phase E.2 24h+ mainnet rehearsal; (de-prioritised) Phase C.2
pipelined fetch+apply.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md)
  step D.2.
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §6.
- Previous round: [R225](2026-05-01-round-225-rollback-depth-histogram.md).
- Captures: `/tmp/ygg-r226-mainnet.log`.
- Touched files (2):
  - `node/src/tracer.rs` — two new atomic fields + setters +
    snapshot mirror + Prometheus rendering.
  - `node/src/runtime.rs` — fold extension + two setter calls.
- Upstream reference: standard Prometheus counter/gauge exposition.
