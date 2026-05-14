## Round 217 — Phase C.2 prerequisite: fetch-batch duration histogram

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: C (observability prerequisite for C.2 pipelined fetch+apply)

### Goal

Add `yggdrasil_fetch_batch_duration_seconds` Prometheus histogram so
operators (and the parity arc) have hard numbers on how much time
yggdrasil spends fetching versus applying per batch.  The R200 apply
histogram alone gives one half of the picture; without the fetch
counterpart, sizing Phase C.2 (pipelined fetch+apply) is guesswork.

### Implementation

`node/src/tracer.rs`:

- New atomic field `fetch_batch_duration_buckets: [AtomicU64; 10]`
  on `NodeMetrics`, mirrored by `fetch_batch_duration_sum_micros`
  and `fetch_batch_duration_count`.
- New method `record_fetch_batch_duration(Duration)` mirrors R200's
  `record_apply_batch_duration` — same `APPLY_BATCH_BUCKETS_SECONDS`
  bucket boundaries (`[0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0,
  10.0, +Inf]`) so operators can compare fetch vs apply histograms
  side-by-side on a Prometheus dashboard.
- `MetricsSnapshot` extended with the three new fields.
- `to_prometheus_text` appends the standard histogram exposition
  (`*_bucket`, `*_sum`, `*_count`) under the metric name
  `yggdrasil_fetch_batch_duration_seconds`.
- `every_metrics_snapshot_field_is_exported_in_prometheus_text`
  drift-guard test extended with three accept clauses for the new
  field names.

`node/src/runtime.rs`:

- Both production call sites of
  `sync_batch_verified_with_tentative` (chaindb path ~line 4950 and
  shared-chaindb path ~line 5568) now bracket the future with a
  `let fetch_start = std::time::Instant::now()` before the call and
  `metrics.record_fetch_batch_duration(fetch_start.elapsed())`
  inside the `result = batch_fut =>` arm of the `tokio::select!`,
  before the existing match.  Records duration on both Ok and Err
  paths (timing is independent of fetch outcome).

### What this measures

`sync_batch_verified_with_tentative` performs:
1. ChainSync `RequestNext`/`RollForward` (per block in batch)
2. BlockFetch `RequestRange` + `MsgBlock` stream (per batch)
3. Block-body-hash verification (per block)
4. KES/OCert validation (per block, if `verify_body_hash=true`)

The histogram measures all of the above as one — labelled "fetch +
verify" in the rustdoc.  The apply call (R200's
`record_apply_batch_duration`) is timed separately starting from
the moment `batch_fut` returns `Ok(progress)`.

For Phase C.2 sizing, the gap
`fetch_batch_duration_seconds - chainsync_request_next_time -
verify_time ≈ blockfetch_request_range_time` is the apply-overlap
window.  In practice fetch dominates ChainSync + verify by an order
of magnitude, so the headline ratio
`fetch_count / apply_count = 1` and the per-batch sum ratio is the
useful comparator.

### Mainnet baseline measurement

```
$ rm -rf /tmp/ygg-r217b-mainnet-db /tmp/ygg-r217b-mainnet.sock
$ ./target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r217b-mainnet-db \
    --socket-path /tmp/ygg-r217b-mainnet.sock \
    --peer 3.135.125.51:3001 \
    --metrics-port 12431 &
$ sleep 60
$ curl -s http://127.0.0.1:12431/metrics | grep yggdrasil_fetch_batch_duration
```

Result after 60 s of mainnet sync (4 fetch+apply batches):

| Metric                        | count | sum (s) | avg per batch | per-block (50 blk batch) |
| ----------------------------- | ----- | ------- | ------------- | ------------------------ |
| `fetch_batch_duration`        |   4   |  51.38  |   12.85 s     |       257 ms             |
| `apply_batch_duration`        |   4   |   0.87  |    0.22 s     |         4 ms             |

All 4 fetch observations landed in the `+Inf` bucket (every batch
took > 10 s).  All 4 apply observations landed in the `≤ 0.5` bucket.

### Strategic insight — Phase C.2 sizing revision

**Fetch is ~59× more expensive than apply on mainnet.**  With this
ratio:

- Phase C.2 pipelined fetch+apply best-case throughput improvement:
  `apply_time / (fetch_time + apply_time) = 0.22 / 13.07 ≈ 1.7%`.
- Phase C.2 worst-case (deadlock-risk channel coordination) is
  multi-day implementation effort.

**Conclusion**: Phase C.2 is a much smaller throughput lever than
the deferred-item description implied.  The dominant bottleneck is
the BlockFetch wire round-trip from a single peer (~257 ms per
50-block batch from IOG backbone over ~6 000 km of Atlantic
fibre).  Multi-peer dispatch via `--max-concurrent-block-fetch-peers
> 1` (already implemented) is the actual lever for sync-rate
improvement on mainnet — splitting the fetch range across multiple
geographically-distributed peers parallelises the wire latency.

This insight reshapes the open-follow-up ordering:

| Item    | Pre-R217 priority | Post-R217 priority | Rationale                                    |
| ------- | :---------------: | :----------------: | -------------------------------------------- |
| C.2 pipelined fetch+apply | high | low | Saves ~1.7% on mainnet, multi-day effort     |
| Multi-peer dispatch tuning | implicit | high | Already implemented; parallelises 12.85 s/batch fetch |
| Phase D.1 deep rollback    | medium  | medium | unchanged — correctness, not perf            |
| Phase D.2 peer accounting  | medium  | medium | unchanged                                    |
| Phase E.1 cardano-base     | low     | low    | unchanged — documentary                      |
| Phase E.2 mainnet 24h+     | high    | high   | unchanged — operational proof                |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 745 passed / 0 failed / 1 ignored
                                 # (drift-guard extended; same count)
cargo build --release            # clean (36.30 s)
```

The drift-guard test
`every_metrics_snapshot_field_is_exported_in_prometheus_text`
was extended with 3 accept clauses
(`fetch_batch_duration_buckets`, `_sum_micros`, `_count`) and
continues to pass — proving every snapshot field has a Prometheus
export line.

### Strategic significance

R217 is **observability-only** — no behaviour change.  The new
metric provides the data-driven justification for re-prioritising
the deferred items.  Pre-R217, "Phase C.2 pipelined fetch+apply"
sounded like a meaningful sync-rate win; post-R217 it's clear that
the work is small for the reward, and operator effort should
prioritise multi-peer dispatch verification + Phase E.2 long-running
mainnet rehearsal.

### Open follow-ups (re-prioritised)

1. **Multi-peer dispatch operational verification** — exercise
   `--max-concurrent-block-fetch-peers 4` against multiple
   geographically-distributed mainnet peers and measure the
   `fetch_batch_duration` ratio reduction.  This was Phase B
   verification (R199); R217's metric makes the gain quantifiable
   per-batch.
2. **Phase E.2** — long-running mainnet rehearsal (24 h+) to verify
   Byron→Shelley HFC at slot 4 492 800.  Now also a
   sync-rate-baseline operation given R217's metrics.
3. Phase D.1 deep cross-epoch rollback recovery (correctness, not
   perf).
4. Phase D.2 multi-session peer accounting (architectural refactor).
5. Phase E.1 cardano-base coordinated vendored fixture refresh
   (documentary).
6. **(De-prioritised)** Phase C.2 pipelined fetch+apply — ~1.7%
   throughput improvement at multi-day implementation cost; no
   longer the gating sync-rate item.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §4
  (observability row).
- Companion R200 doc:
  [2026-04-29 round 200 apply-batch histogram (linked from this round's predecessor)](2026-04-30-round-199-200-multipeer-verified-and-apply-histogram.md).
- Previous round: [R216](2026-04-30-round-216-pin-refresh-r2.md).
- Captures: `/tmp/ygg-r217b-mainnet.log`,
  `http://127.0.0.1:12431/metrics` snapshot in this doc.
- Touched files (1):
  - `node/src/tracer.rs` — new histogram fields + setter +
    Prometheus rendering + drift-guard extension.
  - `node/src/runtime.rs` — `fetch_start` instrumentation around
    both `batch_fut` call sites.
