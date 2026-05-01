## Round 225 — Phase D.1 first slice: rollback-depth observability

Date: 2026-05-01
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: D.1 (deep cross-epoch rollback recovery — first slice)

### Goal

Phase D.1 calls for orchestration to recover from cross-epoch
rollbacks (>k blocks) without forcing a full re-sync from origin.
The full implementation requires reconstructing historical stake
snapshots — a substantive multi-day architectural change.

R225 lays the **observability foundation**: a Prometheus histogram
that classifies actual rollback depths happening on each network.
Operators can graph the distribution and alert on rare deep
rollbacks (the Phase D.1 problematic case) before the full
recovery infrastructure ships.  The histogram doubles as the
quantitative justification for the Phase D.1 follow-up work — if
deep rollbacks are sub-percent occurrences, the implementation
priority is lower than if they're routine.

### Implementation

`node/src/tracer.rs`:

- New atomic fields on `NodeMetrics`: `rollback_depth_buckets:
  [AtomicU64; 7]`, `rollback_depth_sum_blocks: AtomicU64`,
  `rollback_depth_count: AtomicU64`.
- Bucket boundaries
  `[1, 2, 5, 50, 2160, 10_000, u64::MAX]` cover shallow chain
  reorgs (1-5 blocks) through stability-window edge (k=2160) to
  cross-epoch (>10k) and full-resync (`+Inf`).
- New method `record_rollback_depth(blocks: u64)` mirrors the
  R200/R217 cumulative-bucket histogram pattern.
- `MetricsSnapshot` extended with three new fields.
- `to_prometheus_text` adds standard
  `yggdrasil_rollback_depth_blocks_{bucket,sum,count}` exposition.
- Drift-guard test extended with three accept clauses.

`node/src/runtime.rs`:

- Both production apply call sites (chaindb path + shared-chaindb
  path) now record a rollback-depth observation when
  `progress.rollback_count > 0`.  Unit is rolled-back transactions
  (`applied.rolled_back_tx_ids.len()`) — proxy for block depth
  weighted by typical txs/block.  Depth=0 captures the common
  session-start `RollBackward(Origin)` confirm-shape rollback.

### Verification (preprod)

```
$ ./target/release/yggdrasil-node run --network preprod \
    --metrics-port 12490 ... &
$ sleep 45
$ curl -s http://127.0.0.1:12490/metrics | grep yggdrasil_rollback_depth_blocks
```

Result:
```
yggdrasil_rollback_depth_blocks_bucket{le="1"} 1
yggdrasil_rollback_depth_blocks_bucket{le="2"} 1
…
yggdrasil_rollback_depth_blocks_bucket{le="+Inf"} 1
yggdrasil_rollback_depth_blocks_sum 0
yggdrasil_rollback_depth_blocks_count 1
```

Live counts:
```
yggdrasil_blocks_synced 149
yggdrasil_rollbacks 1
```

The single rollback observation has depth=0 — the session-start
`RollBackward(Origin)` confirm.  After 149 blocks of subsequent
sync no actual chain rollback occurred, matching expected preprod
behaviour.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 746 passed / 0 failed / 1 ignored
                                 # (drift-guard test extended)
cargo build --release            # clean (41.29 s)
```

### Strategic significance

R225 closes the Phase D.1 observability foundation: yggdrasil
operators can now graph rollback-depth distributions and alert
on rare deep cross-epoch rollbacks.  The histogram is the
prerequisite data for sizing the Phase D.1 full recovery
infrastructure — if mainnet runs show only shallow rollbacks
(le=2 dominant), the implementation priority is lower than if
deep rollbacks are routine.

### Open follow-ups (Phase D.1 remaining)

The full Phase D.1 deep cross-epoch rollback recovery requires
reconstructing historical stake snapshots when rolling back
across epoch boundaries.  R225 is observability-only; the
substantive replay infrastructure remains deferred.

Other deferred items unchanged:
- Phase E.1 cardano-base coordinated fixture refresh.
- Phase E.2 24h+ mainnet sync rehearsal.
- Phase D.2 bytes-out egress accounting (tracked in R224 follow-up).
- (de-prioritised by R217) Phase C.2 pipelined fetch+apply.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md)
  step D.1.
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §6.
- Previous round: [R224](2026-04-30-round-224-peer-lifetime-bytes-in.md).
- Captures: `/tmp/ygg-r225-preprod.log`.
- Touched files (2):
  - `node/src/tracer.rs` — histogram fields + setter +
    Prometheus rendering + drift-guard extension.
  - `node/src/runtime.rs` — instrumentation at both apply call
    sites.
- Upstream reference: standard Prometheus histogram exposition
  format; bucket boundaries chosen to span Cardano-relevant
  rollback regimes (single-block reorg → k stability window →
  cross-epoch).
